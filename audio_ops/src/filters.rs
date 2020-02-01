//! Filters
//!
//! Basic IIR low/high-pass filters.
//!
//! Sources to connect: input, cut-off frequency.
use audio_vm::{Frame, Op, Sample, Stack, CHANNELS};
use itertools::izip;

#[derive(Clone)]
pub struct LPF {
    output: Frame,
    sample_angular_period: Sample,
}

impl LPF {
    pub fn new(sample_rate: u32) -> Self {
        let sample_angular_period = 2.0 * std::f64::consts::PI / Sample::from(sample_rate);
        LPF {
            output: [0.0; CHANNELS],
            sample_angular_period,
        }
    }
}

impl Op for LPF {
    fn perform(&mut self, stack: &mut Stack) {
        let cut_off_freq = stack.pop();
        let input = stack.pop();
        for (output, &x, &frequency) in izip!(&mut self.output, &input, &cut_off_freq) {
            let k = frequency * self.sample_angular_period;
            let a = k / (k + 1.0);
            *output += a * (x - *output);
        }
        stack.push(&self.output);
    }

    fn fork(&self) -> Box<dyn Op> {
        Box::new(self.clone())
    }
}

#[derive(Clone)]
pub struct HPF {
    output: Frame,
    sample_angular_period: Sample,
    x_prime: Frame,
}

impl HPF {
    pub fn new(sample_rate: u32) -> Self {
        let sample_angular_period = 2.0 * std::f64::consts::PI / Sample::from(sample_rate);
        HPF {
            output: [0.0; CHANNELS],
            sample_angular_period,
            x_prime: [0.0; CHANNELS],
        }
    }
}

impl Op for HPF {
    fn perform(&mut self, stack: &mut Stack) {
        let cut_off_freq = stack.pop();
        let input = stack.pop();
        for (output, &x, &frequency, x_prime) in
            izip!(&mut self.output, &input, &cut_off_freq, &mut self.x_prime)
        {
            let k = frequency * self.sample_angular_period;
            let a = 1.0 / (k + 1.0);
            *output = a * (*output + x - *x_prime);
            *x_prime = x;
        }
        stack.push(&self.output);
    }

    fn fork(&self) -> Box<dyn Op> {
        Box::new(self.clone())
    }
}
