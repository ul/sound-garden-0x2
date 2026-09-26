//! Renders one Sound Garden program for the book: audio, waveform and spectrum figures, and
//! level statistics. Called by the Asciidoctor extension (`book/ext/sound_garden.rb`) for every
//! `sound::` block whose render is not cached yet.
//!
//! ```text
//! book_render PROGRAM|- --seconds S [--audio OUT.mp3] [--wav OUT.wav] [--overview OUT.svg]
//!     [--wave OUT.svg] [--spectrum OUT.svg] [--spectrogram OUT.png]
//!     [--from T] [--to T] [--fmax HZ] [--fscale log|lin]
//! ```
//!
//! Prints a JSON object with per-channel peak/rms/clipped counts and compiler warnings to stdout.

use anyhow::{Context as _, Result, anyhow, bail};
use audio_program::{Context, TextOp, compile_program};
use audio_vm::{CHANNELS, Sample, VM};
use rustfft::{FftPlanner, num_complex::Complex};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

const SAMPLE_RATE: u32 = 48_000;

fn main() -> Result<()> {
    log::set_logger(&WARNINGS).map_err(|e| anyhow!("{e}"))?;
    log::set_max_level(log::LevelFilter::Warn);

    let args = Args::parse()?;
    let text = match &args.program {
        None => std::io::read_to_string(std::io::stdin())?,
        Some(program) => {
            let text = std::fs::read_to_string(program)
                .with_context(|| format!("Failed to read {}", program.display()))?;
            // Relative table/sample paths in a program resolve next to the program file.
            if let Some(dir) = program.parent() {
                std::env::set_current_dir(dir)?;
            }
            text
        }
    };

    let frames = render(&text, args.seconds);
    let stats = Stats::of(&frames);

    let tmp_wav;
    let wav = match &args.wav {
        Some(path) => path.clone(),
        None => {
            tmp_wav = std::env::temp_dir().join(format!("book_render-{}.wav", std::process::id()));
            tmp_wav.clone()
        }
    };
    if args.wav.is_some() || args.audio.is_some() || args.spectrogram.is_some() {
        write_wav(&wav, &frames)?;
    }
    if let Some(out) = &args.audio {
        ffmpeg(&[
            "-i",
            s(&wav)?,
            "-codec:a",
            "libmp3lame",
            "-q:a",
            "2",
            s(out)?,
        ])?;
    }
    if let Some(out) = &args.spectrogram {
        let filter = "showspectrumpic=s=1200x320:legend=0:mode=combined:fscale=log:scale=log:color=magma:drange=100";
        ffmpeg(&["-i", s(&wav)?, "-lavfi", filter, s(out)?])?;
    }
    if args.wav.is_none() {
        let _ = std::fs::remove_file(&wav);
    }

    if let Some(out) = &args.overview {
        std::fs::write(out, overview_svg(&frames))?;
    }
    let window = window(&frames, args.from, args.to);
    if let Some(out) = &args.wave {
        std::fs::write(out, wave_svg(&frames[window], args.from.unwrap_or(0.0)))?;
    }
    if let Some(out) = &args.spectrum {
        // The spectrum averages over the whole render: frequency resolution needs duration, and
        // the waveform window is usually a few periods, far too short to resolve harmonics.
        std::fs::write(out, spectrum_svg(&frames, args.fmax, args.log_freq))?;
    }

    println!("{}", stats.json(&WARNINGS.0.lock().unwrap()));
    Ok(())
}

struct Args {
    /// None reads the program from stdin.
    program: Option<PathBuf>,
    seconds: f64,
    audio: Option<PathBuf>,
    wav: Option<PathBuf>,
    overview: Option<PathBuf>,
    wave: Option<PathBuf>,
    spectrum: Option<PathBuf>,
    spectrogram: Option<PathBuf>,
    from: Option<f64>,
    to: Option<f64>,
    fmax: f64,
    log_freq: bool,
}

