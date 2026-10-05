mod feedback;

use anyhow::Result;
use audio_program::{TextOp, get_help};
use audio_vm::PatternSpan;
#[cfg(not(target_arch = "wasm32"))]
use clap::{Arg, Command, crate_authors, crate_description, crate_name, crate_version};
use crossbeam_channel::{Receiver, Sender};
use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, Vec2 as EVec2};
use feedback::{MeterDisplay, NodeDiagnostics, Spectrum};
#[cfg(not(target_arch = "wasm32"))]
use log::LevelFilter;
#[cfg(not(target_arch = "wasm32"))]
use rkyv::{rancor::Error as RkyvError, to_bytes};
use sound_garden_format::{NodeEdit, NodeRepository};
use sound_garden_types::*;
#[cfg(not(target_arch = "wasm32"))]
use std::time::Instant;
use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex},
    time::Duration,
};
#[cfg(target_arch = "wasm32")]
type SaveDeadline = f64; // JavaScript milliseconds since the epoch
#[cfg(not(target_arch = "wasm32"))]
type SaveDeadline = Instant;
#[cfg(not(target_arch = "wasm32"))]
use thread_worker::Worker;
#[cfg(target_arch = "wasm32")]
mod browser;
#[cfg(target_arch = "wasm32")]
use browser as audio_server;

/// Upper bound on how long an edit stays unsaved.
const SAVE_DELAY: Duration = Duration::from_secs(1);
const FONT_SIZE: f32 = 14.0;
const MODELINE_FONT_SIZE: f32 = 12.0;
const OSCILLOSCOPE_FONT_SIZE: f32 = 12.0;
const GRID_WIDTH: f32 = 8.4;
const GRID_HEIGHT: f32 = 16.0;
const MODELINE_HEIGHT: f32 = 26.0;
const MODELINE_GAP: f32 = 12.0;
const METER_WIDTH: f32 = 80.0;
/// Meter bars turn warm above -6 dBFS.
const METER_HOT_AMPLITUDE: f64 = 0.5;
/// Captured samples kept for waveform, spectrum and readout (~0.7 s at 48 kHz).
const CAPTURE_FRAMES: usize = 32768;
/// The value readout shows the range over this much recent signal.
const READOUT_SECONDS: f64 = 0.25;
/// Below this width the oscilloscope panel shows the waveform only.
const SPECTRUM_MIN_PANEL_WIDTH: f32 = 480.0;
const SPECTRUM_SHARE: f32 = 0.35;
const WARNING_COLOR: Color32 = Color32::from_rgb(0xc8, 0x1e, 0x1e);
const METER_TRACK_COLOR: Color32 = Color32::from_rgb(0xe0, 0xdc, 0xd2);
const BACKGROUND_COLOR: Color32 = Color32::from_rgb(0xf3, 0xf0, 0xe8);
const FOREGROUND_COLOR: Color32 = Color32::from_rgb(0x22, 0x22, 0x20);
const COMMENT_COLOR: Color32 = Color32::from_rgb(0x8f, 0x8c, 0x84);
const NODE_DRAFT_COLOR: Color32 = Color32::from_rgb(0xff, 0x81, 0x2b);
const MODELINE_NORMAL_COLOR: Color32 = Color32::from_rgb(0xcc, 0xcc, 0xcc);
const MODELINE_INSERT_COLOR: Color32 = Color32::from_rgb(0x55, 0xae, 0x39);
const MODELINE_RECORD_COLOR: Color32 = Color32::from_rgb(0xdf, 0x00, 0x00);
const OSCILLOSCOPE_BACKGROUND_COLOR: Color32 = Color32::from_rgb(0x4c, 0x4c, 0x49);

