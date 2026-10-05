//! Sound Garden in the browser: the compiler and VM behind a handful of C-ABI functions, run
//! inside an AudioWorklet by `web/worklet.js`.
//!
//! There is no wasm-bindgen: the worklet scope can't host its glue, and the surface is small
//! enough to drive by hand. Text crosses the boundary as UTF-8 in buffers owned by the engine,
//! and audio comes back as planar f32 (`[L; MAX_FRAMES][R; MAX_FRAMES]`).
//!
//! Compilation happens on the audio thread here, between render quanta, unlike the native
//! server which compiles on its own thread. Typical programs compile in well under a quantum.

mod ids;

use audio_program::{Context, TextOp, compile_program_with_diagnostics};
use audio_vm::{Sample, VM, set_pattern_monitor_ids};
use std::fmt::Write as _;

/// Frames per `sg_process` call at most. Web Audio's render quantum is 128.
pub const MAX_FRAMES: usize = 1024;

/// Minimum captured stereo frames between host monitor polls. Allow a full
/// 30 Hz interval plus one render quantum at the engine's sample rate.
const MIN_MONITOR_FRAMES: usize = 2048;
const MAX_PATTERN_MONITORS: usize = 256;

pub struct Engine {
    vm: VM,
    ctx: Context,
    sample_rate: u32,
    ids: ids::Ids,
    input: Vec<u8>,
    report: String,
    output: Vec<f32>,
    scope_samples: Vec<f32>,
    scope_frames: usize,
    scope_enabled: bool,
    monitor: Vec<f32>,
    pattern_ids: Vec<u64>,
    peak: [f32; 2],
    sum_squares: [f64; 2],
    meter_frames: usize,
    clipped: u32,
    generation: u32,
    midi_pending: Vec<audio_ops::MidiEvent>,
}

impl Engine {
    pub fn new(sample_rate: u32) -> Self {
        Engine {
            vm: VM::new(),
            ctx: Context::new(),
            sample_rate,
            ids: ids::Ids::default(),
            input: Vec::new(),
            report: String::new(),
            output: vec![0.0; 2 * MAX_FRAMES],
            scope_samples: vec![
                0.0;
                2 * (sample_rate as usize / 30 + MAX_FRAMES).max(MIN_MONITOR_FRAMES)
            ],
            scope_frames: 0,
            scope_enabled: false,
            monitor: vec![0.0; 6],
            pattern_ids: Vec::new(),
            peak: [0.0; 2],
            sum_squares: [0.0; 2],
            meter_frames: 0,
            clipped: 0,
            generation: 0,
            midi_pending: Vec::with_capacity(audio_ops::MAX_MIDI_EVENTS_PER_FRAME),
        }
    }

    /// Compile text with diff-inherited IDs; diagnostics use word indices for the playground.
    pub fn load(&mut self, text: &str) -> &str {
        let words = text.split_whitespace().collect::<Vec<_>>();
        let ids = self.ids.assign(&words);
        let ops = words
            .iter()
            .zip(&ids)
            .map(|(word, &id)| TextOp {
                id,
                op: word.to_string(),
            })
            .collect::<Vec<_>>();
        self.compile(&ops, false)
    }

    /// Compile positioned editor nodes with their exact persistent IDs.
    /// Diagnostics use decimal IDs (`-1` means a whole-program warning).
    pub fn load_nodes(&mut self, nodes: &[TextOp]) -> &str {
        self.compile(nodes, true)
    }

    fn compile(&mut self, ops: &[TextOp], keyed: bool) -> &str {
        let (program, diagnostics) =
            compile_program_with_diagnostics(ops, self.sample_rate, &mut self.ctx);
        drop(self.vm.load_program(program));
        self.generation = self.generation.wrapping_add(1);
        self.report.clear();
        for diagnostic in diagnostics {
            let key = if keyed {
                diagnostic
                    .id
                    .map_or_else(|| "-1".to_string(), |id| id.to_string())
            } else {
                diagnostic
                    .id
                    .and_then(|id| ops.iter().position(|op| op.id == id))
                    .map_or(-1, |i| i as i64)
                    .to_string()
            };
            let _ = writeln!(self.report, "{key}\t{}", diagnostic.message);
        }
        &self.report
    }

