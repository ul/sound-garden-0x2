//! Renders one Sound Garden program for the book: audio, waveform and spectrum figures, and
//! level statistics. Called by the Asciidoctor extension (`book/ext/sound_garden.rb`) for every
//! `sound::` block whose render is not cached yet.
//!
//! ```text
//! book_render PROGRAM|- --seconds S [--fade S] [--audio OUT.mp3] [--wav OUT.wav] [--overview OUT.svg]
//!     [--wave OUT.svg] [--spectrum OUT.svg] [--spectrogram OUT.png]
//!     [--from T] [--to T] [--fmax HZ] [--fscale log|lin]
//! ```
//!
//! Prints a JSON object with per-channel peak/rms/clipped counts and compiler warnings to stdout.

use anyhow::{Context as _, Result, anyhow, bail};
use audio_vm::{CHANNELS, Sample};
use book_render::{
    SAMPLE_RATE, SPECTROGRAM_HEIGHT, SPECTROGRAM_WIDTH, fade_out, overview_svg, render,
    spectrogram_rgba, spectrum_svg, wave_svg, window,
};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

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

    let mut frames = render(&text, args.seconds);
    if let Some(fade) = args.fade {
        fade_out(&mut frames, fade);
    }
    let stats = Stats::of(&frames);

    let tmp_wav;
    let wav = match &args.wav {
        Some(path) => path.clone(),
        None => {
            tmp_wav = std::env::temp_dir().join(format!("book_render-{}.wav", std::process::id()));
            tmp_wav.clone()
        }
    };
    if args.wav.is_some() || args.audio.is_some() {
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
        // Drawn by the library, as in the browser; ffmpeg only encodes the pixels as PNG.
        let size = format!("{SPECTROGRAM_WIDTH}x{SPECTROGRAM_HEIGHT}");
        let args = [
            "-f",
            "rawvideo",
            "-pix_fmt",
            "rgba",
            "-s",
            &size,
            "-i",
            "-",
            s(out)?,
        ];
        ffmpeg_with_input(&args, &spectrogram_rgba(&frames))?;
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
    fade: Option<f64>,
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
            fade: None,
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
                "--fade" => args.fade = Some(value()?.parse()?),
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
    ffmpeg_with_input(args, &[])
}

fn ffmpeg_with_input(args: &[&str], input: &[u8]) -> Result<()> {
    use std::io::Write as _;
    let mut child = Command::new("ffmpeg")
        .args(["-hide_banner", "-loglevel", "error", "-y"])
        .args(args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .context("Failed to run ffmpeg; is it installed?")?;
    child.stdin.take().unwrap().write_all(input)?;
    let output = child.wait_with_output()?;
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
