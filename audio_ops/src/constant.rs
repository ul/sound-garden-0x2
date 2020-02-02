use audio_vm::{Frame, Op, Sample, Stack, CHANNELS};

pub struct Constant {
    values: Frame,
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
