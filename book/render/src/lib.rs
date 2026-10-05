//! The book's renderer: a Sound Garden program to frames, and frames to the figures the book
//! prints. Used natively by the `book_render` tool and, compiled to WebAssembly (see `wasm`), by
//! the book's live editing, so figures of an edited program look exactly like the printed ones.

use audio_program::{Context, TextOp, compile_program};
use audio_vm::{CHANNELS, Sample, VM};
use rustfft::{FftPlanner, num_complex::Complex};
use std::fmt::Write as _;

pub const SAMPLE_RATE: u32 = 48_000;

pub type Frame = [Sample; CHANNELS];

/// Render `seconds` of `text` from its first sample, without the VM's fade-in.
pub fn render(text: &str, seconds: f64) -> Vec<Frame> {
    // Stable node ids keep renders of seeded programs identical between builds.
    let ops = text
        .split_whitespace()
        .enumerate()
        .map(|(index, op)| TextOp {
            id: (index + 1) as u64,
            op: op.to_owned(),
        })
        .collect::<Vec<_>>();
    let mut vm = VM::new();
    // Figures show the signal from its very first sample, so no fade-in.
    vm.set_xfade_duration(0.0);
    vm.load_program(compile_program(&ops, SAMPLE_RATE, &mut Context::new()));
    vm.play();
    audio_vm::enable_flush_to_zero();
    (0..(seconds * SAMPLE_RATE as f64) as usize)
        .map(|_| vm.next_frame())
        .collect()
}

/// Fade out the last `seconds` of a render with a raised cosine, so an excerpt of a piece
/// that has no ending stops gently instead of being cut off.
pub fn fade_out(frames: &mut [Frame], seconds: f64) {
    let length = ((seconds * SAMPLE_RATE as f64) as usize).min(frames.len());
    let start = frames.len() - length;
    for (i, frame) in frames[start..].iter_mut().enumerate() {
        let gain = 0.5 + 0.5 * (std::f64::consts::PI * (i + 1) as f64 / length as f64).cos();
        for sample in frame.iter_mut() {
            *sample *= gain as Sample;
        }
    }
}

/// Frame range of the `from`..`to` window in seconds (the whole render by default).
pub fn window(frames: &[Frame], from: Option<f64>, to: Option<f64>) -> std::ops::Range<usize> {
    let index = |t: f64| ((t * SAMPLE_RATE as f64).round() as usize).min(frames.len());
    let start = from.map_or(0, index);
    let end = to.map_or(frames.len(), index).max(start);
    start..end
}

// ---------------------------------------------------------------------------------------------
// Figures. SVGs carry classes only, no colours, so the page stylesheet themes them (dark mode).

const W: f64 = 800.0;
const LANE: f64 = 120.0;
const LEFT: f64 = 34.0;
const RIGHT: f64 = 8.0;
const TOP: f64 = 8.0;
const BOTTOM: f64 = 22.0;
/// Up to this many samples are drawn as individual lollipops, showing that sound is numbers.
const STEM_LIMIT: usize = 96;

