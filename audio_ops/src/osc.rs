//! # Oscillators
//! Sources to connect: frequency (and phase offset for the `*Phase` forms).
//!
//! Every oscillator continues the cycle of whichever oscillator it replaces in a live edit and
//! morphs from that oscillator's shape to its own (see waveform.rs).

use crate::glide::Glide;
use crate::phasor::{phase_to_unit, poly_blep, wrap_phase};
use crate::pure;
use crate::waveform::{Cycle, Draw, Morph, Oscillator, Shape, cycle_of, oscillator_perform};
use audio_vm::{CHANNELS, Frame, Op, Sample, Stack};
use itertools::izip;

/// Shape every channel's phase. A mono patch keeps all channels in the same
/// phase, so shape it once: half the `sin` calls for the usual case.
#[inline(always)]
fn shape_frame(f: fn(Sample) -> Sample, phases: &Frame) -> Frame {
    if phases.iter().all(|&phase| phase == phases[0]) {
        [f(phases[0]); CHANNELS]
    } else {
        phases.map(f)
    }
}

/// A sine, cosine or naive triangle at the frequency on the stack.
pub struct Osc {
    phases: Frame,
    sample_period: Sample,
    shape: Shape,
    f: fn(Sample) -> Sample,
    morph: Morph,
}

impl Osc {
    pub fn new(sample_rate: u32, shape: Shape) -> Self {
        Osc {
            phases: [0.0; CHANNELS],
            sample_period: Sample::from(sample_rate).recip(),
            shape,
            f: shape.function(),
            morph: Morph::new(sample_rate),
        }
    }
}

impl Oscillator for Osc {
    fn cycle(&self) -> Cycle {
        Cycle::new(self.phases, self.shape)
    }
}

impl Draw for Osc {
    #[inline(always)]
    fn draw(&mut self, stack: &mut Stack) -> Frame {
        let frequency = stack.pop();
        let scale = 2.0 * self.sample_period;
        for (phase, &frequency) in self.phases.iter_mut().zip(&frequency) {
            *phase = wrap_phase(*phase + frequency * scale);
        }
        shape_frame(self.f, &self.phases)
    }

    fn morph_and_phases(&mut self) -> (&mut Morph, &Frame) {
        (&mut self.morph, &self.phases)
    }
}

impl Op for Osc {
    oscillator_perform!();

    fn migrate(&mut self, other: &mut dyn Op) {
        if let Some(previous) = cycle_of(other) {
            self.phases = previous.phases;
            self.morph.start(&previous, self.shape);
        }
    }
}

/// An oscillator at a literal frequency (`440 s`), specialised by the compiler. The frequency
/// glides when a live edit changes it (see glide.rs) while the phase carries on.
pub struct FixedOsc {
    phases: Frame,
    frequency: Glide,
    sample_period: Sample,
    shape: Shape,
    f: fn(Sample) -> Sample,
    morph: Morph,
}

impl FixedOsc {
    pub fn new(sample_rate: u32, frequency: Sample, shape: Shape) -> Self {
        Self {
            phases: [0.0; CHANNELS],
            frequency: Glide::new(sample_rate, frequency),
            sample_period: Sample::from(sample_rate).recip(),
            shape,
            f: shape.function(),
            morph: Morph::new(sample_rate),
        }
    }
}

impl Oscillator for FixedOsc {
    fn cycle(&self) -> Cycle {
        Cycle::new(self.phases, self.shape)
    }
}

impl Draw for FixedOsc {
    #[inline(always)]
    fn draw(&mut self, _stack: &mut Stack) -> Frame {
        let scale = 2.0 * self.sample_period;
        for (phase, &frequency) in self.phases.iter_mut().zip(self.frequency.next()) {
            *phase = wrap_phase(*phase + frequency * scale);
        }
        shape_frame(self.f, &self.phases)
    }

    fn morph_and_phases(&mut self) -> (&mut Morph, &Frame) {
        (&mut self.morph, &self.phases)
    }
}

impl Op for FixedOsc {
    oscillator_perform!();

    fn migrate(&mut self, other: &mut dyn Op) {
        if let Some(other) = other.downcast_mut::<Self>() {
            self.frequency.migrate(&other.frequency);
        }
        if let Some(previous) = cycle_of(other) {
            self.phases = previous.phases;
            self.morph.start(&previous, self.shape);
        }
    }
}

/// A sine, cosine or naive triangle with a phase offset input.
pub struct OscPhase {
    phases: Frame,
    phase0: Frame,
    sample_period: Sample,
    shape: Shape,
    f: fn(Sample) -> Sample,
    morph: Morph,
}

impl OscPhase {
    pub fn new(sample_rate: u32, shape: Shape) -> Self {
        OscPhase {
            phases: [0.0; CHANNELS],
            phase0: [0.0; CHANNELS],
            sample_period: Sample::from(sample_rate).recip(),
            shape,
            f: shape.function(),
            morph: Morph::new(sample_rate),
        }
    }
}

impl Oscillator for OscPhase {
    fn cycle(&self) -> Cycle {
        Cycle {
            phase0: self.phase0,
            ..Cycle::new(self.phases, self.shape)
        }
    }
}

