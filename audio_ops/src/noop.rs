use audio_vm::{Op, Stack};

#[derive(Clone)]
pub struct Noop;

impl Noop {
    pub fn new() -> Self {
        Noop {}
    }
}

impl Op for Noop {
    fn perform(&mut self, _stack: &mut Stack) {}

    fn fork(&self) -> Box<dyn Op> {
        Box::new(self.clone())
    }
}
