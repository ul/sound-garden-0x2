//! Performance feedback shown in the GUI: engine status and output meters.
//!
//! Pure state and formatting, kept apart from drawing so it can be tested.
use audio_ops::MidiEventKind;
use audio_server::{Meters, MidiMessage};
use std::sync::Arc;

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
        self.load.update(meters.load, time);
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
            "{} · {} · {:.1}ms · dsp {:.0}%",
            format_rate(self.sample_rate),
            self.buffer_frames,
            1000.0 * self.buffer_frames as f64 / self.sample_rate as f64,
            100.0 * self.load.value,
        );
        match self.dropouts {
            0 => {}
            1 => status.push_str(" · 1 dropout"),
            n => status.push_str(&format!(" · {n} dropouts")),
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
    if !(amplitude > 0.0) {
        return if amplitude.is_nan() { 1.0 } else { 0.0 };
    }
    let db = 20.0 * amplitude.log10();
    ((db - METER_FLOOR_DB) / -METER_FLOOR_DB).clamp(0.0, 1.0) as f32
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
    fn non_integer_rates_keep_a_decimal() {
        assert_eq!(format_rate(44_100), "44.1k");
        assert_eq!(format_rate(96_000), "96k");
    }
}