impl Args {
    fn parse() -> Result<Self> {
        let mut program = None;
        let mut args = Args {
            program: None,
            seconds: 4.0,
            audio: None,
            wav: None,
            overview: None,
            wave: None,
            spectrum: None,
            spectrogram: None,
            from: None,
            to: None,
            fmax: 20_000.0,
            log_freq: true,
        };
        let mut it = std::env::args().skip(1);
        while let Some(arg) = it.next() {
            let mut value = || it.next().ok_or_else(|| anyhow!("{arg} needs a value"));
            // Output paths are made absolute because we chdir next to the program before compiling.
            let path = |v: String| std::path::absolute(v);
            match arg.as_str() {
                "--seconds" => args.seconds = value()?.parse()?,
                "--audio" => args.audio = Some(path(value()?)?),
                "--wav" => args.wav = Some(path(value()?)?),
                "--overview" => args.overview = Some(path(value()?)?),
                "--wave" => args.wave = Some(path(value()?)?),
                "--spectrum" => args.spectrum = Some(path(value()?)?),
                "--spectrogram" => args.spectrogram = Some(path(value()?)?),
                "--from" => args.from = Some(value()?.parse()?),
                "--to" => args.to = Some(value()?.parse()?),
                "--fmax" => args.fmax = value()?.parse()?,
                "--fscale" => args.log_freq = value()? != "lin",
                "-" if program.is_none() => program = Some(None),
                _ if program.is_none() && !arg.starts_with("--") => {
                    program = Some(Some(path(arg)?))
                }
                _ => bail!("Unexpected argument {arg}"),
            }
        }
        args.program = program.ok_or_else(|| anyhow!("Please provide a program path or -."))?;
        Ok(args)
    }
}

fn render(text: &str, seconds: f64) -> Vec<[Sample; CHANNELS]> {
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

fn window(
    frames: &[[Sample; CHANNELS]],
    from: Option<f64>,
    to: Option<f64>,
) -> std::ops::Range<usize> {
    let index = |t: f64| ((t * SAMPLE_RATE as f64).round() as usize).min(frames.len());
    let start = from.map_or(0, index);
    let end = to.map_or(frames.len(), index).max(start);
    start..end
}

fn write_wav(path: &Path, frames: &[[Sample; CHANNELS]]) -> Result<()> {
    let spec = hound::WavSpec {
        channels: CHANNELS as _,
        sample_rate: SAMPLE_RATE,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut writer = hound::WavWriter::create(path, spec)?;
    for frame in frames {
        for &sample in frame {
            writer.write_sample(sample as f32)?;
        }
    }
    writer.finalize()?;
    Ok(())
}

fn ffmpeg(args: &[&str]) -> Result<()> {
    let output = Command::new("ffmpeg")
        .args(["-hide_banner", "-loglevel", "error", "-y"])
        .args(args)
        .output()
        .context("Failed to run ffmpeg; is it installed?")?;
    if !output.status.success() {
        bail!("ffmpeg failed: {}", String::from_utf8_lossy(&output.stderr));
    }
    Ok(())
}

fn s(path: &Path) -> Result<&str> {
    path.to_str()
        .ok_or_else(|| anyhow!("Non-UTF-8 path {}", path.display()))
}

// ---------------------------------------------------------------------------------------------
// Statistics and compiler warnings

struct Warnings(Mutex<Vec<String>>);

static WARNINGS: Warnings = Warnings(Mutex::new(Vec::new()));

impl log::Log for Warnings {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::Level::Warn
    }

    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            self.0.lock().unwrap().push(record.args().to_string());
        }
    }

    fn flush(&self) {}
}

struct Stats {
    peak: [Sample; CHANNELS],
    rms: [Sample; CHANNELS],
    clipped: [usize; CHANNELS],
}

impl Stats {
    fn of(frames: &[[Sample; CHANNELS]]) -> Self {
        let mut stats = Stats {
            peak: [0.0; CHANNELS],
            rms: [0.0; CHANNELS],
            clipped: [0; CHANNELS],
        };
        for frame in frames {
            for (c, &x) in frame.iter().enumerate() {
                stats.peak[c] = stats.peak[c].max(x.abs());
                stats.rms[c] += x * x;
                stats.clipped[c] += (x.abs() > 1.0) as usize;
            }
        }
        for rms in &mut stats.rms {
            *rms = (*rms / frames.len().max(1) as Sample).sqrt();
        }
        stats
    }

    fn json(&self, warnings: &[String]) -> String {
        let list = |xs: &[String]| xs.join(",");
        let nums = |xs: &[Sample]| list(&xs.iter().map(|x| format!("{x:.4}")).collect::<Vec<_>>());
        let warnings = warnings
            .iter()
            .map(|w| format!("\"{}\"", w.replace('\\', "\\\\").replace('"', "\\\"")))
            .collect::<Vec<_>>();
        format!(
            "{{\"peak\":[{}],\"rms\":[{}],\"clipped\":[{}],\"warnings\":[{}]}}",
            nums(&self.peak),
            nums(&self.rms),
            list(
                &self
                    .clipped
                    .iter()
                    .map(|x| x.to_string())
                    .collect::<Vec<_>>()
            ),
            list(&warnings),
        )
    }
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

fn wave_svg(frames: &[[Sample; CHANNELS]], t0: f64) -> String {
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
fn overview_svg(frames: &[[Sample; CHANNELS]]) -> String {
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

fn spectrum_svg(frames: &[[Sample; CHANNELS]], fmax: f64, log_freq: bool) -> String {
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
