use anyhow::Result;
use audio_program::{TextOp, get_help};
use chrono::Local;
use clap::{Arg, Command, crate_authors, crate_description, crate_name, crate_version};
use crossbeam_channel::{Receiver, Sender};
use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, Vec2 as EVec2};
use log::LevelFilter;
use rkyv::{rancor::Error as RkyvError, to_bytes};
use sound_garden_format::{NodeEdit, NodeRepository};
use sound_garden_types::*;
use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex},
};
use thread_worker::Worker;

const FONT_SIZE: f32 = 14.0;
const MODELINE_FONT_SIZE: f32 = 12.0;
const OSCILLOSCOPE_FONT_SIZE: f32 = 12.0;
const GRID_WIDTH: f32 = 8.4;
const GRID_HEIGHT: f32 = 16.0;
const MODELINE_HEIGHT: f32 = 26.0;
const BACKGROUND_COLOR: Color32 = Color32::from_rgb(0xf3, 0xf0, 0xe8);
const FOREGROUND_COLOR: Color32 = Color32::from_rgb(0x22, 0x22, 0x20);
const NODE_DRAFT_COLOR: Color32 = Color32::from_rgb(0xff, 0x81, 0x2b);
const MODELINE_NORMAL_COLOR: Color32 = Color32::from_rgb(0xcc, 0xcc, 0xcc);
const MODELINE_INSERT_COLOR: Color32 = Color32::from_rgb(0x55, 0xae, 0x39);
const MODELINE_RECORD_COLOR: Color32 = Color32::from_rgb(0xdf, 0x00, 0x00);
const OSCILLOSCOPE_BACKGROUND_COLOR: Color32 = Color32::from_rgb(0x4c, 0x4c, 0x49);

fn main() -> Result<()> {
    simple_logger::SimpleLogger::new()
        .with_level(LevelFilter::Info)
        .with_module_level("egui", LevelFilter::Warn)
        .with_module_level("eframe", LevelFilter::Warn)
        .with_module_level("wgpu", LevelFilter::Warn)
        .with_module_level("winit", LevelFilter::Warn)
        .init()
        .unwrap();

    let matches = Command::new(crate_name!())
        .version(crate_version!())
        .author(crate_authors!())
        .about(crate_description!())
        .arg(Arg::new("FILENAME").index(1).help("Path to the tree"))
        .arg(
            Arg::new("audio-port")
                .short('p')
                .long("audio-port")
                .value_name("PORT")
                .help("Port to send programs to"),
        )
        .get_matches();

    let filename = matches
        .get_one::<String>("FILENAME")
        .cloned()
        .unwrap_or_else(|| format!("{}.sg", Local::now().to_rfc3339()));

    let node_repo = Arc::new(Mutex::new(NodeRepository::load(&filename)));

    let audio_control = if let Some(port) = matches.get_one::<String>("audio-port") {
        let address = format!("127.0.0.1:{}", port);
        Worker::spawn(
            "Audio",
            1,
            move |rx: Receiver<audio_server::Message>, _: Sender<audio_server::Monitor>| {
                for msg in rx {
                    if let Ok(mut stream) = std::net::TcpStream::connect(&address)
                        && let Ok(bytes) = to_bytes::<RkyvError>(&msg)
                    {
                        std::io::Write::write_all(&mut stream, &bytes).ok();
                    }
                }
            },
        )
    } else {
        Worker::spawn("Audio", 1, audio_server::run)
    };

    let app = SoundGardenApp::new(
        filename,
        node_repo,
        audio_control.sender().clone(),
        audio_control.receiver().clone(),
    );

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_title("Sound Garden"),
        ..Default::default()
    };

    eframe::run_native(
        "Sound Garden",
        options,
        Box::new(move |_cc| Ok(Box::new(app))),
    )
    .map_err(|e| anyhow::anyhow!(e.to_string()))
}

#[derive(Clone)]
struct UiState {
    cursor: Cursor,
    draft: bool,
    draft_nodes: Arc<Vec<Id>>,
    mode: Mode,
    nodes: Arc<Vec<Node>>,
    play: bool,
    record: bool,
    show_oscilloscope: bool,
    show_op_list: bool,
    show_pattern_highlights: bool,
    oscilloscope_zoom: i16,
}

impl Default for UiState {
    fn default() -> Self {
        Self {
            cursor: Cursor::default(),
            draft: false,
            draft_nodes: Arc::new(Vec::new()),
            mode: Mode::Normal,
            nodes: Arc::new(Vec::new()),
            play: false,
            record: false,
            show_oscilloscope: false,
            show_op_list: false,
            show_pattern_highlights: true,
            oscilloscope_zoom: 0,
        }
    }
}

struct SoundGardenApp {
    node_repo: Arc<Mutex<NodeRepository>>,
    filename: String,
    audio_tx: Sender<audio_server::Message>,
    monitor_rx: Receiver<audio_server::Monitor>,
    undo_group: u64,
    last_committed_program: Vec<(Id, String)>,
    state: UiState,
    dragging_node: Option<NodeDrag>,
    op_help: HashMap<String, String>,
    oscilloscope_values: VecDeque<f64>,
    oscilloscope_min: f64,
    oscilloscope_max: f64,
    monitor_stream_enabled: bool,
    pattern_monitors: HashMap<Id, PatternMonitor>,
}

#[derive(Clone, Copy)]
struct PatternMonitor {
    source_id: u64,
    clocked: bool,
    phase: f64,
    cycle_count: usize,
    last_time: Option<f64>,
}

#[derive(Clone, Copy)]
struct NodeDrag {
    id: Id,
    grab_offset: Vec2,
}

impl Drop for SoundGardenApp {
    fn drop(&mut self) {
        self.audio_tx.send(audio_server::Message::Quit).ok();
    }
}

impl SoundGardenApp {
    fn new(
        filename: String,
        node_repo: Arc<Mutex<NodeRepository>>,
        audio_tx: Sender<audio_server::Message>,
        monitor_rx: Receiver<audio_server::Monitor>,
    ) -> Self {
        let mut app = Self {
            node_repo,
            filename,
            audio_tx,
            monitor_rx,
            undo_group: 0,
            last_committed_program: Vec::new(),
            state: UiState::default(),
            dragging_node: None,
            op_help: get_help(),
            oscilloscope_values: VecDeque::new(),
            oscilloscope_min: -1.0,
            oscilloscope_max: 1.0,
            monitor_stream_enabled: false,
            pattern_monitors: HashMap::new(),
        };
        app.sync_from_repo();
        app.update_audio_monitor();
        app
    }

    fn save(&self) {
        if self.node_repo.lock().unwrap().save(&self.filename).is_err() {
            log::error!("Failed to save {}", &self.filename);
        }
    }

    fn edit(&mut self, edits: HashMap<Id, Vec<NodeEdit>>) {
        self.node_repo
            .lock()
            .unwrap()
            .edit_nodes(edits, self.undo_group);
        self.save();
    }

    fn set_cursor(&mut self) {
        self.node_repo
            .lock()
            .unwrap()
            .set_cursor(&self.state.cursor, self.undo_group);
        self.save();
    }

    fn sync_from_repo(&mut self) {
        let repo = self.node_repo.lock().unwrap();
        self.state.nodes = Arc::new(repo.nodes());
        self.state.cursor = repo.get_cursor();
        drop(repo);

        let current_program = self.current_program_signature();
        self.state.draft = current_program != self.last_committed_program;

        let last_committed_texts = self
            .last_committed_program
            .iter()
            .cloned()
            .collect::<HashMap<_, _>>();
        let mut new_draft_nodes = Vec::new();
        for (index, node) in self.state.nodes.iter().enumerate() {
            let text_changed = last_committed_texts
                .get(&node.id)
                .is_none_or(|text| *text != node.text);
            let sequence_changed = self
                .last_committed_program
                .get(index)
                .is_none_or(|(id, _)| *id != node.id);
            if text_changed || sequence_changed {
                new_draft_nodes.push(node.id);
            }
        }
        self.state.draft_nodes = Arc::new(new_draft_nodes);
    }