    /// Render planar clipped stereo and capture node scope and output meters.
    pub fn process(&mut self, frames: usize) -> &[f32] {
        let frames = frames.min(MAX_FRAMES);
        let (left, right) = self.output.split_at_mut(MAX_FRAMES);
        if frames > 0 && !self.midi_pending.is_empty() {
            self.ctx.midi.set_events(&self.midi_pending);
            self.midi_pending.clear();
        }
        for i in 0..frames {
            let frame = self.vm.next_frame();
            if i == 0 {
                self.ctx.midi.clear();
            }
            left[i] = frame[0].clamp(-1.0 as Sample, 1.0) as f32;
            right[i] = frame[1].clamp(-1.0 as Sample, 1.0) as f32;
            for channel in 0..2 {
                let x = frame[channel] as f32;
                self.peak[channel] = self.peak[channel].max(x.abs());
                self.sum_squares[channel] += f64::from(x) * f64::from(x);
                if !x.is_finite() || x.abs() > 1.0 {
                    self.clipped = self.clipped.saturating_add(1);
                }
            }
            self.meter_frames += 1;
            if self.scope_enabled {
                if self.scope_frames == self.scope_samples.len() / 2 {
                    // Bounded ring: preserve the newest samples if the page stalls.
                    self.scope_samples.copy_within(2.., 0);
                    self.scope_frames -= 1;
                }
                let scope = self.vm.scope();
                let offset = 2 * self.scope_frames;
                self.scope_samples[offset] = scope[0] as f32;
                self.scope_samples[offset + 1] = scope[1] as f32;
                self.scope_frames += 1;
            }
        }
        &self.output
    }

    /// Snapshot the latest monitor values. This is called by the worklet at ~30 Hz.
    /// Layout: scope L/R, peak L/R, RMS L/R, then two floats per requested pattern ID.
    pub fn capture_monitor(&mut self) -> usize {
        let scope = self.vm.scope();
        self.monitor[0] = scope[0] as f32;
        self.monitor[1] = scope[1] as f32;
        for channel in 0..2 {
            self.monitor[2 + channel] = self.peak[channel];
            self.monitor[4 + channel] = if self.meter_frames == 0 {
                0.0
            } else {
                (self.sum_squares[channel] / self.meter_frames as f64).sqrt() as f32
            };
        }
        self.peak = [0.0; 2];
        self.sum_squares = [0.0; 2];
        self.meter_frames = 0;
        if let Ok(patterns) = self.vm.pattern_monitor().lock() {
            for (i, &id) in self.pattern_ids.iter().enumerate() {
                let frame = patterns
                    .iter()
                    .find(|(key, _)| *key == id)
                    .map(|(_, frame)| *frame)
                    .unwrap_or_default();
                self.monitor[6 + i * 2] = frame[0] as f32;
                self.monitor[7 + i * 2] = frame[1] as f32;
            }
        }
        self.monitor.len()
    }
}

/// Parse a count-prefixed list of nodes: u32 count, then u64 LE ID, u32 byte length, UTF-8 text.
/// Reject malformed input before replacing the active program.
fn parse_nodes(input: &[u8]) -> Result<Vec<TextOp>, &'static str> {
    fn take<'a>(input: &mut &'a [u8], n: usize) -> Result<&'a [u8], &'static str> {
        if n > input.len() {
            return Err("truncated nodes");
        }
        let (head, tail) = input.split_at(n);
        *input = tail;
        Ok(head)
    }
    fn u32_le(input: &mut &[u8]) -> Result<u32, &'static str> {
        Ok(u32::from_le_bytes(take(input, 4)?.try_into().unwrap()))
    }
    let mut input = input;
    let count = u32_le(&mut input)? as usize;
    if count > input.len() / 12 {
        return Err("invalid node count");
    }
    let mut nodes = Vec::with_capacity(count);
    for _ in 0..count {
        let id = u64::from_le_bytes(take(&mut input, 8)?.try_into().unwrap());
        let len = u32_le(&mut input)? as usize;
        let op = std::str::from_utf8(take(&mut input, len)?).map_err(|_| "invalid UTF-8")?;
        nodes.push(TextOp {
            id,
            op: op.to_owned(),
        });
    }
    if !input.is_empty() {
        return Err("trailing node data");
    }
    Ok(nodes)
}

