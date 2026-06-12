use audio_vm::{CHANNELS, Frame, Op, Stack};
use itertools::izip;

pub struct SampleAndHold {
    hold: Frame,
    previous_trigger: Frame,
}

impl Default for SampleAndHold {
    fn default() -> Self {
        Self::new()
    }
}

impl SampleAndHold {
    pub fn new() -> Self {
        SampleAndHold {
            hold: [0.0; CHANNELS],
            previous_trigger: [0.0; CHANNELS],
        }
    }
}

impl Op for SampleAndHold {
    fn perform(&mut self, stack: &mut Stack) {
        let trigger = stack.pop();
        let input = stack.pop();
        for (sample, previous_trigger, &t, &x) in
            izip!(&mut self.hold, &mut self.previous_trigger, &trigger, &input)
        {
            if *previous_trigger <= 0.0 && t > 0.0 {
                *sample = x;
            }
            *previous_trigger = t;
        }
        stack.push(&self.hold);
    }

    fn migrate(&mut self, other: &mut dyn Op) {
        if let Some(other) = other.downcast_mut::<Self>() {
            self.hold = other.hold;
            self.previous_trigger = other.previous_trigger;
        }
    }
}

pub struct SmoothSampleAndHold {
    output: Frame,
}

impl Default for SmoothSampleAndHold {
    fn default() -> Self {
        Self::new()
    }
}

impl SmoothSampleAndHold {
    pub fn new() -> Self {
        SmoothSampleAndHold {
            output: [0.0; CHANNELS],
        }
    }
}

impl Op for SmoothSampleAndHold {
    fn perform(&mut self, stack: &mut Stack) {
        let trigger = stack.pop();
        let input = stack.pop();
        for (sample, &t, &x) in izip!(&mut self.output, &trigger, &input) {
            let t = t.clamp(0.0, 1.0);
            *sample = *sample * (1.0 - t) + x * t
        }
        stack.push(&self.output);
    }

    fn migrate(&mut self, other: &mut dyn Op) {
        if let Some(other) = other.downcast_mut::<Self>() {
            self.output = other.output;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn perform(op: &mut dyn Op, input: Frame, trigger: Frame) -> Frame {
        let mut stack = Stack::new();
        stack.push(&input);
        stack.push(&trigger);
        op.perform(&mut stack);
        stack.pop()
    }

    #[test]
    fn sample_and_hold_samples_only_on_rising_edges() {
        let mut sh = SampleAndHold::new();

        assert_eq!(perform(&mut sh, [1.0, 2.0], [1.0, 1.0]), [1.0, 2.0]);
        assert_eq!(perform(&mut sh, [3.0, 4.0], [1.0, 1.0]), [1.0, 2.0]);
        assert_eq!(perform(&mut sh, [5.0, 6.0], [0.0, -1.0]), [1.0, 2.0]);
        assert_eq!(perform(&mut sh, [7.0, 8.0], [1.0, 1.0]), [7.0, 8.0]);
    }

    #[test]
    fn smooth_sample_and_hold_clamps_trigger() {
        let mut ssh = SmoothSampleAndHold::new();

        assert_eq!(perform(&mut ssh, [1.0, 1.0], [2.0, -1.0]), [1.0, 0.0]);
    }
}
