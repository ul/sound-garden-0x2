use audio_vm::{AtomicSample, CHANNELS, Op, Stack};
use std::sync::{Arc, atomic::Ordering};

pub struct Param {
    cell: Arc<AtomicSample>,
}

impl Param {
    pub fn new(cell: Arc<AtomicSample>) -> Self {
        Param { cell }
    }
}

impl Op for Param {
    fn perform(&mut self, stack: &mut Stack) {
        stack.push(&[f64::from_bits(self.cell.load(Ordering::Relaxed)); CHANNELS]);
    }
}
