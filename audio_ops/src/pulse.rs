//! # Pulse wave
//!
//! Sources to connect: frequency, duty cycle.
use crate::function::Fn2;
use crate::phasor::{Phasor, Phasor0};
use crate::pure::rectangle;
use audio_vm::{Op, Stack};

pub struct Pulse {
    phasor: Phasor,
    osc: Fn2,
}

impl Pulse {
    pub fn new(sample_rate: u32) -> Self {
        let phasor = Phasor::new(sample_rate);
        let osc = Fn2::new(rectangle);
        Pulse { phasor, osc }
    }
}

impl Op for Pulse {
    fn perform(&mut self, stack: &mut Stack) {
        let duty_cycle = stack.pop();
        self.phasor.perform(stack);
        stack.push(&duty_cycle);
        self.osc.perform(stack);
    }

    fn migrate(&mut self, other: &Box<dyn Op>) {
        if let Some(other) = other.downcast_ref::<Self>() {
            self.phasor.migrate_same(&other.phasor);
        }
    }
}

pub struct PulsePhase {
    phasor: Phasor0,
    osc: Fn2,
}

impl PulsePhase {
    pub fn new(sample_rate: u32) -> Self {
        let phasor = Phasor0::new(sample_rate);
        let osc = Fn2::new(rectangle);
        PulsePhase { phasor, osc }
    }
}

impl Op for PulsePhase {
    fn perform(&mut self, stack: &mut Stack) {
        let phase0 = stack.pop();
        let duty_cycle = stack.pop();
        stack.push(&phase0);
        self.phasor.perform(stack);
        stack.push(&duty_cycle);
        self.osc.perform(stack);
    }

    fn migrate(&mut self, other: &Box<dyn Op>) {
        if let Some(other) = other.downcast_ref::<Self>() {
            self.phasor.migrate_same(&other.phasor);
        }
    }
}
