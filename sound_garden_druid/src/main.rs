use crate::{commands::*, repository::NodeRepository, types::*};
use anyhow::Result;
use audio_program::TextOp;
use canvas::Cursor;
use chrono::Local;
use clap::{app_from_crate, crate_authors, crate_description, crate_name, crate_version, Arg};
use crdt_engine::Patch;
use crossbeam_channel::{Receiver, Sender};
use druid::{
    widget::Flex, AppDelegate, AppLauncher, Command, DelegateCtx, Env, Lens, Point, Target, Vec2,
    Widget, WidgetExt, WindowDesc,
};
use repository::NodeEdit;
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
};
use thread_worker::Worker;

mod canvas;
mod commands;
mod modeline;
mod repository;
mod theme;
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
    /// Channel to communicate to peer.
    peer_tx: Option<Sender<Patch<MetaKey, MetaValue>>>,
    /// Edits in the same undo group are undone in one go.
    undo_group: u64,
    /// Last known node id to text map to detect draft nodes.
    last_known_node_texts: HashMap<Id, String>,
}

#[derive(Serialize, Deserialize)]
enum JamMessage {
    SyncNodes(Patch<MetaKey, MetaValue>),
}

#[derive(Clone, druid::Data, Default)]
struct Data {
    cursor: Cursor,
    /// Workspace is draft besides of edited nodes (usually deleted nodes).
    draft: bool,
    draft_nodes: Arc<Vec<Id>>,
    mode: Mode,
    nodes: Arc<Vec<Node>>,
    /// Did we ask audio server to play?
    play: bool,
    /// Did we ask audio server to record?
    record: bool,
}

fn main() -> Result<()> {
    simple_logger::init().unwrap();

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
    let launcher = AppLauncher::with_window(WindowDesc::new(build_ui).title("Sound Garden"));

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
            move |rx: Receiver<Patch<MetaKey, MetaValue>>, _: Sender<()>| {
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
        peer_tx: peer.as_ref().map(|x| x.sender().clone()),
        undo_group: 0,
        last_known_node_texts: Default::default(),
    };

    let mut data: Data = Default::default();
    data.nodes = Arc::new(node_repo.lock().unwrap().nodes());
    data.cursor = node_repo.lock().unwrap().get_cursor();

    launcher.delegate(app).launch(data)?;

    // FIXME Audio worker deadlock.
    std::thread::sleep(std::time::Duration::from_secs(1));
    std::process::exit(0);

    // Ok(())
}

impl App {
    fn save(&self) {
        if self.node_repo.lock().unwrap().save(&self.filename).is_err() {
            log::error!("Failed to save {}", &self.filename);
        }
    }

    fn edit(&mut self, edits: HashMap<Id, Vec<NodeEdit>>) {
        let patch = self
            .node_repo
            .lock()
            .unwrap()
            .edit_nodes(edits, self.undo_group);
        self.sync(patch);
        self.save();
    }

    fn set_cursor(&mut self, cursor: &Cursor) {
        let patch = self
            .node_repo
            .lock()
            .unwrap()
            .set_cursor(cursor, self.undo_group);
        self.sync(patch);
        self.save();
    }

    fn sync(&self, patch: Patch<MetaKey, MetaValue>) {
        if let Some(tx) = self.peer_tx.as_ref() {
            tx.send(patch).ok();
        }
    }
}

