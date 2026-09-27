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
use audio_vm::{Sample, VM};
use std::fmt::Write as _;

/// Frames per `sg_process` call at most. Web Audio's render quantum is 128.
pub const MAX_FRAMES: usize = 1024;

pub struct Engine {
    vm: VM,
    ctx: Context,
    sample_rate: u32,
    ids: ids::Ids,
    input: Vec<u8>,
    report: String,
    output: Vec<f32>,
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
        }
    }

    /// Compile `text` and crossfade to it, keeping the state of ops whose words survived the
    /// edit. Returns diagnostics as lines of `word-index<TAB>message` (index -1: whole program).
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
        let (program, diagnostics) =
            compile_program_with_diagnostics(&ops, self.sample_rate, &mut self.ctx);
        drop(self.vm.load_program(program));

        self.report.clear();
        for diagnostic in diagnostics {
            let index = diagnostic
                .id
                .and_then(|id| ids.iter().position(|&x| x == id))
                .map_or(-1, |i| i as i64);
            let _ = writeln!(self.report, "{index}\t{}", diagnostic.message);
        }
        &self.report
    }

    /// Render `frames` frames into the planar output buffer, clipped to -1..1.
    pub fn process(&mut self, frames: usize) -> &[f32] {
        let frames = frames.min(MAX_FRAMES);
        let (left, right) = self.output.split_at_mut(MAX_FRAMES);
        for i in 0..frames {
            let frame = self.vm.next_frame();
            left[i] = frame[0].clamp(-1.0 as Sample, 1.0) as f32;
            right[i] = frame[1].clamp(-1.0 as Sample, 1.0) as f32;
        }
        &self.output
    }
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

/// # Safety
/// `engine` must come from `sg_new`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sg_process(engine: *mut Engine, frames: usize) -> *const f32 {
    unsafe { &mut *engine }.process(frames).as_ptr()
}

// -------------------------------------------------------------------------------------------
// Randomness. getrandom's browser backend needs wasm-bindgen's `crypto` glue, which the worklet
// can't load, so getrandom (0.3 via ahash, 0.4 via rand) is pointed at this generator, seeded
// from the page. `seed:<N>` programs don't use it and stay reproducible.

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

/// Both getrandom 0.3 and 0.4 link their "custom" backend to this symbol.
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

    #[test]
    fn diagnostics_point_at_words() {
        let mut engine = engine();
        let report = engine.load("440 sine2 0.2 *").to_owned();
        assert!(report.starts_with("1\t"), "{report}");
    }
}
