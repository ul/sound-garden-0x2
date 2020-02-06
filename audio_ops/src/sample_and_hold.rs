use audio_vm::{Frame, Op, Stack, CHANNELS};
use itertools::izip;

pub struct SampleAndHold {
    hold: Frame,
}

impl SampleAndHold {
    pub fn new() -> Self {
        SampleAndHold {
            hold: [0.0; CHANNELS],
        }
    }
}

impl Op for SampleAndHold {
    fn perform(&mut self, stack: &mut Stack) {
        let trigger = stack.pop();
        let input = stack.pop();
        for (sample, &t, &x) in izip!(&mut self.hold, &trigger, &input) {
            if t > 0.0 {
                *sample = x;
            }
        }
        stack.push(&self.hold);
    }

    fn migrate(&mut self, other: &Box<dyn Op>) {
        if let Some(other) = other.downcast_ref::<Self>() {
            self.hold = other.hold;
        }
    }
}

pub struct SmoothSampleAndHold {
    output: Frame,
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
            *sample = *sample * (1.0 - t) + x * t
        }
        stack.push(&self.output);
    }

    fn migrate(&mut self, other: &Box<dyn Op>) {
        if let Some(other) = other.downcast_ref::<Self>() {
            self.output = other.output;
        }
    }
}