impl Draw for OscPhase {
    #[inline(always)]
    fn draw(&mut self, stack: &mut Stack) -> Frame {
        let phase0 = stack.pop();
        let frequency = stack.pop();
        self.phase0 = phase0;
        // Advance every channel first, then draw: calling the shape between phase updates
        // made this op twice as slow.
        let scale = 2.0 * self.sample_period;
        let mut frame = [0.0; CHANNELS];
        for (phase, sample, &frequency, &phase0) in
            izip!(&mut self.phases, &mut frame, &frequency, &phase0)
        {
            *phase = wrap_phase(*phase + frequency * scale);
            *sample = wrap_phase(*phase + phase0);
        }
        shape_frame(self.f, &frame)
    }

    fn morph_and_phases(&mut self) -> (&mut Morph, &Frame) {
        (&mut self.morph, &self.phases)
    }
}

impl Op for OscPhase {
    oscillator_perform!();

    fn migrate(&mut self, other: &mut dyn Op) {
        if let Some(previous) = cycle_of(other) {
            self.phases = previous.phases;
            self.morph.start(&previous, self.shape);
        }
    }
}

/// Band-limited triangle (`t`).
pub struct PolyBlepTriangle {
    phases: Frame,
    outputs: Frame,
    sample_period: Sample,
    morph: Morph,
}

impl PolyBlepTriangle {
    pub fn new(sample_rate: u32) -> Self {
        Self {
            phases: [0.0; CHANNELS],
            outputs: [-1.0; CHANNELS],
            sample_period: Sample::from(sample_rate).recip(),
            morph: Morph::new(sample_rate),
        }
    }
}

impl Oscillator for PolyBlepTriangle {
    fn cycle(&self) -> Cycle {
        Cycle::new(self.phases, Shape::Triangle)
    }
}

impl Draw for PolyBlepTriangle {
    #[inline(always)]
    fn draw(&mut self, stack: &mut Stack) -> Frame {
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
        frame
    }

    fn morph_and_phases(&mut self) -> (&mut Morph, &Frame) {
        (&mut self.morph, &self.phases)
    }
}

impl Op for PolyBlepTriangle {
    oscillator_perform!();

    fn migrate(&mut self, other: &mut dyn Op) {
        if let Some(other) = other.downcast_mut::<Self>() {
            self.phases = other.phases;
            self.outputs = other.outputs;
        } else if let Some(previous) = cycle_of(other) {
            // The triangle integrates its slope, so seed it where the triangle is at this phase.
            self.phases = previous.phases;
            self.outputs = previous.phases.map(pure::triangle);
            self.morph.start(&previous, Shape::Triangle);
        }
    }
}

/// Band-limited triangle with a phase offset input (`tri`).
pub struct PolyBlepTrianglePhase {
    phases: Frame,
    phase0: Frame,
    outputs: Frame,
    sample_period: Sample,
    morph: Morph,
}

impl PolyBlepTrianglePhase {
    pub fn new(sample_rate: u32) -> Self {
        Self {
            phases: [0.0; CHANNELS],
            phase0: [0.0; CHANNELS],
            outputs: [-1.0; CHANNELS],
            sample_period: Sample::from(sample_rate).recip(),
            morph: Morph::new(sample_rate),
        }
    }
}

impl Oscillator for PolyBlepTrianglePhase {
    fn cycle(&self) -> Cycle {
        Cycle {
            phase0: self.phase0,
            ..Cycle::new(self.phases, Shape::Triangle)
        }
    }
}

impl Draw for PolyBlepTrianglePhase {
    #[inline(always)]
    fn draw(&mut self, stack: &mut Stack) -> Frame {
        self.phase0 = stack.pop();
        let frequency = stack.pop();
        let mut frame = [0.0; CHANNELS];
        for (out, phase, tri, &frequency, &phase0) in izip!(
            &mut frame,
            &mut self.phases,
            &mut self.outputs,
            &frequency,
            &self.phase0
        ) {
            let dt = frequency * self.sample_period;
            *phase = wrap_phase(*phase + 2.0 * dt);
            *tri = poly_blep_triangle_step(wrap_phase(*phase + phase0), dt, *tri);
            *out = *tri;
        }
        frame
    }

    fn morph_and_phases(&mut self) -> (&mut Morph, &Frame) {
        (&mut self.morph, &self.phases)
    }
}

impl Op for PolyBlepTrianglePhase {
    oscillator_perform!();

    fn migrate(&mut self, other: &mut dyn Op) {
        if let Some(other) = other.downcast_mut::<Self>() {
            self.phases = other.phases;
            self.outputs = other.outputs;
        } else if let Some(previous) = cycle_of(other) {
            // Seed the integrator at this phase, assuming the offset input stays as it was.
            self.phases = previous.phases;
            for (output, (&phase, &phase0)) in self
                .outputs
                .iter_mut()
                .zip(previous.phases.iter().zip(&previous.phase0))
            {
                *output = pure::triangle(wrap_phase(phase + phase0));
            }
            self.phase0 = previous.phase0;
            self.morph.start(&previous, Shape::Triangle);
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
