use audio_vm::{Op, Stack};

pub struct Noop;

impl Noop {
    pub fn new() -> Self {
        Noop {}
    }
}

impl Op for Noop {
    fn perform(&mut self, _stack: &mut Stack) {}
}