pub fn wave_svg(frames: &[Frame], t0: f64) -> String {
    let stereo = frames.iter().any(|f| (f[0] - f[1]).abs() > 1e-9);
    let lanes = if stereo { CHANNELS } else { 1 };
    let height = TOP + LANE * lanes as f64 + BOTTOM;
    let plot_w = W - LEFT - RIGHT;
    let n = frames.len();
    let duration = n as f64 / SAMPLE_RATE as f64;
    let mut svg = svg_open(
        height,
        "sg-wave",
        &format!(" data-t0=\"{t0}\" data-duration=\"{duration}\""),
    );

    let x_of = |i: f64| LEFT + plot_w * if n > 1 { i / (n - 1) as f64 } else { 0.5 };
    for lane in 0..lanes {
        let top = TOP + LANE * lane as f64;
        let y_of = |v: Sample| top + LANE * 0.5 * (1.0 - v.clamp(-1.2, 1.2) / 1.2);
        for (v, label) in [(1.0, "1"), (0.0, "0"), (-1.0, "−1")] {
            let y = y_of(v);
            let class = if v == 0.0 { "axis" } else { "grid" };
            let _ = write!(
                svg,
                r#"<line class="{class}" x1="{LEFT}" x2="{}" y1="{y:.1}" y2="{y:.1}"/>"#,
                W - RIGHT
            );
            let _ = write!(
                svg,
                r#"<text class="label" x="{}" y="{:.1}" text-anchor="end">{label}</text>"#,
                LEFT - 5.0,
                y + 3.5
            );
        }
        if stereo {
            let _ = write!(
                svg,
                r#"<text class="label channel" x="{}" y="{}">{}</text>"#,
                LEFT + 4.0,
                top + 12.0,
                ["L", "R"][lane]
            );
        }
        let c = lane;
        if n <= STEM_LIMIT {
            let y0 = y_of(0.0);
            for (i, f) in frames.iter().enumerate() {
                let (x, y) = (x_of(i as f64), y_of(f[c]));
                let _ = write!(
                    svg,
                    r#"<line class="stem" x1="{x:.1}" x2="{x:.1}" y1="{y0:.1}" y2="{y:.1}"/><circle class="dot" cx="{x:.1}" cy="{y:.1}" r="2.6"/>"#
                );
            }
        } else if n as f64 <= plot_w * 2.0 {
            svg.push_str(r#"<polyline class="trace" points=""#);
            for (i, f) in frames.iter().enumerate() {
                let _ = write!(svg, "{:.1},{:.1} ", x_of(i as f64), y_of(f[c]));
            }
            svg.push_str(r#""/>"#);
        } else {
            // Min/max envelope per column: the shape a long sound makes on an oscilloscope.
            let columns = plot_w as usize;
            let (mut upper, mut lower) = (String::new(), Vec::new());
            for col in 0..columns {
                let a = col * n / columns;
                let b = ((col + 1) * n / columns).max(a + 1).min(n);
                let (lo, hi) = frames[a..b]
                    .iter()
                    .fold((Sample::MAX, Sample::MIN), |(lo, hi), f| {
                        (lo.min(f[c]), hi.max(f[c]))
                    });
                let x = LEFT + col as f64 + 0.5;
                let _ = write!(upper, "{x:.1},{:.1} ", y_of(hi) - 0.4);
                lower.push(format!("{x:.1},{:.1}", y_of(lo) + 0.4));
            }
            lower.reverse();
            let _ = write!(
                svg,
                r#"<polygon class="envelope" points="{upper}{}"/>"#,
                lower.join(" ")
            );
        }
    }

    let axis_y = TOP + LANE * lanes as f64;
    for t in nice_ticks(t0, t0 + duration, 7) {
        let x = LEFT + plot_w * (t - t0) / duration.max(1e-12);
        let _ = write!(
            svg,
            r#"<line class="tick" x1="{x:.1}" x2="{x:.1}" y1="{axis_y}" y2="{}"/>"#,
            axis_y + 4.0
        );
        let _ = write!(
            svg,
            r#"<text class="label" x="{x:.1}" y="{}" text-anchor="middle">{}</text>"#,
            axis_y + 16.0,
            time_label(t, duration)
        );
    }
    svg.push_str(
        r#"<line class="playhead" x1="0" x2="0" y1="0" y2="100%" visibility="hidden"/></svg>"#,
    );
    svg
}

/// A compact whole-clip envelope for the player's scrubber; stretched to any width by CSS.
pub fn overview_svg(frames: &[Frame]) -> String {
    const COLUMNS: usize = 400;
    const H: f64 = 40.0;
    let n = frames.len().max(1);
    let peaks = (0..COLUMNS)
        .map(|col| {
            let a = (col * n / COLUMNS).min(frames.len());
            let b = ((col + 1) * n / COLUMNS).min(frames.len());
            frames[a..b]
                .iter()
                .flatten()
                .fold(0.0 as Sample, |m, x| m.max(x.abs()))
                .min(1.0)
        })
        .collect::<Vec<_>>();
    // Quiet passages stay visible: the envelope is drawn on a gentle power curve.
    let y = |v: Sample, sign: f64| H / 2.0 - sign * (0.6 + v.powf(0.6) * (H / 2.0 - 1.0));
    let mut points = String::new();
    for (i, v) in peaks.iter().enumerate() {
        let _ = write!(points, "{i}.5,{:.1} ", y(*v, 1.0));
    }
    for (i, v) in peaks.iter().enumerate().rev() {
        let _ = write!(points, "{i}.5,{:.1} ", y(*v, -1.0));
    }
    format!(
        r#"<svg class="sg-overview" viewBox="0 0 {COLUMNS} {H}" preserveAspectRatio="none" xmlns="http://www.w3.org/2000/svg" aria-hidden="true"><polygon points="{points}"/></svg>"#
    )
}

pub fn spectrum_svg(frames: &[Frame], fmax: f64, log_freq: bool) -> String {
    let mono = frames
        .iter()
        .map(|f| f.iter().sum::<Sample>() / CHANNELS as Sample)
        .collect::<Vec<_>>();
    let size = 8192.min(mono.len().next_power_of_two() / 2).max(256);
    let hann = (0..size)
        .map(|i| 0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / size as f64).cos())
        .collect::<Vec<_>>();
    let gain = 2.0 / hann.iter().sum::<f64>();
    let fft = FftPlanner::new().plan_fft_forward(size);
    // Welch: average power over half-overlapping frames.
    let mut power = vec![0.0; size / 2];
    let mut count = 0;
    let mut start = 0;
    while start + size <= mono.len().max(size) {
        let mut buf = (0..size)
            .map(|i| Complex::new(mono.get(start + i).copied().unwrap_or(0.0) * hann[i], 0.0))
            .collect::<Vec<_>>();
        fft.process(&mut buf);
        for (p, x) in power.iter_mut().zip(&buf) {
            *p += (x.norm() * gain).powi(2);
        }
        count += 1;
        start += size / 2;
    }
    let bin_hz = SAMPLE_RATE as f64 / size as f64;
    let (fmin, db_min) = (if log_freq { 20.0 } else { 0.0 }, -100.0);
    let height = TOP + LANE * 1.5 + BOTTOM;
    let plot_w = W - LEFT - RIGHT;
    let plot_h = LANE * 1.5;
    let x_of = |f: f64| {
        let u = if log_freq {
            (f / fmin).ln() / (fmax / fmin).ln()
        } else {
            (f - fmin) / (fmax - fmin)
        };
        LEFT + plot_w * u
    };
    let y_of = |db: f64| TOP + plot_h * (db.clamp(db_min, 0.0) / db_min);

    let mut svg = svg_open(height, "sg-spectrum", "");
    for db in (0..=100).step_by(20).map(|d| (0 - d) as f64) {
        let y = y_of(db);
        let _ = write!(
            svg,
            r#"<line class="grid" x1="{LEFT}" x2="{}" y1="{y:.1}" y2="{y:.1}"/>"#,
            W - RIGHT
        );
        let _ = write!(
            svg,
            r#"<text class="label" x="{}" y="{:.1}" text-anchor="end">{db}</text>"#,
            LEFT - 5.0,
            y + 3.5
        );
    }
    let ticks = if log_freq {
        [20.0, 50.0, 100.0, 200.0, 500.0, 1e3, 2e3, 5e3, 1e4, 2e4]
            .into_iter()
            .filter(|&f| f <= fmax)
            .collect()
    } else {
        nice_ticks(0.0, fmax, 8)
    };
    for f in ticks {
        let x = x_of(f);
        let _ = write!(
            svg,
            r#"<line class="grid" x1="{x:.1}" x2="{x:.1}" y1="{TOP}" y2="{}"/>"#,
            TOP + plot_h
        );
        let label = if f >= 1000.0 {
            format!("{}k", f / 1000.0)
        } else {
            format!("{f}")
        };
        let _ = write!(
            svg,
            r#"<text class="label" x="{x:.1}" y="{}" text-anchor="middle">{label}</text>"#,
            TOP + plot_h + 16.0
        );
    }
    let _ = write!(
        svg,
        r#"<text class="label unit" x="{}" y="{}" text-anchor="end">dB over Hz</text>"#,
        W - RIGHT - 4.0,
        TOP + 12.0
    );

    // Keep the loudest bin per pixel column so narrow peaks survive decimation.
    let mut columns: Vec<Option<f64>> = vec![None; plot_w as usize + 1];
    for (k, p) in power.iter().enumerate().skip(1) {
        let f = k as f64 * bin_hz;
        if f < fmin || f > fmax {
            continue;
        }
        let db = 10.0 * (p / count as f64).max(1e-20).log10();
        let col = (x_of(f) - LEFT) as usize;
        let slot = &mut columns[col.min(plot_w as usize)];
        *slot = Some(slot.map_or(db, |d: f64| d.max(db)));
    }
    svg.push_str(r#"<polyline class="trace" points=""#);
    for (col, db) in columns.iter().enumerate() {
        if let Some(db) = db {
            let _ = write!(svg, "{:.1},{:.1} ", LEFT + col as f64, y_of(*db));
        }
    }
    svg.push_str(r#""/></svg>"#);
    svg
}

fn svg_open(height: f64, class: &str, extra: &str) -> String {
    format!(
        r#"<svg class="{class}" viewBox="0 0 {W} {height}" preserveAspectRatio="xMidYMid meet" xmlns="http://www.w3.org/2000/svg" role="img"{extra}>"#
    )
}

fn nice_ticks(a: f64, b: f64, target: usize) -> Vec<f64> {
    let range = (b - a).max(1e-12);
    let raw = range / target as f64;
    let mag = 10f64.powf(raw.log10().floor());
    let step = [1.0, 2.0, 5.0, 10.0]
        .into_iter()
        .map(|m| m * mag)
        .find(|s| range / s <= target as f64)
        .unwrap();
    let first = (a / step).ceil() as i64;
    let last = (b / step + 1e-9).floor() as i64;
    (first..=last).map(|i| i as f64 * step).collect()
}

fn time_label(t: f64, span: f64) -> String {
    let trim = |s: String| s.trim_end_matches('0').trim_end_matches('.').to_owned();
    if span < 0.5 {
        format!("{} ms", trim(format!("{:.3}", t * 1000.0)))
    } else {
        format!("{} s", trim(format!("{t:.3}")))
    }
}

// ---------------------------------------------------------------------------------------------
// Spectrogram: time left to right, frequency on a log scale from 20 Hz (bottom) to 20 kHz (top),
// level from -100 dB (black) to 0 dB (pale yellow) in a magma-like palette.

pub const SPECTROGRAM_WIDTH: usize = 1200;
pub const SPECTROGRAM_HEIGHT: usize = 320;

/// The spectrogram as RGBA pixels, row by row from the top.
pub fn spectrogram_rgba(frames: &[Frame]) -> Vec<u8> {
    let (w, h) = (SPECTROGRAM_WIDTH, SPECTROGRAM_HEIGHT);
    // The window trades time for frequency resolution: about 1/40 of the render, between 21 ms
    // (sharp clicks in a short clip) and 170 ms (resolved bass in a long piece).
    let target = (frames.len() / 40).max(1);
    let n = (1usize << target.ilog2()).clamp(1024, 8192);
    let mono = frames
        .iter()
        .map(|f| f.iter().sum::<Sample>() / CHANNELS as Sample)
        .collect::<Vec<_>>();
    let hann = (0..n)
        .map(|i| 0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / n as f64).cos())
        .collect::<Vec<_>>();
    // A full-scale sine reads 0 dB.
    let gain = 2.0 / hann.iter().sum::<f64>();
    let bin_hz = SAMPLE_RATE as f64 / n as f64;
    let (fmin, fmax) = (20.0_f64, 20_000.0_f64);
    // Bin range of each pixel row, top row first.
    let rows = (0..h)
        .map(|r| {
            let edge = |k: f64| fmin * (fmax / fmin).powf(k / h as f64);
            let (lo, hi) = (edge((h - 1 - r) as f64), edge((h - r) as f64));
            (
                (lo / bin_hz) as usize,
                ((hi / bin_hz) as usize).max((lo / bin_hz) as usize),
            )
        })
        .collect::<Vec<_>>();

    let fft = FftPlanner::new().plan_fft_forward(n);
    let mut buffer = vec![Complex::new(0.0, 0.0); n];
    let mut pixels = vec![0u8; w * h * 4];
    for col in 0..w {
        let centre = (col as f64 + 0.5) * mono.len() as f64 / w as f64;
        let start = centre as isize - (n / 2) as isize;
        for (i, slot) in buffer.iter_mut().enumerate() {
            let x = usize::try_from(start + i as isize)
                .ok()
                .and_then(|j| mono.get(j))
                .copied()
                .unwrap_or(0.0);
            *slot = Complex::new(x * hann[i], 0.0);
        }
        fft.process(&mut buffer);
        for (r, &(lo, hi)) in rows.iter().enumerate() {
            let peak = buffer[lo.min(n / 2)..=hi.min(n / 2)]
                .iter()
                .map(|x| x.norm())
                .fold(0.0, f64::max);
            let db = 20.0 * (peak * gain).max(1e-10).log10();
            let [red, green, blue] = magma(((db + 100.0) / 100.0).clamp(0.0, 1.0));
            pixels[(r * w + col) * 4..][..4].copy_from_slice(&[red, green, blue, 255]);
        }
    }
    pixels
}

fn magma(t: f64) -> [u8; 3] {
    const STOPS: [[f64; 3]; 9] = [
        [0.0, 0.0, 4.0],
        [28.0, 16.0, 68.0],
        [79.0, 18.0, 123.0],
        [129.0, 37.0, 129.0],
        [181.0, 54.0, 122.0],
        [229.0, 80.0, 100.0],
        [251.0, 135.0, 97.0],
        [254.0, 194.0, 135.0],
        [252.0, 253.0, 191.0],
    ];
    let x = t * (STOPS.len() - 1) as f64;
    let i = (x as usize).min(STOPS.len() - 2);
    let u = x - i as f64;
    std::array::from_fn(|c| (STOPS[i][c] + (STOPS[i + 1][c] - STOPS[i][c]) * u).round() as u8)
}

// ---------------------------------------------------------------------------------------------
// WebAssembly interface for the book's live editing (theme/figures-worker.js): write a program
// into `br_input`, call `br_render`, then ask for figures; each returns the byte length of its
// result at `br_output` (UTF-8 SVG, or RGBA for the spectrogram).

#[cfg(target_arch = "wasm32")]
mod wasm {
    use super::*;
    use std::cell::RefCell;

    #[derive(Default)]
    struct State {
        input: Vec<u8>,
        frames: Vec<Frame>,
        output: Vec<u8>,
    }

    thread_local! {
        static STATE: RefCell<State> = RefCell::default();
    }

    fn with<R>(f: impl FnOnce(&mut State) -> R) -> R {
        STATE.with(|state| f(&mut state.borrow_mut()))
    }

    fn put(state: &mut State, bytes: Vec<u8>) -> usize {
        state.output = bytes;
        state.output.len()
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn br_input(len: usize) -> *mut u8 {
        with(|s| {
            s.input.resize(len, 0);
            s.input.as_mut_ptr()
        })
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn br_output() -> *const u8 {
        with(|s| s.output.as_ptr())
    }

    /// Render the program in `br_input`; returns the number of frames.
    #[unsafe(no_mangle)]
    pub extern "C" fn br_render(seconds: f64) -> usize {
        with(|s| {
            s.frames = render(&String::from_utf8_lossy(&s.input), seconds);
            s.frames.len()
        })
    }

    /// Fade out the last `seconds` of the current render.
    #[unsafe(no_mangle)]
    pub extern "C" fn br_fade(seconds: f64) {
        with(|s| fade_out(&mut s.frames, seconds))
    }

    /// Waveform of the `from`..`to` window in seconds; NaN means the start or the end.
    #[unsafe(no_mangle)]
    pub extern "C" fn br_wave(from: f64, to: f64) -> usize {
        let some = |x: f64| (!x.is_nan()).then_some(x);
        with(|s| {
            let range = window(&s.frames, some(from), some(to));
            let svg = wave_svg(&s.frames[range], some(from).unwrap_or(0.0));
            put(s, svg.into_bytes())
        })
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn br_spectrum(fmax: f64, log_freq: u32) -> usize {
        with(|s| {
            let svg = spectrum_svg(&s.frames, fmax, log_freq != 0);
            put(s, svg.into_bytes())
        })
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn br_overview() -> usize {
        with(|s| {
            let svg = overview_svg(&s.frames);
            put(s, svg.into_bytes())
        })
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn br_spectrogram() -> usize {
        with(|s| {
            let pixels = spectrogram_rgba(&s.frames);
            put(s, pixels)
        })
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn br_spectrogram_width() -> usize {
        SPECTROGRAM_WIDTH
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn br_spectrogram_height() -> usize {
        SPECTROGRAM_HEIGHT
    }

    // getrandom (via rand) uses its "custom" backend on wasm (see
    // .cargo/config.toml). Unseeded noise in a figure is arbitrary anyway, so a fixed SplitMix64
    // stream is enough and keeps figures stable between commits.
    static RNG: std::sync::atomic::AtomicU64 =
        std::sync::atomic::AtomicU64::new(0x853c_49e6_748f_ea9b);

    #[unsafe(no_mangle)]
    unsafe extern "Rust" fn __getrandom_v03_custom(
        dest: *mut u8,
        len: usize,
    ) -> Result<(), getrandom::Error> {
        use std::sync::atomic::Ordering;
        let dest = unsafe { std::slice::from_raw_parts_mut(dest, len) };
        for chunk in dest.chunks_mut(8) {
            let mut z = RNG
                .fetch_add(0x9E37_79B9_7F4A_7C15, Ordering::Relaxed)
                .wrapping_add(0x9E37_79B9_7F4A_7C15);
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            z ^= z >> 31;
            chunk.copy_from_slice(&z.to_le_bytes()[..chunk.len()]);
        }
        Ok(())
    }
}