    fn current_program_signature(&self) -> Vec<(Id, String)> {
        self.state
            .nodes
            .iter()
            .map(|node| (node.id, node.text.to_owned()))
            .collect()
    }

    fn node_at_cursor(&self) -> Option<(Node, usize)> {
        let cursor = self.state.cursor.position;
        self.state.nodes.iter().find_map(|node| {
            let len = node.text.chars().count() as isize;
            let index = (cursor.x - node.position.x) as isize;
            if node.position.y == cursor.y && 0 <= index && index <= len {
                Some((node.clone(), index as usize))
            } else {
                None
            }
        })
    }

    fn node_at_position(&self, position: Point) -> Option<Node> {
        self.state.nodes.iter().find_map(|node| {
            let width = node.text.chars().count().max(1) as f64;
            let end = node.position.x + width;
            (node.position.y == position.y && node.position.x <= position.x && position.x < end)
                .then(|| node.clone())
        })
    }

    fn op_at_cursor(&self) -> Option<String> {
        self.node_at_cursor()
            .and_then(|(node, _)| node.text.split(':').next().map(|s| s.to_owned()))
    }

    fn reset_oscilloscope(&mut self) {
        self.oscilloscope_values.clear();
        self.oscilloscope_min = -1.0;
        self.oscilloscope_max = 1.0;
    }

