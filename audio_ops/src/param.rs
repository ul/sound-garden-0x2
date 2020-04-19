use audio_vm::{AtomicSample, Op, Stack, CHANNELS};
use std::sync::{atomic::Ordering, Arc};

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
