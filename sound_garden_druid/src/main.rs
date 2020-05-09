use crate::{commands::*, types::Id};
use anyhow::Result;
use audio_program::TextOp;
use chrono::Local;
use clap::{app_from_crate, crate_authors, crate_description, crate_name, crate_version, Arg};
use crdt_engine::{Delta, Engine, Patch};
use crossbeam_channel::{Receiver, Sender};
use druid::{AppDelegate, AppLauncher, Command, DelegateCtx, Env, Point, Target, WindowDesc};
use serde::{Deserialize, Serialize};
use std::{
    convert::TryFrom,
    sync::{Arc, Mutex},
};
use thread_worker::Worker;

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
 * -- We have to build own engine for that.
 *    Based on Xi work but implemented for P2P from the ground up.
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
    engine: Arc<Mutex<Engine>>,
    /// Persistence location for the Tree.
    filename: String,
    /// Channel to control audio server.
    audio_tx: Sender<audio_server::Message>,
    /// Did we ask audio server to play?
    play: bool,
    /// Did we ask audio server to record?
    record: bool,
    /// Channel to communicate to peer.
    peer_tx: Option<Sender<Patch>>,
    /// Edits in the same undo group are undone in one go.
    undo_group: u64,
}

#[derive(Serialize, Deserialize)]
enum JamMessage {
    Hello(Engine),
    Sync(Patch),
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
    let engine = Arc::new(Mutex::new(load_engine(&filename)));
    let launcher = AppLauncher::with_window(WindowDesc::new(canvas::Widget::default));

    // Jam mode.
    // Start a thread to listen to the peer updates.
    if let Some(jam_port) = matches.value_of("jam-local-port") {
        let socket = nng::Socket::new(nng::Protocol::Bus0)?;
        let url = format!("tcp://:{}", jam_port);
        socket.listen(&url)?;
        let event_sink = launcher.get_external_handle();
        let engine = Arc::clone(&engine);
        std::thread::spawn(move || {
            while let Ok(msg) = socket.recv() {
                if let Ok(msg) = serde_cbor::from_reader::<JamMessage, _>(
                    snap::read::FrameDecoder::new(&msg[..]),
                ) {
                    let mut engine = engine.lock().unwrap();
                    match msg {
                        JamMessage::Hello(other) => {
                            engine.merge(other);
                        }
                        JamMessage::Sync(patch) => {
                            engine.apply(patch);
                        }
                    }
                    event_sink.submit_command(SAVE, (), None).ok();
                }
            }
        });
    }

    // Start a worker to send updates to the peer.
    let peer = matches.value_of("jam-remote-address").map(|address| {
        let socket = nng::Socket::new(nng::Protocol::Bus0).unwrap();
        let url = format!("tcp://{}", address);
        socket.dial_async(&url).unwrap();
        Worker::spawn(
            "Send to peer",
            1024,
            move |rx: Receiver<Patch>, _: Sender<()>| {
                for patch in rx {
                    let mut msg = nng::Message::new();
                    let stream = snap::write::FrameEncoder::new(&mut msg);
                    serde_cbor::to_writer(stream, &JamMessage::Sync(patch))
                        .ok()
                        .and_then(|_| socket.send(msg).ok())
                        .or_else(|| None);
                }
            },
        )
    });

    // Start a worker to send messages to the audio server.
    let audio_control = {
        if let Some(port) = matches.value_of("audio-port") {
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
        } else {
            Worker::spawn("Audio", 1, audio_server::run)
        }
    };

    // Finish building app and launch it.
    let app = App {
        engine: Arc::clone(&engine),
        filename: String::from(filename),
        audio_tx: audio_control.sender().clone(),
        play: false, // REVIEW it assumes server starts in pause mode
        record: false,
        peer_tx: peer.as_ref().map(|x| x.sender().clone()),
        undo_group: 0,
    };

    let mut data: canvas::Data = Default::default();
    data.nodes = Arc::new(app.generate_nodes());

    launcher.delegate(app).launch(data)?;

    drop(peer);

    Ok(())
}

impl App {
    // TODO Atomic write.
    fn save(&self) {
        std::fs::File::create(&self.filename).ok().and_then(|f| {
            serde_cbor::to_writer(
                snap::write::FrameEncoder::new(f),
                &self.engine.lock().unwrap().clone(),
            )
            .ok()
        });
    }

    fn edit(&mut self, deltas: &[Delta]) {
        let patch = self.engine.lock().unwrap().edit(deltas);
        self.sync(patch);
    }