// -------------------------------------------------------------------------------------------
// C ABI for the worklet. Pointers to the engine come from `sg_new` and are never freed: a page
// makes one engine per audio context.

#[unsafe(no_mangle)]
pub extern "C" fn sg_new(sample_rate: u32, seed: u32) -> *mut Engine {
    rng::seed(seed);
    Box::into_raw(Box::new(Engine::new(sample_rate)))
}

#[unsafe(no_mangle)]
pub extern "C" fn sg_max_frames() -> usize {
    MAX_FRAMES
}

/// A buffer of `len` bytes for the next program text.
///
/// # Safety
/// `engine` must come from `sg_new`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sg_input(engine: *mut Engine, len: usize) -> *mut u8 {
    let engine = unsafe { &mut *engine };
    engine.input.resize(len, 0);
    engine.input.as_mut_ptr()
}

/// Compile and load the text written to `sg_input`; returns the diagnostics' length in bytes,
/// readable at `sg_report`.
///
/// # Safety
/// `engine` must come from `sg_new`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sg_load(engine: *mut Engine) -> usize {
    let engine = unsafe { &mut *engine };
    let input = std::mem::take(&mut engine.input);
    let len = engine.load(&String::from_utf8_lossy(&input)).len();
    engine.input = input;
    len
}

/// Compile count-prefixed binary nodes written to `sg_input`. On invalid input, return a
/// whole-program warning without disturbing the active program.
/// # Safety
/// `engine` must come from `sg_new`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sg_load_nodes(engine: *mut Engine) -> usize {
    let engine = unsafe { &mut *engine };
    let input = std::mem::take(&mut engine.input);
    let len = match parse_nodes(&input) {
        Ok(nodes) => engine.load_nodes(&nodes).len(),
        Err(error) => {
            engine.report = format!("-1\t{error}\n");
            engine.report.len()
        }
    };
    engine.input = input;
    len
}

/// # Safety
/// `engine` must come from `sg_new`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sg_generation(engine: *const Engine) -> u32 {
    unsafe { &*engine }.generation
}

/// # Safety
/// `engine` must come from `sg_new`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sg_set_monitor(engine: *mut Engine, lo: u32, hi: u32) {
    unsafe { &mut *engine }
        .vm
        .set_monitor_id(u64::from(lo) | (u64::from(hi) << 32));
}

/// Read a count-prefixed list of u64 little-endian IDs from `sg_input`.
/// # Safety
/// `engine` must come from `sg_new`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sg_set_pattern_monitors(engine: *mut Engine) {
    let engine = unsafe { &mut *engine };
    let bytes = &engine.input;
    if bytes.len() < 4 {
        return;
    }
    let count = u32::from_le_bytes(bytes[..4].try_into().unwrap()) as usize;
    if count > MAX_PATTERN_MONITORS || bytes.len() != 4 + count * 8 {
        return;
    }
    let ids = bytes[4..]
        .chunks_exact(8)
        .map(|bytes| u64::from_le_bytes(bytes.try_into().unwrap()))
        .collect::<Vec<_>>();
    set_pattern_monitor_ids(&engine.vm.pattern_monitor(), &ids);
    engine.monitor.resize(6 + 2 * count, 0.0);
    engine.pattern_ids = ids;
}

/// # Safety
/// `engine` must come from `sg_new`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sg_set_oscilloscope(engine: *mut Engine, enabled: bool) {
    let engine = unsafe { &mut *engine };
    engine.scope_enabled = enabled;
    engine.scope_frames = 0;
}

/// # Safety
/// `engine` must come from `sg_new`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sg_capture_monitor(engine: *mut Engine) -> usize {
    unsafe { &mut *engine }.capture_monitor()
}

/// # Safety
/// `engine` must come from `sg_new`; call `sg_capture_monitor` first.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sg_monitor(engine: *const Engine) -> *const f32 {
    unsafe { &*engine }.monitor.as_ptr()
}

