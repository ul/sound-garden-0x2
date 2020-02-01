//! BiQuad Filters
//!
//! Sources to connect: input, cut-off frequency, Q.
use audio_vm::{Frame, Op, Sample, Stack, CHANNELS};
use itertools::izip;

type MakeCoefficients =
    fn(Sample, Sample, Sample) -> (Sample, Sample, Sample, Sample, Sample, Sample);

pub fn make_lpf_coefficients(
    _sin_o: Sample,
    cos_o: Sample,
    alpha: Sample,
) -> (Sample, Sample, Sample, Sample, Sample, Sample) {
    let b1 = 1.0 - cos_o;
    let b0 = 0.5 * b1;
    (b0, b1, b0, 1.0 + alpha, -2.0 * cos_o, 1.0 - alpha)
}

pub fn make_hpf_coefficients(
    _sin_o: Sample,
    cos_o: Sample,
    alpha: Sample,
) -> (Sample, Sample, Sample, Sample, Sample, Sample) {
    let k = 1.0 + cos_o;
    let b0 = 0.5 * k;
    let b1 = -k;
    (b0, b1, b0, 1.0 + alpha, -2.0 * cos_o, 1.0 - alpha)
}

#[derive(Clone)]
pub struct BiQuad {
    make_coefficients: MakeCoefficients,
    sample_angular_period: Sample,
    x1: Frame,
    x2: Frame,
    y1: Frame,
    y2: Frame,
}

impl BiQuad {
    pub fn new(sample_rate: u32, make_coefficients: MakeCoefficients) -> Self {
        let sample_angular_period = 2.0 * std::f64::consts::PI / Sample::from(sample_rate);
        BiQuad {
            make_coefficients,
            sample_angular_period,
            x1: [0.0; CHANNELS],
            x2: [0.0; CHANNELS],
            y1: [0.0; CHANNELS],
            y2: [0.0; CHANNELS],
        }
    }
}

impl Op for BiQuad {
    fn perform(&mut self, stack: &mut Stack) {
        let q = stack.pop();
        let cut_off_freq = stack.pop();
        let input = stack.pop();
        for (y, &x, &frequency, &q, x1, x2, y2) in izip!(
            &mut self.y1,
            &input,
            &cut_off_freq,
            &q,
            &mut self.x1,
            &mut self.x2,
            &mut self.y2
        ) {
            let y1 = *y;

            let o = frequency * self.sample_angular_period;
            let sin_o = o.sin();
            let cos_o = o.cos();
            let alpha = sin_o / (2.0 * q);
            let (b0, b1, b2, a0, a1, a2) = (self.make_coefficients)(sin_o, cos_o, alpha);
            *y = (x * b0 + *x1 * b1 + *x2 * b2 - y1 * a1 - *y2 * a2) / a0;

            *x2 = *x1;
            *x1 = x;
            *y2 = y1;
        }
        stack.push(&self.y1);
    }

    fn fork(&self) -> Box<dyn Op> {
        Box::new(self.clone())
    }
}
