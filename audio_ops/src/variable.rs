use audio_vm::{AtomicFrame, Op, Stack, CHANNELS};
use std::sync::{atomic::Ordering, Arc};

pub struct WriteVariable {
    cell: Arc<AtomicFrame>,
}

impl WriteVariable {
    pub fn new(cell: Arc<AtomicFrame>) -> Self {
        WriteVariable { cell }
    }
}

impl Op for WriteVariable {
    fn perform(&mut self, stack: &mut Stack) {
        for (a, &x) in self.cell.iter().zip(&stack.peek()) {
            a.store(x.to_bits(), Ordering::Relaxed);
        }
    }
}

pub struct ReadVariable {
    cell: Arc<AtomicFrame>,
}

impl ReadVariable {
    pub fn new(cell: Arc<AtomicFrame>) -> Self {
        ReadVariable { cell }
    }
}

impl Op for ReadVariable {
    fn perform(&mut self, stack: &mut Stack) {
        let mut frame = [0.0; CHANNELS];
        for (a, x) in self.cell.iter().zip(&mut frame) {
            *x = f64::from_bits(a.load(Ordering::Relaxed));
        }
        stack.push(&frame);
    }
}

pub struct TakeVariable {
    cell: Arc<AtomicFrame>,
}

impl TakeVariable {
    pub fn new(cell: Arc<AtomicFrame>) -> Self {
        TakeVariable { cell }
    }
}

impl Op for TakeVariable {
    fn perform(&mut self, stack: &mut Stack) {
        for (a, &x) in self.cell.iter().zip(&stack.pop()) {
            a.store(x.to_bits(), Ordering::Relaxed);
        }
    }
}
