//! Performance feedback shown in the GUI: engine status and output meters.
//!
//! Pure state and formatting, kept apart from drawing so it can be tested.
#[cfg(target_arch = "wasm32")]
use crate::browser::{Meters, MidiMessage};
use audio_ops::MidiEventKind;
use audio_program::Diagnostic;
#[cfg(not(target_arch = "wasm32"))]
use audio_server::{Meters, MidiMessage};
use std::{collections::HashMap, sync::Arc};

/// How long a load peak, level peak, clip light or dropout warning stays up.
const HOLD_SECONDS: f64 = 1.5;
/// Level meters show -60..0 dBFS.
pub const METER_FLOOR_DB: f64 = -60.0;

/// Accumulates [`Meters`] snapshots (arriving every ~10 ms) into what the
/// modeline shows: held peaks, a latched clip light and a dropout warning.
#[derive(Default)]
pub struct MeterDisplay {
    sample_rate: u32,
    buffer_frames: u32,
    load: Held,
    #[cfg(target_arch = "wasm32")]
    load_available: bool,
    #[cfg(target_arch = "wasm32")]
    dropouts_estimated: bool,
    peak: [Held; 2],
    rms: [f64; 2],
    clip_until: f64,
    dropouts: u64,
    dropout_until: f64,
    received: bool,
    midi_device: Option<Arc<str>>,
    last_midi: Option<MidiMessage>,
}

/// Longest MIDI device name shown before it is shortened with an ellipsis.
const MIDI_NAME_CHARS: usize = 18;

/// A value that holds its peak for [`HOLD_SECONDS`], then follows the input.
#[derive(Default, Clone, Copy)]
struct Held {
    value: f64,
    until: f64,
}

impl Held {
    fn update(&mut self, x: f64, time: f64) {
        if x >= self.value || time >= self.until {
            self.value = x;
            self.until = time + HOLD_SECONDS;
        }
    }
}

impl MeterDisplay {
    pub fn update(&mut self, meters: &Meters, midi_device: Option<&Arc<str>>, time: f64) {
        self.received = true;
        self.midi_device = midi_device.cloned();
        if let Some((_, message)) = meters.last_midi {
            self.last_midi = Some(message);
        }
        self.sample_rate = meters.sample_rate;
        self.buffer_frames = meters.buffer_frames;
        #[cfg(not(target_arch = "wasm32"))]
        self.load.update(meters.load, time);
        #[cfg(target_arch = "wasm32")]
        {
            self.load_available = meters.load_available;
            self.dropouts_estimated = meters.dropouts_estimated;
            if meters.load_available {
                self.load.update(meters.load, time);
            }
        }
        for channel in 0..2 {
            self.peak[channel].update(meters.peak[channel], time);
            self.rms[channel] = meters.rms[channel];
        }
        if meters.clipped > 0 {
            self.clip_until = time + HOLD_SECONDS;
        }
        if meters.dropouts > self.dropouts {
            self.dropout_until = time + HOLD_SECONDS;
        }
        self.dropouts = meters.dropouts;
    }

    /// Whether anything is still animating (held peaks falling, lights on),
    /// so the GUI knows to keep repainting.
    pub fn is_animating(&self, time: f64) -> bool {
        time < self.clip_until
            || time < self.dropout_until
            || self.peak.iter().any(|peak| peak.value > 0.0)
    }

    pub fn clipping(&self, time: f64) -> bool {
        time < self.clip_until
    }

    pub fn dropout_warning(&self, time: f64) -> bool {
        time < self.dropout_until
    }

    pub fn sample_rate(&self) -> Option<f64> {
        (self.sample_rate > 0).then_some(self.sample_rate as f64)
    }

    /// e.g. `midi: Keystation · cc 74 = 0.62`; None without a MIDI input.
    pub fn midi_status(&self) -> Option<String> {
        let device = self.midi_device.as_ref()?;
        let mut name = device.chars().take(MIDI_NAME_CHARS).collect::<String>();
        if device.chars().count() > MIDI_NAME_CHARS {
            name.truncate(name.trim_end().len());
            name.push('…');
        }
        Some(match self.last_midi {
            Some(message) => format!("midi: {name} · {}", format_midi(message)),
            None => format!("midi: {name}"),
        })
    }