#[cfg(not(target_arch = "wasm32"))]
pub fn native_main() -> Result<()> {
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
        .arg(
            Arg::new("midi")
                .long("midi")
                .value_name("DEVICE")
                .help("Connect a MIDI input for the embedded audio server: 'auto', device index, or case-insensitive name substring."),
        )
        .arg(
            Arg::new("audio-device")
                .long("audio-device")
                .value_name("DEVICE")
                .help("Audio output: index from --list-audio or case-insensitive name substring. Default: the system's."),
        )
        .arg(
            Arg::new("sample-rate")
                .long("sample-rate")
                .value_name("HZ")
                .value_parser(clap::value_parser!(u32).range(8000..=384000))
                .help("Sample rate in Hz, e.g. 96000; falls back to the device's own if unsupported. On macOS this sets the device's rate system-wide. Default: the device's own."),
        )
        .arg(
            Arg::new("buffer")
                .long("buffer")
                .value_name("FRAMES")
                .value_parser(clap::value_parser!(u32).range(16..=8192))
                .help("Audio buffer size in frames, e.g. 128 for low latency; clamped to what the device supports. Default: the device's own."),
        )
        .arg(
            Arg::new("list-audio")
                .long("list-audio")
                .action(clap::ArgAction::SetTrue)
                .help("List audio output devices with their supported sample rates and exit."),
        )
        .arg(
            Arg::new("list-midi")
                .long("list-midi")
                .action(clap::ArgAction::SetTrue)
                .help("List available MIDI input devices and exit."),
        )
        .get_matches();

    if matches.get_flag("list-audio") {
        for line in audio_server::list_audio_outputs()? {
            println!("{line}");
        }
        return Ok(());
    }

    if matches.get_flag("list-midi") {
        for line in audio_server::list_midi_inputs()? {
            println!("{line}");
        }
        return Ok(());
    }

    let filename = matches
        .get_one::<String>("FILENAME")
        .cloned()
        .unwrap_or_else(|| format!("{}.sg", audio_server::timestamp()));

    // Refuse to start on an unreadable project: opening it as empty would
    // overwrite it on the first save.
    let node_repo = Arc::new(Mutex::new(NodeRepository::load(&filename).map_err(
        |err| {
            anyhow::anyhow!(
                "{err}\nThe file was left untouched; move or repair it, or open another file."
            )
        },
    )?));

    let midi = matches
        .get_one::<String>("midi")
        .map(|selection| {
            if selection == "auto" {
                audio_server::MidiInputSelection::Auto
            } else {
                audio_server::MidiInputSelection::Match(selection.clone())
            }
        })
        .unwrap_or_default();

    let audio_device = matches.get_one::<String>("audio-device").cloned();
    let sample_rate = matches.get_one::<u32>("sample-rate").copied();
    let buffer_frames = matches.get_one::<u32>("buffer").copied();
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
        Worker::spawn("Audio", 1, move |rx, tx| {
            audio_server::run_with_options(
                rx,
                tx,
                audio_server::Options {
                    midi,
                    audio_device,
                    sample_rate,
                    buffer_frames,
                },
            );
        })
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
/// Start the browser editor in an existing focusable HTML canvas.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub async fn start_sound_garden_editor(canvas_id: String) -> Result<(), wasm_bindgen::JsValue> {
    use wasm_bindgen::JsCast;
    let canvas: web_sys::HtmlCanvasElement = web_sys::window()
        .and_then(|window| window.document())
        .and_then(|document| document.get_element_by_id(&canvas_id))
        .ok_or_else(|| wasm_bindgen::JsValue::from_str("Editor canvas not found"))?
        .dyn_into()?;
    let repo = Arc::new(Mutex::new(browser::initial_project()?));
    browser::set_repository(Arc::clone(&repo));
    let (audio_tx, command_rx) = crossbeam_channel::unbounded();
    let (monitor_tx, monitor_rx) = crossbeam_channel::unbounded();
    let app = SoundGardenApp::new(repo, audio_tx, monitor_rx);
    let runner = eframe::WebRunner::new();
    runner
        .start(
            canvas,
            eframe::WebOptions::default(),
            Box::new(move |_cc| {
                Ok(Box::new(BrowserEditor {
                    app,
                    command_rx,
                    monitor_tx,
                }))
            }),
        )
        .await?;
    // The runner installs browser event listeners and lives until the page unloads.
    std::mem::forget(runner);
    Ok(())
}

/// A JS-controlled project change resets the editor state on its next frame.
#[cfg(target_arch = "wasm32")]
struct BrowserEditor {
    app: SoundGardenApp,
    command_rx: Receiver<audio_server::Message>,
    monitor_tx: Sender<audio_server::Monitor>,
}

#[cfg(target_arch = "wasm32")]
impl eframe::App for BrowserEditor {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        if browser::take_replaced() {
            self.app.state = UiState::default();
            self.app.last_committed_program.clear();
            self.app.save_due = None;
            self.app.sync_from_repo();
            self.app.saved_nodes = self.app.node_repo.lock().unwrap().nodes();
            self.app.update_audio_monitor();
            self.app.commit_program();
        }
        browser::flush_commands(&self.command_rx);
        browser::poll_monitors(&self.monitor_tx);
        self.app.ui(ui, frame);
        browser::flush_commands(&self.command_rx);
        ui.ctx().request_repaint_after(Duration::from_millis(33));
    }
}

