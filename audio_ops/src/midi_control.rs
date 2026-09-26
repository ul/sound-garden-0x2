//! # MIDI controllers as signals
//!
//! - `cc:N:DEFAULT` — controller N (0..127, any channel) as `0..1`.
//! - `bend` — pitch bend (any channel) as `-1..1`.
//!
//! Values live in a shared [`MidiControls`] store written by the audio
//! callback at each message's frame, so they persist across program reloads
//! (knobs don't move when code is committed). Until a controller first moves,
//! the op outputs its default, which is also what offline renders hear.
//! Unprimed forms glide over [`CONTROL_SMOOTHING_SECONDS`] so 7-bit steps
//! don't zipper; primed forms (`cc':N`, `bend'`) output the raw value.
//! See docs/adr/0006-midi-controllers.md.
use audio_vm::{CHANNELS, Op, Sample, Stack};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

pub const MIDI_CONTROLLERS: usize = 128;
pub const CONTROL_SMOOTHING_SECONDS: Sample = 0.01;

/// Bit pattern for "never received"; no clamped finite value encodes to it.
const UNSET: u64 = u64::MAX;

/// Latest controller and pitch-bend values, shared between the audio callback
/// (writer) and compiled ops (readers).
pub struct MidiControls {
    controllers: [AtomicU64; MIDI_CONTROLLERS],
    bend: AtomicU64,
}

impl Default for MidiControls {
    fn default() -> Self {
        MidiControls {
            controllers: std::array::from_fn(|_| AtomicU64::new(UNSET)),
            bend: AtomicU64::new(UNSET),
        }
    }
}

impl MidiControls {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_controller(&self, controller: u8, value: Sample) {
        if let Some(slot) = self.controllers.get(controller as usize) {
            slot.store(value.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
        }
    }

    pub fn set_bend(&self, value: Sample) {
        self.bend
            .store(value.clamp(-1.0, 1.0).to_bits(), Ordering::Relaxed);
    }

    fn read(slot: &AtomicU64) -> Option<Sample> {
        match slot.load(Ordering::Relaxed) {
            UNSET => None,
            bits => Some(Sample::from_bits(bits)),
        }
    }

    pub fn controller(&self, controller: u8) -> Option<Sample> {
        self.controllers
            .get(controller as usize)
            .and_then(Self::read)
    }

    pub fn bend(&self) -> Option<Sample> {
        Self::read(&self.bend)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControlSource {
    Controller(u8),
    Bend,
}

/// `cc:N`, `cc':N`, `bend`, `bend'`. Raw and smoothed forms are one type so
/// switching between them in a live edit keeps the current value.
pub struct MidiControl {
    controls: Arc<MidiControls>,
    source: ControlSource,
    default: Sample,
    /// Fraction of the remaining distance covered per frame; 1 is raw.
    step: Sample,
    /// None until the first frame, which starts at the target instead of
    /// gliding up from zero.
    value: Option<Sample>,
}

impl MidiControl {
    pub fn new(
        sample_rate: u32,
        controls: Arc<MidiControls>,
        source: ControlSource,
        default: Sample,
        smoothed: bool,
    ) -> Self {
        let step = if smoothed {
            1.0 - (-1.0 / (CONTROL_SMOOTHING_SECONDS * Sample::from(sample_rate))).exp()
        } else {
            1.0
        };
        MidiControl {
            controls,
            source,
            default,
            step,
            value: None,
        }
    }
}

impl Op for MidiControl {
    fn perform(&mut self, stack: &mut Stack) {
        let target = match self.source {
            ControlSource::Controller(controller) => self.controls.controller(controller),
            ControlSource::Bend => self.controls.bend(),
        }
        .unwrap_or(self.default);
        let value = match self.value {
            Some(value) if (target - value).abs() > 1e-9 => value + self.step * (target - value),
            // Snap once close, so the glide never decays into denormals.
            _ => target,
        };
        self.value = Some(value);
        stack.push(&[value; CHANNELS]);
    }

    fn migrate(&mut self, other: &mut dyn Op) {
        if let Some(other) = other.downcast_mut::<Self>()
            && other.source == self.source
        {
            self.value = other.value;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: u32 = 1000;

    fn frame(op: &mut MidiControl) -> Sample {
        let mut stack = Stack::new();
        op.perform(&mut stack);
        stack.pop()[0]
    }

    #[test]
    fn default_until_first_message_then_the_value() {
        let controls = Arc::new(MidiControls::new());
        let mut op = MidiControl::new(
            SR,
            Arc::clone(&controls),
            ControlSource::Controller(74),
            0.25,
            false,
        );
        assert_eq!(frame(&mut op), 0.25);
        controls.set_controller(74, 0.8);
        assert_eq!(frame(&mut op), 0.8);
        // Other controllers don't leak in.
        controls.set_controller(1, 0.1);
        assert_eq!(frame(&mut op), 0.8);
    }

    #[test]
    fn bend_defaults_to_centre_and_clamps() {
        let controls = Arc::new(MidiControls::new());
        let mut op = MidiControl::new(SR, Arc::clone(&controls), ControlSource::Bend, 0.0, false);
        assert_eq!(frame(&mut op), 0.0);
        controls.set_bend(-3.0);
        assert_eq!(frame(&mut op), -1.0);
    }

    #[test]
    fn smoothed_value_glides_with_the_documented_time_constant() {
        let controls = Arc::new(MidiControls::new());
        controls.set_controller(7, 0.0);
        let mut op = MidiControl::new(
            SR,
            Arc::clone(&controls),
            ControlSource::Controller(7),
            0.0,
            true,
        );
        assert_eq!(frame(&mut op), 0.0);
        controls.set_controller(7, 1.0);
        // After one time constant (10 frames at 1 kHz) a one-pole glide has
        // covered 1 - 1/e of the way.
        let mut value = 0.0;
        for _ in 0..10 {
            value = frame(&mut op);
        }
        assert!((value - (1.0 - (-1.0f64).exp())).abs() < 1e-9, "{value}");
        for _ in 0..1000 {
            value = frame(&mut op);
        }
        assert_eq!(value, 1.0);
    }

    #[test]
    fn a_new_op_starts_at_the_knob_instead_of_gliding_from_zero() {
        let controls = Arc::new(MidiControls::new());
        controls.set_controller(7, 0.6);
        let mut op = MidiControl::new(SR, controls, ControlSource::Controller(7), 0.0, true);
        assert_eq!(frame(&mut op), 0.6);
    }

    #[test]
    fn migration_continues_a_glide_only_for_the_same_control() {
        let controls = Arc::new(MidiControls::new());
        controls.set_controller(7, 0.0);
        let mut old = MidiControl::new(
            SR,
            Arc::clone(&controls),
            ControlSource::Controller(7),
            0.0,
            true,
        );
        frame(&mut old);
        controls.set_controller(7, 1.0);
        let mid = frame(&mut old);
        assert!(mid > 0.0 && mid < 1.0);

        // A recompiled op for the same control continues the glide.
        let mut same = MidiControl::new(
            SR,
            Arc::clone(&controls),
            ControlSource::Controller(7),
            0.0,
            true,
        );
        same.migrate(&mut old);
        let next = frame(&mut same);
        assert!(next > mid && next < 1.0, "{mid} -> {next}");

        // A different controller starts fresh at its own value.
        let mut other = MidiControl::new(
            SR,
            Arc::clone(&controls),
            ControlSource::Controller(8),
            0.3,
            true,
        );
        other.migrate(&mut old);
        assert_eq!(frame(&mut other), 0.3);
    }
}
