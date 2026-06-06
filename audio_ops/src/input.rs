use audio_vm::{AtomicFrame, CHANNELS, Op, Stack};
use std::sync::{Arc, atomic::Ordering};

pub struct Input {
    cell: Arc<AtomicFrame>,
}

impl Input {
    pub fn new(cell: Arc<AtomicFrame>) -> Self {
        Input { cell }
    }
}

impl Op for Input {
    fn perform(&mut self, stack: &mut Stack) {
        let mut frame = [0.0; CHANNELS];
        for (a, x) in self.cell.iter().zip(&mut frame) {
            *x = f64::from_bits(a.load(Ordering::Relaxed));
        }
        stack.push(&frame);
    }
}