    fn handle_action(&mut self, action: Action) {
        let prev_cursor_position = self.state.cursor.position;
        let prev_scope_node_id = self.node_at_cursor().map(|(node, _)| node.id);

        match action {
            Action::MoveCursor(delta) => self.state.cursor.position += delta,
            Action::SetCursor(position) => self.state.cursor.position = position,
            Action::InsertMode => {
                self.state.mode = Mode::Insert;
                self.undo_group += 1;
            }
            Action::AppendMode => {
                self.state.mode = Mode::Insert;
                self.undo_group += 1;
                self.state.cursor.position += Vec2::new(1.0, 0.0);
            }
            Action::Splash => self.splash(),
            Action::NormalMode => {
                self.state.mode = Mode::Normal;
                self.undo_group += 1;
            }
            Action::InsertText(text) => self.insert_text(&text),
            Action::PasteText(text) => self.paste_text(&text),
            Action::DeleteChar => self.delete_char(),
            Action::DeleteNode => {
                if let Some((node, _)) = self.node_at_cursor() {
                    self.node_repo
                        .lock()
                        .unwrap()
                        .delete_nodes(&[node.id], self.undo_group);
                    self.save();
                }
            }
            Action::DeleteLine => {
                let cursor = self.state.cursor.position;
                let ids = self
                    .state
                    .nodes
                    .iter()
                    .filter_map(|node| (node.position.y == cursor.y).then_some(node.id))
                    .collect::<Vec<_>>();
                self.node_repo
                    .lock()
                    .unwrap()
                    .delete_nodes(&ids, self.undo_group);
                self.save();
            }
            Action::CutNode => {
                self.state.mode = Mode::Insert;
                self.undo_group += 1;
                if let Some((Node { id, text, .. }, index)) = self.node_at_cursor() {
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
            }
            Action::CommitProgram => self.commit_program(),
            Action::PlayPause => {
                self.state.play = !self.state.play;
                self.audio_tx
                    .send(audio_server::Message::Play(self.state.play))
                    .ok();
            }
            Action::ToggleRecord => {
                self.state.record = !self.state.record;
                self.audio_tx
                    .send(audio_server::Message::Record(self.state.record))
                    .ok();
            }
            Action::Undo => {
                self.node_repo.lock().unwrap().undo();
                self.save();
            }
            Action::Redo => {
                self.node_repo.lock().unwrap().redo();
                self.save();
            }
            Action::Debug => {
                let repo = self.node_repo.lock().unwrap();
                log::debug!("\nText:\n\n{}\n\nMeta:\n\n{:?}", repo.text(), repo.meta());
            }
            Action::CycleUp => self.cycle(true),
            Action::CycleDown => self.cycle(false),
            Action::MoveNode(delta) => {
                if let Some((Node { id, position, text }, index)) = self.node_at_cursor()
                    && index < text.chars().count()
                {
                    let target = position + delta;
                    if !self.node_position_is_blocked(id, target, text.chars().count()) {
                        let mut edits = HashMap::new();
                        edits.insert(id, vec![NodeEdit::Move(target)]);
                        self.edit(edits);
                        self.state.cursor.position += delta;
                    }
                }
            }
            Action::MoveLine(delta) => {
                let cursor = self.state.cursor.position;
                let edits = self
                    .state
                    .nodes
                    .iter()
                    .filter_map(|node| {
                        (node.position.y == cursor.y)
                            .then_some((node.id, vec![NodeEdit::Move(node.position + delta)]))
                    })
                    .collect::<HashMap<_, _>>();
                self.edit(edits);
                self.state.cursor.position += delta;
            }
            Action::ToggleOscilloscope => {
                self.state.show_oscilloscope = !self.state.show_oscilloscope;
                if self.state.show_oscilloscope {
                    self.reset_oscilloscope();
                }
                self.update_monitor_stream();
            }
            Action::ResetOscilloscope => self.reset_oscilloscope(),
            Action::ToggleOpList => {
                self.state.show_op_list = !self.state.show_op_list;
            }
            Action::TogglePatternHighlights => {
                self.state.show_pattern_highlights = !self.state.show_pattern_highlights;
                self.update_audio_monitor();
            }
            Action::OscilloscopeZoomIn => self.state.oscilloscope_zoom += 1,
            Action::OscilloscopeZoomOut => self.state.oscilloscope_zoom -= 1,
            Action::MoveRightToLeft => self.move_nodes_on_cursor_line(-1.0, |node, cursor| {
                node.position.x + node.text.chars().count() as f64 > cursor.x
            }),
            Action::MoveRightToRight => self.move_nodes_on_cursor_line(1.0, |node, cursor| {
                node.position.x + node.text.chars().count() as f64 > cursor.x
            }),
            Action::MoveLeftToLeft => {
                self.move_nodes_on_cursor_line(-1.0, |node, cursor| node.position.x <= cursor.x)
            }
            Action::MoveLeftToRight => {
                self.move_nodes_on_cursor_line(1.0, |node, cursor| node.position.x <= cursor.x)
            }
            Action::MoveBelow(delta_y) => {
                self.move_nodes_vertical(delta_y, |node, cursor| node.position.y >= cursor.y)
            }
            Action::MoveAbove(delta_y) => {
                self.move_nodes_vertical(delta_y, |node, cursor| node.position.y <= cursor.y)
            }
            Action::InsertNewLineBelow => self.insert_new_line(true),
            Action::InsertNewLineAbove => self.insert_new_line(false),
            Action::SplitLine => self.split_line(),
        }

        if self.state.cursor.position != prev_cursor_position {
            self.set_cursor();
        }
        self.sync_from_repo();
        if self.state.show_oscilloscope
            && self.node_at_cursor().map(|(node, _)| node.id) != prev_scope_node_id
        {
            self.reset_oscilloscope();
        }
        self.update_audio_monitor();
    }

    fn update_audio_monitor(&mut self) {
        let cursor_node_id = self.node_at_cursor().map(|(node, _)| node.id);
        self.audio_tx
            .send(audio_server::Message::Monitor(
                cursor_node_id.map(u64::from).unwrap_or_default(),
            ))
            .ok();

        let old_monitors = std::mem::take(&mut self.pattern_monitors);
        self.pattern_monitors = if self.state.show_pattern_highlights {
            self.pattern_monitors(old_monitors)
        } else {
            HashMap::new()
        };
        self.audio_tx
            .send(audio_server::Message::PatternMonitors(
                self.pattern_monitors
                    .values()
                    .map(|monitor| monitor.source_id)
                    .collect(),
            ))
            .ok();
        self.update_monitor_stream();
    }

    fn pattern_monitors(
        &self,
        old_monitors: HashMap<Id, PatternMonitor>,
    ) -> HashMap<Id, PatternMonitor> {
        self.state
            .nodes
            .iter()
            .enumerate()
            .filter_map(|(index, node)| {
                let (clocked, _, _) = pattern_text(&node.text)?;
                let source_id = self
                    .state
                    .nodes
                    .get(index.checked_sub(1)?)
                    .map(|node| u64::from(node.id))?;
                let old = old_monitors.get(&node.id);
                Some((
                    node.id,
                    PatternMonitor {
                        source_id,
                        clocked,
                        phase: old
                            .filter(|monitor| {
                                monitor.source_id == source_id && monitor.clocked == clocked
                            })
                            .map(|monitor| monitor.phase)
                            .unwrap_or(0.0),
                        cycle_count: old
                            .filter(|monitor| {
                                monitor.source_id == source_id && monitor.clocked == clocked
                            })
                            .map(|monitor| monitor.cycle_count)
                            .unwrap_or(0),
                        last_time: None,
                    },
                ))
            })
            .collect()
    }

    fn update_monitor_stream(&mut self) {
        let enabled = self.state.show_oscilloscope || !self.pattern_monitors.is_empty();
        if enabled != self.monitor_stream_enabled {
            self.monitor_stream_enabled = enabled;
            self.audio_tx
                .send(audio_server::Message::Oscilloscope(enabled))
                .ok();
        }
    }

    fn paste_text(&mut self, text: &str) {
        let line_start_x = self.state.cursor.position.x;
        for ch in text.chars() {
            match ch {
                '\r' => {}
                '\n' => self.handle_action(Action::SetCursor(Point::new(
                    line_start_x,
                    self.state.cursor.position.y + 1.0,
                ))),
                '\t' | ' ' => self.handle_action(Action::MoveRightToRight),
                ch if !ch.is_control() => self.handle_action(Action::InsertText(ch.to_string())),
                _ => {}
            }
        }
    }

    fn insert_text(&mut self, text: &str) {
        if let Some((Node { id, .. }, index)) = self.node_at_cursor() {
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
        } else {
            let id = Id::random();
            self.node_repo.lock().unwrap().add_node(
                Node {
                    id,
                    position: self.state.cursor.position,
                    text: text.to_owned(),
                },
                self.undo_group,
            );
        }

        let cursor = self.state.cursor.position;
        let edits = self
            .state
            .nodes
            .iter()
            .filter_map(|node| {
                (node.position.y == cursor.y && node.position.x > cursor.x).then_some((
                    node.id,
                    vec![NodeEdit::Move(
                        node.position + Vec2::new(text.chars().count() as f64, 0.0),
                    )],
                ))
            })
            .collect::<HashMap<_, _>>();
        self.edit(edits);
        self.state.cursor.position.x += text.chars().count() as f64;
    }

    fn delete_char(&mut self) {
        if let Some((Node { id, text, .. }, index)) = self.node_at_cursor() {
            if index >= text.chars().count() {
                return;
            }
            let cursor = self.state.cursor.position;
            let mut edits = self
                .state
                .nodes
                .iter()
                .filter_map(|node| {
                    (node.position.y == cursor.y && node.position.x > cursor.x).then_some((
                        node.id,
                        vec![NodeEdit::Move(node.position - Vec2::new(1.0, 0.0))],
                    ))
                })
                .collect::<HashMap<_, _>>();
            edits.entry(id).or_default().push(NodeEdit::Edit {
                start: index,
                end: index + 1,
                text: String::new(),
            });
            self.edit(edits);
        }
    }

    fn splash(&mut self) {
        self.state.mode = Mode::Insert;
        self.undo_group += 1;
        if let Some((node, _)) = self.node_at_cursor() {
            let len = node.text.chars().count();
            self.state.cursor.position.x =
                if node.position.x == self.state.cursor.position.x && len > 1 {
                    node.position.x
                } else {
                    node.position.x + ((len + 1) as f64)
                };
            if self.node_at_cursor().is_some() {
                let cursor = self.state.cursor.position;
                let edits = self
                    .state
                    .nodes
                    .iter()
                    .filter_map(|node| {
                        (node.position.y == cursor.y && node.position.x >= cursor.x).then_some((
                            node.id,
                            vec![NodeEdit::Move(node.position + Vec2::new(1.0, 0.0))],
                        ))
                    })
                    .collect::<HashMap<_, _>>();
                self.edit(edits);
            }
        }
    }

    fn cycle(&mut self, up: bool) {
        if let Some((Node { id, text, .. }, index)) = self.node_at_cursor() {
            if let Some(d) = text
                .get(index..(index + 1))
                .and_then(|c| c.parse::<u8>().ok())
            {
                let d = if up { (d + 1) % 10 } else { (d + 9) % 10 };
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
                self.sync_from_repo();
                self.commit_program();
            } else {
                for cycle in default_cycles() {
                    let replacement = if up {
                        cycle
                            .windows(2)
                            .find(|ops| ops[0] == text)
                            .map(|ops| &ops[1])
                    } else {
                        cycle
                            .windows(2)
                            .find(|ops| ops[1] == text)
                            .map(|ops| &ops[0])
                    };
                    if let Some(replacement) = replacement {
                        let mut edits = HashMap::new();
                        edits.insert(
                            id,
                            vec![NodeEdit::Edit {
                                start: 0,
                                end: text.len(),
                                text: replacement.to_owned(),
                            }],
                        );
                        self.edit(edits);
                        self.sync_from_repo();
                        self.commit_program();
                        break;
                    }
                }
            }
        }
    }

    fn node_position_is_blocked(&self, moving_id: Id, position: Point, text_len: usize) -> bool {
        let width = text_len.max(1) as f64;
        let end = position.x + width;
        self.state.nodes.iter().any(|node| {
            if node.id == moving_id || node.position.y != position.y {
                return false;
            }
            let node_width = node.text.chars().count().max(1) as f64;
            let node_end = node.position.x + node_width;
            position.x <= node_end && node.position.x <= end
        })
    }

    fn move_nodes_on_cursor_line(&mut self, dx: f64, predicate: impl Fn(&Node, Point) -> bool) {
        let cursor = self.state.cursor.position;
        let edits = self
            .state
            .nodes
            .iter()
            .filter_map(|node| {
                (node.position.y == cursor.y && predicate(node, cursor)).then_some((
                    node.id,
                    vec![NodeEdit::Move(node.position + Vec2::new(dx, 0.0))],
                ))
            })
            .collect::<HashMap<_, _>>();
        self.edit(edits);
        self.state.cursor.position.x += dx;
    }

    fn move_nodes_vertical(&mut self, dy: f64, predicate: impl Fn(&Node, Point) -> bool) {
        let cursor = self.state.cursor.position;
        let edits = self
            .state
            .nodes
            .iter()
            .filter_map(|node| {
                predicate(node, cursor).then_some((
                    node.id,
                    vec![NodeEdit::Move(node.position + Vec2::new(0.0, dy))],
                ))
            })
            .collect::<HashMap<_, _>>();
        self.edit(edits);
        self.state.cursor.position.y += dy;
    }

    fn insert_new_line(&mut self, below: bool) {
        self.state.mode = Mode::Insert;
        self.undo_group += 1;
        let cursor = self.state.cursor.position;
        let x = self
            .state
            .nodes
            .iter()
            .fold(cursor.x, |acc, node| acc.min(node.position.x));
        let edits = self
            .state
            .nodes
            .iter()
            .filter_map(|node| {
                let should_move = if below {
                    node.position.y > cursor.y
                } else {
                    node.position.y < cursor.y
                };
                should_move.then_some((
                    node.id,
                    vec![NodeEdit::Move(
                        node.position + Vec2::new(0.0, if below { 1.0 } else { -1.0 }),
                    )],
                ))
            })
            .collect::<HashMap<_, _>>();
        self.edit(edits);
        self.state.cursor.position.x = x;
        self.state.cursor.position.y += if below { 1.0 } else { -1.0 };
    }

    fn split_line(&mut self) {
        self.undo_group += 1;
        let cursor = self.state.cursor.position;
        let split_node = self.node_at_cursor();
        let split_node_id = split_node.as_ref().map(|(node, _)| node.id);
        let mut new_node = None;

        let mut edits = self
            .state
            .nodes
            .iter()
            .filter_map(|node| {
                if node.position.y > cursor.y
                    || (node.position.y == cursor.y
                        && node.position.x >= cursor.x
                        && Some(node.id) != split_node_id)
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

        if let Some((node, index)) = split_node {
            let len = node.text.chars().count();
            if index == 0 {
                edits
                    .entry(node.id)
                    .or_default()
                    .push(NodeEdit::Move(node.position + Vec2::new(0.0, 1.0)));
            } else if index < len {
                let left = node.text.chars().take(index).collect::<String>();
                let right = node.text.chars().skip(index).collect::<String>();
                edits.entry(node.id).or_default().push(NodeEdit::Edit {
                    start: 0,
                    end: len,
                    text: left,
                });
                new_node = Some(Node {
                    id: Id::random(),
                    position: Point::new(cursor.x, cursor.y + 1.0),
                    text: right,
                });
            }
        }

        {
            let mut repo = self.node_repo.lock().unwrap();
            if !edits.is_empty() {
                repo.edit_nodes(edits, self.undo_group);
            }
            if let Some(node) = new_node {
                repo.add_node(node, self.undo_group);
            }
        }
        self.state.cursor.position.y += 1.0;
        self.set_cursor();
        self.save();
    }

    fn commit_program(&mut self) {
        let ops = self
            .state
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
        self.last_committed_program = self.current_program_signature();
        self.undo_group += 1;
    }

    fn collect_input(&mut self, ctx: &egui::Context) -> Vec<Action> {
        let mut actions = Vec::new();
        ctx.input(|input| {
            for event in &input.events {
                match event {
                    egui::Event::Paste(text) if !text.is_empty() => {
                        actions.push(Action::PasteText(text.clone()));
                    }
                    egui::Event::Text(text) if self.state.mode == Mode::Insert => {
                        if text == " " {
                            actions.push(Action::MoveRightToRight);
                        } else if !text.is_empty() {
                            actions.push(Action::InsertText(text.clone()));
                        }
                    }
                    egui::Event::Key {
                        key: egui::Key::Backspace,
                        pressed: true,
                        ..
                    } if self.state.mode == Mode::Insert => {
                        actions.push(Action::MoveCursor(Vec2::new(-1.0, 0.0)));
                        actions.push(Action::DeleteChar);
                    }
                    egui::Event::Key {
                        key,
                        pressed: true,
                        modifiers,
                        ..
                    } => {
                        if let Some(action) = key_action(*key, *modifiers, self.state.mode) {
                            actions.push(action);
                        }
                    }
                    _ => {}
                }
            }
        });

        actions
    }

    fn draw_canvas(&mut self, ui: &mut egui::Ui) {
        let content_size = self.canvas_content_size(ui.available_size());
        egui::ScrollArea::both().show(ui, |ui| {
            let (rect, response) = ui.allocate_exact_size(content_size, Sense::click_and_drag());
            let pointer_grid_position = response
                .interact_pointer_pos()
                .map(|pos| canvas_grid_position(rect, pos));

            if response.drag_started()
                && let Some(position) = pointer_grid_position
                && let Some(node) = self.node_at_position(position)
            {
                self.undo_group += 1;
                self.dragging_node = Some(NodeDrag {
                    id: node.id,
                    grab_offset: Vec2::new(
                        position.x - node.position.x,
                        position.y - node.position.y,
                    ),
                });
                self.handle_action(Action::SetCursor(position));
            }

            if response.dragged()
                && let (Some(drag), Some(position)) = (self.dragging_node, pointer_grid_position)
                && let Some(node) = self
                    .state
                    .nodes
                    .iter()
                    .find(|node| node.id == drag.id)
                    .cloned()
            {
                let target = position - drag.grab_offset;
                if node.position != target
                    && !self.node_position_is_blocked(drag.id, target, node.text.chars().count())
                {
                    let mut edits = HashMap::new();
                    edits.insert(drag.id, vec![NodeEdit::Move(target)]);
                    self.edit(edits);
                    self.state.cursor.position = position;
                    self.set_cursor();
                    self.sync_from_repo();
                }
            }

            if response.drag_stopped() {
                self.dragging_node = None;
            }

            if response.clicked()
                && let Some(position) = pointer_grid_position
            {
                self.handle_action(Action::SetCursor(position));
            }

            let painter = ui.painter_at(rect);
            painter.rect_filled(rect, 0.0, BACKGROUND_COLOR);
            self.paint_cursor(&painter, rect.min);

            for node in self.state.nodes.iter() {
                self.paint_pattern_highlight(&painter, rect.min, node);
                let color = if self.state.draft_nodes.contains(&node.id) {
                    NODE_DRAFT_COLOR
                } else {
                    FOREGROUND_COLOR
                };
                painter.text(
                    Pos2::new(
                        rect.min.x + node.position.x as f32 * GRID_WIDTH,
                        rect.min.y + node.position.y as f32 * GRID_HEIGHT,
                    ),
                    Align2::LEFT_TOP,
                    &node.text,
                    FontId::monospace(FONT_SIZE),
                    color,
                );
            }
        });
    }

    fn canvas_content_size(&self, available: EVec2) -> EVec2 {
        let (width, height) =
            self.state
                .nodes
                .iter()
                .fold((available.x, available.y), |(width, height), node| {
                    let x = (node.position.x as f32 + node.text.chars().count() as f32 + 1.0)
                        * GRID_WIDTH;
                    let y = (node.position.y as f32 + 2.0) * GRID_HEIGHT;
                    (width.max(x), height.max(y))
                });
        EVec2::new(width, height)
    }

    fn paint_pattern_highlight(&self, painter: &egui::Painter, origin: Pos2, node: &Node) {
        let Some(monitor) = self.pattern_monitors.get(&node.id) else {
            return;
        };
        let Some((_, dense, pattern)) = pattern_text(&node.text) else {
            return;
        };
        let Some((start, end)) = active_pattern_span(
            pattern,
            dense,
            monitor.phase.rem_euclid(1.0),
            monitor.cycle_count,
        ) else {
            return;
        };
        let pattern_start = node.text.chars().count() - pattern.chars().count();
        let x = origin.x + (node.position.x as f32 + (pattern_start + start) as f32) * GRID_WIDTH;
        let y = origin.y + node.position.y as f32 * GRID_HEIGHT;
        painter.rect_filled(
            Rect::from_min_size(
                Pos2::new(x, y),
                EVec2::new((end - start) as f32 * GRID_WIDTH, GRID_HEIGHT),
            ),
            0.0,
            Color32::from_rgba_unmultiplied(0x55, 0xae, 0x39, 96),
        );
    }

    fn paint_cursor(&self, painter: &egui::Painter, origin: Pos2) {
        let x = origin.x + self.state.cursor.position.x as f32 * GRID_WIDTH;
        let y = origin.y + self.state.cursor.position.y as f32 * GRID_HEIGHT;
        match self.state.mode {
            Mode::Normal => painter.rect_filled(
                Rect::from_min_size(Pos2::new(x, y), EVec2::new(GRID_WIDTH, GRID_HEIGHT)),
                0.0,
                Color32::from_rgba_unmultiplied(0x22, 0x22, 0x20, 84),
            ),
            Mode::Insert => painter.rect_filled(
                Rect::from_min_size(Pos2::new(x - 1.0, y), EVec2::new(2.0, GRID_HEIGHT)),
                0.0,
                Color32::from_rgba_unmultiplied(0x22, 0x22, 0x20, 168),
            ),
        };
    }

    fn draw_modeline(&mut self, ui: &mut egui::Ui) {
        let rect = ui.max_rect();
        let response = ui.allocate_rect(rect, Sense::click());
        if response.clicked_by(egui::PointerButton::Primary)
            && let Some(pos) = response.interact_pointer_pos()
            && Rect::from_min_max(
                Pos2::new(rect.min.x + 11.0, rect.min.y + 5.0),
                Pos2::new(rect.min.x + 31.0, rect.min.y + 23.0),
            )
            .contains(pos)
        {
            self.handle_action(Action::PlayPause);
        }
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, BACKGROUND_COLOR);

        let color = match self.state.mode {
            Mode::Normal if self.state.draft || !self.state.draft_nodes.is_empty() => {
                NODE_DRAFT_COLOR
            }
            Mode::Normal => MODELINE_NORMAL_COLOR,
            Mode::Insert => MODELINE_INSERT_COLOR,
        };
        painter.line_segment(
            [
                Pos2::new(rect.min.x, rect.min.y + 2.0),
                Pos2::new(rect.max.x, rect.min.y + 2.0),
            ],
            Stroke::new(4.0, color),
        );

        let transport_color =
            if self.state.record && ui.input(|input| (input.time as u64).is_multiple_of(2)) {
                ui.ctx()
                    .request_repaint_after(std::time::Duration::from_secs(1));
                MODELINE_RECORD_COLOR
            } else {
                FOREGROUND_COLOR
            };
        if self.state.play {
            painter.rect_filled(
                Rect::from_min_max(
                    Pos2::new(15.0, rect.min.y + 7.0),
                    Pos2::new(19.0, rect.min.y + 21.0),
                ),
                0.0,
                transport_color,
            );
            painter.rect_filled(
                Rect::from_min_max(
                    Pos2::new(23.0, rect.min.y + 7.0),
                    Pos2::new(27.0, rect.min.y + 21.0),
                ),
                0.0,
                transport_color,
            );
        } else {
            painter.add(egui::Shape::convex_polygon(
                vec![
                    Pos2::new(15.0, rect.min.y + 7.0),
                    Pos2::new(27.0, rect.min.y + 14.0),
                    Pos2::new(15.0, rect.min.y + 21.0),
                ],
                transport_color,
                Stroke::NONE,
            ));
        }

        if let Some(help) = self
            .op_at_cursor()
            .and_then(|op| self.op_help.get(&op).cloned())
        {
            painter.text(
                Pos2::new(35.0, rect.min.y + 5.0),
                Align2::LEFT_TOP,
                help,
                FontId::monospace(MODELINE_FONT_SIZE),
                FOREGROUND_COLOR,
            );
        }
    }

    fn draw_op_list(&mut self, ctx: &egui::Context) {
        let mut open = self.state.show_op_list;
        egui::Window::new("Sound Garden ops")
            .open(&mut open)
            .vscroll(true)
            .show(ctx, |ui| {
                let mut help = self.op_help.iter().collect::<Vec<_>>();
                help.sort_by_key(|(a, _)| *a);
                for (op, description) in help {
                    ui.horizontal_wrapped(|ui| {
                        ui.monospace(op);
                        ui.label(description);
                    });
                }
            });
        self.state.show_op_list = open;
    }

    fn draw_oscilloscope(&mut self, ui: &mut egui::Ui) {
        ui.take_available_space();
        let rect = ui.max_rect();
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, OSCILLOSCOPE_BACKGROUND_COLOR);

        let zoom = self.state.oscilloscope_zoom + self.state.oscilloscope_zoom.signum();
        let max_len = rect.width() as usize * if zoom >= 0 { 1 } else { -zoom as usize };
        if self.oscilloscope_values.len() > max_len {
            self.oscilloscope_values
                .drain(..(self.oscilloscope_values.len() - max_len));
        }

        let min = self
            .oscilloscope_values
            .iter()
            .copied()
            .reduce(f64::min)
            .unwrap_or(self.oscilloscope_min);
        let max = self
            .oscilloscope_values
            .iter()
            .copied()
            .reduce(f64::max)
            .unwrap_or(self.oscilloscope_max);
        let (min, max) = if min == max {
            (min - 1.0, max + 1.0)
        } else {
            (min, max)
        };
        self.oscilloscope_min = 0.5 * (self.oscilloscope_min + min);
        self.oscilloscope_max = 0.5 * (self.oscilloscope_max + max);

        let screen_step = if zoom > 0 { zoom as usize } else { 1 };
        let values_step = if zoom < 0 { -zoom as usize } else { 1 };
        let values_width = values_step * rect.width() as usize / screen_step;
        let points = (0..rect.width() as usize)
            .step_by(screen_step)
            .zip(
                self.oscilloscope_values
                    .iter()
                    .rev()
                    .take(values_width)
                    .rev()
                    .step_by(values_step),
            )
            .map(|(x, &y)| {
                let y = remap(
                    y,
                    self.oscilloscope_max,
                    self.oscilloscope_min,
                    16.0,
                    rect.height() as f64 - 16.0,
                );
                Pos2::new(rect.min.x + x as f32, rect.min.y + y as f32)
            })
            .collect::<Vec<_>>();
        if points.len() > 1 {
            painter.add(egui::Shape::line(
                points,
                Stroke::new(0.75, BACKGROUND_COLOR),
            ));
        }

        painter.text(
            rect.min,
            Align2::LEFT_TOP,
            format!("{}", self.oscilloscope_max),
            FontId::monospace(OSCILLOSCOPE_FONT_SIZE),
            BACKGROUND_COLOR,
        );
        painter.text(
            Pos2::new(rect.min.x, rect.max.y - GRID_HEIGHT),
            Align2::LEFT_TOP,
            format!("{}", self.oscilloscope_min),
            FontId::monospace(OSCILLOSCOPE_FONT_SIZE),
            BACKGROUND_COLOR,
        );
    }
}

impl eframe::App for SoundGardenApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();

