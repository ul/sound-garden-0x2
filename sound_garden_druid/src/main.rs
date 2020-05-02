use crate::{commands::*, types::Id};
use anyhow::Result;
use audio_program::TextOp;
use chrono::Local;
use clap::{app_from_crate, crate_authors, crate_description, crate_name, crate_version, Arg};
use crossbeam_channel::{Receiver, Sender};
use druid::{AppDelegate, AppLauncher, Command, DelegateCtx, Env, Point, Target, WindowDesc};
use serde::{Deserialize, Serialize};
use std::{
    convert::TryFrom,
    sync::{Arc, Mutex},
};
use thread_worker::Worker;
use xi_rope::{
    engine::{Engine, RevToken},
    Delta, DeltaBuilder, Rope, RopeInfo,
};

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
 * -- Until we emphasize remote audio servers.
 * - Connectivity module to exchange programs edits with peers.
 * -- Let's consider something like NNG.
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
 *    Okay, deltas don't work, we need to send the entire editor.
 *    Potential improvements:
 * --- First of all, set undo limit. The main source of payload size is unbounded undos.
 * --- Fork Engine to allow sending only a subset of revisions and send only new revisions since last sync.
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

/// Application business logic is associated with this structure,
/// we implement druid's AppDelegate for it.
struct App {
    /// Local editor state.
    /// Maintains Sound Garden Tree representation in the form friendly to collaborative editing.
    /// Manages editing history.
    editor: Arc<Mutex<Editor>>,
    /// Persistence location for the Tree.
    filename: String,
    /// Channel to control audio server.
    audio_tx: Sender<audio_server::Message>,
    /// Did we ask audio server to play?
    play: bool,
    /// Did we ask audio server to record?
    record: bool,
    /// Channel to communicate to peer.
    peer_tx: Option<Sender<()>>,
    /// The most recent revision converted to nodes.
    last_rendered_rev: RevToken,
}

#[derive(Serialize, Deserialize)]
struct Editor {
    engine: Engine,
    undo_group: usize,
}

fn main() -> Result<()> {
    // CLI interface.
    let matches = app_from_crate!()
        .arg(Arg::with_name("FILENAME").index(1).help("Path to the tree"))
        .arg(
            Arg::with_name("audio-port")
                .short("p")
                .long("audio-port")
                .value_name("PORT")
                .default_value("31337")
                .help("Port to send programs to"),
        )
        .arg(
            Arg::with_name("jam-local-port")
                .short("i")
                .long("jam-local-port")
                .value_name("PORT")
                .requires("jam-remote-address")
                .help("Port to expect the peer to connect to for a jam"),
        )
        .arg(
            Arg::with_name("jam-remote-address")
                .short("o")
                .long("jam-remote-address")
                .value_name("ADDRESS:PORT")
                .requires("jam-local-port")
                .help("Address of the peer to connect to for a jam"),
        )
        .get_matches();

    // Load or create Tree and start building the app.
    let filename = matches
        .value_of("FILENAME")
        .map(|s| s.to_owned())
        .unwrap_or_else(|| format!("{}.sg", Local::now().to_rfc3339()));
    let editor = Arc::new(Mutex::new(Editor::load(&filename)));
    let launcher = AppLauncher::with_window(WindowDesc::new(canvas::Widget::default));

    // Jam mode.
    // Start a thread to listen to the peer updates.
    if let Some(jam_port) = matches.value_of("jam-local-port") {
        let socket = nng::Socket::new(nng::Protocol::Bus0)?;
        let url = format!("tcp://:{}", jam_port);
        socket.listen(&url)?;
        let event_sink = launcher.get_external_handle();
        let editor = Arc::clone(&editor);
        std::thread::spawn(move || {
            while let Ok(msg) = socket.recv() {
                if let Ok(engine) =
                    serde_cbor::from_reader::<Engine, _>(snap::read::FrameDecoder::new(&msg[..]))
                {
                    editor.lock().unwrap().engine.merge(&engine);
                    event_sink.submit_command(REGENERATE_NODES, (), None).ok();
                }
            }
        });
    }

    // Start a worker to send updates to the peer.
    let peer = matches.value_of("jam-remote-address").map(|address| {
        let socket = nng::Socket::new(nng::Protocol::Bus0).unwrap();
        let url = format!("tcp://{}", address);
        socket.dial_async(&url).unwrap();
        let editor = Arc::clone(&editor);
        Worker::spawn(
            "Send to peer",
            1024,
            move |rx: Receiver<()>, _: Sender<()>| {
                for _ in rx {
                    let mut msg = nng::Message::new();
                    let stream = snap::write::FrameEncoder::new(&mut msg);
                    serde_cbor::to_writer(stream, &editor.lock().unwrap().engine).ok();
                    socket.send(msg).ok();
                }
            },
        )
    });

    // Start a worker to send messages to the audio server.
    let audio_control = {
        let port = matches.value_of("audio-port").unwrap();
        let address = format!("127.0.0.1:{}", port);
        Worker::spawn(
            "Audio",
            1,
            move |rx: Receiver<audio_server::Message>, _: Sender<()>| {
                for msg in rx {
                    if let Ok(stream) = std::net::TcpStream::connect(&address) {
                        serde_json::to_writer(stream, &msg).ok();
                    }
                }
            },
        )
    };

    // Finish building app and launch it.
    let app = App {
        editor: Arc::clone(&editor),
        filename: String::from(filename),
        audio_tx: audio_control.sender().clone(),
        play: false, // REVIEW it assumes server starts in pause mode
        record: false,
        peer_tx: peer.map(|x| x.sender().clone()),
        last_rendered_rev: Engine::empty().get_head_rev_id().token(),
    };

    let mut data: canvas::Data = Default::default();
    data.nodes = Arc::new(app.generate_nodes());

    launcher.delegate(app).launch(data)?;

    Ok(())
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

    fn load(filename: &str) -> Self {
        std::fs::File::open(filename)
            .ok()
            .map(|f| snap::read::FrameDecoder::new(f))
            .and_then(|f| serde_cbor::from_reader::<Self, _>(f).ok())
            .unwrap_or_else(|| Self::new())
    }

    // TODO Atomic write.
    fn save(&self, filename: &str) -> Result<()> {
        let f = std::fs::File::create(filename)?;
        serde_cbor::to_writer(snap::write::FrameEncoder::new(f), self)?;
        Ok(())
    }
}

