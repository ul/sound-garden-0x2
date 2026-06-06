//! # Oscillator
//! Sources to connect: frequency.

use crate::function::Fn1;
use crate::phasor::{Phasor, Phasor0};
use audio_vm::{CHANNELS, Frame, Op, Sample, Stack};

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

    fn migrate(&mut self, other: &dyn Op) {
        if let Some(other) = other.downcast_ref::<Self>() {
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
        let dx = self.frequency * self.sample_period;
        let mut frame = [0.0; CHANNELS];

        for (phase, sample) in self.phases.iter_mut().zip(&mut frame) {
            *phase = ((*phase + dx + 1.0) % 2.0) - 1.0;
            *sample = (self.f)(*phase);
        }

        stack.push(&frame);
    }

    fn migrate(&mut self, other: &dyn Op) {
        if let Some(other) = other.downcast_ref::<Self>() {
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

    fn migrate(&mut self, other: &dyn Op) {
        if let Some(other) = other.downcast_ref::<Self>() {
            self.phasor.migrate_same(&other.phasor);
        }
    }
}