    /// e.g. `48k · 128 · 2.7ms · dsp 12%`, plus `· 3 dropouts` once any occur.
    pub fn status(&self) -> Option<String> {
        if !self.received || self.sample_rate == 0 {
            return None;
        }
        let mut status = format!(
            "{} · {} · {:.1}ms",
            format_rate(self.sample_rate),
            self.buffer_frames,
            1000.0 * self.buffer_frames as f64 / self.sample_rate as f64,
        );
        #[cfg(not(target_arch = "wasm32"))]
        status.push_str(&format!(" · dsp {:.0}%", 100.0 * self.load.value));
        #[cfg(target_arch = "wasm32")]
        if self.load_available {
            status.push_str(&format!(" · dsp {:.0}%", 100.0 * self.load.value));
        } else {
            status.push_str(" · dsp N/A");
        }
        #[cfg(not(target_arch = "wasm32"))]
        match self.dropouts {
            0 => {}
            1 => status.push_str(" · 1 dropout"),
            n => status.push_str(&format!(" · {n} dropouts")),
        }
        #[cfg(target_arch = "wasm32")]
        if self.dropouts > 0 {
            let label = if self.dropouts_estimated {
                "estimated processing overruns"
            } else {
                "processing overruns"
            };
            status.push_str(&format!(" · {} {label}", self.dropouts));
        }
        Some(status)
    }

    /// Per channel: (rms, held peak) as 0..1 positions on a -60..0 dB scale.
    pub fn levels(&self) -> [(f32, f32); 2] {
        std::array::from_fn(|channel| {
            (
                meter_position(self.rms[channel]),
                meter_position(self.peak[channel].value),
            )
        })
    }
}

/// Compile warnings grouped by the node that caused them, plus the number
/// that couldn't be tied to a node.
#[derive(Default)]
pub struct NodeDiagnostics {
    pub generation: u64,
    pub by_node: HashMap<u64, Vec<String>>,
    pub unattributed: usize,
}

impl NodeDiagnostics {
    pub fn new(generation: u64, items: &[Diagnostic]) -> Self {
        let mut diagnostics = NodeDiagnostics {
            generation,
            ..Default::default()
        };
        for item in items {
            match item.id {
                Some(id) => diagnostics
                    .by_node
                    .entry(id)
                    .or_default()
                    .push(item.message.clone()),
                None => diagnostics.unattributed += 1,
            }
        }
        diagnostics
    }

    pub fn count(&self) -> usize {
        self.by_node.values().map(Vec::len).sum::<usize>() + self.unattributed
    }

    /// e.g. `2 warnings`; None when the program compiled cleanly.
    pub fn summary(&self) -> Option<String> {
        match self.count() {
            0 => None,
            1 => Some("1 warning".to_owned()),
            n => Some(format!("{n} warnings")),
        }
    }
}

/// How a MIDI message reads in the modeline: what to type to use it.
pub fn format_midi(message: MidiMessage) -> String {
    match message {
        MidiMessage::Note(event) => match event.kind {
            MidiEventKind::NoteOn => format!("{} {:.2}", note_name(event.note), event.velocity),
            MidiEventKind::NoteOff => format!("{} off", note_name(event.note)),
        },
        MidiMessage::Controller { controller, value } => format!("cc {controller} = {value:.2}"),
        MidiMessage::Bend(value) => format!("bend {value:+.2}"),
    }
}

/// Scientific pitch notation with MIDI 60 = C4, matching the language's
/// note constants.
pub fn note_name(note: u8) -> String {
    const NAMES: [&str; 12] = [
        "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
    ];
    format!("{}{}", NAMES[note as usize % 12], note as i32 / 12 - 1)
}

fn format_rate(sample_rate: u32) -> String {
    if sample_rate.is_multiple_of(1000) {
        format!("{}k", sample_rate / 1000)
    } else {
        format!("{:.1}k", sample_rate as f64 / 1000.0)
    }
}

/// Map an amplitude to 0..1 on a dB scale from [`METER_FLOOR_DB`] to 0 dBFS.
pub fn meter_position(amplitude: f64) -> f32 {
    if amplitude.is_nan() {
        return 1.0;
    }
    if amplitude <= 0.0 {
        return 0.0;
    }
    let db = 20.0 * amplitude.log10();
    ((db - METER_FLOOR_DB) / -METER_FLOOR_DB).clamp(0.0, 1.0) as f32
}

