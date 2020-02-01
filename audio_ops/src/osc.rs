//! # Oscillator
//!
//! Sources to connect: frequency.
use crate::function::Fn1;
use crate::phasor::{Phasor, Phasor0};
use audio_vm::{Op, Sample, Stack};

#[derive(Clone)]
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

    fn fork(&self) -> Box<dyn Op> {
        Box::new(self.clone())
    }
}

#[derive(Clone)]
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

    fn fork(&self) -> Box<dyn Op> {
        Box::new(self.clone())
    }
}