/// Decode a portable .sg project before replacing the active editor document.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn replace_sound_garden_project(bytes: &[u8]) -> Result<(), wasm_bindgen::JsValue> {
    browser::replace(bytes)
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn current_sound_garden_project() -> Result<Vec<u8>, wasm_bindgen::JsValue> {
    browser::project_bytes()
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn empty_sound_garden_project() -> Result<Vec<u8>, wasm_bindgen::JsValue> {
    NodeRepository::new()
        .to_bytes()
        .map_err(|error| wasm_bindgen::JsValue::from_str(&error.to_string()))
}

/// Lay out tokens from playground text in their original rows, assigning new IDs.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn project_from_text(text: String) -> Result<Vec<u8>, wasm_bindgen::JsValue> {
    let mut repo = NodeRepository::new();
    for (y, line) in text.lines().enumerate() {
        let mut x = 0usize;
        for token in line.split_whitespace() {
            repo.add_node(
                Node {
                    id: Id::random(),
                    position: Point::new(x as f64, y as f64),
                    text: token.to_owned(),
                },
                0,
            );
            x += token.chars().count() + 1;
        }
    }
    repo.to_bytes()
        .map_err(|error| wasm_bindgen::JsValue::from_str(&error.to_string()))
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
    #[cfg(not(target_arch = "wasm32"))]
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
    /// Sounding part of each committed pattern node's text, as the engine reports it.
    pattern_monitors: HashMap<Id, PatternSpan>,
    /// When the pending save should be written; None when nothing is unsaved.
    save_due: Option<SaveDeadline>,
    /// Latest node snapshot given to browser persistence (cursor moves alone do not fork demos).
    #[cfg(target_arch = "wasm32")]
    saved_nodes: Vec<Node>,
    meters: MeterDisplay,
    diagnostics: NodeDiagnostics,
    /// Every sample (left channel) of the node under the cursor, newest last.
    capture: VecDeque<f64>,
    spectrum: Spectrum,
}

#[derive(Clone, Copy)]
struct NodeDrag {
    id: Id,
    grab_offset: Vec2,
}

impl Drop for SoundGardenApp {
    fn drop(&mut self) {
        if self.save_due.is_some() {
            self.save_now();
        }
        self.audio_tx.send(audio_server::Message::Quit).ok();
    }
}

impl SoundGardenApp {
    fn new(
        #[cfg(not(target_arch = "wasm32"))] filename: String,
        node_repo: Arc<Mutex<NodeRepository>>,
        audio_tx: Sender<audio_server::Message>,
        monitor_rx: Receiver<audio_server::Monitor>,
    ) -> Self {
        let mut app = Self {
            node_repo,
            #[cfg(not(target_arch = "wasm32"))]
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
            save_due: None,
            #[cfg(target_arch = "wasm32")]
            saved_nodes: Vec::new(),
            meters: MeterDisplay::default(),
            diagnostics: NodeDiagnostics::default(),
            capture: VecDeque::with_capacity(CAPTURE_FRAMES),
            spectrum: Spectrum::default(),
        };
        app.sync_from_repo();
        #[cfg(target_arch = "wasm32")]
        {
            app.saved_nodes = app.node_repo.lock().unwrap().nodes();
        }
        app.update_audio_monitor();
        #[cfg(target_arch = "wasm32")]
        app.commit_program();
        app
    }

    /// Saving rewrites the whole document, so edits schedule a save rather than
    /// writing on every keystroke. The deadline is set by the first unsaved
    /// edit and not pushed back by later ones, so continuous typing still
    /// saves at least every SAVE_DELAY.
    fn request_save(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        self.save_due
            .get_or_insert_with(|| Instant::now() + SAVE_DELAY);
        #[cfg(target_arch = "wasm32")]
        self.save_due
            .get_or_insert_with(|| js_sys::Date::now() + SAVE_DELAY.as_millis() as f64);
    }

    fn save_if_due(&mut self, ctx: &egui::Context) {
        if let Some(due) = self.save_due {
            #[cfg(not(target_arch = "wasm32"))]
            let remaining = due.saturating_duration_since(Instant::now());
            #[cfg(target_arch = "wasm32")]
            let remaining =
                Duration::from_secs_f64(((due - js_sys::Date::now()) / 1000.0).max(0.0));
            if remaining.is_zero() {
                self.save_now();
            } else {
                ctx.request_repaint_after(remaining);
            }
        }
    }

    fn save_now(&mut self) {
        self.save_due = None;
        #[cfg(not(target_arch = "wasm32"))]
        if let Err(err) = self.node_repo.lock().unwrap().save(&self.filename) {
            log::error!("Failed to save {}: {err}", &self.filename);
        }
        #[cfg(target_arch = "wasm32")]
        {
            let repo = self.node_repo.lock().unwrap();
            let nodes = repo.nodes();
            let changed = nodes != self.saved_nodes;
            match browser::save(&repo, changed) {
                Ok(()) => self.saved_nodes = nodes,
                Err(err) => log::error!("Failed to save browser project: {err}"),
            }
        }
    }

    fn edit(&mut self, edits: HashMap<Id, Vec<NodeEdit>>) {
        self.node_repo
            .lock()
            .unwrap()
            .edit_nodes(edits, self.undo_group);
        self.request_save();
    }