/// Start of the latest window of `window` samples that begins on a rising
/// crossing of the signal's midpoint, so periodic waveforms stand still from
/// one repaint to the next. None when the signal doesn't repeat within the
/// capture (at least two crossings), e.g. slow LFOs and envelopes, which read
/// better as a rolling trend.
pub fn find_trigger(samples: &[f64], window: usize) -> Option<usize> {
    if window == 0 || samples.len() < window + 2 {
        return None;
    }
    let (min, max) = samples
        .iter()
        .filter(|x| x.is_finite())
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), &x| {
            (lo.min(x), hi.max(x))
        });
    let range = max - min;
    // Also catches a capture with no finite samples (range is -inf).
    if range <= 1e-9 {
        return None;
    }
    let level = 0.5 * (min + max);
    // Hysteresis so noise around the level doesn't retrigger.
    let arm_below = level - 0.05 * range;
    let latest_start = samples.len() - window;
    let mut armed = false;
    let mut crossings = 0;
    let mut found = None;
    for (index, &x) in samples.iter().enumerate() {
        if x < arm_below {
            armed = true;
        } else if armed && x >= level {
            armed = false;
            crossings += 1;
            if index <= latest_start {
                found = Some(index);
            }
        }
    }
    if crossings >= 2 { found } else { None }
}

/// Magnitude spectrum in dBFS: a full-scale sine reads 0 dB.
pub struct Spectrum {
    fft: Arc<dyn rustfft::Fft<f64>>,
    window: Vec<f64>,
    buffer: Vec<rustfft::num_complex::Complex<f64>>,
    scratch: Vec<rustfft::num_complex::Complex<f64>>,
}

pub const SPECTRUM_SIZE: usize = 4096;
pub const SPECTRUM_FLOOR_DB: f64 = -96.0;

impl Default for Spectrum {
    fn default() -> Self {
        let fft = rustfft::FftPlanner::new().plan_fft_forward(SPECTRUM_SIZE);
        let scratch = vec![Default::default(); fft.get_inplace_scratch_len()];
        let window = (0..SPECTRUM_SIZE)
            .map(|i| 0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / SPECTRUM_SIZE as f64).cos())
            .collect();
        Spectrum {
            fft,
            window,
            buffer: vec![Default::default(); SPECTRUM_SIZE],
            scratch,
        }
    }
}

impl Spectrum {
    /// dB per bin (0..=N/2) of the last `SPECTRUM_SIZE` samples, or None if
    /// fewer were captured.
    pub fn analyse(&mut self, samples: &[f64]) -> Option<Vec<f64>> {
        let samples = samples.get(samples.len().checked_sub(SPECTRUM_SIZE)?..)?;
        for ((bin, &x), &w) in self.buffer.iter_mut().zip(samples).zip(&self.window) {
            *bin = (if x.is_finite() { x * w } else { 0.0 }).into();
        }
        self.fft
            .process_with_scratch(&mut self.buffer, &mut self.scratch);
        // A sine of amplitude A gives |X| = A * sum(w) / 2 at its bin.
        let reference = self.window.iter().sum::<f64>() / 2.0;
        Some(
            self.buffer[..=SPECTRUM_SIZE / 2]
                .iter()
                .map(|bin| (20.0 * (bin.norm() / reference).log10()).max(SPECTRUM_FLOOR_DB))
                .collect(),
        )
    }
}

/// Resample a spectrum onto `columns` columns spaced logarithmically from
/// `low` to `high` Hz. Where a column spans several bins it takes the loudest;
/// where it is narrower than a bin (the low end) it interpolates between the
/// neighbouring bins, so the curve doesn't turn into stair steps.
pub fn spectrum_columns(
    db: &[f64],
    sample_rate: f64,
    columns: usize,
    low: f64,
    high: f64,
) -> Vec<f64> {
    let bin_hz = sample_rate / (2.0 * (db.len() - 1) as f64);
    let ratio = (high / low).ln();
    let last_bin = (db.len() - 1) as f64;
    let bin_at =
        |column: f64| (low * (ratio * column / columns as f64).exp() / bin_hz).min(last_bin);
    (0..columns)
        .map(|column| {
            let start = bin_at(column as f64);
            let end = bin_at(column as f64 + 1.0);
            if end - start >= 1.0 {
                let first = start.round() as usize;
                let last = (end.round() as usize).clamp(first + 1, db.len());
                db[first..last]
                    .iter()
                    .copied()
                    .fold(SPECTRUM_FLOOR_DB, f64::max)
            } else {
                let centre = 0.5 * (start + end);
                let below = centre.floor() as usize;
                let above = (below + 1).min(db.len() - 1);
                let t = centre - below as f64;
                db[below] + (db[above] - db[below]) * t
            }
        })
        .collect()
}