        let time = ctx.input(|input| input.time);
        let mut received_monitor_frame = false;
        while let Ok(monitor_frame) = self.monitor_rx.try_recv() {
            for (source_id, frame) in monitor_frame.patterns {
                for monitor in self
                    .pattern_monitors
                    .values_mut()
                    .filter(|monitor| monitor.source_id == source_id)
                {
                    let previous_phase = monitor.phase;
                    if monitor.clocked {
                        if let Some(last_time) = monitor.last_time {
                            let dt = (time - last_time).max(0.0);
                            monitor.phase = (monitor.phase + frame[0] * dt).rem_euclid(1.0);
                            if previous_phase > monitor.phase {
                                monitor.cycle_count = monitor.cycle_count.wrapping_add(1);
                            }
                        }
                        monitor.last_time = Some(time);
                    } else {
                        monitor.phase = frame[0].rem_euclid(1.0);
                        if previous_phase > monitor.phase {
                            monitor.cycle_count = monitor.cycle_count.wrapping_add(1);
                        }
                    }
                    received_monitor_frame = true;
                }
            }
            if self.state.show_oscilloscope {
                self.oscilloscope_values.push_back(monitor_frame.scope[0]);
                received_monitor_frame = true;
            }
        }
        if received_monitor_frame {
            ctx.request_repaint();
        }