impl AppDelegate<Data> for App {
    fn command(
        &mut self,
        ctx: &mut DelegateCtx,
        _target: Target,
        cmd: &Command,
        data: &mut Data,
        _env: &Env,
    ) -> bool {
        let prev_cursor_position = data.cursor.position;
        let result = match cmd {
            _ if cmd.is(NODE_INSERT_TEXT) => {
                let text = cmd.get_unchecked(NODE_INSERT_TEXT);
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
                    .map(|_| {
                        let cursor = data.cursor.position;
                        let edits = data
                            .nodes
                            .iter()
                            .filter_map(|node| {
                                if node.position.y == cursor.y && node.position.x > cursor.x {
                                    Some((
                                        node.id,
                                        vec![NodeEdit::Move(node.position + Vec2::new(1.0, 0.0))],
                                    ))
                                } else {
                                    None
                                }
                            })
                            .collect::<HashMap<_, _>>();
                        self.edit(edits);
                    });
                data.cursor.position.x += text.chars().count() as f64;
                false
            }
            _ if cmd.is(NODE_DELETE_CHAR) => {
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
                                Some((
                                    node.id,
                                    vec![NodeEdit::Move(node.position - Vec2::new(1.0, 0.0))],
                                ))
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
                });
                false
            }
            _ if cmd.is(COMMIT_PROGRAM) => {
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
                self.last_known_node_texts = data
                    .nodes
                    .iter()
                    .map(|node| (node.id, node.text.to_owned()))
                    .collect();
                false
            }
            _ if cmd.is(PLAY_PAUSE) => {
                data.play = !data.play;
                self.audio_tx
                    .send(audio_server::Message::Play(data.play))
                    .ok();
                false
            }
            _ if cmd.is(TOGGLE_RECORD) => {
                data.record = !data.record;
                self.audio_tx
                    .send(audio_server::Message::Record(data.record))
                    .ok();
                false
            }
            _ if cmd.is(UNDO) => {
                if let Some(patch) = self.node_repo.lock().unwrap().undo() {
                    self.sync(patch);
                }
                self.save();
                false
            }
            _ if cmd.is(REDO) => {
                if let Some(patch) = self.node_repo.lock().unwrap().redo() {
                    self.sync(patch);
                }
                self.save();
                false
            }
            _ if cmd.is(SAVE) => {
                self.save();
                false
            }
            _ if cmd.is(SPLASH) => {
                data.mode = Mode::Insert;
                self.undo_group += 1;
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
                                    Some((
                                        node.id,
                                        vec![NodeEdit::Move(node.position + Vec2::new(1.0, 0.0))],
                                    ))
                                } else {
                                    None
                                }
                            })
                            .collect::<HashMap<_, _>>();
                        self.edit(edits);
                    }
                }
                false
            }
            _ if cmd.is(DELETE_NODE) => {
                if let Some((node, _)) = data.node_at_cursor() {
                    let patch = self
                        .node_repo
                        .lock()
                        .unwrap()
                        .delete_nodes(&[node.id], self.undo_group);
                    self.sync(patch);
                    self.save();
                }
                false
            }
            _ if cmd.is(DELETE_LINE) => {
                let cursor = data.cursor.position;
                let ids = data
                    .nodes
                    .iter()
                    .filter_map(|node| {
                        if node.position.y == cursor.y {
                            Some(node.id)
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>();
                let patch = self
                    .node_repo
                    .lock()
                    .unwrap()
                    .delete_nodes(&ids, self.undo_group);
                self.sync(patch);
                self.save();
                false
            }
            _ if cmd.is(CUT_NODE) => {
                data.mode = Mode::Insert;
                self.undo_group += 1;
                if let Some((Node { id, text, .. }, index)) = data.node_at_cursor() {
                    let mut edits = HashMap::new();
                    edits.insert(
                        id,
                        vec![NodeEdit::Edit {
                            start: index,
                            end: text.chars().count(),
                            text: String::new(),
                        }],
                    );
                    self.edit(edits);
                }
                false
            }
            _ if cmd.is(CYCLE_UP) => {
                if let Some((Node { id, text, .. }, index)) = data.node_at_cursor() {
                    if let Some(d) = text
                        .get(index..(index + 1))
                        .and_then(|c| c.parse::<u8>().ok())
                    {
                        let d = (d + 1) % 10;
                        let mut edits = HashMap::new();
                        edits.insert(
                            id,
                            vec![NodeEdit::Edit {
                                start: index,
                                end: index + 1,
                                text: d.to_string(),
                            }],
                        );
                        self.edit(edits);
                        ctx.submit_command(Command::from(COMMIT_PROGRAM), None);
                    } else {
                        for cycle in default_cycles() {
                            if let Some(ops) = cycle.windows(2).find(|ops| ops[0] == text) {
                                let mut edits = HashMap::new();
                                edits.insert(
                                    id,
                                    vec![NodeEdit::Edit {
                                        start: index,
                                        end: index + 1,
                                        text: ops[1].to_owned(),
                                    }],
                                );
                                self.edit(edits);
                                ctx.submit_command(Command::from(COMMIT_PROGRAM), None);
                                break;
                            }
                        }
                    }
                }
                false
            }
            _ if cmd.is(CYCLE_DOWN) => {
                if let Some((Node { id, text, .. }, index)) = data.node_at_cursor() {
                    if let Some(d) = text
                        .get(index..(index + 1))
                        .and_then(|c| c.parse::<u8>().ok())
                    {
                        let d = (d + 9) % 10;
                        let mut edits = HashMap::new();
                        edits.insert(
                            id,
                            vec![NodeEdit::Edit {
                                start: index,
                                end: index + 1,
                                text: d.to_string(),
                            }],
                        );
                        self.edit(edits);
                        ctx.submit_command(Command::from(COMMIT_PROGRAM), None);
                    } else {
                        for cycle in default_cycles() {
                            if let Some(ops) = cycle.windows(2).find(|ops| ops[1] == text) {
                                let mut edits = HashMap::new();
                                edits.insert(
                                    id,
                                    vec![NodeEdit::Edit {
                                        start: index,
                                        end: index + 1,
                                        text: ops[0].to_owned(),
                                    }],
                                );
                                self.edit(edits);
                                ctx.submit_command(Command::from(COMMIT_PROGRAM), None);
                                break;
                            }
                        }
                    }
                }
                false
            }
            _ if cmd.is(MOVE_RIGHT_TO_LEFT) => {
                let cursor = data.cursor.position;
                let edits = data
                    .nodes
                    .iter()
                    .filter_map(|node| {
                        if node.position.y == cursor.y
                            && node.position.x + node.text.chars().count() as f64 > cursor.x
                        {
                            Some((
                                node.id,
                                vec![NodeEdit::Move(node.position - Vec2::new(1.0, 0.0))],
                            ))
                        } else {
                            None
                        }
                    })
                    .collect::<HashMap<_, _>>();
                self.edit(edits);
                data.cursor.position.x -= 1.0;
                false
            }
            _ if cmd.is(MOVE_RIGHT_TO_RIGHT) => {
                let cursor = data.cursor.position;
                let edits = data
                    .nodes
                    .iter()
                    .filter_map(|node| {
                        if node.position.y == cursor.y
                            && node.position.x + node.text.chars().count() as f64 > cursor.x
                        {
                            Some((
                                node.id,
                                vec![NodeEdit::Move(node.position + Vec2::new(1.0, 0.0))],
                            ))
                        } else {
                            None
                        }
                    })
                    .collect::<HashMap<_, _>>();
                self.edit(edits);
                data.cursor.position.x += 1.0;
                false
            }
            _ if cmd.is(MOVE_LEFT_TO_LEFT) => {
                let cursor = data.cursor.position;
                let edits = data
                    .nodes
                    .iter()
                    .filter_map(|node| {
                        if node.position.y == cursor.y && node.position.x <= cursor.x {
                            Some((
                                node.id,
                                vec![NodeEdit::Move(node.position - Vec2::new(1.0, 0.0))],
                            ))
                        } else {
                            None
                        }
                    })
                    .collect::<HashMap<_, _>>();
                self.edit(edits);
                data.cursor.position.x -= 1.0;
                false
            }
            _ if cmd.is(MOVE_LEFT_TO_RIGHT) => {
                let cursor = data.cursor.position;
                let edits = data
                    .nodes
                    .iter()
                    .filter_map(|node| {
                        if node.position.y == cursor.y && node.position.x <= cursor.x {
                            Some((
                                node.id,
                                vec![NodeEdit::Move(node.position + Vec2::new(1.0, 0.0))],
                            ))
                        } else {
                            None
                        }
                    })
                    .collect::<HashMap<_, _>>();
                self.edit(edits);
                data.cursor.position.x += 1.0;
                false
            }
            _ if cmd.is(MOVE_NODE_LEFT) => {
                if let Some((Node { id, position, .. }, _)) = data.node_at_cursor() {
                    let mut edits = HashMap::new();
                    edits.insert(id, vec![NodeEdit::Move(position - Vec2::new(1.0, 0.0))]);
                    self.edit(edits);
                }
                data.cursor.position.x -= 1.0;
                false
            }
            _ if cmd.is(MOVE_NODE_RIGHT) => {
                if let Some((Node { id, position, .. }, _)) = data.node_at_cursor() {
                    let mut edits = HashMap::new();
                    edits.insert(id, vec![NodeEdit::Move(position + Vec2::new(1.0, 0.0))]);
                    self.edit(edits);
                }
                data.cursor.position.x += 1.0;
                false
            }
            _ if cmd.is(MOVE_NODE_UP) => {
                if let Some((Node { id, position, .. }, _)) = data.node_at_cursor() {
                    let mut edits = HashMap::new();
                    edits.insert(id, vec![NodeEdit::Move(position - Vec2::new(0.0, 1.0))]);
                    self.edit(edits);
                }
                data.cursor.position.y -= 1.0;
                false
            }
            _ if cmd.is(MOVE_NODE_DOWN) => {
                if let Some((Node { id, position, .. }, _)) = data.node_at_cursor() {
                    let mut edits = HashMap::new();
                    edits.insert(id, vec![NodeEdit::Move(position + Vec2::new(0.0, 1.0))]);
                    self.edit(edits);
                }
                data.cursor.position.y += 1.0;
                false
            }
            _ if cmd.is(MOVE_LINE_UP) => {
                let cursor = data.cursor.position;
                let edits = data
                    .nodes
                    .iter()
                    .filter_map(|node| {
                        if node.position.y == cursor.y {
                            Some((
                                node.id,
                                vec![NodeEdit::Move(node.position - Vec2::new(0.0, 1.0))],
                            ))
                        } else {
                            None
                        }
                    })
                    .collect::<HashMap<_, _>>();
                self.edit(edits);
                data.cursor.position.y -= 1.0;
                false
            }
            _ if cmd.is(MOVE_LINE_DOWN) => {
                let cursor = data.cursor.position;
                let edits = data
                    .nodes
                    .iter()
                    .filter_map(|node| {
                        if node.position.y == cursor.y {
                            Some((
                                node.id,
                                vec![NodeEdit::Move(node.position + Vec2::new(0.0, 1.0))],
                            ))
                        } else {
                            None
                        }
                    })
                    .collect::<HashMap<_, _>>();
                self.edit(edits);
                data.cursor.position.y += 1.0;
                false
            }
            _ if cmd.is(MOVE_LEFT_UP) => {
                let cursor = data.cursor.position;
                let edits = data
                    .nodes
                    .iter()
                    .filter_map(|node| {
                        if node.position.y < cursor.y
                            || node.position.y == cursor.y && node.position.x <= cursor.x
                        {
                            Some((
                                node.id,
                                vec![NodeEdit::Move(node.position - Vec2::new(0.0, 1.0))],
                            ))
                        } else {
                            None
                        }
                    })
                    .collect::<HashMap<_, _>>();
                self.edit(edits);
                data.cursor.position.y -= 1.0;
                false
            }
            _ if cmd.is(MOVE_RIGHT_DOWN) => {
                let cursor = data.cursor.position;
                let edits = data
                    .nodes
                    .iter()
                    .filter_map(|node| {
                        if node.position.y > cursor.y
                            || node.position.y == cursor.y
                                && node.position.x + node.text.chars().count() as f64 > cursor.x
                        {
                            Some((
                                node.id,
                                vec![NodeEdit::Move(node.position + Vec2::new(0.0, 1.0))],
                            ))
                        } else {
                            None
                        }
                    })
                    .collect::<HashMap<_, _>>();
                self.edit(edits);
                data.cursor.position.y += 1.0;
                false
            }
            _ if cmd.is(INSERT_NEW_LINE_BELOW) => {
                data.mode = Mode::Insert;
                self.undo_group += 1;
                let cursor = data.cursor.position;
                let x = data
                    .nodes
                    .iter()
                    .fold(cursor.x, |acc, node| acc.min(node.position.x));
                let edits = data
                    .nodes
                    .iter()
                    .filter_map(|node| {
                        if node.position.y > cursor.y {
                            Some((
                                node.id,
                                vec![NodeEdit::Move(node.position + Vec2::new(0.0, 1.0))],
                            ))
                        } else {
                            None
                        }
                    })
                    .collect::<HashMap<_, _>>();
                self.edit(edits);
                data.cursor.position.x = x;
                data.cursor.position.y += 1.0;
                false
            }
            _ if cmd.is(INSERT_NEW_LINE_ABOVE) => {
                data.mode = Mode::Insert;
                self.undo_group += 1;
                let cursor = data.cursor.position;
                let x = data
                    .nodes
                    .iter()
                    .fold(cursor.x, |acc, node| acc.min(node.position.x));
                let edits = data
                    .nodes
                    .iter()
                    .filter_map(|node| {
                        if node.position.y < cursor.y {
                            Some((
                                node.id,
                                vec![NodeEdit::Move(node.position + Vec2::new(0.0, -1.0))],
                            ))
                        } else {
                            None
                        }
                    })
                    .collect::<HashMap<_, _>>();
                self.edit(edits);
                data.cursor.position.x = x;
                data.cursor.position.y -= 1.0;
                false
            }
            _ if cmd.is(MOVE_CURSOR) => {
                let delta: Vec2 = *cmd.get_unchecked(MOVE_CURSOR);
                data.cursor.position += delta;
                false
            }
            _ if cmd.is(SET_CURSOR) => {
                let position: Point = *cmd.get_unchecked(SET_CURSOR);
                data.cursor.position = position;
                false
            }
            _ if cmd.is(INSERT_MODE) => {
                data.mode = Mode::Insert;
                self.undo_group += 1;
                false
            }
            _ if cmd.is(NORMAL_MODE) => {
                data.mode = Mode::Normal;
                self.undo_group += 1;
                false
            }
            _ if cmd.is(DEBUG) => {
                let repo = self.node_repo.lock().unwrap();
                log::debug!("\nText:\n\n{}\n\nMeta:\n\n{:?}", repo.text(), repo.meta());
                false
            }
            _ => {
                log::debug!("{:?} is not handled in delegate.", cmd);
                true
            }
        };
        if data.cursor.position != prev_cursor_position {
            self.set_cursor(&data.cursor);
        }
        // TODO Regenerate nodes only if necessary.
        data.nodes = Arc::new(self.node_repo.lock().unwrap().nodes());
        data.cursor = self.node_repo.lock().unwrap().get_cursor();
        let mut new_draft_nodes = Vec::new();
        for node in data.nodes.iter() {
            match self.last_known_node_texts.get(&node.id) {
                Some(text) => {
                    if *text != node.text {
                        new_draft_nodes.push(node.id);
                    }
                }
                None => {
                    new_draft_nodes.push(node.id);
                }
            }
        }
        data.draft = !(data
            .nodes
            .iter()
            .map(|node| node.id)
            .collect::<HashSet<_>>()
            .symmetric_difference(
                &self
                    .last_known_node_texts
                    .keys()
                    .copied()
                    .collect::<HashSet<_>>(),
            ))
        .collect::<Vec<_>>()
        .is_empty();
        data.draft_nodes = Arc::new(new_draft_nodes);
        result
    }
}

