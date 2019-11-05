use audio_vm::{Op, Stack, CHANNELS};
use itertools::izip;

pub struct SampleAndHold {}

impl SampleAndHold {
    pub fn new() -> Self {
        SampleAndHold {}
    }
}

impl Op for SampleAndHold {
    fn perform(&mut self, stack: &mut Stack) {
        let mut frame = [0.0; CHANNELS];
        let input = stack.pop();
        let trigger = stack.pop();
        for (sample, &t, &x) in izip!(&mut frame, &trigger, &input) {
            *sample = *sample * (1.0 - t) + x * t
        }
        stack.push(&frame);
    }
}
