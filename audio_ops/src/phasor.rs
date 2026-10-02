//! # Phasor
//!
//! ```text
//!  1     /|    /|    /|    /|
//!       / |   / |   / |   / |
//!  0   /  |  /  |  /  |  /  |
//!     /   | /   | /   | /   |
//! -1 /    |/    |/    |/    |
//! ```
//!
//! Phasor module generates a saw wave in the range -1..1.
//! Frequency is controlled by the input for each channel separately and can be variable.
//!
//! It is called phasor because it could be used as input phase for other oscillators, which become
//! just pure transformations then and are not required to care about handling varying frequency by
//! themselves anymore.
//!
//! Sources to connect: frequency.
use crate::waveform::{Cycle, Draw, Morph, Oscillator, Shape, cycle_of, oscillator_perform};
use audio_vm::{CHANNELS, Frame, Op, Sample, Stack};
use itertools::izip;

/// Wrap phase into -1..1. Floor-based rather than `%` so negative frequencies
/// wrap too (`%` keeps the dividend's sign) and it compiles to a single rounding
/// instruction instead of an fmod call.
#[inline]
pub(crate) fn wrap_phase(phase: Sample) -> Sample {
    phase - 2.0 * ((phase + 1.0) * 0.5).floor()
}

/// Phase spans -1..1, i.e. two units per cycle: a frequency `f` advances it by
/// `2 * f / sample_rate` per sample. PolyBLEP widths are in cycles per sample
/// (`dt = f / sample_rate`), matching `phase_to_unit`'s 0..1 range.
#[inline]
pub(crate) fn phase_to_unit(phase: Sample) -> Sample {
    (phase + 1.0) * 0.5
}

/// Correction for a discontinuity at `t` = 0 (wrapping from 1), `t` in 0..1 of a cycle.
///
/// Written so that the common case, away from the edge, returns before dividing and only one
/// division happens otherwise: when the branches were symmetric the compiler could turn them
/// into selects and divide on every sample, which made pulses 2.5x slower.
#[inline]
pub(crate) fn poly_blep(t: Sample, dt: Sample) -> Sample {
    let dt = dt.abs().clamp(1.0e-12, 0.5);
    let start = t < dt;
    if !start && t <= 1.0 - dt {
        return 0.0;
    }
    let x = if start { t } else { t - 1.0 } / dt;
    if start {
        x + x - x * x - 1.0
    } else {
        x * x + x + x + 1.0
    }
}

#[inline]
pub(crate) fn poly_blep_saw_sample(phase: Sample, dt: Sample) -> Sample {
    phase - 2.0 * poly_blep(phase_to_unit(phase), dt)
}

/// Raw phasor (`w`): a naive saw, also used as a phase source.
pub struct Phasor {
    phases: [Sample; CHANNELS],
    sample_period: Sample,
    morph: Morph,
}

impl Phasor {
    pub fn new(sample_rate: u32) -> Self {
        Phasor {
            phases: [0.0; CHANNELS],
            sample_period: Sample::from(sample_rate).recip(),
            morph: Morph::new(),
        }
    }
}

impl Oscillator for Phasor {
    fn cycle(&self) -> Cycle {
        Cycle::new(self.phases, Shape::Saw)
    }
}

impl Draw for Phasor {
    #[inline(always)]
    fn draw(&mut self, stack: &mut Stack) -> Frame {
        for (phase, &frequency) in self.phases.iter_mut().zip(&stack.pop()) {
            let dx = 2.0 * frequency * self.sample_period;
            *phase = wrap_phase(*phase + dx);
        }
        self.phases
    }

    fn morph_and_phases(&mut self) -> (&mut Morph, &Frame) {
        (&mut self.morph, &self.phases)
    }
}

impl Op for Phasor {
    oscillator_perform!();

    fn migrate(&mut self, other: &mut dyn Op) {
        if let Some(previous) = cycle_of(other) {
            self.phases = previous.phases;
            self.morph.start(&previous, Shape::Saw);
        }
    }
}

/// Raw phasor with a phase offset input (`saw'`).
pub struct Phasor0 {
    phases: [Sample; CHANNELS],
    phase0: Frame,
    sample_period: Sample,
    morph: Morph,
}

impl Phasor0 {
    pub fn new(sample_rate: u32) -> Self {
        Phasor0 {
            phases: [0.0; CHANNELS],
            phase0: [0.0; CHANNELS],
            sample_period: Sample::from(sample_rate).recip(),
            morph: Morph::new(),
        }
    }
}

impl Oscillator for Phasor0 {
    fn cycle(&self) -> Cycle {
        Cycle {
            phase0: self.phase0,
            ..Cycle::new(self.phases, Shape::Saw)
        }
    }
}