fn build_ui() -> impl Widget<Data> {
    Flex::column()
        .with_flex_child(canvas::Widget::default().lens(CanvasLens {}), 1.0)
        .with_flex_child(
            modeline::Widget::default()
                .fix_height(36.0)
                .lens(ModelineLens {}),
            0.0,
        )
}

fn default_cycles() -> Vec<Vec<String>> {
    // NOTE Always repeat the first element at the end.
    [
        vec!["s", "t", "w", "c", "s"],
        vec!["sine", "tri", "saw", "cosine", "sine"],
        vec!["sh", "ssh", "sh"],
        vec!["l", "h", "l"],
        vec!["lpf", "hpf", "lpf"],
        vec!["bqlpf", "bqhpf", "bqlpf"],
        vec!["clip", "wrap", "clip"],
        vec!["tline", "tquad", "tline"],
        vec!["m", "mh", "dm", "dmh", "m"],
    ]
    .iter()
    .map(|cycle| cycle.iter().map(|s| s.to_string()).collect())
    .collect()
}

struct CanvasLens {}

impl Lens<Data, canvas::Data> for CanvasLens {
    fn with<V, F: FnOnce(&canvas::Data) -> V>(&self, data: &Data, f: F) -> V {
        let Data {
            cursor,
            draft_nodes,
            mode,
            nodes,
            ..
        } = data;
        let data = canvas::Data {
            cursor: cursor.clone(),
            draft_nodes: Arc::clone(draft_nodes),
            mode: *mode,
            nodes: Arc::clone(nodes),
        };
        f(&data)
    }

