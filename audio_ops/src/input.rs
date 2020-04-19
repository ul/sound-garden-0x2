use audio_vm::{AtomicFrame, Op, Stack, CHANNELS};
use std::sync::{atomic::Ordering, Arc};

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
