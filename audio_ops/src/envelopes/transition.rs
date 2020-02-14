use audio_vm::{Frame, Op, Sample, Stack};
use itertools::izip;

// (a, dx) -> dy
pub type Curve = fn(Sample, Sample) -> Sample;

pub struct Transition {
    curve: Curve,
    start: Frame,
    previous_value: Frame,
    current_value: Frame,
    next_value: Frame,
    sample_period: Sample,
    frame: usize,
}

impl Transition {
    pub fn new(sample_rate: u32, curve: Curve) -> Self {
        Transition {
            curve,
            start: Default::default(),
            previous_value: Default::default(),
            current_value: Default::default(),
            next_value: Default::default(),
            sample_period: Sample::from(sample_rate).recip(),
            frame: 0,
        }
    }
}

impl Op for Transition {
    fn perform(&mut self, stack: &mut Stack) {
        let delta = stack.pop();
        let value = stack.pop();
        let now = self.frame as Sample * self.sample_period;
        for (&value, &delta, start, previous_value, current_value, next_value) in izip!(
            &value,
            &delta,
            &mut self.start,
            &mut self.previous_value,
            &mut self.current_value,
            &mut self.next_value
        ) {
            if value != *next_value {
                *previous_value = *current_value;
                *next_value = value;
                *start = now;
            }
            let dt = now - *start;
            *current_value = if delta > 0.0 && dt < delta {
                *previous_value + ((self.curve)(dt / delta, value - *previous_value))
            } else {
                value
            }
        }
        self.frame += 1;
        stack.push(&self.current_value);
    }

    fn migrate(&mut self, other: &Box<dyn Op>) {
        if let Some(other) = other.downcast_ref::<Self>() {
            self.start = other.start;
            self.previous_value = other.previous_value;
            self.current_value = other.current_value;
            self.next_value = other.next_value;
            self.frame = other.frame;
        }
    }
}
