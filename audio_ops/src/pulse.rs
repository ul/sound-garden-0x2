//! # Pulse wave
//!
//! Sources to connect: frequency, duty cycle.
use crate::function::Fn2;
use crate::phasor::{Phasor, Phasor0, phase_to_unit, poly_blep, wrap_phase};
use crate::pure::rectangle;
use audio_vm::{CHANNELS, Frame, Op, Sample, Stack};
use itertools::izip;

pub struct Pulse {
    phases: Frame,
    sample_period: Sample,
}

impl Pulse {
    pub fn new(sample_rate: u32) -> Self {
        Self {
            phases: [0.0; CHANNELS],
            sample_period: Sample::from(sample_rate).recip(),
        }
    }
}

impl Op for Pulse {
    fn perform(&mut self, stack: &mut Stack) {
        let duty_cycle = stack.pop();
        let frequency = stack.pop();
        let mut frame = [0.0; CHANNELS];
        for (out, phase, &frequency, &width) in
            izip!(&mut frame, &mut self.phases, &frequency, &duty_cycle)
        {
            let dx = frequency * self.sample_period;
            *phase = wrap_phase(*phase + dx);
            *out = poly_blep_pulse_sample(*phase, width, dx);
        }
        stack.push(&frame);
    }

    fn migrate(&mut self, other: &mut dyn Op) {
        if let Some(other) = other.downcast_mut::<Self>() {
            self.phases = other.phases;
        }
    }
}

pub struct PulsePhase {
    phases: Frame,
    sample_period: Sample,
}

impl PulsePhase {
    pub fn new(sample_rate: u32) -> Self {
        Self {
            phases: [0.0; CHANNELS],
            sample_period: Sample::from(sample_rate).recip(),
        }
    }
}

impl Op for PulsePhase {
    fn perform(&mut self, stack: &mut Stack) {
        let phase0 = stack.pop();
        let duty_cycle = stack.pop();
        let frequency = stack.pop();
        let mut frame = [0.0; CHANNELS];
        for (out, phase, &frequency, &width, &phase0) in izip!(
            &mut frame,
            &mut self.phases,
            &frequency,
            &duty_cycle,
            &phase0
        ) {
            let dx = frequency * self.sample_period;
            *phase = wrap_phase(*phase + dx);
            *out = poly_blep_pulse_sample(wrap_phase(*phase + phase0), width, dx);
        }
        stack.push(&frame);
    }

    fn migrate(&mut self, other: &mut dyn Op) {
        if let Some(other) = other.downcast_mut::<Self>() {
            self.phases = other.phases;
        }
    }
}

pub struct NaivePulse {
    phasor: Phasor,
    osc: Fn2,
}

impl NaivePulse {
    pub fn new(sample_rate: u32) -> Self {
        let phasor = Phasor::new(sample_rate);
        let osc = Fn2::new(rectangle);
        Self { phasor, osc }
    }
}

impl Op for NaivePulse {
    fn perform(&mut self, stack: &mut Stack) {
        let duty_cycle = stack.pop();
        self.phasor.perform(stack);
        stack.push(&duty_cycle);
        self.osc.perform(stack);
    }

    fn migrate(&mut self, other: &mut dyn Op) {
        if let Some(other) = other.downcast_mut::<Self>() {
            self.phasor.migrate_same(&other.phasor);
        }
    }
}

pub struct NaivePulsePhase {
    phasor: Phasor0,
    osc: Fn2,
}

impl NaivePulsePhase {
    pub fn new(sample_rate: u32) -> Self {
        let phasor = Phasor0::new(sample_rate);
        let osc = Fn2::new(rectangle);
        Self { phasor, osc }
    }
}

impl Op for NaivePulsePhase {
    fn perform(&mut self, stack: &mut Stack) {
        let phase0 = stack.pop();
        let duty_cycle = stack.pop();
        stack.push(&phase0);
        self.phasor.perform(stack);
        stack.push(&duty_cycle);
        self.osc.perform(stack);
    }

    fn migrate(&mut self, other: &mut dyn Op) {
        if let Some(other) = other.downcast_mut::<Self>() {
            self.phasor.migrate_same(&other.phasor);
        }
    }
}

fn poly_blep_pulse_sample(phase: Sample, width: Sample, dx: Sample) -> Sample {
    let width = width.clamp(0.0, 1.0);
    let t = phase_to_unit(phase);
    let mut y = if t < width { 1.0 } else { -1.0 };
    y += poly_blep(t, dx);
    y -= poly_blep((t - width + 1.0) % 1.0, dx);
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
