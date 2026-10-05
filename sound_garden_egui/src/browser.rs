//! Browser boundary for the native editor. No audio or filesystem access occurs in this module.
//! IDs are always 16-digit hex strings on the JavaScript side (JSON numbers lose u64 precision).
use audio_ops::MidiEvent;
use audio_program::{Diagnostic, TextOp};
use audio_vm::PatternSpan;
use sound_garden_format::NodeRepository;
use std::{
    cell::{Cell, RefCell},
    sync::{Arc, Mutex},
};
use wasm_bindgen::prelude::*;
/// Web Crypto supplies entropy for random node IDs. This WASM module is separate
/// from the AudioWorklet module, which provides its own custom RNG backend.
/// The workspace config selects getrandom's custom WASM backend.
///
/// # Safety
/// `dest` is a writable buffer of `len` bytes provided by getrandom.
#[unsafe(no_mangle)]
unsafe extern "Rust" fn __getrandom_v03_custom(
    dest: *mut u8,
    len: usize,
) -> Result<(), getrandom::Error> {
    // Web Crypto limits each getRandomValues call to 65,536 bytes. Initialize
    // the destination before exposing it to the JS slice wrapper.
    unsafe { std::ptr::write_bytes(dest, 0, len) };
    let bytes = unsafe { std::slice::from_raw_parts_mut(dest, len) };
    let crypto = web_sys::window()
        .and_then(|window| window.crypto().ok())
        .ok_or(getrandom::Error::UNSUPPORTED)?;
    for chunk in bytes.chunks_mut(65_536) {
        crypto
            .get_random_values_with_u8_array(chunk)
            .map_err(|_| getrandom::Error::UNSUPPORTED)?;
    }
    Ok(())
}

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = ["window", "soundGardenBridge"], js_name = dispatch)]
    fn dispatch(kind: &str, payload: &str);
    #[wasm_bindgen(js_namespace = ["window", "soundGardenBridge"], js_name = poll)]
    fn poll() -> String;
    #[wasm_bindgen(js_namespace = ["window", "soundGardenBridge"], js_name = loadProject)]
    fn load_project() -> Vec<u8>;
    #[wasm_bindgen(js_namespace = ["window", "soundGardenBridge"], js_name = saveProject)]
    fn save_project(bytes: &[u8], content_changed: bool);
}

thread_local! {
    static REPOSITORY: RefCell<Option<Arc<Mutex<NodeRepository>>>> = const { RefCell::new(None) };
    static REPLACED: Cell<bool> = const { Cell::new(false) };
}

pub(crate) fn set_repository(repo: Arc<Mutex<NodeRepository>>) {
    REPOSITORY.with(|slot| *slot.borrow_mut() = Some(repo));
    REPLACED.with(|flag| flag.set(false));
}

pub(crate) fn repository() -> Result<Arc<Mutex<NodeRepository>>, JsValue> {
    REPOSITORY
        .with(|slot| slot.borrow().clone())
        .ok_or_else(|| JsValue::from_str("Editor has not started"))
}

pub(crate) fn take_replaced() -> bool {
    REPLACED.with(|flag| flag.replace(false))
}

pub(crate) fn initial_project() -> Result<NodeRepository, JsValue> {
    let bytes = load_project();
    if bytes.is_empty() {
        Ok(NodeRepository::new())
    } else {
        NodeRepository::from_bytes(&bytes)
            .map_err(|error| JsValue::from_str(&format!("Invalid project: {error}")))
    }
}

pub(crate) fn save(repo: &NodeRepository, content_changed: bool) -> anyhow::Result<()> {
    save_project(&repo.to_bytes()?, content_changed);
    Ok(())
}

/// The bridge may save asynchronously; JS owns conflict detection and resolving the save.
pub(crate) fn replace(bytes: &[u8]) -> Result<(), JsValue> {
    let next = if bytes.is_empty() {
        Ok(NodeRepository::new())
    } else {
        NodeRepository::from_bytes(bytes)
    }
    .map_err(|error| JsValue::from_str(&format!("Invalid project: {error}")))?;
    let repo = repository()?;
    *repo.lock().unwrap() = next;
    REPLACED.with(|flag| flag.set(true));
    Ok(())
}

pub(crate) fn project_bytes() -> Result<Vec<u8>, JsValue> {
    repository()?
        .lock()
        .unwrap()
        .to_bytes()
        .map_err(|error| JsValue::from_str(&error.to_string()))
}

/// Local browser transport mirrors the native message/monitor channels.
#[derive(Clone)]
#[allow(dead_code)]
pub(crate) enum Message {
    Play(bool),
    Record(bool),
    LoadProgram(Vec<TextOp>),
    Monitor(u64),
    PatternMonitors(Vec<u64>),
    Oscilloscope(bool),
    Quit,
}