/// Horizontal position (0..1) of `frequency` on the log axis from `low` to
/// `high` Hz.
pub fn log_position(frequency: f64, low: f64, high: f64) -> f64 {
    (frequency / low).ln() / (high / low).ln()
}

/// The node-under-cursor readout: current value and range over the recent
/// capture.
pub fn readout(samples: &[f64]) -> Option<(f64, f64, f64)> {
    let &current = samples.last()?;
    let (min, max) = samples
        .iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), &x| {
            (lo.min(x), hi.max(x))
        });
    Some((current, min, max))
}

/// Four significant digits, switching to exponent form outside 1e-3..1e6.
pub fn format_value(x: f64) -> String {
    if !x.is_finite() {
        return format!("{x}");
    }
    let magnitude = x.abs();
    if magnitude != 0.0 && !(1e-3..1e6).contains(&magnitude) {
        return format!("{x:.3e}");
    }
    let decimals = if magnitude == 0.0 {
        0
    } else {
        (3 - magnitude.log10().floor() as i32).max(0) as usize
    };
    format!("{x:.decimals$}")
}

/// Width of every `format_fixed` result: a sign column and six characters.
pub const FIXED_WIDTH: usize = 7;

/// A value in exactly `FIXED_WIDTH` characters, right-aligned, for readouts that
/// change while you watch them: as many digits as fit in six characters (`0.2500`,
/// `12.345`, `1234.5`), or exponent form outside 1e-3..1e6 (`1.5e-5`), with room
/// left for a minus sign so a value crossing zero doesn't shift.
pub fn format_fixed(x: f64) -> String {
    if !x.is_finite() {
        return format!("{x:>FIXED_WIDTH$}");
    }
    let sign = if x < 0.0 { "-" } else { "" };
    format!(
        "{:>FIXED_WIDTH$}",
        format!("{sign}{}", fixed_magnitude(x.abs()))
    )
}

