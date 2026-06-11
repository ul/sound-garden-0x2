use crate::delay::Delay;
use audio_vm::{CHANNELS, Frame, Op, Sample, Stack};
use itertools::izip;

pub struct Feedback {
    delay: Delay,
    delay_input: Frame,
    shaper: fn(Sample) -> Sample,
}

impl Feedback {
    pub fn new(sample_rate: u32, max_delay: f64) -> Self {
        Self::with_shaper(sample_rate, max_delay, |x| x)
    }

    pub fn with_shaper(sample_rate: u32, max_delay: f64, shaper: fn(Sample) -> Sample) -> Self {
        let delay = Delay::new(sample_rate, max_delay);
        Feedback {
            delay,
            delay_input: [0.0; CHANNELS],
            shaper,
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
            *sample = (self.shaper)(x + gain * delayed);
        }

        stack.push(&self.delay_input);
    }

    fn migrate(&mut self, other: &mut dyn Op) {
        if let Some(other) = other.downcast_mut::<Self>() {
            self.delay.migrate_same(&mut other.delay);
            self.delay_input = other.delay_input;
        }
    }
}
