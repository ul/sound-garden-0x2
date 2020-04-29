use crate::{commands::*, types::Id};
use anyhow::Result;
use audio_program::TextOp;
use clap::{app_from_crate, crate_authors, crate_description, crate_name, crate_version, Arg};
use crossbeam_channel::{Receiver, Sender};
use druid::{
    AppDelegate, AppLauncher, Command, DelegateCtx, Env, Lens, Point, Target, Widget, WidgetExt,
    WindowDesc,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use thread_worker::Worker;
use types::{id_to_string, str_to_id};
use xi_rope::{engine::Engine, DeltaBuilder, Rope};

mod canvas;
mod commands;
mod types;

/* Sound Garden Druid
 *
 * SGD is a collaborative interface for Sound Garden's Audio Synth Server and VST plugin.
 * It features modal editing of SG programs suitable for livecoding and supports P2P jams.
 * Eventually it could incorporate visual feedback like oscilloscope.
 *
 * Let's talk about design. SGD consists of
 * - UI, initially mimicking basic terminal, grid of monospace characters.
 * -- This is going to be implemented using druid. We benefit from its widget paint flexibility
 *    and beautiful font rendering. It's widget library and layout engine is less of use for now.
 * - Persistence layer which allows to save and load programs.
 * -- Let's ser/de engine state with serde (see below).
 * - Connectivity module to send programs and commands to synth server (standalone or VST plugin).
 * -- Regular std::net::TCPStream + serde should be enough.
 * - Connectivity module to exchange programs edits with peers.
 * -- Ideally we want some robust fully decentralised P2P protocol but having "LAN-oriented"
 *    naive code paired with TailScale should be enough for the start.
 * - Editing engine to support edits exchange with undo/redo.
 * -- We are going to use xi_rope::Engine for that. We imagine it'd work as follows:
 *    each peer creates a master engine for itself and one engine per connected peer.
 *    When user changes node model is converted into a normalised text representation
 *    of nodes with their metadata. Change in this representation is used to produce a delta
 *    which is sent to other peers. When peer receives delta it applies it to the corresponding
 *    peer engine and them merges it to master engine.
 *    Undo history is local and based on master engine.
 * - Persistent undo/redo.
 * -- Should come for free when persisting engine state.
 *
 * Physically editing screen could look like:
 *
 * /--------------\
 * |  440 s       |
 * |              |
 * |   .2 *       |
 * |              |
 * \--------------/
 *
 * While for editing engine it could be represented as
 * `123:0:1:440 124:0:5:s 125:2:2:.2 126:2:5:*`
 * (other separators or even format as a whole should be considered)
 * (for now `:` → `\t` and ` ` → `\n`, id is formatted as hex)
 * and treated as if was edited as such.
 *
 * This is a bit of a hack but should be safe and it would allow us to not invent own CRDT solution.
 */

/* Plan of Attack
 *
 * [ ] First, let's implement basic canvas to render nodes.
 * [ ] Then we can have some basic editing with a master engine only.
 * [ ] ???
 * [ ] Profit!
 */

#[derive(Serialize, Deserialize)]
struct Editor {
    engine: Engine,
    undo_group: usize,
}

struct App {
    editor: Editor,
    filename: String,
    audio_tx: Sender<audio_server::Message>,
    play: bool,
    record: bool,
}

#[derive(Clone, druid::Data, Default)]
struct Data {
    nodes: Arc<Vec<canvas::Node>>,
    draft_nodes: Arc<Vec<Id>>,
}

fn main() -> Result<()> {
    let matches = app_from_crate!()
        .arg(
            Arg::with_name("FILENAME")
                .required(true)
                .index(1)
                .help("Path to the tree"),
        )
        .arg(
            Arg::with_name("audio_port")
                .short("p")
                .long("audio_port")
                .value_name("PORT")
                .default_value("31337")
                .help("Port to send programs to"),
        )
        .get_matches();

    let filename = matches.value_of("FILENAME").unwrap();
    let audio_port = matches.value_of("audio_port").unwrap();
    let audio_address = format!("127.0.0.1:{}", audio_port);

    let audio_control = Worker::spawn(
        "Audio",
        1,
        move |rx: Receiver<audio_server::Message>, _: Sender<()>| {
            for msg in rx {
                if let Ok(stream) = std::net::TcpStream::connect(&audio_address) {
                    serde_json::to_writer(stream, &msg).ok();
                }
            }
        },
    );

    let editor = Editor::load(filename);

    let app = App {
        editor,
        filename: String::from(filename),
        audio_tx: audio_control.sender().clone(),
        play: false, // REVIEW it assumes server starts in pause mode
        record: false,
    };
    let mut data: Data = Default::default();
    data.nodes = Arc::new(app.generate_nodes());

    AppLauncher::with_window(WindowDesc::new(build_ui))
        .delegate(app)
        .launch(data)?;

    Ok(())
}

fn build_ui() -> impl Widget<Data> {
    canvas::Widget::default().lens(CanvasLens {})
}

impl Editor {
    fn new() -> Self {
        let mut engine = Engine::empty();
        engine.set_session_id((rand::random(), rand::random()));
        Editor {
            engine,
            undo_group: 0,
        }
    }

    // TODO Atomic write.
    fn save(&self, filename: &str) {
        serde_json::to_writer(std::fs::File::create(filename).unwrap(), self).ok();
    }

    fn load(filename: &str) -> Self {
        std::fs::File::open(filename)
            .ok()
            .and_then(|f| serde_json::from_reader::<_, Self>(f).ok())
            .unwrap_or_else(|| Self::new())
    }
}

impl App {
    fn save(&self) {
        self.editor.save(&self.filename);
    }

    fn generate_nodes(&self) -> Vec<canvas::Node> {
        self.editor
            .engine
            .get_head()
            .lines(..)
            .filter_map(|line| {
                if let [id, x, y, text] = line.split('\t').collect::<Vec<_>>()[..] {
                    Some(canvas::Node {
                        id: str_to_id(id),
                        position: Point::new(x.parse().unwrap(), y.parse().unwrap()),
                        text: text.to_string(),
                    })
                } else {
                    None
                }
            })
            .collect()
    }
}

impl AppDelegate<Data> for App {
    fn command(
        &mut self,
        _ctx: &mut DelegateCtx,
        _target: &Target,
        cmd: &Command,
        data: &mut Data,
        _env: &Env,
    ) -> bool {
        let result = match cmd.selector {
            NODE_INSERT_TEXT => {
                let NodeInsertText { id, index, text } = cmd.get_object().unwrap();
                let code = &self.editor.engine.get_head();
                let base_rev = self.editor.engine.get_head_rev_id().token();
                let id_prefix = id_to_string(*id) + "\t";
                if let Some(offset) = code
                    .lines(..)
                    .enumerate()
                    .find(|(_, line)| line.starts_with(&id_prefix))
                    .map(|(line, record)| {
                        let line_offset = code.offset_of_line(line);
                        let text_field_offset = record.rfind('\t').unwrap() + 1;
                        let text_field = &record[text_field_offset..];
                        text_field.char_indices().nth(*index).map_or_else(
                            || line_offset + text_field_offset + text_field.len(),
                            |(index, _)| line_offset + text_field_offset + index,
                        )
                    })
                {
                    let mut delta = DeltaBuilder::new(code.len());
                    delta.replace(offset..offset, Rope::from(text));
                    // REVIEW What should I put as a priority argument?
                    self.editor
                        .engine
                        .edit_rev(0, self.editor.undo_group, base_rev, delta.build());
                    data.draft_nodes =
                        Arc::new(data.draft_nodes.iter().chain(Some(id)).copied().collect());
                    self.save();
                }
                false
            }
            NODE_DELETE_CHAR => {
                let NodeDeleteChar { id, index } = cmd.get_object().unwrap();
                let code = &self.editor.engine.get_head();
                let base_rev = self.editor.engine.get_head_rev_id().token();
                let id_prefix = id_to_string(*id) + "\t";
                if let Some((start, end)) = code
                    .lines(..)
                    .enumerate()
                    .find(|(_, line)| line.starts_with(&id_prefix))
                    .and_then(|(line, record)| {
                        let line_offset = code.offset_of_line(line);
                        let text_field_offset = record.rfind('\t').unwrap() + 1;
                        let text_field = &record[text_field_offset..];
                        text_field.char_indices().nth(*index).map(|(index, char)| {
                            let start = line_offset + text_field_offset + index;
                            (start, start + char.len_utf8())
                        })
                    })
                {
                    let mut delta = DeltaBuilder::new(code.len());
                    delta.delete(start..end);
                    // REVIEW What should I put as a priority argument?
                    self.editor
                        .engine
                        .edit_rev(0, self.editor.undo_group, base_rev, delta.build());
                    data.draft_nodes =
                        Arc::new(data.draft_nodes.iter().chain(Some(id)).copied().collect());
                    self.save();
                }
                false
            }
            CREATE_NODE => {
                let CreateNode { text, position } = cmd.get_object().unwrap();
                let id = rand::random::<Id>();
                let code = &self.editor.engine.get_head();
                let base_rev = self.editor.engine.get_head_rev_id().token();
                let offset = code.len();
                let mut delta = DeltaBuilder::new(code.len());
                delta.replace(
                    offset..offset,
                    Rope::from(format!(
                        "{}\t{}\t{}\t{}\n",
                        id_to_string(id),
                        position.x,
                        position.y,
                        text
                    )),
                );
                // REVIEW What should I put as a priority argument?
                self.editor
                    .engine
                    .edit_rev(0, self.editor.undo_group, base_rev, delta.build());
                data.draft_nodes =
                    Arc::new(data.draft_nodes.iter().copied().chain(Some(id)).collect());
                self.save();
                false
            }
            NEW_UNDO_GROUP => {
                self.editor.undo_group += 1;
                false
            }
            COMMIT_PROGRAM => {
                let ops = self
                    .editor
                    .engine
                    .get_head()
                    .lines(..)
                    .filter_map(|line| {
                        if let [id, _, _, text] = line.split('\t').collect::<Vec<_>>()[..] {
                            if !text.is_empty() {
                                Some(TextOp {
                                    id: str_to_id(id),
                                    op: text.to_string(),
                                })
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    })
                    .collect();
                self.audio_tx
                    .send(audio_server::Message::LoadProgram(ops))
                    .ok();
                false
            }
            PLAY_PAUSE => {
                self.play = !self.play;
                self.audio_tx
                    .send(audio_server::Message::Play(self.play))
                    .ok();
                false
            }
            TOGGLE_RECORD => {
                self.record = !self.record;
                self.audio_tx
                    .send(audio_server::Message::Record(self.record))
                    .ok();
                false
            }
            _ => true,
        };
        data.nodes = Arc::new(self.generate_nodes());
        result
    }
}

struct CanvasLens {}

// REVIEW If it would stay that simple consider replacing with a derived lens.
impl Lens<Data, canvas::Data> for CanvasLens {
    fn with<V, F: FnOnce(&canvas::Data) -> V>(&self, data: &Data, f: F) -> V {
        let x = canvas::Data {
            nodes: Arc::clone(&data.nodes),
            draft_nodes: Arc::clone(&data.draft_nodes),
        };
        f(&x)
    }

    fn with_mut<V, F: FnOnce(&mut canvas::Data) -> V>(&self, data: &mut Data, f: F) -> V {
        let mut x = canvas::Data {
            nodes: Arc::clone(&data.nodes),
            draft_nodes: Arc::clone(&data.draft_nodes),
        };
        f(&mut x)
    }
}
