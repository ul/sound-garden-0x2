use audio_vm::{Frame, Op, Sample, Stack, CHANNELS};
use itertools::izip;

#[derive(Clone)]
pub struct Impulse {
    frame: usize,
    last_trigger: Frame,
    sample_period: Sample,
    trigger_frame: [usize; CHANNELS],
}

impl Impulse {
    pub fn new(sample_rate: u32) -> Self {
        Impulse {
            frame: 0,
            last_trigger: [0.0; CHANNELS],
            sample_period: Sample::from(sample_rate).recip(),
            trigger_frame: [0; CHANNELS],
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
            let time = (self.frame - *trigger_frame) as Sample * self.sample_period;
            let h = time / apex;
            *output = h * (1.0 - h).exp();
            *last_trigger = trigger;
        }
        self.frame += 1;
        stack.push(&frame);
    }

    fn fork(&self) -> Box<dyn Op> {
        Box::new(self.clone())
    }
}