        for action in self.collect_input(&ctx) {
            self.handle_action(action);
        }

        if self.state.show_op_list {
            self.draw_op_list(&ctx);
        }

        if self.state.show_oscilloscope || !self.pattern_monitors.is_empty() {
            ctx.request_repaint_after(std::time::Duration::from_millis(16));
        }

        if self.state.show_oscilloscope {
            egui::Panel::bottom("oscilloscope")
                .resizable(true)
                .default_size(140.0)
                .show_inside(ui, |ui| self.draw_oscilloscope(ui));
        }

        egui::Panel::bottom("modeline")
            .exact_size(MODELINE_HEIGHT)
            .show_inside(ui, |ui| self.draw_modeline(ui));

        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(BACKGROUND_COLOR))
            .show_inside(ui, |ui| self.draw_canvas(ui));
    }
}

#[derive(Clone)]
enum Action {
    MoveCursor(Vec2),
    SetCursor(Point),
    InsertMode,
    AppendMode,
    Splash,
    NormalMode,
    InsertText(String),
    PasteText(String),
    DeleteChar,
    DeleteNode,
    DeleteLine,
    CutNode,
    CommitProgram,
    PlayPause,
    ToggleRecord,
    Undo,
    Redo,
    Debug,
    CycleUp,
    CycleDown,
    MoveNode(Vec2),
    MoveLine(Vec2),
    MoveBelow(f64),
    MoveAbove(f64),
    InsertNewLineBelow,
    InsertNewLineAbove,
    ToggleOscilloscope,
    ResetOscilloscope,
    ToggleOpList,
    TogglePatternHighlights,
    OscilloscopeZoomIn,
    OscilloscopeZoomOut,
    MoveRightToLeft,
    MoveRightToRight,
    MoveLeftToLeft,
    MoveLeftToRight,
    SplitLine,
}