/// Number of pre-clip channel samples exceeding ±1 since the last poll.
/// # Safety
/// `engine` must come from `sg_new`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sg_take_clipped(engine: *mut Engine) -> u32 {
    let engine = unsafe { &mut *engine };
    std::mem::take(&mut engine.clipped)
}

/// # Safety
/// `engine` must come from `sg_new`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sg_scope_frames(engine: *const Engine) -> usize {
    unsafe { &*engine }.scope_frames
}

/// # Safety
/// `engine` must come from `sg_new`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sg_scope_samples(engine: *const Engine) -> *const f32 {
    unsafe { &*engine }.scope_samples.as_ptr()
}

/// # Safety
/// `engine` must come from `sg_new`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sg_clear_scope(engine: *mut Engine) {
    unsafe { &mut *engine }.scope_frames = 0;
}

/// MIDI note event (0=off, 1=on). Controllers and bend are separate kinds (2, 3),
/// normalized via 7-bit and 14-bit MIDI values. Note events are delivered to the next frame.
/// # Safety
/// `engine` must come from `sg_new`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sg_midi(
    engine: *mut Engine,
    kind: u32,
    channel: u32,
    data1: u32,
    data2: u32,
) {
    use audio_ops::MidiEvent;
    let engine = unsafe { &mut *engine };
    match kind {
        0 | 1 if channel < 16 && data1 < 128 && data2 < 128 => {
            if engine.midi_pending.len() < audio_ops::MAX_MIDI_EVENTS_PER_FRAME {
                engine.midi_pending.push(if kind == 1 && data2 > 0 {
                    MidiEvent::note_on(channel as u8, data1 as u8, data2 as f64 / 127.0)
                } else {
                    MidiEvent::note_off(channel as u8, data1 as u8)
                });
            }
        }
        2 if data1 < 128 && data2 < 128 => engine
            .ctx
            .midi_controls
            .set_controller(data1 as u8, data2 as f64 / 127.0),
        3 if data1 < 128 && data2 < 128 => {
            let raw = ((data2 << 7) | data1) as i32 - 8192;
            engine.ctx.midi_controls.set_bend(if raw >= 0 {
                raw as f64 / 8191.0
            } else {
                raw as f64 / 8192.0
            });
        }
        _ => {}
    }
}

/// # Safety
/// `engine` must come from `sg_new`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sg_report(engine: *mut Engine) -> *const u8 {
    unsafe { &*engine }.report.as_ptr()
}

/// Fade in or out, like the editor's play/pause.
///
/// # Safety
/// `engine` must come from `sg_new`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sg_play(engine: *mut Engine, play: bool) {
    let vm = &mut unsafe { &mut *engine }.vm;
    if play { vm.play() } else { vm.pause() }
}

/// Start the next program with fresh ids, e.g. when switching to an unrelated program.
///
/// # Safety
/// `engine` must come from `sg_new`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sg_forget(engine: *mut Engine) {
    unsafe { &mut *engine }.ids.forget();
}

/// Discard previous op state before opening an unrelated positioned project.
/// # Safety
/// `engine` must come from `sg_new`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sg_reset_program(engine: *mut Engine) {
    let engine = unsafe { &mut *engine };
    drop(engine.vm.load_program(Vec::new()));
    engine.scope_frames = 0;
}

/// # Safety
/// `engine` must come from `sg_new`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sg_process(engine: *mut Engine, frames: usize) -> *const f32 {
    unsafe { &mut *engine }.process(frames).as_ptr()
}

// -------------------------------------------------------------------------------------------
// Randomness. getrandom's browser backend needs wasm-bindgen's `crypto` glue, which the worklet
// can't load, so getrandom (via rand) is pointed at this generator, seeded from the page.
// `seed:<N>` programs don't use it and stay reproducible.

#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))] // getrandom only calls in on wasm
mod rng {
    use std::sync::atomic::{AtomicU64, Ordering};

    static STATE: AtomicU64 = AtomicU64::new(0x853c_49e6_748f_ea9b);

