//! # Pulse wave
//!
//! Sources to connect: frequency, duty cycle.
use crate::function::Fn2;
use crate::phasor::Phasor;
use crate::pure::rectangle;
use audio_vm::{Op, Stack};

#[derive(Clone)]
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

    fn fork(&self) -> Box<dyn Op> {
        Box::new(self.clone())
    }
}
