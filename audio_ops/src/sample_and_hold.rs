use audio_vm::{Frame, Op, Stack, CHANNELS};
use itertools::izip;

pub struct SampleAndHold {
    output: Frame,
}

impl SampleAndHold {
    pub fn new() -> Self {
        SampleAndHold {
            output: [0.0; CHANNELS],
        }
    }
}

impl Op for SampleAndHold {
    fn perform(&mut self, stack: &mut Stack) {
        let trigger = stack.pop();
        let input = stack.pop();
        for (sample, &t, &x) in izip!(&mut self.output, &trigger, &input) {
            *sample = *sample * (1.0 - t) + x * t
        }
        stack.push(&self.output);
    }
}
