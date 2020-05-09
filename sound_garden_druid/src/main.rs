use crate::{commands::*, repository::NodeRepository, types::*};
use anyhow::Result;
use audio_program::TextOp;
use chrono::Local;
use clap::{app_from_crate, crate_authors, crate_description, crate_name, crate_version, Arg};
use crdt_engine::Patch;
use crossbeam_channel::{Receiver, Sender};
use druid::{AppDelegate, AppLauncher, Command, DelegateCtx, Env, Target, WindowDesc};
use repository::NodeEdit;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
use thread_worker::Worker;

mod canvas;
mod commands;
mod repository;
mod types;

/// Application business logic is associated with this structure,
/// we implement druid's AppDelegate for it.
struct App {
    /// Distributed state.
    node_repo: Arc<Mutex<NodeRepository>>,
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
    SyncNodes(Patch),
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
    let node_repo = Arc::new(Mutex::new(NodeRepository::load(&filename)));
    let launcher = AppLauncher::with_window(WindowDesc::new(canvas::Widget::default));

    // Jam mode.
    // Start a thread to listen to the peer updates.
    if let Some(jam_port) = matches.value_of("jam-local-port") {
        let socket = nng::Socket::new(nng::Protocol::Bus0)?;
        let url = format!("tcp://:{}", jam_port);
        socket.listen(&url)?;
        let event_sink = launcher.get_external_handle();
        let node_repo = Arc::clone(&node_repo);
        std::thread::spawn(move || {
            while let Ok(msg) = socket.recv() {
                if let Ok(msg) = serde_cbor::from_reader::<JamMessage, _>(
                    snap::read::FrameDecoder::new(&msg[..]),
                ) {
                    let mut node_repo = node_repo.lock().unwrap();
                    match msg {
                        JamMessage::SyncNodes(patch) => {
                            node_repo.apply(patch);
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
                    serde_cbor::to_writer(stream, &JamMessage::SyncNodes(patch))
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
        node_repo: Arc::clone(&node_repo),
        filename: String::from(filename),
        audio_tx: audio_control.sender().clone(),
        play: false, // REVIEW it assumes server starts in pause mode
        record: false,
        peer_tx: peer.as_ref().map(|x| x.sender().clone()),
        undo_group: 0,
    };

    let mut data: canvas::Data = Default::default();
    data.nodes = Arc::new(node_repo.lock().unwrap().nodes());

    launcher.delegate(app).launch(data)?;

    drop(peer);

    Ok(())
}

impl App {
    fn save(&self) {
        self.node_repo.lock().unwrap().save(&self.filename);
    }

    fn edit(&mut self, edits: HashMap<Id, Vec<NodeEdit>>) {
        let patch = self
            .node_repo
            .lock()
            .unwrap()
            .edit_nodes(edits, self.undo_group);
        self.sync(patch);
    }

    fn sync(&self, patch: Patch) {
        if let Some(tx) = self.peer_tx.as_ref() {
            tx.send(patch).ok();
        }
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
                let NodeInsertText { text } = cmd.get_object().unwrap();
                data.node_at_cursor()
                    .map(|(Node { id, .. }, index)| {
                        let mut edits = HashMap::new();
                        edits.insert(
                            id,
                            vec![NodeEdit::Edit {
                                start: index,
                                end: index,
                                text: text.to_owned(),
                            }],
                        );
                        self.edit(edits);
                        id
                    })
                    .or_else(|| {
                        let id = Id::random();
                        let patch = self.node_repo.lock().unwrap().add_node(
                            Node {
                                id,
                                position: data.cursor.position,
                                text: text.to_owned(),
                            },
                            self.undo_group,
                        );
                        self.sync(patch);
                        Some(id)
                    })
                    .map(|id| {
                        let cursor = data.cursor.position;
                        let edits = data
                            .nodes
                            .iter()
                            .filter_map(|node| {
                                if node.position.y == cursor.y && node.position.x > cursor.x {
                                    Some((node.id, vec![NodeEdit::MoveX(node.position.x + 1.0)]))
                                } else {
                                    None
                                }
                            })
                            .collect::<HashMap<_, _>>();
                        self.edit(edits);
                        data.draft_nodes =
                            Arc::new(data.draft_nodes.iter().chain(Some(&id)).copied().collect());
                        self.save();
                    });
                data.cursor.position.x += text.chars().count() as f64;
                false
            }
            NODE_DELETE_CHAR => {
                data.node_at_cursor().map(|(Node { id, text, .. }, index)| {
                    if index >= text.chars().count() {
                        return;
                    }

                    let cursor = data.cursor.position;
                    let mut edits = data
                        .nodes
                        .iter()
                        .filter_map(|node| {
                            if node.position.y == cursor.y && node.position.x > cursor.x {
                                Some((node.id, vec![NodeEdit::MoveX(node.position.x - 1.0)]))
                            } else {
                                None
                            }
                        })
                        .collect::<HashMap<_, _>>();

                    edits.entry(id).or_default().push(NodeEdit::Edit {
                        start: index,
                        end: index + 1,
                        text: String::new(),
                    });

                    self.edit(edits);
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
            // TODO cursor undo/redo tracking
            NEW_UNDO_GROUP => {
                self.undo_group += 1;
                false
            }
            UNDO => {
                if let Some(patch) = self.node_repo.lock().unwrap().undo() {
                    self.sync(patch);
                }
                self.save();
                false
            }
            REDO => {
                if let Some(patch) = self.node_repo.lock().unwrap().redo() {
                    self.sync(patch);
                }
                self.save();
                false
            }
            SAVE => {
                self.save();
                false
            }
            SPLASH => {
                if let Some((node, _)) = data.node_at_cursor() {
                    let len = node.text.chars().count();
                    data.cursor.position.x = if node.position.x == data.cursor.position.x && len > 1
                    {
                        node.position.x
                    } else {
                        node.position.x + ((len + 1) as f64)
                    };
                    if data.node_at_cursor().is_some() {
                        let cursor = data.cursor.position;

                        let edits = data
                            .nodes
                            .iter()
                            .filter_map(|node| {
                                if node.position.y == cursor.y && node.position.x >= cursor.x {
                                    Some((node.id, vec![NodeEdit::MoveX(node.position.x + 1.0)]))
                                } else {
                                    None
                                }
                            })
                            .collect::<HashMap<_, _>>();
                        self.edit(edits);
                    }
                }
                self.save();
                false
            }
            _ => true,
        };
        // TODO Regenerate nodes only if necessary.
        data.nodes = Arc::new(self.node_repo.lock().unwrap().nodes());
        result
    }
}