    pub fn seed(seed: u32) {
        STATE.store(0x9E37_79B9_7F4A_7C15 ^ seed as u64, Ordering::Relaxed);
    }

    /// SplitMix64.
    pub fn next() -> u64 {
        let mut z = STATE
            .fetch_add(0x9E37_79B9_7F4A_7C15, Ordering::Relaxed)
            .wrapping_add(0x9E37_79B9_7F4A_7C15);
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
}

/// getrandom 0.4 still links its "custom" backend to the 0.3-named symbol.
///
/// # Safety
/// Called by getrandom with a valid `dest` of `len` bytes.
#[cfg(target_arch = "wasm32")]
#[unsafe(no_mangle)]
unsafe extern "Rust" fn __getrandom_v03_custom(
    dest: *mut u8,
    len: usize,
) -> Result<(), getrandom::Error> {
    let dest = unsafe { std::slice::from_raw_parts_mut(dest, len) };
    for chunk in dest.chunks_mut(8) {
        chunk.copy_from_slice(&rng::next().to_le_bytes()[..chunk.len()]);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn engine() -> Engine {
        let mut engine = Engine::new(48_000);
        engine.vm.play();
        engine
    }

    fn run(engine: &mut Engine, frames: usize) -> f32 {
        let mut last = 0.0;
        for _ in 0..frames {
            last = engine.process(1)[0];
        }
        last
    }

    #[test]
    fn edits_keep_oscillator_phase() {
        // Edited a quarter period in: the sine keeps its id and so its phase, and once the
        // crossfade is over the edit sounds like the original sine at half level.
        let mut edited = engine();
        edited.load("100 s");
        run(&mut edited, 120);
        edited.load("100 s 0.5 *");
        let edited = run(&mut edited, 12_000 + 60);

        let mut original = engine();
        original.load("100 s");
        let original = run(&mut original, 120 + 12_000 + 60);

        assert!(original.abs() > 0.5);
        assert!(
            (edited - 0.5 * original).abs() < 1e-3,
            "{edited} vs {original}"
        );
    }

    fn collect(engine: &mut Engine, frames: usize) -> Vec<f32> {
        (0..frames).map(|_| engine.process(1)[0]).collect()
    }

    /// Energy above `cutoff` Hz relative to all energy, in dB, over a Hann window: how much of
    /// a 220 Hz tone's window is click.
    fn high_band_db(xs: &[f32], cutoff: f64) -> f64 {
        let n = xs.len();
        let windowed = xs
            .iter()
            .enumerate()
            .map(|(i, &x)| {
                x as f64 * (0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / n as f64).cos())
            })
            .collect::<Vec<_>>();
        let (mut high, mut total) = (0.0, 0.0);
        for k in 1..n / 2 {
            let w = std::f64::consts::TAU * k as f64 / n as f64;
            let (re, im) = windowed
                .iter()
                .enumerate()
                .fold((0.0, 0.0), |(re, im), (i, &x)| {
                    (re + x * (w * i as f64).cos(), im - x * (w * i as f64).sin())
                });
            let power = re * re + im * im;
            total += power;
            if k as f64 * 48_000.0 / n as f64 > cutoff {
                high += power;
            }
        }
        10.0 * (high / total).log10()
    }

    #[test]
    fn number_edits_glide_without_a_click() {
        // Halving the amplitude of a running sine: an instant change clicks even after the
        // reload declick (about -43 dB of the window above 1 kHz); the glide is inaudible.
        let mut engine = engine();
        engine.load("220 s 0.2 *");
        run(&mut engine, 12_000);
        let mut window = collect(&mut engine, 1024);
        engine.load("220 s 0.1 *");
        window.extend(collect(&mut engine, 1024));
        let click = high_band_db(&window, 1_000.0);
        assert!(click < -70.0, "click energy {click:.1} dB");
    }

    /// Replace `from` with `to` at several points in the cycle. Returns the worst error (dB, relative
    /// to the signal) of the 10 ms morph against an ideal crossfade from an engine still playing
    /// `from` to one that has played `to` all along, and of the 20 ms after it against the latter.
    fn swap_errors(from: &str, to: &str) -> (f64, f64) {
        let error_db = |actual: &[f32], ideal: &[f32]| {
            let error: f64 = actual
                .iter()
                .zip(ideal)
                .map(|(x, y)| ((x - y) as f64).powi(2))
                .sum();
            let signal: f64 = ideal.iter().map(|y| (*y as f64).powi(2)).sum();
            10.0 * (error / signal + 1e-30).log10()
        };
        let (mut morph, mut after) = (f64::MIN, f64::MIN);
        for at in [20_000, 20_037, 20_111, 20_230, 20_301] {
            let (mut edited, mut old, mut new) = (engine(), engine(), engine());
            edited.load(from);
            old.load(from);
            new.load(to);
            for e in [&mut edited, &mut old, &mut new] {
                run(e, at);
            }
            edited.load(to);
            let (e, a, b) = (
                collect(&mut edited, 480),
                collect(&mut old, 480),
                collect(&mut new, 480),
            );
            let ideal = (0..480)
                .map(|i| {
                    let w = 0.5 - 0.5 * (std::f32::consts::PI * (i + 1) as f32 / 480.0).cos();
                    a[i] * (1.0 - w) + b[i] * w
                })
                .collect::<Vec<_>>();
            morph = morph.max(error_db(&e, &ideal));
            after = after.max(error_db(
                &collect(&mut edited, 960),
                &collect(&mut new, 960),
            ));
        }
        (morph, after)
    }

    #[test]
    fn waveform_swaps_morph_and_continue_the_cycle() {
        for (from, to) in [
            ("110 s 0.2 *", "110 0 saw 0.2 *"),
            ("110 0 saw 0.2 *", "110 0.5 p 0.2 *"),
            ("110 0.5 p 0.2 *", "110 s 0.2 *"),
            ("110 s' 0.2 *", "110 t' 0.2 *"),
            ("110 s 0.2 *", "110 >f <f s 0.2 *"),
            ("110 w 0.2 *", "110 c 0.2 *"),
        ] {
            let (morph, after) = swap_errors(from, to);
            assert!(
                morph < -100.0,
                "{from} -> {to}: morph {morph:.1} dB from an ideal crossfade"
            );
            assert!(
                after < -100.0,
                "{from} -> {to}: {after:.1} dB from a fresh oscillator"
            );
        }
    }

    #[test]
    fn band_limited_triangle_swaps_are_continuous() {
        // A fresh band-limited triangle locks onto its cycle with a sub-sample offset on its first
        // wrap; one taking over a cycle is seeded exactly. That offset (about -40 dB) is all that
        // separates them; a click would show tens of dB more.
        for (from, to) in [
            ("110 s 0.2 *", "110 t 0.2 *"),
            ("110 t 0.2 *", "110 s 0.2 *"),
        ] {
            let (morph, after) = swap_errors(from, to);
            assert!(
                morph < -38.0 && after < -38.0,
                "{from} -> {to}: {morph:.1} / {after:.1} dB"
            );
        }
    }

    #[test]
    fn a_gate_turned_on_by_an_edit_opens_fully() {
        // A plain constant may be a gate: switching it on must be an edge, not a glide, or
        // adsr would latch the first tiny step of the glide as its peak.
        let mut engine = engine();
        engine.load("0 0.001 0.01 1 0.1 adsr");
        run(&mut engine, 12_000);
        engine.load("1 0.001 0.01 1 0.1 adsr");
        let level = run(&mut engine, 2_000);
        assert!(level > 0.99, "envelope reached {level}");
    }

    #[test]
    fn diagnostics_point_at_words() {
        let mut engine = engine();
        let report = engine.load("440 sine2 0.2 *").to_owned();
        assert!(report.starts_with("1\t"), "{report}");
    }

    #[test]
    fn typed_nodes_preserve_full_ids_and_key_diagnostics() {
        let mut engine = engine();
        let id = 0xdead_beef_1234_5678;
        let nodes = [TextOp {
            id,
            op: "sine2".into(),
        }];
        assert!(engine.load_nodes(&nodes).starts_with(&format!("{id}\t")));
        assert_eq!(engine.generation, 1);
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&id.to_le_bytes());
        bytes.extend_from_slice(&5u32.to_le_bytes());
        bytes.extend_from_slice(b"sine2");
        assert_eq!(parse_nodes(&bytes).unwrap()[0].id, id);
        assert!(parse_nodes(&bytes[..bytes.len() - 1]).is_err());
        engine.input = bytes;
        let len = unsafe { sg_load_nodes(&mut engine) };
        assert!(engine.report[..len].starts_with(&format!("{id}\t")));
        assert_eq!(engine.generation, 2);
    }

