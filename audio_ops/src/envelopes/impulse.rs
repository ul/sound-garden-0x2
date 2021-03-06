use audio_vm::{Frame, Op, Sample, Stack, CHANNELS};
use itertools::izip;

pub struct Impulse {
    frame: u64,
    last_trigger: Frame,
    sample_period: Sample,
    trigger_frame: [u64; CHANNELS],
}

impl Impulse {
    pub fn new(sample_rate: u32) -> Self {
        Impulse {
            frame: 0,
            last_trigger: [0.0; CHANNELS],
            sample_period: Sample::from(sample_rate).recip(),
            trigger_frame: [std::u64::MAX; CHANNELS],
        }
    }
}

impl Op for Impulse {
    fn perform(&mut self, stack: &mut Stack) {
        let mut frame = [0.0; CHANNELS];
        let apex = stack.pop();
        let trigger = stack.pop();
        for (output, &trigger, &apex, last_trigger, trigger_frame) in izip!(
            &mut frame,
            &trigger,
            &apex,
            &mut self.last_trigger,
            &mut self.trigger_frame
        ) {
            if *last_trigger <= 0.0 && trigger > 0.0 {
                *trigger_frame = self.frame;
            }
            *last_trigger = trigger;

            if self.frame < *trigger_frame {
                continue;
            }

            let time = (self.frame - *trigger_frame) as Sample * self.sample_period;
            let h = time / apex;
            *output = h * (1.0 - h).exp();
        }
        self.frame += 1;
        stack.push(&frame);
    }

    fn migrate(&mut self, other: &Box<dyn Op>) {
        if let Some(other) = other.downcast_ref::<Self>() {
            self.frame = other.frame;
            self.last_trigger = other.last_trigger;
            self.trigger_frame = other.trigger_frame;
        }
    }
}