fn key_action(key: egui::Key, modifiers: egui::Modifiers, mode: Mode) -> Option<Action> {
    let alt = modifiers.alt;
    let shift = modifiers.shift;
    match mode {
        Mode::Normal => match key {
            egui::Key::H | egui::Key::ArrowLeft | egui::Key::Backspace if !alt && !shift => {
                Some(Action::MoveCursor(Vec2::new(-1.0, 0.0)))
            }
            egui::Key::J | egui::Key::ArrowDown if !alt && !shift => {
                Some(Action::MoveCursor(Vec2::new(0.0, 1.0)))
            }
            egui::Key::K | egui::Key::ArrowUp if !alt && !shift => {
                Some(Action::MoveCursor(Vec2::new(0.0, -1.0)))
            }
            egui::Key::L | egui::Key::ArrowRight | egui::Key::Space if !alt && !shift => {
                Some(Action::MoveCursor(Vec2::new(1.0, 0.0)))
            }
            egui::Key::J | egui::Key::ArrowDown if alt && shift => Some(Action::MoveAbove(1.0)),
            egui::Key::K | egui::Key::ArrowUp if alt && shift => Some(Action::MoveAbove(-1.0)),
            egui::Key::H | egui::Key::ArrowLeft if alt => {
                Some(Action::MoveNode(Vec2::new(-1.0, 0.0)))
            }
            egui::Key::J | egui::Key::ArrowDown if alt => {
                Some(Action::MoveNode(Vec2::new(0.0, 1.0)))
            }
            egui::Key::K | egui::Key::ArrowUp if alt => {
                Some(Action::MoveNode(Vec2::new(0.0, -1.0)))
            }
            egui::Key::L | egui::Key::ArrowRight if alt => {
                Some(Action::MoveNode(Vec2::new(1.0, 0.0)))
            }
            egui::Key::J | egui::Key::ArrowDown if shift => Some(Action::MoveBelow(1.0)),
            egui::Key::K | egui::Key::ArrowUp if shift => Some(Action::MoveBelow(-1.0)),
            egui::Key::H | egui::Key::ArrowLeft if shift => {
                Some(Action::MoveLine(Vec2::new(0.0, -1.0)))
            }
            egui::Key::L | egui::Key::ArrowRight if shift => {
                Some(Action::MoveLine(Vec2::new(0.0, 1.0)))
            }
            egui::Key::I if shift => Some(Action::Splash),
            egui::Key::I if !shift => Some(Action::InsertMode),
            egui::Key::A if !shift => Some(Action::AppendMode),
            egui::Key::C if !shift => Some(Action::CutNode),
            egui::Key::D if !shift => Some(Action::DeleteNode),
            egui::Key::D if shift => Some(Action::DeleteLine),
            egui::Key::Enter => Some(Action::CommitProgram),
            egui::Key::Backslash => Some(Action::PlayPause),
            egui::Key::R if !shift => Some(Action::ToggleRecord),
            egui::Key::U if !shift => Some(Action::Undo),
            egui::Key::U if shift => Some(Action::Redo),
            egui::Key::Equals if alt => Some(Action::OscilloscopeZoomIn),
            egui::Key::Minus if alt => Some(Action::OscilloscopeZoomOut),
            egui::Key::Equals if !alt => Some(Action::CycleUp),
            egui::Key::Minus if !alt => Some(Action::CycleDown),
            egui::Key::Comma if !shift => Some(Action::MoveRightToLeft),
            egui::Key::Period if !shift => Some(Action::MoveRightToRight),
            egui::Key::Period if shift => Some(Action::MoveLeftToLeft),
            egui::Key::Comma if shift => Some(Action::MoveLeftToRight),
            egui::Key::Backtick => Some(Action::Debug),
            egui::Key::Slash => Some(Action::ToggleOpList),
            egui::Key::O if shift => Some(Action::InsertNewLineAbove),
            egui::Key::O if !shift => Some(Action::InsertNewLineBelow),
            egui::Key::S if !shift => Some(Action::SplitLine),
            egui::Key::V if !shift => Some(Action::ToggleOscilloscope),
            egui::Key::V if shift => Some(Action::ResetOscilloscope),
            egui::Key::P if !shift => Some(Action::TogglePatternHighlights),
            _ => None,
        },
        Mode::Insert => match key {
            egui::Key::Escape | egui::Key::Enter => Some(Action::NormalMode),
            egui::Key::ArrowLeft => Some(Action::MoveCursor(Vec2::new(-1.0, 0.0))),
            egui::Key::ArrowDown => Some(Action::MoveCursor(Vec2::new(0.0, 1.0))),
            egui::Key::ArrowUp => Some(Action::MoveCursor(Vec2::new(0.0, -1.0))),
            egui::Key::ArrowRight => Some(Action::MoveCursor(Vec2::new(1.0, 0.0))),
            egui::Key::Backspace => Some(Action::MoveCursor(Vec2::new(-1.0, 0.0))),
            _ => None,
        },
    }
}

