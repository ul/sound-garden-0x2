//! # Pulse wave
//!
//! Sources to connect: frequency, duty cycle.
use crate::phasor::{phase_to_unit, poly_blep, wrap_phase};
use crate::pure::rectangle;
use crate::waveform::{Cycle, Draw, Morph, Oscillator, Shape, cycle_of, oscillator_perform};
use audio_vm::{CHANNELS, Frame, Op, Sample, Stack};
use itertools::izip;

/// The four pulse oscillators share everything but how they draw a sample: band-limited or
/// naive, with or without a phase offset input.
macro_rules! pulse_op {
    ($(#[$doc:meta])* $name:ident, phase0: $has_phase0:expr, band_limited: $band_limited:expr, $sample:expr) => {
        $(#[$doc])*
        pub struct $name {
            phases: Frame,
            phase0: Frame,
            widths: Frame,
            dts: Frame,
            sample_period: Sample,
            morph: Morph,
        }

        impl $name {
            pub fn new(sample_rate: u32) -> Self {
                Self {
                    phases: [0.0; CHANNELS],
                    phase0: [0.0; CHANNELS],
                    widths: [0.5; CHANNELS],
                    dts: [0.0; CHANNELS],
                    sample_period: Sample::from(sample_rate).recip(),
                    morph: Morph::new(sample_rate),
                }
            }
        }

        impl Oscillator for $name {
            fn cycle(&self) -> Cycle {
                Cycle {
                    phase0: self.phase0,
                    width: self.widths,
                    dt: if $band_limited { self.dts } else { [0.0; CHANNELS] },
                    ..Cycle::new(self.phases, Shape::Pulse)
                }
            }
        }

        impl Draw for $name {
            #[inline(always)]
            fn draw(&mut self, stack: &mut Stack) -> Frame {
                if $has_phase0 {
                    self.phase0 = stack.pop();
                }
                self.widths = stack.pop();
                let frequency = stack.pop();
                let mut frame = [0.0; CHANNELS];
                for (out, phase, dt, &frequency, &width, &phase0) in izip!(
                    &mut frame,
                    &mut self.phases,
                    &mut self.dts,
                    &frequency,
                    &self.widths,
                    &self.phase0
                ) {
                    *dt = frequency * self.sample_period;
                    let dt = *dt;
                    *phase = wrap_phase(*phase + 2.0 * dt);
                    *out = ($sample)(wrap_phase(*phase + phase0), width, dt);
                }
                frame
            }

            fn morph_and_phases(&mut self) -> (&mut Morph, &Frame) {
                (&mut self.morph, &self.phases)
            }
        }

        impl Op for $name {
            oscillator_perform!();

            fn migrate(&mut self, other: &mut dyn Op) {
                if let Some(previous) = cycle_of(other) {
                    self.phases = previous.phases;
                    self.morph.start(&previous, Shape::Pulse);
                }
            }
        }
    };
}

pulse_op!(
    /// Band-limited pulse (`p`).
    Pulse, phase0: false, band_limited: true, poly_blep_pulse_sample
);
pulse_op!(
    /// Band-limited pulse with a phase offset input (`pulse`).
    PulsePhase, phase0: true, band_limited: true, poly_blep_pulse_sample
);
pulse_op!(
    /// Naive pulse (`p'`).
    NaivePulse, phase0: false, band_limited: false, naive_pulse_sample
);
pulse_op!(
    /// Naive pulse with a phase offset input (`pulse'`).
    NaivePulsePhase, phase0: true, band_limited: false, naive_pulse_sample
);

#[inline]
fn naive_pulse_sample(phase: Sample, width: Sample, _dt: Sample) -> Sample {
    rectangle(phase, width)
}

#[inline]
pub(crate) fn poly_blep_pulse_sample(phase: Sample, width: Sample, dt: Sample) -> Sample {
    let width = width.clamp(0.0, 1.0);
    let t = phase_to_unit(phase);
    let mut y = if t < width { 1.0 } else { -1.0 };
    y += poly_blep(t, dt);
    y -= poly_blep((t - width + 1.0) % 1.0, dt);
    y.clamp(-1.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn poly_blep_pulse_differs_from_naive_near_nyquist_and_matches_at_low_frequency() {
        let low = poly_blep_pulse_sample(-0.25, 0.5, 10.0 / 48_000.0);
        assert!((low - 1.0).abs() < 0.01);

        let high = poly_blep_pulse_sample(-0.99, 0.5, 20_000.0 / 48_000.0);
        assert!((high - 1.0).abs() > 0.01);
    }
}