impl Draw for Phasor0 {
    #[inline(always)]
    fn draw(&mut self, stack: &mut Stack) -> Frame {
        self.phase0 = stack.pop();
        let frequency = stack.pop();
        for (phase, &frequency) in self.phases.iter_mut().zip(&frequency) {
            let dx = 2.0 * frequency * self.sample_period;
            *phase = wrap_phase(*phase + dx);
        }
        let mut output = [0.0; CHANNELS];
        for (out, &phase, &phase0) in izip!(&mut output, &self.phases, &self.phase0) {
            *out = wrap_phase(phase + phase0);
        }
        output
    }

    fn morph_and_phases(&mut self) -> (&mut Morph, &Frame) {
        (&mut self.morph, &self.phases)
    }
}

impl Op for Phasor0 {
    oscillator_perform!();

    fn migrate(&mut self, other: &mut dyn Op) {
        if let Some(previous) = cycle_of(other) {
            self.phases = previous.phases;
            self.morph.start(&previous, Shape::Saw);
        }
    }
}

/// Band-limited saw with a phase offset input (`saw`).
pub struct PolyBlepSawPhase {
    phases: [Sample; CHANNELS],
    phase0: Frame,
    dts: Frame,
    sample_period: Sample,
    morph: Morph,
}

impl PolyBlepSawPhase {
    pub fn new(sample_rate: u32) -> Self {
        Self {
            phases: [0.0; CHANNELS],
            phase0: [0.0; CHANNELS],
            dts: [0.0; CHANNELS],
            sample_period: Sample::from(sample_rate).recip(),
            morph: Morph::new(),
        }
    }
}

impl Oscillator for PolyBlepSawPhase {
    fn cycle(&self) -> Cycle {
        Cycle {
            phase0: self.phase0,
            dt: self.dts,
            ..Cycle::new(self.phases, Shape::Saw)
        }
    }
}

impl Draw for PolyBlepSawPhase {
    #[inline(always)]
    fn draw(&mut self, stack: &mut Stack) -> Frame {
        self.phase0 = stack.pop();
        let frequency = stack.pop();
        let mut output: Frame = [0.0; CHANNELS];
        for (out, phase, dt, &frequency, &phase0) in izip!(
            &mut output,
            &mut self.phases,
            &mut self.dts,
            &frequency,
            &self.phase0
        ) {
            *dt = frequency * self.sample_period;
            *phase = wrap_phase(*phase + 2.0 * *dt);
            *out = poly_blep_saw_sample(wrap_phase(*phase + phase0), *dt);
        }
        output
    }

    fn morph_and_phases(&mut self) -> (&mut Morph, &Frame) {
        (&mut self.morph, &self.phases)
    }
}

impl Op for PolyBlepSawPhase {
    oscillator_perform!();

    fn migrate(&mut self, other: &mut dyn Op) {
        if let Some(previous) = cycle_of(other) {
            self.phases = previous.phases;
            self.morph.start(&previous, Shape::Saw);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pop_after(op: &mut dyn Op, frequency: Sample, phase0: Sample) -> Frame {
        let mut stack = Stack::new();
        stack.push(&[frequency; CHANNELS]);
        stack.push(&[phase0; CHANNELS]);
        op.perform(&mut stack);
        stack.pop()
    }

    #[test]
    fn negative_frequency_stays_in_range_and_runs_backwards() {
        let mut phasor = Phasor::new(100);
        let mut previous = 0.0;
        let mut wraps = 0;
        for _ in 0..1000 {
            let mut stack = Stack::new();
            stack.push(&[-7.0; CHANNELS]);
            phasor.perform(&mut stack);
            let phase = stack.pop()[0];
            assert!((-1.0..1.0).contains(&phase), "phase {phase} out of range");
            if phase > previous {
                wraps += 1;
            }
            previous = phase;
        }
        // 7 Hz over 10 s: one wrap per cycle.
        assert_eq!(wraps, 70);
    }

    #[test]
    fn phasor0_applies_phase_offset_without_changing_frequency() {
        let mut shifted = Phasor0::new(100);
        let mut base = Phasor0::new(100);

        for _ in 0..16 {
            let shifted_frame = pop_after(&mut shifted, 10.0, 0.25);
            let base_frame = pop_after(&mut base, 10.0, 0.0);
            for (&shifted, &base) in shifted_frame.iter().zip(&base_frame) {
                assert!((shifted - wrap_phase(base + 0.25)).abs() < 1.0e-12);
            }
        }
    }

    #[test]
    fn poly_blep_saw_differs_from_naive_near_nyquist_and_matches_at_low_frequency() {
        let low = poly_blep_saw_sample(0.5, 10.0 / 48_000.0);
        assert!((low - 0.5).abs() < 1.0e-9);

        let high = poly_blep_saw_sample(-0.99, 20_000.0 / 48_000.0);
        assert!((high - -0.99).abs() > 0.01);
    }
}