fn fixed_magnitude(magnitude: f64) -> String {
    let width = FIXED_WIDTH - 1;
    if magnitude == 0.0 || (1e-3..1e6).contains(&magnitude) {
        let integer_digits = if magnitude < 1.0 {
            1
        } else {
            magnitude.log10().floor() as usize + 1
        };
        // Rounding can carry into a new digit (9.99996 -> 10.0000): retry with one fewer.
        let mut decimals = width.saturating_sub(integer_digits + 1);
        loop {
            let text = format!("{magnitude:.decimals$}");
            if text.len() <= width {
                return text;
            }
            if decimals == 0 {
                break;
            }
            decimals -= 1;
        }
    }
    let text = format!("{magnitude:.1e}");
    if text.len() <= width {
        text
    } else {
        format!("{magnitude:.0e}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    impl MeterDisplay {
        fn update_meters(&mut self, meters: &Meters, time: f64) {
            self.update(meters, None, time);
        }
    }

    fn meters(load: f64, peak: f64, clipped: u64, dropouts: u64) -> Meters {
        Meters {
            sample_rate: 48_000,
            buffer_frames: 128,
            load,
            dropouts,
            peak: [peak; 2],
            rms: [peak / 2.0; 2],
            clipped,
            last_midi: None,
        }
    }

    #[test]
    fn status_shows_latency_and_held_load() {
        let mut display = MeterDisplay::default();
        assert_eq!(display.status(), None, "nothing until the engine reports");
        display.update_meters(&meters(0.4, 0.0, 0, 0), 0.0);
        display.update_meters(&meters(0.1, 0.0, 0, 0), 0.5);
        assert_eq!(display.status().unwrap(), "48k · 128 · 2.7ms · dsp 40%");
        // After the hold the load follows the input again.
        display.update_meters(&meters(0.1, 0.0, 0, 0), 2.0);
        assert_eq!(display.status().unwrap(), "48k · 128 · 2.7ms · dsp 10%");
    }

    #[test]
    fn clip_light_and_dropout_warning_latch_then_clear() {
        let mut display = MeterDisplay::default();
        display.update_meters(&meters(0.1, 1.2, 3, 0), 10.0);
        assert!(display.clipping(10.0) && display.clipping(11.0));
        assert!(!display.clipping(11.6));

        display.update_meters(&meters(0.1, 0.1, 0, 2), 20.0);
        assert!(display.dropout_warning(20.5));
        assert!(display.status().unwrap().ends_with("· 2 dropouts"));
        display.update_meters(&meters(0.1, 0.1, 0, 2), 22.0);
        assert!(
            !display.dropout_warning(22.0),
            "count unchanged: no new warning"
        );
    }

    #[test]
    fn meter_positions_use_a_60_db_scale() {
        assert_eq!(meter_position(1.0), 1.0);
        assert_eq!(meter_position(2.0), 1.0);
        assert!(
            (meter_position(0.001) - 0.0).abs() < 1e-6,
            "-60 dB is the floor"
        );
        assert!((meter_position(10f64.powf(-30.0 / 20.0)) - 0.5).abs() < 1e-6);
        assert_eq!(meter_position(0.0), 0.0);
        assert_eq!(meter_position(f64::NAN), 1.0, "NaN reads as full scale");
        assert_eq!(meter_position(f64::INFINITY), 1.0);
    }

    #[test]
    fn midi_status_names_the_device_and_what_to_type() {
        use audio_ops::MidiEvent;
        let mut display = MeterDisplay::default();
        let mut with = |message: Option<MidiMessage>| {
            let mut m = meters(0.0, 0.0, 0, 0);
            m.last_midi = message.map(|message| (1, message));
            display.update(&m, Some(&Arc::from("Keystation 49 MK3 USB MIDI")), 0.0);
            display.midi_status().unwrap()
        };
        assert_eq!(with(None), "midi: Keystation 49 MK3…");
        assert_eq!(
            with(Some(MidiMessage::Controller {
                controller: 74,
                value: 0.625
            })),
            "midi: Keystation 49 MK3… · cc 74 = 0.62"
        );
        assert_eq!(
            with(Some(MidiMessage::Note(MidiEvent::note_on(0, 61, 0.8)))),
            "midi: Keystation 49 MK3… · C#4 0.80"
        );
        assert_eq!(
            with(Some(MidiMessage::Note(MidiEvent::note_off(0, 21)))),
            "midi: Keystation 49 MK3… · A0 off"
        );
        assert_eq!(
            with(Some(MidiMessage::Bend(-0.5))),
            "midi: Keystation 49 MK3… · bend -0.50"
        );
        // The last message stays until a new one arrives.
        assert!(with(None).ends_with("bend -0.50"));

        let mut none = MeterDisplay::default();
        none.update(&meters(0.0, 0.0, 0, 0), None, 0.0);
        assert_eq!(none.midi_status(), None);
    }

    #[test]
    fn diagnostics_group_by_node() {
        let item = |id: Option<u64>, message: &str| Diagnostic {
            id,
            message: message.to_owned(),
        };
        let diagnostics = NodeDiagnostics::new(
            3,
            &[
                item(Some(7), "Unknown token: sine-ish"),
                item(Some(7), "another"),
                item(Some(9), "bad pattern"),
                item(None, "somewhere"),
            ],
        );
        assert_eq!(
            diagnostics.by_node[&7],
            ["Unknown token: sine-ish", "another"]
        );
        assert_eq!(diagnostics.unattributed, 1);
        assert_eq!(diagnostics.summary().unwrap(), "4 warnings");
        assert_eq!(NodeDiagnostics::new(4, &[]).summary(), None);
    }

    fn sine(frequency: f64, sample_rate: f64, len: usize, amplitude: f64) -> Vec<f64> {
        (0..len)
            .map(|n| amplitude * (std::f64::consts::TAU * frequency * n as f64 / sample_rate).sin())
            .collect()
    }

    #[test]
    fn trigger_starts_periodic_windows_on_the_same_phase() {
        // 480 Hz at 48 kHz: exactly 100 samples per period.
        let samples = sine(480.0, 48_000.0, 4000, 0.8);
        let start = find_trigger(&samples, 600).unwrap();
        assert!(start + 600 <= samples.len());
        assert!(
            samples[start - 1] < 0.0 && samples[start] >= 0.0,
            "rising midpoint crossing"
        );
        // Removing a few samples from the end still lands on the same phase.
        let shifted = find_trigger(&samples[..3937], 600).unwrap();
        assert_eq!(start % 100, shifted % 100);
    }

    #[test]
    fn slow_or_flat_signals_have_no_trigger() {
        assert_eq!(find_trigger(&[0.3; 4000], 600), None);
        // Under two periods in the capture: better as a trend.
        assert_eq!(find_trigger(&sine(5.0, 48_000.0, 4000, 1.0), 600), None);
        assert_eq!(find_trigger(&[0.0; 10], 600), None);
    }

    #[test]
    fn full_scale_sine_reads_zero_db_at_its_frequency() {
        let sample_rate = 48_000.0;
        let mut spectrum = Spectrum::default();
        assert_eq!(spectrum.analyse(&[0.0; 100]), None, "needs a full window");
        // Put the tone exactly on bin 100.
        let frequency = 100.0 * sample_rate / SPECTRUM_SIZE as f64;
        let db = spectrum
            .analyse(&sine(frequency, sample_rate, SPECTRUM_SIZE, 1.0))
            .unwrap();
        assert!(db[100].abs() < 0.01, "{}", db[100]);
        assert!(db[300] < -80.0, "far from the tone: {}", db[300]);
        let half = spectrum
            .analyse(&sine(frequency, sample_rate, SPECTRUM_SIZE, 0.5))
            .unwrap();
        assert!((half[100] + 6.02).abs() < 0.01, "{}", half[100]);
    }

    #[test]
    fn log_columns_put_a_tone_where_the_axis_says() {
        let sample_rate = 48_000.0;
        let mut spectrum = Spectrum::default();
        let db = spectrum
            .analyse(&sine(1000.0, sample_rate, SPECTRUM_SIZE, 1.0))
            .unwrap();
        let columns = spectrum_columns(&db, sample_rate, 400, 20.0, 20_000.0);
        let loudest = (0..columns.len())
            .max_by(|&a, &b| columns[a].total_cmp(&columns[b]))
            .unwrap();
        let expected = log_position(1000.0, 20.0, 20_000.0) * 400.0;
        assert!(
            (loudest as f64 - expected).abs() <= 1.5,
            "{loudest} vs {expected}"
        );
        assert!(columns[loudest] > -1.5, "{}", columns[loudest]);
    }

    #[test]
    fn low_end_of_the_spectrum_is_smooth_not_stepped() {
        // A ramp in dB across bins: columns narrower than a bin must follow it
        // continuously instead of repeating one bin's value.
        let db = (0..=SPECTRUM_SIZE / 2)
            .map(|bin| -(bin as f64))
            .collect::<Vec<_>>();
        let columns = spectrum_columns(&db, 48_000.0, 400, 20.0, 20_000.0);
        let low = &columns[..40];
        assert!(
            low.windows(2).all(|pair| pair[1] < pair[0]),
            "strictly falling, no flat steps: {low:?}"
        );
    }

    #[test]
    fn readout_and_value_formatting() {
        assert_eq!(readout(&[0.1, -0.5, 0.9, 0.25]), Some((0.25, -0.5, 0.9)));
        assert_eq!(readout(&[]), None);
        assert_eq!(format_value(0.25), "0.2500");
        assert_eq!(format_value(1234.5678), "1235");
        assert_eq!(format_value(-12.345), "-12.35");
        assert_eq!(format_value(0.0), "0");
        assert_eq!(format_value(1.5e-5), "1.500e-5");
        assert_eq!(format_value(f64::NAN), "NaN");
    }

    #[test]
    fn fixed_values_keep_their_width() {
        for (x, text) in [
            (0.25, " 0.2500"),
            (-0.6188, "-0.6188"),
            (12.345, " 12.345"),
            (-1234.56, "-1234.6"),
            (123456.0, " 123456"),
            (9.99996, " 10.000"),
            (0.0, " 0.0000"),
            (-0.0, " 0.0000"),
            (0.0012345, " 0.0012"),
            (1.5e-5, " 1.5e-5"),
            (-2.5e7, " -2.5e7"),
            (1e-12, "  1e-12"),
            (f64::NAN, "    NaN"),
            (f64::NEG_INFINITY, "   -inf"),
        ] {
            assert_eq!(format_fixed(x), text, "{x}");
        }
        for i in 0..2000 {
            let x = (i as f64 * 0.37).sin() * 10f64.powf(i as f64 % 17.0 - 8.0);
            assert_eq!(format_fixed(x).chars().count(), FIXED_WIDTH, "{x}");
        }
    }

    #[test]
    fn non_integer_rates_keep_a_decimal() {
        assert_eq!(format_rate(44_100), "44.1k");
        assert_eq!(format_rate(96_000), "96k");
    }
}
