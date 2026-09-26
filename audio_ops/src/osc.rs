//! # Oscillator
//! Sources to connect: frequency.

use crate::function::Fn1;
use crate::phasor::{Phasor, Phasor0, phase_to_unit, poly_blep, wrap_phase};
use audio_vm::{CHANNELS, Frame, Op, Sample, Stack};
use itertools::izip;

pub struct Osc {
    phasor: Phasor,
    osc: Fn1,
}

impl Osc {
    pub fn new(sample_rate: u32, f: fn(Sample) -> Sample) -> Self {
        let phasor = Phasor::new(sample_rate);
        let osc = Fn1::new(f);
        Osc { phasor, osc }
    }
}

impl Op for Osc {
    fn perform(&mut self, stack: &mut Stack) {
        self.phasor.perform(stack);
        self.osc.perform(stack);
    }

    fn migrate(&mut self, other: &mut dyn Op) {
        if let Some(other) = other.downcast_mut::<Self>() {
            self.phasor.migrate_same(&other.phasor);
        }
    }
}

pub struct FixedOsc {
    phases: Frame,
    frequency: Sample,
    sample_period: Sample,
    f: fn(Sample) -> Sample,
}

impl FixedOsc {
    pub fn new(sample_rate: u32, frequency: Sample, f: fn(Sample) -> Sample) -> Self {
        Self {
            phases: [0.0; CHANNELS],
            frequency,
            sample_period: Sample::from(sample_rate).recip(),
            f,
        }
    }
}

impl Op for FixedOsc {
    fn perform(&mut self, stack: &mut Stack) {
        let dx = 2.0 * self.frequency * self.sample_period;
        let mut frame = [0.0; CHANNELS];

        for (phase, sample) in self.phases.iter_mut().zip(&mut frame) {
            *phase = wrap_phase(*phase + dx);
            *sample = (self.f)(*phase);
        }

        stack.push(&frame);
    }

    fn migrate(&mut self, other: &mut dyn Op) {
        if let Some(other) = other.downcast_mut::<Self>() {
            self.phases = other.phases;
        }
    }
}

pub struct OscPhase {
    phasor: Phasor0,
    osc: Fn1,
}

impl OscPhase {
    pub fn new(sample_rate: u32, f: fn(Sample) -> Sample) -> Self {
        let phasor = Phasor0::new(sample_rate);
        let osc = Fn1::new(f);
        OscPhase { phasor, osc }
    }
}

impl Op for OscPhase {
    fn perform(&mut self, stack: &mut Stack) {
        self.phasor.perform(stack);
        self.osc.perform(stack);
    }

    fn migrate(&mut self, other: &mut dyn Op) {
        if let Some(other) = other.downcast_mut::<Self>() {
            self.phasor.migrate_same(&other.phasor);
        }
    }
}

pub struct PolyBlepTriangle {
    phases: Frame,
    outputs: Frame,
    sample_period: Sample,
}

impl PolyBlepTriangle {
    pub fn new(sample_rate: u32) -> Self {
        Self {
            phases: [0.0; CHANNELS],
            outputs: [-1.0; CHANNELS],
            sample_period: Sample::from(sample_rate).recip(),
        }
    }
}

impl Op for PolyBlepTriangle {
    fn perform(&mut self, stack: &mut Stack) {
        let frequency = stack.pop();
        let mut frame = [0.0; CHANNELS];
        for (out, phase, tri, &frequency) in
            izip!(&mut frame, &mut self.phases, &mut self.outputs, &frequency)
        {
            let dt = frequency * self.sample_period;
            *phase = wrap_phase(*phase + 2.0 * dt);
            *tri = poly_blep_triangle_step(*phase, dt, *tri);
            *out = *tri;
        }
        stack.push(&frame);
    }

    fn migrate(&mut self, other: &mut dyn Op) {
        if let Some(other) = other.downcast_mut::<Self>() {
            self.phases = other.phases;
            self.outputs = other.outputs;
        }
    }
}

pub struct PolyBlepTrianglePhase {
    phases: Frame,
    outputs: Frame,
    sample_period: Sample,
}

impl PolyBlepTrianglePhase {
    pub fn new(sample_rate: u32) -> Self {
        Self {
            phases: [0.0; CHANNELS],
            outputs: [-1.0; CHANNELS],
            sample_period: Sample::from(sample_rate).recip(),
        }
    }
}

impl Op for PolyBlepTrianglePhase {
    fn perform(&mut self, stack: &mut Stack) {
        let phase0 = stack.pop();
        let frequency = stack.pop();
        let mut frame = [0.0; CHANNELS];
        for (out, phase, tri, &frequency, &phase0) in izip!(
            &mut frame,
            &mut self.phases,
            &mut self.outputs,
            &frequency,
            &phase0
        ) {
            let dt = frequency * self.sample_period;
            *phase = wrap_phase(*phase + 2.0 * dt);
            *tri = poly_blep_triangle_step(wrap_phase(*phase + phase0), dt, *tri);
            *out = *tri;
        }
        stack.push(&frame);
    }

    fn migrate(&mut self, other: &mut dyn Op) {
        if let Some(other) = other.downcast_mut::<Self>() {
            self.phases = other.phases;
            self.outputs = other.outputs;
        }
    }
}

/// `dt` is the signed frequency in cycles per sample. The triangle is the
/// integrated band-limited square: it travels 4 units (-1..1..-1) per cycle.
fn poly_blep_triangle_step(phase: Sample, dt: Sample, previous: Sample) -> Sample {
    let t = phase_to_unit(phase);
    let width = dt.abs();
    let mut square = if t < 0.5 { 1.0 } else { -1.0 };
    square += poly_blep(t, width);
    square -= poly_blep((t + 0.5) % 1.0, width);
    (previous + square * dt * 4.0).clamp(-1.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pure;

    #[test]
    fn poly_blep_triangle_differs_from_naive_near_nyquist_and_matches_at_low_frequency() {
        let low = poly_blep_triangle_step(0.0, 10.0 / 48_000.0, pure::triangle(0.0));
        assert!((low - pure::triangle(2.0 * 10.0 / 48_000.0)).abs() < 0.01);

        let high = poly_blep_triangle_step(-0.99, 20_000.0 / 48_000.0, -1.0);
        let naive_next = pure::triangle(wrap_phase(-0.99 + 2.0 * 20_000.0 / 48_000.0));
        assert!((high - naive_next).abs() > 0.01);
    }
}
