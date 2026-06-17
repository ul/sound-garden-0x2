use audio_vm::{CHANNELS, Frame, Op, Sample, Stack};
use itertools::izip;

pub struct Impulse {
    frame: u64,
    last_trigger: Frame,
    sample_period: Sample,
    trigger_frame: [u64; CHANNELS],
    trigger_amplitude: Frame,
}

impl Impulse {
    pub fn new(sample_rate: u32) -> Self {
        Impulse {
            frame: 0,
            last_trigger: [0.0; CHANNELS],
            sample_period: Sample::from(sample_rate).recip(),
            trigger_frame: [u64::MAX; CHANNELS],
            trigger_amplitude: [0.0; CHANNELS],
        }
    }
}

impl Op for Impulse {
    fn perform(&mut self, stack: &mut Stack) {
        let mut frame = [0.0; CHANNELS];
        let apex = stack.pop();
        let trigger = stack.pop();
        for (output, &trigger, &apex, last_trigger, trigger_frame, trigger_amplitude) in izip!(
            &mut frame,
            &trigger,
            &apex,
            &mut self.last_trigger,
            &mut self.trigger_frame,
            &mut self.trigger_amplitude
        ) {
            if *last_trigger <= 0.0 && trigger > 0.0 {
                *trigger_frame = self.frame;
                *trigger_amplitude = trigger;
            }
            *last_trigger = trigger;

            if self.frame < *trigger_frame {
                continue;
            }

            let time = (self.frame - *trigger_frame) as Sample * self.sample_period;
            let h = time / apex;
            *output = *trigger_amplitude * h * (1.0 - h).exp();
        }
        self.frame += 1;
        stack.push(&frame);
    }

    fn migrate(&mut self, other: &mut dyn Op) {
        if let Some(other) = other.downcast_mut::<Self>() {
            self.frame = other.frame;
            self.last_trigger = other.last_trigger;
            self.trigger_frame = other.trigger_frame;
            self.trigger_amplitude = other.trigger_amplitude;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn perform(op: &mut Impulse, trigger: Sample, apex: Sample) -> Sample {
        let mut stack = Stack::new();
        stack.push(&[trigger; CHANNELS]);
        stack.push(&[apex; CHANNELS]);
        op.perform(&mut stack);
        stack.pop()[0]
    }

    #[test]
    fn trigger_amplitude_scales_whole_impulse_and_is_latched() {
        let mut low = Impulse::new(1000);
        let mut high = Impulse::new(1000);
        perform(&mut low, 0.5, 0.01);
        perform(&mut high, 1.0, 0.01);
        let mut low_y = 0.0;
        let mut high_y = 0.0;
        for _ in 0..10 {
            low_y = perform(&mut low, 1.0, 0.01);
            high_y = perform(&mut high, 1.0, 0.01);
        }
        assert!((low_y * 2.0 - high_y).abs() < 1e-9);
        assert!(low_y > 0.0);
    }
}
