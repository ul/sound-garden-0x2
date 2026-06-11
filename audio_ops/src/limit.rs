use audio_vm::{CHANNELS, Frame, Op, Sample, Stack};

pub struct Limit {
    env: Frame,
    release_coefficient: Sample,
}

impl Limit {
    pub fn new(sample_rate: u32, release_seconds: f64) -> Self {
        let release_seconds = release_seconds.max(Sample::EPSILON);
        Self {
            env: [0.0; CHANNELS],
            release_coefficient: (-1.0 / (release_seconds * sample_rate as Sample)).exp(),
        }
    }
}

impl Op for Limit {
    fn perform(&mut self, stack: &mut Stack) {
        let threshold = stack.pop();
        let input = stack.pop();
        let mut output = [0.0; CHANNELS];

        for (((output, env), &x), &threshold) in output
            .iter_mut()
            .zip(&mut self.env)
            .zip(&input)
            .zip(&threshold)
        {
            if threshold <= 0.0 {
                *env = 0.0;
                *output = 0.0;
                continue;
            }

            *env = x.abs().max(*env * self.release_coefficient);
            let gain = if *env > threshold && *env > 0.0 {
                threshold / *env
            } else {
                1.0
            };
            *output = x * gain;
        }

        stack.push(&output);
    }

    fn migrate(&mut self, other: &mut dyn Op) {
        if let Some(other) = other.downcast_mut::<Self>() {
            self.env = other.env;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn perform(op: &mut Limit, x: Frame, threshold: Frame) -> Frame {
        let mut stack = Stack::new();
        stack.push(&x);
        stack.push(&threshold);
        op.perform(&mut stack);
        stack.pop()
    }

    #[test]
    fn signal_below_threshold_passes_unchanged() {
        let mut limit = Limit::new(100, 0.1);
        assert_eq!(perform(&mut limit, [0.5, -0.25], [1.0, 1.0]), [0.5, -0.25]);
    }

    #[test]
    fn constant_loud_signal_is_limited_immediately() {
        let mut limit = Limit::new(100, 0.1);
        assert_eq!(perform(&mut limit, [2.0, -2.0], [1.0, 1.0]), [1.0, -1.0]);
        assert_eq!(perform(&mut limit, [2.0, -2.0], [1.0, 1.0]), [1.0, -1.0]);
    }

    #[test]
    fn release_recovers_gain_toward_one() {
        let mut limit = Limit::new(10, 0.1);
        assert_eq!(perform(&mut limit, [10.0, 10.0], [1.0, 1.0]), [1.0, 1.0]);
        let first = perform(&mut limit, [0.5, 0.5], [1.0, 1.0]);
        let second = perform(&mut limit, [0.5, 0.5], [1.0, 1.0]);

        assert!(first[0] < second[0]);
        assert!(second[0] <= 0.5);
    }

    #[test]
    fn zero_threshold_outputs_silence() {
        let mut limit = Limit::new(100, 0.1);
        assert_eq!(perform(&mut limit, [1.0, -1.0], [0.0, 0.0]), [0.0, 0.0]);
    }
}