fn default_cycles() -> Vec<Vec<String>> {
    // NOTE Always repeat the first element at the end.
    [
        vec!["+", "*", "+"],
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

fn canvas_grid_position(rect: Rect, pos: Pos2) -> Point {
    let local = pos - rect.min;
    Point::new(
        (local.x / GRID_WIDTH - 0.5).round() as f64,
        (local.y / GRID_HEIGHT - 0.5).round() as f64,
    )
}

fn remap(x: f64, from_min: f64, from_max: f64, to_min: f64, to_max: f64) -> f64 {
    to_min + (x - from_min) * (to_max - to_min) / (from_max - from_min)
}

fn pattern_text(text: &str) -> Option<(bool, bool, &str)> {
    let (op, pattern) = text.split_once(':')?;
    match op {
        "pat" => Some((false, false, pattern)),
        "gate" | "trig" => Some((false, true, pattern)),
        "cpat" => Some((true, false, pattern)),
        "cgate" | "ctrig" => Some((true, true, pattern)),
        _ => None,
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum VisualPatternElement {
    Atom((usize, usize)),
    Group(Vec<VisualPatternElement>),
    Alternate(Vec<Vec<VisualPatternElement>>),
}

fn active_pattern_span(
    pattern: &str,
    dense: bool,
    phase: f64,
    cycle: usize,
) -> Option<(usize, usize)> {
    active_span(
        &mut VisualPatternParser::new(pattern, dense).parse(),
        phase,
        cycle,
    )
}

fn active_span(
    elements: &mut [VisualPatternElement],
    phase: f64,
    cycle: usize,
) -> Option<(usize, usize)> {
    let len = elements.len();
    if len == 0 {
        return None;
    }
    let position = phase.rem_euclid(1.0) * len as f64;
    let index = (position as usize).min(len - 1);
    let local_phase = position - index as f64;
    match &mut elements[index] {
        VisualPatternElement::Atom(span) => Some(*span),
        VisualPatternElement::Group(children) => active_span(children, local_phase, cycle),
        VisualPatternElement::Alternate(alternatives) => {
            let alternative_index = cycle % alternatives.len();
            let alternative = alternatives.get_mut(alternative_index)?;
            active_span(alternative, local_phase, cycle)
        }
    }
}

fn parse_euclidean_steps(args: &str) -> Option<usize> {
    let mut parts = args.split(',');
    let pulses = parts.next()?.trim().parse::<usize>().ok()?;
    let steps = parts.next()?.trim().parse::<usize>().ok()?;
    (steps > 0 && pulses <= steps).then_some(steps)
}

struct VisualPatternParser<'a> {
    pattern: &'a str,
    dense: bool,
    index: usize,
}

impl<'a> VisualPatternParser<'a> {
    fn new(pattern: &'a str, dense: bool) -> Self {
        Self {
            pattern,
            dense,
            index: 0,
        }
    }

    fn parse(mut self) -> Vec<VisualPatternElement> {
        self.sequence(&[])
    }

    fn sequence(&mut self, stops: &[char]) -> Vec<VisualPatternElement> {
        let mut elements = Vec::new();
        while let Some(ch) = self.peek() {
            if stops.contains(&ch) {
                break;
            }
            if ch == ',' || ch.is_whitespace() {
                self.bump();
                continue;
            }
            if let Some(element) = self.item() {
                elements.extend(element);
            } else {
                self.bump();
            }
        }
        elements
    }

    fn item(&mut self) -> Option<Vec<VisualPatternElement>> {
        let elements = match self.peek()? {
            '[' => vec![self.group('[', ']')?],
            '<' => vec![self.alternate()?],
            _ => self.atom()?,
        };
        let repeat = self.repeat();
        Some((0..repeat).flat_map(|_| elements.clone()).collect())
    }

    fn group(&mut self, open: char, close: char) -> Option<VisualPatternElement> {
        debug_assert_eq!(self.peek(), Some(open));
        self.bump();
        let children = self.sequence(&[close]);
        if self.peek() == Some(close) {
            self.bump();
        }
        Some(VisualPatternElement::Group(children))
    }

    fn alternate(&mut self) -> Option<VisualPatternElement> {
        debug_assert_eq!(self.peek(), Some('<'));
        self.bump();
        let mut alternatives = Vec::new();
        while self.peek().is_some() && self.peek() != Some('>') {
            alternatives.push(self.sequence(&[';', '>']));
            if self.peek() == Some(';') {
                self.bump();
            }
        }
        if self.peek() == Some('>') {
            self.bump();
        }
        (!alternatives.is_empty()).then_some(VisualPatternElement::Alternate(alternatives))
    }

    fn atom(&mut self) -> Option<Vec<VisualPatternElement>> {
        let start = self.index;
        let steps = if self.dense && matches!(self.peek()?, 'x' | 'X' | 'e' | '.') {
            let ch = self.bump()?;
            if matches!(ch, 'x' | 'X' | 'e') {
                self.euclidean_suffix_steps()
            } else {
                None
            }
        } else {
            while let Some(ch) = self.peek() {
                if ch == ','
                    || ch == ']'
                    || ch == '>'
                    || ch == ';'
                    || ch == '*'
                    || ch == '|'
                    || ch.is_whitespace()
                    || matches!(ch, '[' | '<' | '(')
                {
                    break;
                }
                self.bump();
            }
            self.euclidean_suffix_steps()
        };
        if self.index <= start {
            return None;
        }
        let span = (start, self.index);
        Some(vec![VisualPatternElement::Atom(span); steps.unwrap_or(1)])
    }

    fn euclidean_suffix_steps(&mut self) -> Option<usize> {
        if self.peek() != Some('(') {
            return None;
        }
        let content_start = self.index + '('.len_utf8();
        let mut depth = 0usize;
        while let Some(ch) = self.peek() {
            self.bump();
            match ch {
                '(' => depth += 1,
                ')' => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        let content_end = self.index - ch.len_utf8();
                        return parse_euclidean_steps(&self.pattern[content_start..content_end]);
                    }
                }
                _ => {}
            }
        }
        None
    }

    fn repeat(&mut self) -> usize {
        if self.peek() != Some('*') {
            return 1;
        }
        self.bump();
        let start = self.index;
        while self.peek().is_some_and(|ch| ch.is_ascii_digit()) {
            self.bump();
        }
        self.pattern[start..self.index].parse().unwrap_or(1)
    }

    fn peek(&self) -> Option<char> {
        self.pattern[self.index..].chars().next()
    }

    fn bump(&mut self) -> Option<char> {
        let ch = self.peek()?;
        self.index += ch.len_utf8();
        Some(ch)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: u64, x: f64, y: f64, text: &str) -> Node {
        Node {
            id: Id::from(id),
            position: Point::new(x, y),
            text: text.to_owned(),
        }
    }

    fn app_with_nodes(nodes: Vec<Node>, cursor: Point) -> SoundGardenApp {
        let mut repo = NodeRepository::new();
        for node in nodes {
            repo.add_node(node, 0);
        }
        repo.set_cursor(&Cursor { position: cursor }, 0);

        let filename = std::env::temp_dir()
            .join(format!("sound-garden-egui-test-{:?}.sg", Id::random()))
            .to_string_lossy()
            .into_owned();
        let (audio_tx, _audio_rx) = crossbeam_channel::unbounded();
        let (_monitor_tx, monitor_rx) = crossbeam_channel::unbounded();

        SoundGardenApp::new(filename, Arc::new(Mutex::new(repo)), audio_tx, monitor_rx)
    }

    fn position(app: &SoundGardenApp, id: u64) -> Point {
        app.state
            .nodes
            .iter()
            .find(|node| node.id == Id::from(id))
            .unwrap()
            .position
    }

    #[test]
    fn paste_text_splits_spaces_and_newlines_into_grid_positions() {
        let mut app = app_with_nodes(Vec::new(), Point::new(2.0, 3.0));

        app.handle_action(Action::PasteText("foo bar\nbaz".to_owned()));

        let nodes = app.state.nodes.iter().collect::<Vec<_>>();
        assert_eq!(nodes.len(), 3);
        assert_eq!(nodes[0].text, "foo");
        assert_eq!(nodes[0].position, Point::new(2.0, 3.0));
        assert_eq!(nodes[1].text, "bar");
        assert_eq!(nodes[1].position, Point::new(6.0, 3.0));
        assert_eq!(nodes[2].text, "baz");
        assert_eq!(nodes[2].position, Point::new(2.0, 4.0));
        assert_eq!(app.state.cursor.position, Point::new(5.0, 4.0));
    }

    #[test]
    fn split_line_moves_right_side_and_lines_below_down() {
        let mut app = app_with_nodes(
            vec![
                node(1, 0.0, 0.0, "abcdef"),
                node(2, 8.0, 0.0, "right"),
                node(3, 0.0, 1.0, "below"),
            ],
            Point::new(3.0, 0.0),
        );

        app.handle_action(Action::SplitLine);

        let nodes = app.state.nodes.iter().collect::<Vec<_>>();
        assert_eq!(nodes.len(), 4);
        assert_eq!(nodes[0].text, "abc");
        assert_eq!(nodes[0].position, Point::new(0.0, 0.0));
        assert!(
            nodes
                .iter()
                .any(|node| node.text == "def" && node.position == Point::new(3.0, 1.0))
        );
        assert_eq!(position(&app, 2), Point::new(8.0, 1.0));
        assert_eq!(position(&app, 3), Point::new(0.0, 2.0));
        assert_eq!(app.state.cursor.position, Point::new(3.0, 1.0));
    }

    #[test]
    fn normal_s_splits_line() {
        assert!(matches!(
            key_action(egui::Key::S, egui::Modifiers::NONE, Mode::Normal),
            Some(Action::SplitLine)
        ));
    }

    #[test]
    fn active_pattern_span_enters_alternations() {
        assert_eq!(
            active_pattern_span("60,<64;67>,72", false, 0.0, 0),
            Some((0, 2))
        );
        assert_eq!(
            active_pattern_span("60,<64;67>,72", false, 0.4, 0),
            Some((4, 6))
        );
        assert_eq!(
            active_pattern_span("60,<64;67>,72", false, 0.4, 1),
            Some((7, 9))
        );
        assert_eq!(
            active_pattern_span("60,<64;67,68>,72(3,8)", false, 0.8, 0),
            Some((14, 21))
        );
        assert_eq!(
            active_pattern_span("<60,64;67,72>", false, 0.25, 0),
            Some((1, 3))
        );
        assert_eq!(
            active_pattern_span("<60,64;67,72>", false, 0.25, 1),
            Some((7, 9))
        );
    }

    #[test]
    fn active_pattern_span_enters_nested_groups() {
        assert_eq!(
            active_pattern_span("x[<x.;.x>]x", true, 0.0, 0),
            Some((0, 1))
        );
        assert_eq!(
            active_pattern_span("x[<x.;.x>]x", true, 0.5, 0),
            Some((4, 5))
        );
        assert_eq!(
            active_pattern_span("x[<x.;.x>]x", true, 0.5, 1),
            Some((7, 8))
        );
        assert_eq!(active_pattern_span("x(3,8).", true, 0.25, 0), Some((0, 6)));
        assert_eq!(active_pattern_span("x(3,8).", true, 0.75, 0), Some((0, 6)));
        assert_eq!(active_pattern_span("x(3,8).", true, 0.95, 0), Some((6, 7)));
        assert_eq!(active_pattern_span("e(1,2).", true, 0.4, 0), Some((0, 6)));
        assert_eq!(
            active_pattern_span("60(3,8),72", false, 0.75, 0),
            Some((0, 7))
        );
        assert_eq!(
            active_pattern_span("60(3,8),72", false, 0.95, 0),
            Some((8, 10))
        );
    }

    #[test]
    fn move_node_moves_node_and_cursor_when_target_has_room() {
        let mut app = app_with_nodes(
            vec![node(1, 0.0, 0.0, "abc"), node(2, 5.0, 0.0, "x")],
            Point::new(1.0, 0.0),
        );

        app.handle_action(Action::MoveNode(Vec2::new(1.0, 0.0)));

        assert_eq!(position(&app, 1), Point::new(1.0, 0.0));
        assert_eq!(position(&app, 2), Point::new(5.0, 0.0));
        assert_eq!(app.state.cursor.position, Point::new(2.0, 0.0));
    }

    #[test]
    fn move_node_rejects_overlap_with_another_node() {
        let mut app = app_with_nodes(
            vec![node(1, 0.0, 0.0, "abc"), node(2, 3.0, 1.0, "x")],
            Point::new(1.0, 0.0),
        );

        app.handle_action(Action::MoveNode(Vec2::new(3.0, 1.0)));

        assert_eq!(position(&app, 1), Point::new(0.0, 0.0));
        assert_eq!(position(&app, 2), Point::new(3.0, 1.0));
        assert_eq!(app.state.cursor.position, Point::new(1.0, 0.0));
    }

    #[test]
    fn move_node_rejects_direct_adjacency_without_empty_cell() {
        let mut app = app_with_nodes(
            vec![node(1, 0.0, 0.0, "abc"), node(2, 4.0, 0.0, "x")],
            Point::new(1.0, 0.0),
        );

        app.handle_action(Action::MoveNode(Vec2::new(1.0, 0.0)));

        assert_eq!(position(&app, 1), Point::new(0.0, 0.0));
        assert_eq!(position(&app, 2), Point::new(4.0, 0.0));
        assert_eq!(app.state.cursor.position, Point::new(1.0, 0.0));
    }

    #[test]
    fn move_node_allows_one_empty_cell_between_nodes() {
        let mut app = app_with_nodes(
            vec![node(1, 0.0, 0.0, "abc"), node(2, 5.0, 0.0, "x")],
            Point::new(1.0, 0.0),
        );

        app.handle_action(Action::MoveNode(Vec2::new(1.0, 0.0)));

        assert_eq!(position(&app, 1), Point::new(1.0, 0.0));
        assert_eq!(position(&app, 2), Point::new(5.0, 0.0));
        assert_eq!(app.state.cursor.position, Point::new(2.0, 0.0));
    }

    #[test]
    fn move_node_does_not_select_node_when_cursor_is_after_text() {
        let mut app = app_with_nodes(vec![node(1, 0.0, 0.0, "abc")], Point::new(3.0, 0.0));

        app.handle_action(Action::MoveNode(Vec2::new(1.0, 0.0)));

        assert_eq!(position(&app, 1), Point::new(0.0, 0.0));
        assert_eq!(app.state.cursor.position, Point::new(3.0, 0.0));
    }

    #[test]
    fn move_node_can_move_away_from_existing_adjacency() {
        let mut app = app_with_nodes(
            vec![node(1, 0.0, 0.0, "abc"), node(2, 3.0, 0.0, "x")],
            Point::new(1.0, 0.0),
        );

        app.handle_action(Action::MoveNode(Vec2::new(-1.0, 0.0)));

        assert_eq!(position(&app, 1), Point::new(-1.0, 0.0));
        assert_eq!(position(&app, 2), Point::new(3.0, 0.0));
        assert_eq!(app.state.cursor.position, Point::new(0.0, 0.0));
    }

    #[test]
    fn moving_node_before_another_node_marks_program_as_draft() {
        let mut app = app_with_nodes(
            vec![node(1, 0.0, 0.0, "first"), node(2, -10.0, 1.0, "second")],
            Point::new(-10.0, 1.0),
        );
        app.commit_program();
        app.sync_from_repo();
        assert!(!app.state.draft);
        assert!(app.state.draft_nodes.is_empty());

        app.handle_action(Action::MoveNode(Vec2::new(0.0, -1.0)));

        assert!(app.state.draft);
        assert!(app.state.draft_nodes.contains(&Id::from(1)));
        assert!(app.state.draft_nodes.contains(&Id::from(2)));
    }

    #[test]
    fn moving_node_without_changing_program_order_does_not_mark_draft() {
        let mut app = app_with_nodes(
            vec![node(1, 0.0, 0.0, "first"), node(2, 10.0, 1.0, "second")],
            Point::new(10.0, 1.0),
        );
        app.commit_program();
        app.sync_from_repo();

        app.handle_action(Action::MoveNode(Vec2::new(0.0, -1.0)));

        assert!(!app.state.draft);
        assert!(app.state.draft_nodes.is_empty());
    }
}
