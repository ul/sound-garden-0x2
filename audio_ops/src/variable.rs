use audio_vm::{Frame, Op, Stack};
use std::sync::{Arc, Mutex};

pub struct WriteVariable {
    cell: Arc<Mutex<Frame>>,
}

impl WriteVariable {
    pub fn new(cell: Arc<Mutex<Frame>>) -> Self {
        WriteVariable { cell }
    }
}

impl Op for WriteVariable {
    fn perform(&mut self, stack: &mut Stack) {
        *self.cell.lock().unwrap() = stack.peek();
    }
}

pub struct ReadVariable {
    cell: Arc<Mutex<Frame>>,
}

impl ReadVariable {
    pub fn new(cell: Arc<Mutex<Frame>>) -> Self {
        ReadVariable { cell }
    }
}

impl Op for ReadVariable {
    fn perform(&mut self, stack: &mut Stack) {
        stack.push(&self.cell.lock().unwrap());
    }
}

pub struct TakeVariable {
    cell: Arc<Mutex<Frame>>,
}

impl TakeVariable {
    pub fn new(cell: Arc<Mutex<Frame>>) -> Self {
        TakeVariable { cell }
    }
}

impl Op for TakeVariable {
    fn perform(&mut self, stack: &mut Stack) {
        *self.cell.lock().unwrap() = stack.pop();
    }
}