    #[test]
    fn typed_nodes_migrate_oscillator_state() {
        let base = [
            TextOp {
                id: 100,
                op: "100".into(),
            },
            TextOp {
                id: 0xfedc_ba98_7654_3210,
                op: "s".into(),
            },
        ];
        let mut edited = engine();
        edited.load_nodes(&base);
        run(&mut edited, 120);
        let mut update = base.to_vec();
        update.push(TextOp {
            id: 103,
            op: "0.5".into(),
        });
        update.push(TextOp {
            id: 104,
            op: "*".into(),
        });
        edited.load_nodes(&update);
        let edited = run(&mut edited, 12_060);
        let mut original = engine();
        original.load_nodes(&base);
        let original = run(&mut original, 12_180);
        assert!((edited - original * 0.5).abs() < 1e-3);
    }

    #[test]
    fn monitor_samples_patterns_and_stereo_meters_are_bounded() {
        let mut engine = engine();
        engine.load_nodes(&[TextOp {
            id: 123,
            op: "0.5".into(),
        }]);
        set_pattern_monitor_ids(&engine.vm.pattern_monitor(), &[123]);
        engine.pattern_ids.push(123);
        engine.monitor.resize(8, 0.0);
        engine.vm.set_monitor_id(123);
        engine.scope_enabled = true;
        for _ in 0..3 {
            engine.process(1024);
        }
        assert_eq!(engine.scope_frames, engine.scope_samples.len() / 2);
        assert_eq!(engine.capture_monitor(), 8);
        assert!(engine.monitor[2] > 0.0 && engine.monitor[3] > 0.0);
        assert!(engine.monitor[4] > 0.0 && engine.monitor[5] > 0.0);
        assert!((engine.monitor[6] - 0.5).abs() < 1e-6);
        assert!((engine.monitor[0] - 0.5).abs() < 1e-6);
        assert!(
            engine
                .scope_samples
                .iter()
                .take(2 * engine.scope_frames)
                .any(|x| *x != 0.0)
        );
        assert_eq!(engine.capture_monitor(), 8);
        assert_eq!(&engine.monitor[2..6], &[0.0; 4]);
    }

