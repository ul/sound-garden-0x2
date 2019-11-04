use crate::delay::Delay;
use audio_vm::{Op, Stack, CHANNELS};
use itertools::izip;

pub struct Feedback {
    delay: Delay,
}

impl Feedback {
    pub fn new(sample_rate: u32, max_delay: f64) -> Self {
        let delay = Delay::new(sample_rate, max_delay);
        Feedback { delay }
    }
}

impl Op for Feedback {
    fn perform(&mut self, stack: &mut Stack) {
        let gain = stack.pop();
        let delay = stack.pop();
        let input = stack.peek();

        stack.push(&delay);
        self.delay.perform(stack);

        let delayed = stack.pop();

        let mut frame = [0.0; CHANNELS];
        for (sample, &x, &gain, &delayed) in izip!(&mut frame, &input, &gain, &delayed) {
            *sample = x + gain * delayed;
        }

        stack.push(&frame);
    }
}
