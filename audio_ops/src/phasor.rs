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
use audio_vm::{CHANNELS, Frame, Op, Sample, Stack};
use itertools::izip;

#[inline]
pub(crate) fn wrap_phase(phase: Sample) -> Sample {
    ((phase + 1.0) % 2.0) - 1.0
}

#[inline]
pub(crate) fn phase_to_unit(phase: Sample) -> Sample {
    (phase + 1.0) * 0.5
}

#[inline]
pub(crate) fn poly_blep(t: Sample, dt: Sample) -> Sample {
    let dt = dt.abs().clamp(1.0e-12, 0.5);
    if t < dt {
        let x = t / dt;
        x + x - x * x - 1.0
    } else if t > 1.0 - dt {
        let x = (t - 1.0) / dt;
        x * x + x + x + 1.0
    } else {
        0.0
    }
}

#[inline]
pub(crate) fn poly_blep_saw_sample(phase: Sample, dx: Sample) -> Sample {
    phase - 2.0 * poly_blep(phase_to_unit(phase), dx)
}

pub struct Phasor {
    phases: [Sample; CHANNELS],
    sample_period: Sample,
}

impl Phasor {
    pub fn new(sample_rate: u32) -> Self {
        Phasor {
            phases: [0.0; CHANNELS],
            sample_period: Sample::from(sample_rate).recip(),
        }
    }

    pub fn migrate_same(&mut self, other: &Self) {
        self.phases = other.phases;
    }
}

impl Op for Phasor {
    fn perform(&mut self, stack: &mut Stack) {
        for (phase, &frequency) in self.phases.iter_mut().zip(&stack.pop()) {
            let dx = frequency * self.sample_period;
            *phase = wrap_phase(*phase + dx);
        }
        stack.push(&self.phases);
    }

    fn migrate(&mut self, other: &mut dyn Op) {
        if let Some(other) = other.downcast_mut::<Self>() {
            self.migrate_same(other);
        }
    }
}

pub struct Phasor0 {
    phases: [Sample; CHANNELS],
    sample_period: Sample,
}

impl Phasor0 {
    pub fn new(sample_rate: u32) -> Self {
        Phasor0 {
            phases: [0.0; CHANNELS],
            sample_period: Sample::from(sample_rate).recip(),
        }
    }

    pub fn migrate_same(&mut self, other: &Self) {
        self.phases = other.phases;
    }
}

impl Op for Phasor0 {
    fn perform(&mut self, stack: &mut Stack) {
        let phase0 = stack.pop();
        let frequency = stack.pop();
        for (phase, &frequency) in self.phases.iter_mut().zip(&frequency) {
            let dx = frequency * self.sample_period;
            *phase = wrap_phase(*phase + dx);
        }
        let mut output = [0.0; CHANNELS];
        for (out, &phase, &phase0) in izip!(&mut output, &self.phases, &phase0) {
            *out = wrap_phase(phase + phase0);
        }
        stack.push(&output);
    }

    fn migrate(&mut self, other: &mut dyn Op) {
        if let Some(other) = other.downcast_mut::<Self>() {
            self.migrate_same(other);
        }
    }
}

pub struct PolyBlepSawPhase {
    phases: [Sample; CHANNELS],
    sample_period: Sample,
}

impl PolyBlepSawPhase {
    pub fn new(sample_rate: u32) -> Self {
        Self {
            phases: [0.0; CHANNELS],
            sample_period: Sample::from(sample_rate).recip(),
        }
    }
}

impl Op for PolyBlepSawPhase {
    fn perform(&mut self, stack: &mut Stack) {
        let phase0 = stack.pop();
        let frequency = stack.pop();
        let mut output: Frame = [0.0; CHANNELS];
        for (out, phase, &frequency, &phase0) in
            izip!(&mut output, &mut self.phases, &frequency, &phase0)
        {
            let dx = frequency * self.sample_period;
            *phase = wrap_phase(*phase + dx);
            *out = poly_blep_saw_sample(wrap_phase(*phase + phase0), dx);
        }
        stack.push(&output);
    }

    fn migrate(&mut self, other: &mut dyn Op) {
        if let Some(other) = other.downcast_mut::<Self>() {
            self.phases = other.phases;
        } else if let Some(other) = other.downcast_mut::<Phasor0>() {
            self.phases = other.phases;
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