pub(crate) fn flush_commands(rx: &crossbeam_channel::Receiver<Message>) {
    while let Ok(message) = rx.try_recv() {
        match message {
            Message::Play(play) => dispatch("play", if play { "true" } else { "false" }),
            Message::LoadProgram(ops) => {
                let ops = ops
                    .iter()
                    .map(|op| {
                        serde_json::json!({
                            "id": format!("{:016x}", op.id), "text": op.op,
                        })
                    })
                    .collect::<Vec<_>>();
                dispatch("program", &serde_json::to_string(&ops).unwrap());
            }
            Message::Monitor(id) => dispatch("monitor", &format!("\"{id:016x}\"")),
            Message::PatternMonitors(ids) => dispatch(
                "patternMonitors",
                &serde_json::to_string(
                    &ids.iter()
                        .map(|id| format!("{id:016x}"))
                        .collect::<Vec<_>>(),
                )
                .unwrap(),
            ),
            Message::Oscilloscope(on) => {
                dispatch("oscilloscope", if on { "true" } else { "false" })
            }
            Message::Record(_) | Message::Quit => {}
        }
    }
}

/// Mirrors the native MIDI message so the modeline formatting compiles; the
/// browser engine reports no MIDI messages yet.
#[derive(Clone, Copy, Debug, PartialEq)]
#[allow(dead_code)]
pub(crate) enum MidiMessage {
    Note(MidiEvent),
    Controller { controller: u8, value: f64 },
    Bend(f64),
}

#[derive(Clone, Copy, Default)]
pub(crate) struct Meters {
    pub sample_rate: u32,
    pub buffer_frames: u32,
    pub load: f64,
    pub load_available: bool,
    pub dropouts_estimated: bool,
    pub dropouts: u64,
    pub peak: [f64; 2],
    pub rms: [f64; 2],
    pub clipped: u64,
    pub last_midi: Option<(u64, MidiMessage)>,
}

#[derive(Default)]
pub(crate) struct Diagnostics {
    pub generation: u64,
    pub items: Vec<Diagnostic>,
}

pub(crate) struct Monitor {
    pub scope: [f64; 2],
    pub patterns: Vec<(u64, PatternSpan)>,
    pub meters: Meters,
    pub midi_device: Option<Arc<str>>,
    pub diagnostics: Arc<Diagnostics>,
    pub samples: Vec<[f64; 2]>,
}

#[derive(serde::Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct RawMeters {
    sample_rate: u32,
    buffer_frames: u32,
    load: Option<f64>,
    load_available: bool,
    dropouts_estimated: bool,
    dropouts: u64,
    peak: [f64; 2],
    rms: [f64; 2],
    clipped: u64,
}

#[derive(serde::Deserialize, Default)]
#[serde(default)]
struct RawDiagnostics {
    generation: u64,
    items: Vec<RawDiagnostic>,
}

#[derive(serde::Deserialize)]
struct RawDiagnostic {
    id: Option<String>,
    message: String,
}

#[derive(serde::Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct RawMonitor {
    scope: [f64; 2],
    /// Sounding byte range `[start, end]` of each monitored pattern's text.
    patterns: Vec<(String, [f64; 2])>,
    samples: Vec<[f64; 2]>,
    meters: RawMeters,
    diagnostics: RawDiagnostics,
    midi_device: Option<String>,
}

fn parse_id(id: &str) -> Option<u64> {
    u64::from_str_radix(id, 16).ok()
}

pub(crate) fn poll_monitors(tx: &crossbeam_channel::Sender<Monitor>) {
    let json = poll();
    if json.is_empty() {
        return;
    }
    let frames: Vec<RawMonitor> = match serde_json::from_str(&json) {
        Ok(frames) => frames,
        Err(error) => {
            log::warn!("Invalid browser monitor frame: {error}");
            return;
        }
    };
    for raw in frames.into_iter().take(128) {
        let meters = Meters {
            sample_rate: raw.meters.sample_rate,
            buffer_frames: raw.meters.buffer_frames,
            load: raw.meters.load.unwrap_or(0.0),
            load_available: raw.meters.load_available && raw.meters.load.is_some(),
            dropouts_estimated: raw.meters.dropouts_estimated,
            dropouts: raw.meters.dropouts,
            peak: raw.meters.peak,
            rms: raw.meters.rms,
            clipped: raw.meters.clipped,
            last_midi: None,
        };
        let diagnostics = Diagnostics {
            generation: raw.diagnostics.generation,
            items: raw
                .diagnostics
                .items
                .into_iter()
                .map(|item| Diagnostic {
                    id: item.id.as_deref().and_then(parse_id),
                    message: item.message,
                })
                .collect(),
        };
        let _ = tx.send(Monitor {
            scope: raw.scope,
            patterns: raw
                .patterns
                .into_iter()
                .filter_map(|(id, [start, end])| {
                    Some((
                        parse_id(&id)?,
                        PatternSpan {
                            start: start as u32,
                            end: end as u32,
                        },
                    ))
                })
                .collect(),
            samples: raw.samples,
            meters,
            midi_device: raw.midi_device.map(Arc::from),
            diagnostics: Arc::new(diagnostics),
        });
    }
}
