use crate::delay::Delay;
use audio_vm::{Frame, Op, Stack, CHANNELS};
use itertools::izip;

pub struct Feedback {
    delay: Delay,
    delay_input: Frame,
}

impl Feedback {
    pub fn new(sample_rate: u32, max_delay: f64) -> Self {
        let delay = Delay::new(sample_rate, max_delay);
        Feedback {
            delay,
            delay_input: [0.0; CHANNELS],
        }
    }
}

impl Op for Feedback {
    fn perform(&mut self, stack: &mut Stack) {
        let gain = stack.pop();
        let delay = stack.pop();
        let input = stack.pop();

        stack.push(&self.delay_input);
        stack.push(&delay);

        self.delay.perform(stack);

        let delayed = stack.pop();

        for (sample, &x, &gain, &delayed) in izip!(&mut self.delay_input, &input, &gain, &delayed) {
            *sample = x + gain * delayed;
        }

        stack.push(&self.delay_input);
    }
}