    fn set_cursor(&mut self) {
        self.node_repo
            .lock()
            .unwrap()
            .set_cursor(&self.state.cursor, self.undo_group);
        self.request_save();
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

    fn current_line_text(&self) -> String {
        let y = self.state.cursor.position.y;
        render_nodes_text(self.state.nodes.iter().filter(|node| node.position.y == y))
    }

    fn program_text(&self) -> String {
        render_nodes_text(self.state.nodes.iter())
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
                    self.request_save();
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
                self.request_save();
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
            #[cfg(not(target_arch = "wasm32"))]
            Action::ToggleRecord => {
                self.state.record = !self.state.record;
                self.audio_tx
                    .send(audio_server::Message::Record(self.state.record))
                    .ok();
            }
            Action::Undo => {
                self.node_repo.lock().unwrap().undo();
                self.request_save();
            }
            Action::Redo => {
                self.node_repo.lock().unwrap().redo();
                self.request_save();
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
            Action::CopyCurrentLine | Action::CopyProgram => {}
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
        let mut pattern_monitor_ids = self
            .pattern_monitors
            .keys()
            .map(|&id| u64::from(id))
            .collect::<Vec<_>>();
        pattern_monitor_ids.sort_unstable();
        self.audio_tx
            .send(audio_server::Message::PatternMonitors(pattern_monitor_ids))
            .ok();
        self.update_monitor_stream();
    }

    /// Committed pattern nodes, keeping the spans already known. A node the
    /// engine doesn't run (commented out, or inside a quotation) stays unlit.
    fn pattern_monitors(&self, old_monitors: HashMap<Id, PatternSpan>) -> HashMap<Id, PatternSpan> {
        self.state
            .nodes
            .iter()
            .filter(|node| {
                !self.state.draft_nodes.contains(&node.id) && pattern_text(&node.text).is_some()
            })
            .map(|node| {
                let span = old_monitors.get(&node.id).copied().unwrap_or_default();
                (node.id, span)
            })
            .collect()
    }

    fn update_monitor_stream(&mut self) {
        // Always on: the modeline's engine status and meters need it.
        let enabled = true;
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
        self.request_save();
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
            let comment_node_ids = commented_node_ids(&self.state.nodes);

            for node in self.state.nodes.iter() {
                self.paint_pattern_highlight(&painter, rect.min, node);
                // A node the last compile warned about, unless it has been
                // edited since (then it is a draft and the warning is stale).
                let flagged = !self.state.draft_nodes.contains(&node.id)
                    && self.diagnostics.by_node.contains_key(&u64::from(node.id));
                let color = if comment_node_ids.contains(&node.id) {
                    COMMENT_COLOR
                } else if self.state.draft_nodes.contains(&node.id) {
                    NODE_DRAFT_COLOR
                } else if flagged {
                    WARNING_COLOR
                } else {
                    FOREGROUND_COLOR
                };
                let text_rect = painter.text(
                    Pos2::new(
                        rect.min.x + node.position.x as f32 * GRID_WIDTH,
                        rect.min.y + node.position.y as f32 * GRID_HEIGHT,
                    ),
                    Align2::LEFT_TOP,
                    &node.text,
                    FontId::monospace(FONT_SIZE),
                    color,
                );
                if flagged {
                    painter.line_segment(
                        [text_rect.left_bottom(), text_rect.right_bottom()],
                        Stroke::new(1.5_f32, WARNING_COLOR),
                    );
                }
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
        if self.state.draft_nodes.contains(&node.id) {
            return;
        }
        let Some(&span) = self.pattern_monitors.get(&node.id) else {
            return;
        };
        let Some((column, width)) = highlight_columns(&node.text, span) else {
            return;
        };
        let x = origin.x + (node.position.x as f32 + column as f32) * GRID_WIDTH;
        let y = origin.y + node.position.y as f32 * GRID_HEIGHT;
        painter.rect_filled(
            Rect::from_min_size(
                Pos2::new(x, y),
                EVec2::new(width as f32 * GRID_WIDTH, GRID_HEIGHT),
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
            Stroke::new(4.0_f32, color),
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

        let time = ui.input(|input| input.time);
        let status_left = self.draw_engine_status(&painter, rect, time);

        // A warning for the node under the cursor takes the help text's place.
        let warning = self
            .node_at_cursor()
            .filter(|(node, _)| !self.state.draft_nodes.contains(&node.id))
            .and_then(|(node, _)| self.diagnostics.by_node.get(&u64::from(node.id)))
            .map(|messages| messages.join(" · "));
        let (text, color) = match warning {
            Some(warning) => (Some(warning), WARNING_COLOR),
            None => (
                self.op_at_cursor()
                    .and_then(|op| self.op_help.get(&op).cloned()),
                FOREGROUND_COLOR,
            ),
        };
        let mut left = 35.0;
        if let Some(readout) = self.readout_text() {
            let galley = painter.layout_no_wrap(
                readout,
                FontId::monospace(MODELINE_FONT_SIZE),
                FOREGROUND_COLOR,
            );
            let width = galley.size().x;
            painter.galley(Pos2::new(left, rect.min.y + 5.0), galley, FOREGROUND_COLOR);
            left += width + MODELINE_GAP;
        }
        if let Some(help) = text {
            let mut job = egui::text::LayoutJob::simple_singleline(
                help,
                FontId::monospace(MODELINE_FONT_SIZE),
                color,
            );
            job.wrap = egui::text::TextWrapping::truncate_at_width(
                (status_left - left - MODELINE_GAP).max(0.0),
            );
            let galley = painter.layout_job(job);
            painter.galley(Pos2::new(left, rect.min.y + 5.0), galley, color);
        }
    }

    /// Value of the node under the cursor, e.g. ` 0.2500  -0.9800… 0.9900`,
    /// from the last READOUT_SECONDS of its capture. Always the same width, so
    /// the help text after it holds still while the numbers change.
    fn readout_text(&self) -> Option<String> {
        self.node_at_cursor()?;
        let sample_rate = self.meters.sample_rate()?;
        let frames = ((READOUT_SECONDS * sample_rate) as usize).min(self.capture.len());
        let recent = self
            .capture
            .range(self.capture.len() - frames..)
            .copied()
            .collect::<Vec<_>>();
        let (current, min, max) = feedback::readout(&recent)?;
        let range = if min == max {
            " ".repeat(2 * feedback::FIXED_WIDTH + 1)
        } else {
            format!(
                "{}…{}",
                feedback::format_fixed(min),
                feedback::format_fixed(max)
            )
        };
        Some(format!("{}  {range}", feedback::format_fixed(current)))
    }

    /// Right end of the modeline: engine status text, stereo level meters and
    /// the clip light. Returns the x where it starts, so other text can stop
    /// short of it.
    fn draw_engine_status(&self, painter: &egui::Painter, rect: Rect, time: f64) -> f32 {
        let mut right = rect.max.x - MODELINE_GAP;

        // Clip light: latched for a moment after any sample goes past ±1.
        let clip_center = Pos2::new(right - 4.0, rect.center().y + 1.0);
        if self.meters.clipping(time) {
            painter.circle_filled(clip_center, 4.0, MODELINE_RECORD_COLOR);
        } else {
            painter.circle_stroke(clip_center, 3.5, Stroke::new(1.0_f32, COMMENT_COLOR));
        }
        right -= 8.0 + MODELINE_GAP;

        // Level meters, one bar per channel: RMS filled, held peak as a tick.
        let left = right - METER_WIDTH;
        for (channel, (rms, peak)) in self.meters.levels().into_iter().enumerate() {
            let top = rect.min.y + 8.0 + channel as f32 * 7.0;
            let track = Rect::from_min_max(Pos2::new(left, top), Pos2::new(right, top + 4.0));
            painter.rect_filled(track, 0.0, METER_TRACK_COLOR);
            let hot = peak > feedback::meter_position(METER_HOT_AMPLITUDE);
            let fill =
                Rect::from_min_max(track.min, Pos2::new(left + rms * METER_WIDTH, track.max.y));
            painter.rect_filled(
                fill,
                0.0,
                if hot { NODE_DRAFT_COLOR } else { COMMENT_COLOR },
            );
            let x = left + peak * METER_WIDTH;
            painter.line_segment(
                [
                    Pos2::new(x, track.min.y - 1.0),
                    Pos2::new(x, track.max.y + 1.0),
                ],
                Stroke::new(
                    1.5_f32,
                    if hot {
                        NODE_DRAFT_COLOR
                    } else {
                        FOREGROUND_COLOR
                    },
                ),
            );
        }
        right = left - MODELINE_GAP;

        let status = match (self.meters.midi_status(), self.meters.status()) {
            (Some(midi), Some(engine)) => Some(format!("{midi} · {engine}")),
            (midi, engine) => midi.or(engine),
        };
        if let Some(status) = status {
            let color = if self.meters.dropout_warning(time) {
                MODELINE_RECORD_COLOR
            } else {
                COMMENT_COLOR
            };
            let galley =
                painter.layout_no_wrap(status, FontId::monospace(MODELINE_FONT_SIZE), color);
            right -= galley.size().x;
            painter.galley(Pos2::new(right, rect.min.y + 5.0), galley, color);
            right -= MODELINE_GAP;
        }

        if let Some(summary) = self.diagnostics.summary() {
            let galley = painter.layout_no_wrap(
                summary,
                FontId::monospace(MODELINE_FONT_SIZE),
                WARNING_COLOR,
            );
            right -= galley.size().x;
            painter.galley(Pos2::new(right, rect.min.y + 5.0), galley, WARNING_COLOR);
            right -= MODELINE_GAP;
        }
        right
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

    /// Periodic signals: a window of the per-sample capture starting on a
    /// rising crossing, so the waveform stands still. Returns false for slow
    /// or flat signals, which fall back to the rolling trend view.
    fn draw_triggered_waveform(&mut self, painter: &egui::Painter, rect: Rect) -> bool {
        // Zoom 0 is one sample per pixel; each step halves or doubles that.
        let samples_per_pixel = 2f32.powi(-i32::from(self.state.oscilloscope_zoom));
        let window = ((rect.width() * samples_per_pixel) as usize).clamp(16, CAPTURE_FRAMES / 2);
        let samples = self.capture.make_contiguous();
        let Some(start) = feedback::find_trigger(samples, window) else {
            return false;
        };
        let shown = &samples[start..start + window];
        let (min, max) = shown
            .iter()
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), &x| {
                (lo.min(x), hi.max(x))
            });
        let (min, max) = if max - min < 1e-9 {
            (min - 1.0, max + 1.0)
        } else {
            (min, max)
        };
        let points = shown
            .iter()
            .enumerate()
            .map(|(i, &y)| {
                let x = rect.min.x + i as f32 * rect.width() / window as f32;
                let y = remap(y, max, min, 16.0, rect.height() as f64 - 16.0);
                Pos2::new(x, rect.min.y + y as f32)
            })
            .collect::<Vec<_>>();
        painter.add(egui::Shape::line(
            points,
            Stroke::new(0.75_f32, BACKGROUND_COLOR),
        ));
        for (y, value) in [(rect.min.y, max), (rect.max.y - GRID_HEIGHT, min)] {
            painter.text(
                Pos2::new(rect.min.x, y),
                Align2::LEFT_TOP,
                feedback::format_value(value),
                FontId::monospace(OSCILLOSCOPE_FONT_SIZE),
                BACKGROUND_COLOR,
            );
        }
        true
    }

    /// Log-frequency magnitude spectrum of the node under the cursor, with a
    /// C-note grid.
    fn draw_spectrum(&mut self, painter: &egui::Painter, rect: Rect) {
        const LOW: f64 = 20.0;
        let Some(sample_rate) = self.meters.sample_rate() else {
            return;
        };
        let high = (sample_rate / 2.0).min(20_000.0);
        let grid = Color32::from_rgba_unmultiplied(0xf3, 0xf0, 0xe8, 40);
        for octave in 1..=9 {
            let note = 12 * (octave + 1);
            let frequency = 440.0 * 2f64.powf((note as f64 - 69.0) / 12.0);
            if !(LOW..high).contains(&frequency) {
                continue;
            }
            let x = rect.min.x + feedback::log_position(frequency, LOW, high) as f32 * rect.width();
            painter.line_segment(
                [Pos2::new(x, rect.min.y), Pos2::new(x, rect.max.y)],
                Stroke::new(1.0_f32, grid),
            );
            let label = painter.layout_no_wrap(
                format!("C{octave}"),
                FontId::monospace(OSCILLOSCOPE_FONT_SIZE),
                grid,
            );
            // Skip labels that would be cut off at the right edge.
            if x + 2.0 + label.size().x <= rect.max.x {
                painter.galley(Pos2::new(x + 2.0, rect.max.y - GRID_HEIGHT), label, grid);
            }
        }
        let samples = self.capture.make_contiguous();
        let Some(db) = self.spectrum.analyse(samples) else {
            return;
        };
        let columns = rect.width() as usize;
        let levels = feedback::spectrum_columns(&db, sample_rate, columns, LOW, high);
        let points = levels
            .iter()
            .enumerate()
            .map(|(x, &level)| {
                let y = remap(
                    level,
                    0.0,
                    feedback::SPECTRUM_FLOOR_DB,
                    8.0,
                    rect.height() as f64 - 8.0,
                );
                Pos2::new(rect.min.x + x as f32, rect.min.y + y as f32)
            })
            .collect::<Vec<_>>();
        painter.add(egui::Shape::line(
            points,
            Stroke::new(0.75_f32, BACKGROUND_COLOR),
        ));
    }

    fn draw_oscilloscope(&mut self, ui: &mut egui::Ui) {
        ui.take_available_space();
        let panel = ui.max_rect();
        ui.painter_at(panel)
            .rect_filled(panel, 0.0, OSCILLOSCOPE_BACKGROUND_COLOR);
        let rect = if panel.width() >= SPECTRUM_MIN_PANEL_WIDTH {
            let split = panel.max.x - panel.width() * SPECTRUM_SHARE;
            self.draw_spectrum(
                &ui.painter_at(panel),
                Rect::from_min_max(Pos2::new(split, panel.min.y), panel.max),
            );
            Rect::from_min_max(panel.min, Pos2::new(split, panel.max.y))
        } else {
            panel
        };
        let painter = ui.painter_at(rect);
        if self.draw_triggered_waveform(&painter, rect) {
            return;
        }

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
                Stroke::new(0.75_f32, BACKGROUND_COLOR),
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
            for (id, span) in monitor_frame.patterns {
                if let Some(monitor) = self.pattern_monitors.get_mut(&Id::from(id)) {
                    *monitor = span;
                    received_monitor_frame = true;
                }
            }
            if self.state.show_oscilloscope {
                self.oscilloscope_values.push_back(monitor_frame.scope[0]);
                received_monitor_frame = true;
            }
            self.capture
                .extend(monitor_frame.samples.iter().map(|frame| frame[0]));
            if self.capture.len() > CAPTURE_FRAMES {
                self.capture.drain(..self.capture.len() - CAPTURE_FRAMES);
            }
            self.meters.update(
                &monitor_frame.meters,
                monitor_frame.midi_device.as_ref(),
                time,
            );
            if monitor_frame.diagnostics.generation != self.diagnostics.generation {
                self.diagnostics = NodeDiagnostics::new(
                    monitor_frame.diagnostics.generation,
                    &monitor_frame.diagnostics.items,
                );
                ctx.request_repaint();
            }
        }
        if self.state.play || self.meters.is_animating(time) {
            // ~30 fps is plenty for meters; pattern highlights and the
            // oscilloscope request their own faster repaints below.
            ctx.request_repaint_after(std::time::Duration::from_millis(33));
        }
        if received_monitor_frame {
            ctx.request_repaint();
        }

        for action in self.collect_input(&ctx) {
            match action {
                Action::CopyCurrentLine => ctx.copy_text(self.current_line_text()),
                Action::CopyProgram => ctx.copy_text(self.program_text()),
                _ => self.handle_action(action),
            }
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
                .show(ui, |ui| self.draw_oscilloscope(ui));
        }

        egui::Panel::bottom("modeline")
            .exact_size(MODELINE_HEIGHT)
            .show(ui, |ui| self.draw_modeline(ui));

        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(BACKGROUND_COLOR))
            .show(ui, |ui| self.draw_canvas(ui));

        // Last, so edits made while drawing (e.g. node drags) are included.
        self.save_if_due(&ctx);
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
    #[cfg(not(target_arch = "wasm32"))]
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
    CopyCurrentLine,
    CopyProgram,
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
            #[cfg(not(target_arch = "wasm32"))]
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
            egui::Key::Y if !shift => Some(Action::CopyCurrentLine),
            egui::Key::Y if shift => Some(Action::CopyProgram),
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

/// Nodes shown as comments: `( … )` groups, which run until their parentheses balance,
/// and quotations followed by `drop` or `--`.
fn commented_node_ids(nodes: &[Node]) -> Vec<Id> {
    let mut sorted = nodes.iter().collect::<Vec<_>>();
    sorted.sort_unstable_by_key(|node| (node.position.y as i64, node.position.x as i64));

    let mut result = Vec::new();
    let mut paren_depth = 0isize;
    let mut quote_start = None;
    let mut quote_depth = 0usize;
    for (index, node) in sorted.iter().enumerate() {
        if paren_depth > 0 || node.text.starts_with('(') {
            let balance = node.text.chars().fold(0isize, |n, ch| match ch {
                '(' => n + 1,
                ')' => n - 1,
                _ => n,
            });
            paren_depth = (paren_depth + balance).max(0);
            result.push(node.id);
            continue;
        }

        if node.text.starts_with('[') {
            if quote_depth == 0 {
                quote_start = Some(index);
            }
            quote_depth += 1;
        }

        if quote_depth > 0 && node.text.ends_with(']') {
            quote_depth -= 1;
            if quote_depth == 0 {
                if sorted
                    .get(index + 1)
                    .is_some_and(|next| next.text == "drop" || next.text == "--")
                {
                    if let Some(start) = quote_start.take() {
                        result.extend(sorted[start..=index].iter().map(|node| node.id));
                        result.push(sorted[index + 1].id);
                    }
                } else {
                    quote_start = None;
                }
            }
        }
    }
    result
}

fn render_nodes_text<'a>(nodes: impl Iterator<Item = &'a Node>) -> String {
    let mut nodes = nodes.collect::<Vec<_>>();
    if nodes.is_empty() {
        return String::new();
    }

    nodes.sort_unstable_by_key(|node| (node.position.y as i64, node.position.x as i64));
    let min_x = nodes
        .iter()
        .map(|node| node.position.x as i64)
        .min()
        .unwrap_or_default();
    let min_y = nodes
        .iter()
        .map(|node| node.position.y as i64)
        .min()
        .unwrap_or_default();
    let max_y = nodes
        .iter()
        .map(|node| node.position.y as i64)
        .max()
        .unwrap_or(min_y);
    let mut lines = vec![String::new(); (max_y - min_y + 1) as usize];

    for node in nodes {
        let line = &mut lines[(node.position.y as i64 - min_y) as usize];
        let x = (node.position.x as i64 - min_x).max(0) as usize;
        let len = line.chars().count();
        if len < x {
            line.push_str(&" ".repeat(x - len));
        }
        line.push_str(&node.text);
    }

    lines.join("\n")
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

/// The pattern part of a pattern op's text, e.g. `x.x` of `gate:x.x`.
fn pattern_text(text: &str) -> Option<&str> {
    let (op, pattern) = text.split_once(':')?;
    matches!(op, "pat" | "gate" | "trig" | "cpat" | "cgate" | "ctrig").then_some(pattern)
}

/// Grid column (from the node's start) and width of a span of a pattern node's
/// text. The span is in bytes of the pattern part; the grid counts characters.
fn highlight_columns(text: &str, span: PatternSpan) -> Option<(usize, usize)> {
    let pattern = pattern_text(text)?;
    let sounding = pattern.get(span.start as usize..span.end as usize)?;
    if sounding.is_empty() {
        return None;
    }
    let pattern_start = text.len() - pattern.len();
    let column = text[..pattern_start + span.start as usize].chars().count();
    Some((column, sounding.chars().count()))
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
    fn normal_y_copies_current_line() {
        assert!(matches!(
            key_action(egui::Key::Y, egui::Modifiers::NONE, Mode::Normal),
            Some(Action::CopyCurrentLine)
        ));
    }

    #[test]
    fn normal_shift_y_copies_program() {
        assert!(matches!(
            key_action(
                egui::Key::Y,
                egui::Modifiers {
                    shift: true,
                    ..Default::default()
                },
                Mode::Normal
            ),
            Some(Action::CopyProgram)
        ));
    }

    #[test]
    fn copy_text_renders_line_and_program_relative_to_content() {
        let app = app_with_nodes(
            vec![
                node(1, 4.0, 2.0, "foo"),
                node(2, 9.0, 2.0, "bar"),
                node(3, 6.0, 4.0, "baz"),
            ],
            Point::new(4.0, 2.0),
        );

        assert_eq!(app.current_line_text(), "foo  bar");
        assert_eq!(app.program_text(), "foo  bar\n\n  baz");
    }

    #[test]
    fn highlight_columns_map_pattern_bytes_to_grid_columns() {
        let span = |start, end| PatternSpan { start, end };
        assert_eq!(highlight_columns("pat:60,64", span(3, 5)), Some((7, 2)));
        assert_eq!(highlight_columns("gate:x(3,8).", span(0, 6)), Some((5, 6)));
        assert_eq!(highlight_columns("pat:é,60", span(3, 5)), Some((6, 2)));
        assert_eq!(highlight_columns("pat:60,64", span(0, 0)), None);
        assert_eq!(highlight_columns("pat:60,64", span(3, 9)), None);
        assert_eq!(highlight_columns("pat:é,60", span(1, 3)), None);
        assert_eq!(highlight_columns("sine", span(0, 1)), None);
    }

    #[test]
    fn pattern_monitors_watch_committed_pattern_nodes() {
        let mut app = app_with_nodes(
            vec![
                node(1, 0.0, 0.0, "1"),
                node(2, 2.0, 0.0, "phasor"),
                node(3, 9.0, 0.0, "gate:x."),
                node(4, 17.0, 0.0, "cpat:60,64"),
            ],
            Point::new(0.0, 0.0),
        );
        app.commit_program();
        app.sync_from_repo();
        let kept = PatternSpan { start: 0, end: 1 };
        let old = HashMap::from([(Id::from(3), kept), (Id::from(2), kept)]);

        let monitors = app.pattern_monitors(old);

        assert_eq!(
            monitors,
            HashMap::from([(Id::from(3), kept), (Id::from(4), PatternSpan::default())])
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

    #[test]
    fn edits_are_saved_after_a_delay_not_immediately() {
        let mut app = app_with_nodes(vec![node(1, 0.0, 0.0, "110")], Point::new(0.0, 0.0));
        let path = std::path::PathBuf::from(&app.filename);
        app.handle_action(Action::DeleteNode);
        assert!(!path.exists(), "saved on the edit itself");

        app.save_due = Some(Instant::now() - Duration::from_millis(1));
        app.save_if_due(&egui::Context::default());
        assert!(path.exists(), "not saved once due");
        assert!(app.save_due.is_none());
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn pending_save_is_flushed_when_the_app_closes() {
        let mut app = app_with_nodes(vec![node(1, 0.0, 0.0, "110")], Point::new(0.0, 0.0));
        let path = std::path::PathBuf::from(&app.filename);
        app.handle_action(Action::DeleteNode);
        drop(app);
        assert!(path.exists());
        assert!(
            NodeRepository::load(path.to_str().unwrap())
                .unwrap()
                .nodes()
                .is_empty()
        );
        std::fs::remove_file(path).ok();
    }
}
