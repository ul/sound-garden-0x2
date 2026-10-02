//! Filters
//!
//! Basic IIR low/high-pass filters and a DC blocker.
//!
//! Sources to connect: input, cut-off frequency (none for the DC blocker).
use audio_vm::{CHANNELS, Frame, Op, Sample, Stack};
use itertools::izip;

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

    fn migrate(&mut self, other: &mut dyn Op) {
        if let Some(other) = other.downcast_mut::<Self>() {
            self.output = other.output;
        }
    }
}

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

    fn migrate(&mut self, other: &mut dyn Op) {
        if let Some(other) = other.downcast_mut::<Self>() {
            self.output = other.output;
            self.x_prime = other.x_prime;
        }
    }
}

/// Corner frequency of the DC blocker: low enough to keep a 30 Hz kick, high
/// enough to settle within a fraction of a second.
const DC_BLOCK_CORNER: Sample = 10.0;

/// One-zero, one-pole DC blocker, `y = x - x' + R y'`.
/// https://ccrma.stanford.edu/~jos/filters/DC_Blocker_Software_Implementations.html
pub struct DCBlock {
    output: Frame,
    pole: Sample,
    x_prime: Frame,
}

impl DCBlock {
    pub fn new(sample_rate: u32) -> Self {
        let pole = 1.0 - 2.0 * std::f64::consts::PI * DC_BLOCK_CORNER / Sample::from(sample_rate);
        DCBlock {
            output: [0.0; CHANNELS],
            pole,
            x_prime: [0.0; CHANNELS],
        }
    }
}

impl Op for DCBlock {
    fn perform(&mut self, stack: &mut Stack) {
        let input = stack.pop();
        for (output, &x, x_prime) in izip!(&mut self.output, &input, &mut self.x_prime) {
            *output = x - *x_prime + self.pole * *output;
            *x_prime = x;
        }
        stack.push(&self.output);
    }

    fn migrate(&mut self, other: &mut dyn Op) {
        if let Some(other) = other.downcast_mut::<Self>() {
            self.output = other.output;
            self.x_prime = other.x_prime;
        }
    }
}