    fn sync(&self, patch: Patch) {
        if let Some(tx) = self.peer_tx.as_ref() {
            tx.send(patch).ok();
        }
    }

    fn generate_nodes(&self) -> Vec<canvas::Node> {
        let mut nodes = self
            .engine
            .lock()
            .unwrap()
            .text()
            .lines()
            .filter_map(|line| {
                if let [id, x, y, text] =
                    String::from(line).trim().split('\t').collect::<Vec<_>>()[..]
                {
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
                let NodeInsertText { cursor, text } = cmd.get_object().unwrap();
                data.node_under_cursor(*cursor)
                    .and_then(|(canvas::Node { id, .. }, index)| {
                        let id_prefix = String::from(id) + "\t";
                        let engine = self.engine.lock().unwrap();
                        let code = engine.text();
                        code.lines()
                            .map(String::from)
                            .enumerate()
                            .find(|(_, record)| record.starts_with(&id_prefix))
                            .map(|(line, record)| {
                                let line_offset = code.line_to_char(line);
                                let text_field_offset = record
                                    .chars()
                                    .enumerate()
                                    .filter_map(|(i, c)| if c == '\t' { Some(i) } else { None })
                                    .last()
                                    .unwrap()
                                    + 1;
                                let offset = line_offset + text_field_offset + index;
                                (
                                    id,
                                    Delta {
                                        range: (offset, offset),
                                        new_text: text.to_owned(),
                                        color: self.undo_group,
                                    },
                                )
                            })
                    })
                    .or_else(|| {
                        let id = Id::random();
                        let engine = self.engine.lock().unwrap();
                        let offset = engine.text().len_chars();
                        Some((
                            id,
                            Delta {
                                range: (offset, offset),
                                new_text: format!(
                                    "{}\t{}\t{}\t{}\n",
                                    String::from(id),
                                    cursor.x,
                                    cursor.y,
                                    text
                                ),
                                color: self.undo_group,
                            },
                        ))
                    })
                    .map(|(id, delta)| {
                        self.edit(&[delta]);
                        data.draft_nodes =
                            Arc::new(data.draft_nodes.iter().chain(Some(&id)).copied().collect());
                        self.save();
                    });
                false
            }
            NODE_DELETE_CHAR => {
                let NodeDeleteChar { cursor } = cmd.get_object().unwrap();
                data.node_under_cursor(*cursor)
                    .and_then(|(canvas::Node { id, .. }, index)| {
                        let engine = self.engine.lock().unwrap();
                        let code = engine.text();
                        let id_prefix = String::from(id) + "\t";
                        code.lines()
                            .map(String::from)
                            .enumerate()
                            .find(|(_, record)| record.starts_with(&id_prefix))
                            .map(|(line, record)| {
                                let line_offset = code.line_to_char(line);
                                let text_field_offset = record
                                    .chars()
                                    .enumerate()
                                    .filter_map(|(i, c)| if c == '\t' { Some(i) } else { None })
                                    .last()
                                    .unwrap()
                                    + 1;
                                line_offset + text_field_offset + index
                            })
                            .map(|offset| {
                                (
                                    id,
                                    Delta {
                                        range: (offset, offset + 1),
                                        new_text: String::new(),
                                        color: self.undo_group,
                                    },
                                )
                            })
                    })
                    .map(|(id, delta)| {
                        self.edit(&[delta]);
                        data.draft_nodes =
                            Arc::new(data.draft_nodes.iter().chain(Some(&id)).copied().collect());
                        self.save();
                    });
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
            NEW_UNDO_GROUP => {
                self.undo_group += 1;
                false
            }
            UNDO => {
                if let Some(patch) = self.engine.lock().unwrap().undo() {
                    self.sync(patch);
                }
                self.save();
                false
            }
            REDO => {
                if let Some(patch) = self.engine.lock().unwrap().redo() {
                    self.sync(patch);
                }
                self.save();
                false
            }
            SAVE => {
                self.save();
                false
            }
            _ => true,
        };
        // TODO Regenerate nodes only if necessary.
        data.nodes = Arc::new(self.generate_nodes());
        result
    }
}

fn load_engine(filename: &str) -> Engine {
    std::fs::File::open(filename)
        .ok()
        .map(|f| snap::read::FrameDecoder::new(f))
        .and_then(|f| serde_cbor::from_reader::<Engine, _>(f).ok())
        .map(|mut engine| {
            engine.rebuild();
            engine
        })
        .unwrap_or_else(|| Engine::new())
}