    fn with_mut<V, F: FnOnce(&mut canvas::Data) -> V>(&self, data: &mut Data, f: F) -> V {
        // This lens is read-only, mutation is ignored. Please use commands instead.
        let Data {
            cursor,
            draft_nodes,
            mode,
            nodes,
            ..
        } = data;
        let mut data = canvas::Data {
            cursor: cursor.clone(),
            draft_nodes: Arc::clone(draft_nodes),
            mode: *mode,
            nodes: Arc::clone(nodes),
        };
        f(&mut data)
    }
}

struct ModelineLens {}

impl Lens<Data, modeline::Data> for ModelineLens {
    fn with<V, F: FnOnce(&modeline::Data) -> V>(&self, data: &Data, f: F) -> V {
        let Data {
            draft,
            draft_nodes,
            mode,
            play,
            record,
            ..
        } = data;
        let data = modeline::Data {
            draft: *draft || !draft_nodes.is_empty(),
            mode: *mode,
            op_at_cursor: data
                .node_at_cursor()
                .and_then(|(node, _)| node.text.split(":").next().map(|s| s.to_owned())),
            play: *play,
            record: *record,
        };
        f(&data)
    }

    fn with_mut<V, F: FnOnce(&mut modeline::Data) -> V>(&self, data: &mut Data, f: F) -> V {
        // This lens is read-only, mutation is ignored. Please use commands instead.
        let op_at_cursor = data
            .node_at_cursor()
            .and_then(|(node, _)| node.text.split(":").next().map(|s| s.to_owned()));
        let Data {
            draft,
            draft_nodes,
            mode,
            play,
            record,
            ..
        } = data;
        let mut data = modeline::Data {
            draft: *draft || !draft_nodes.is_empty(),
            mode: *mode,
            op_at_cursor,
            play: *play,
            record: *record,
        };
        f(&mut data)
    }
}

impl Data {
    pub fn node_at_cursor(&self) -> Option<(Node, usize)> {
        let cursor = self.cursor.position;
        self.nodes.iter().find_map(|node| {
            let len = node.text.chars().count() as isize;
            let index = (cursor.x - node.position.x) as isize;
            // index <= len instead of strict inequality as we treat trailing space as a part of node.
            if node.position.y == cursor.y && 0 <= index && index <= len {
                Some((node.clone(), index as _))
            } else {
                None
            }
        })
    }
}
