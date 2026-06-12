use audio_vm::{CHANNELS, Frame, Op, Sample, Stack};

#[inline]
fn release_coefficient(sample_rate: u32, release_seconds: f64) -> Sample {
    let release_seconds = release_seconds.max(Sample::EPSILON);
    (-1.0 / (release_seconds * sample_rate as Sample)).exp()
}

pub struct Limit {
    env: Frame,
    release_coefficient: Sample,
}

impl Limit {
    pub fn new(sample_rate: u32, release_seconds: f64) -> Self {
        Self {
            env: [0.0; CHANNELS],
            release_coefficient: release_coefficient(sample_rate, release_seconds),
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

pub struct Comp {
    env: Frame,
    release_coefficient: Sample,
}

impl Comp {
    pub fn new(sample_rate: u32, release_seconds: f64) -> Self {
        Self {
            env: [0.0; CHANNELS],
            release_coefficient: release_coefficient(sample_rate, release_seconds),
        }
    }
}

impl Op for Comp {
    fn perform(&mut self, stack: &mut Stack) {
        let ratio = stack.pop();
        let threshold = stack.pop();
        let input = stack.pop();
        let mut output = [0.0; CHANNELS];

        for ((((output, env), &x), &threshold), &ratio) in output
            .iter_mut()
            .zip(&mut self.env)
            .zip(&input)
            .zip(&threshold)
            .zip(&ratio)
        {
            if threshold <= 0.0 || ratio <= 1.0 {
                *env = x.abs().max(*env * self.release_coefficient);
                *output = x;
                continue;
            }

            *env = x.abs().max(*env * self.release_coefficient);
            let gain = if *env > threshold && *env > 0.0 {
                (threshold / *env).powf(1.0 - ratio.recip())
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

    fn perform_comp(op: &mut Comp, x: Frame, threshold: Frame, ratio: Frame) -> Frame {
        let mut stack = Stack::new();
        stack.push(&x);
        stack.push(&threshold);
        stack.push(&ratio);
        op.perform(&mut stack);
        stack.pop()
    }

    #[test]
    fn compressor_attenuates_loud_constant_input() {
        let mut comp = Comp::new(100, 0.1);
        let out = perform_comp(&mut comp, [2.0, -2.0], [1.0, 1.0], [2.0, 2.0]);
        let expected = 2.0 * (1.0f64 / 2.0).sqrt();
        assert!(
            (out[0] - expected).abs() < 1.0e-12,
            "{} != {expected}",
            out[0]
        );
        assert!(
            (out[1] + expected).abs() < 1.0e-12,
            "{} != -{expected}",
            out[1]
        );
    }

    #[test]
    fn compressor_passes_quiet_input() {
        let mut comp = Comp::new(100, 0.1);
        assert_eq!(
            perform_comp(&mut comp, [0.5, -0.25], [1.0, 1.0], [4.0, 4.0]),
            [0.5, -0.25]
        );
    }

    #[test]
    fn compressor_extreme_args_stay_finite() {
        let mut comp = Comp::new(100, 0.1);
        for frame in [
            perform_comp(&mut comp, [1.0e100, -1.0e100], [0.0, -1.0], [100.0, 0.5]),
            perform_comp(
                &mut comp,
                [1.0e-100, -1.0e-100],
                [1.0e-50, 1.0e-50],
                [1000.0, 2.0],
            ),
        ] {
            assert!(frame.iter().all(|x| x.is_finite()), "{frame:?}");
        }
    }
}
