use audio_vm::{CHANNELS, Frame, Op, Sample, Stack};

#[inline]
fn coefficient(time: Sample, sample_rate: Sample) -> Sample {
    let time = time.clamp(0.0, 60.0);
    if time <= 0.0 {
        0.0
    } else {
        (-1.0 / (time * sample_rate)).exp()
    }
}

pub struct Lag {
    y: Frame,
    last_time: Frame,
    coefficients: Frame,
    sample_rate: Sample,
}

impl Lag {
    pub fn new(sample_rate: u32) -> Self {
        Self {
            y: [0.0; CHANNELS],
            last_time: [Sample::NAN; CHANNELS],
            coefficients: [0.0; CHANNELS],
            sample_rate: sample_rate as Sample,
        }
    }
}

impl Op for Lag {
    fn perform(&mut self, stack: &mut Stack) {
        let time = stack.pop();
        let input = stack.pop();
        let mut output = [0.0; CHANNELS];

        for channel in 0..CHANNELS {
            let time = time[channel].clamp(0.0, 60.0);
            if time != self.last_time[channel] {
                self.last_time[channel] = time;
                self.coefficients[channel] = coefficient(time, self.sample_rate);
            }
            let a = self.coefficients[channel];
            self.y[channel] = if time <= 0.0 {
                input[channel]
            } else {
                input[channel] + a * (self.y[channel] - input[channel])
            };
            output[channel] = self.y[channel];
        }

        stack.push(&output);
    }

    fn migrate(&mut self, other: &mut dyn Op) {
        if let Some(other) = other.downcast_mut::<Self>() {
            self.y = other.y;
            self.last_time = other.last_time;
            self.coefficients = other.coefficients;
        }
    }
}

pub struct Lag2 {
    y: Frame,
    last_up: Frame,
    last_down: Frame,
    up_coefficients: Frame,
    down_coefficients: Frame,
    sample_rate: Sample,
}

impl Lag2 {
    pub fn new(sample_rate: u32) -> Self {
        Self {
            y: [0.0; CHANNELS],
            last_up: [Sample::NAN; CHANNELS],
            last_down: [Sample::NAN; CHANNELS],
            up_coefficients: [0.0; CHANNELS],
            down_coefficients: [0.0; CHANNELS],
            sample_rate: sample_rate as Sample,
        }
    }
}

impl Op for Lag2 {
    fn perform(&mut self, stack: &mut Stack) {
        let down = stack.pop();
        let up = stack.pop();
        let input = stack.pop();
        let mut output = [0.0; CHANNELS];

        for channel in 0..CHANNELS {
            let up = up[channel].clamp(0.0, 60.0);
            if up != self.last_up[channel] {
                self.last_up[channel] = up;
                self.up_coefficients[channel] = coefficient(up, self.sample_rate);
            }
            let down = down[channel].clamp(0.0, 60.0);
            if down != self.last_down[channel] {
                self.last_down[channel] = down;
                self.down_coefficients[channel] = coefficient(down, self.sample_rate);
            }

            let time = if input[channel] > self.y[channel] {
                up
            } else if input[channel] < self.y[channel] {
                down
            } else {
                0.0
            };
            let a = if input[channel] > self.y[channel] {
                self.up_coefficients[channel]
            } else {
                self.down_coefficients[channel]
            };
            self.y[channel] = if time <= 0.0 {
                input[channel]
            } else {
                input[channel] + a * (self.y[channel] - input[channel])
            };
            output[channel] = self.y[channel];
        }

        stack.push(&output);
    }

    fn migrate(&mut self, other: &mut dyn Op) {
        if let Some(other) = other.downcast_mut::<Self>() {
            self.y = other.y;
            self.last_up = other.last_up;
            self.last_down = other.last_down;
            self.up_coefficients = other.up_coefficients;
            self.down_coefficients = other.down_coefficients;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn perform_lag(op: &mut Lag, x: Sample, time: Sample) -> Sample {
        let mut stack = Stack::new();
        stack.push(&[x; CHANNELS]);
        stack.push(&[time; CHANNELS]);
        op.perform(&mut stack);
        stack.pop()[0]
    }

    fn perform_lag2(op: &mut Lag2, x: Sample, up: Sample, down: Sample) -> Sample {
        let mut stack = Stack::new();
        stack.push(&[x; CHANNELS]);
        stack.push(&[up; CHANNELS]);
        stack.push(&[down; CHANNELS]);
        op.perform(&mut stack);
        stack.pop()[0]
    }

    #[test]
    fn lag_step_reaches_about_one_time_constant() {
        let mut lag = Lag::new(100);
        let mut y = 0.0;
        for _ in 0..10 {
            y = perform_lag(&mut lag, 1.0, 0.1);
        }
        assert!((y - (1.0 - (-1.0f64).exp())).abs() < 1.0e-12, "{y}");
    }

    #[test]
    fn lag_step_is_monotonic() {
        let mut lag = Lag::new(100);
        let mut previous = 0.0;
        for _ in 0..32 {
            let y = perform_lag(&mut lag, 1.0, 0.2);
            assert!(y >= previous);
            previous = y;
        }
    }

    #[test]
    fn lag_zero_time_passes_through() {
        let mut lag = Lag::new(100);
        assert_eq!(perform_lag(&mut lag, 0.75, 0.0), 0.75);
        assert_eq!(perform_lag(&mut lag, -0.25, -1.0), -0.25);
    }

    #[test]
    fn lag2_uses_separate_rise_and_fall_times() {
        let mut lag2 = Lag2::new(100);
        let rising = perform_lag2(&mut lag2, 1.0, 0.01, 1.0);
        let falling = perform_lag2(&mut lag2, 0.0, 0.01, 1.0);
        assert!(rising > 0.5, "{rising}");
        assert!(falling > rising * 0.9, "{falling} <= {rising}");
    }
}