impl App {
    fn save(&self) {
        self.editor.lock().unwrap().save(&self.filename).ok();
    }

    fn edit(&mut self, delta: Delta<RopeInfo>) {
        let mut editor = self.editor.lock().unwrap();
        let base_rev = editor.engine.get_head_rev_id().token();
        let undo_group = editor.undo_group;
        editor.engine.edit_rev(0, undo_group, base_rev, delta);
        if let Some(tx) = self.peer_tx.as_ref() {
            tx.send(()).ok();
        }
    }

    fn generate_nodes(&self) -> Vec<canvas::Node> {
        let mut nodes = self
            .editor
            .lock()
            .unwrap()
            .engine
            .get_head()
            .lines(..)
            .filter_map(|line| {
                if let [id, x, y, text] = line.split('\t').collect::<Vec<_>>()[..] {
                    Some(canvas::Node {
                        id: Id::try_from(id).unwrap(),
                        position: Point::new(x.parse().unwrap(), y.parse().unwrap()),
                        text: text.to_string(),
                    })
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        nodes.sort_unstable_by_key(|node| (node.position.y as i64, node.position.x as i64));
        nodes
    }
}

impl AppDelegate<canvas::Data> for App {
    fn command(
        &mut self,
        _ctx: &mut DelegateCtx,
        _target: &Target,
        cmd: &Command,
        data: &mut canvas::Data,
        _env: &Env,
    ) -> bool {
        let result = match cmd.selector {
            NODE_INSERT_TEXT => {
                let NodeInsertText { id, index, text } = cmd.get_object().unwrap();
                if let Some(delta) = {
                    let editor = self.editor.lock().unwrap();
                    let code = editor.engine.get_head();
                    let id_prefix = String::from(*id) + "\t";
                    code.lines(..)
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
                        .map(|offset| {
                            let mut delta = DeltaBuilder::new(code.len());
                            delta.replace(offset..offset, Rope::from(text));
                            delta.build()
                        })
                } {
                    self.edit(delta);
                    data.draft_nodes =
                        Arc::new(data.draft_nodes.iter().chain(Some(id)).copied().collect());
                    self.save();
                }
                false
            }
            NODE_DELETE_CHAR => {
                let NodeDeleteChar { id, index } = cmd.get_object().unwrap();
                if let Some(delta) = {
                    let editor = self.editor.lock().unwrap();
                    let code = editor.engine.get_head();
                    let id_prefix = String::from(*id) + "\t";
                    code.lines(..)
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
                        .map(|(start, end)| {
                            let mut delta = DeltaBuilder::<RopeInfo>::new(code.len());
                            delta.delete(start..end);
                            delta.build()
                        })
                } {
                    self.edit(delta);
                    data.draft_nodes =
                        Arc::new(data.draft_nodes.iter().chain(Some(id)).copied().collect());
                    self.save();
                }
                false
            }
            CREATE_NODE => {
                let CreateNode { text, position } = cmd.get_object().unwrap();
                let id = Id::random();
                let delta = {
                    let editor = self.editor.lock().unwrap();
                    let code = editor.engine.get_head();
                    let offset = code.len();
                    let mut delta = DeltaBuilder::new(code.len());
                    delta.replace(
                        offset..offset,
                        Rope::from(format!(
                            "{}\t{}\t{}\t{}\n",
                            String::from(id),
                            position.x,
                            position.y,
                            text
                        )),
                    );
                    delta.build()
                };
                self.edit(delta);
                data.draft_nodes =
                    Arc::new(data.draft_nodes.iter().copied().chain(Some(id)).collect());
                self.save();
                false
            }
            NEW_UNDO_GROUP => {
                self.editor.lock().unwrap().undo_group += 1;
                false
            }
            COMMIT_PROGRAM => {
                let ops = data
                    .nodes
                    .iter()
                    .map(|node| TextOp {
                        id: u64::from(node.id),
                        op: node.text.to_owned(),
                    })
                    .collect();
                self.audio_tx
                    .send(audio_server::Message::LoadProgram(ops))
                    .ok();
                data.draft_nodes = Arc::new(Vec::new());
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
        // Regenerate nodes if necessary.
        let base_rev = self.editor.lock().unwrap().engine.get_head_rev_id().token();
        if self.last_rendered_rev != base_rev {
            self.last_rendered_rev = base_rev;
            data.nodes = Arc::new(self.generate_nodes());
        }
        result
    }
}