    #[test]
    fn monitor_keeps_a_full_30_hz_batch_at_96_khz() {
        let mut engine = Engine::new(96_000);
        engine.scope_enabled = true;
        for _ in 0..25 {
            engine.process(128);
        }
        assert_eq!(engine.scope_frames, 3_200);
        assert!(engine.scope_samples.len() / 2 >= engine.scope_frames);
    }

    #[test]
    fn clipped_samples_are_counted_before_clamping_and_reset_on_poll() {
        let mut engine = engine();
        engine.load("2");
        for _ in 0..12 {
            engine.process(1024);
        }
        assert_eq!(engine.output[1023], 1.0);
        assert!(unsafe { sg_take_clipped(&mut engine) } > 0);
        assert_eq!(unsafe { sg_take_clipped(&mut engine) }, 0);
    }
    #[test]
    fn midi_note_cc_and_bend_reach_shared_buses() {
        let mut engine = engine();
        unsafe {
            sg_midi(&mut engine, 1, 3, 60, 127);
        }
        unsafe {
            sg_midi(&mut engine, 2, 3, 74, 64);
        }
        unsafe {
            sg_midi(&mut engine, 3, 3, 127, 127);
        }
        assert_eq!(engine.midi_pending.len(), 1);
        assert_eq!(engine.ctx.midi_controls.controller(74), Some(64.0 / 127.0));
        assert_eq!(engine.ctx.midi_controls.bend(), Some(1.0));
        engine.process(1);
        assert!(engine.midi_pending.is_empty());
    }
}
