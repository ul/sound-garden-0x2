use audio_vm::{Op, Sample, Stack, CHANNELS};

pub struct Constant {
    values: [Sample; CHANNELS],
}

impl Constant {
    pub fn new(x: Sample) -> Self {
        Constant {
            values: [x; CHANNELS],
        }
    }
}

impl Op for Constant {
    fn perform(&mut self, stack: &mut Stack) {
        stack.push(&self.values);
    }
}
