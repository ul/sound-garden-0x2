use audio_vm::{CHANNELS, Frame, Op, Sample, Stack};
use itertools::izip;

pub struct Crush {
    held: Frame,
    accumulator: Frame,
    sample_rate: Sample,
}

impl Crush {
    pub fn new(sample_rate: u32) -> Self {
        Self {
            held: [0.0; CHANNELS],
            accumulator: [1.0; CHANNELS],
            sample_rate: sample_rate as Sample,
        }
    }
}

#[inline]
fn quantize(x: Sample, bits: Sample) -> Sample {
    if !x.is_finite() {
        return 0.0;
    }
    let bits = bits.clamp(1.0, 32.0);
    if bits >= 32.0 {
        x
    } else {
        let scale = 2.0_f64.powf(bits.floor() - 1.0);
        (x * scale).round() / scale
    }
}

impl Op for Crush {
    fn perform(&mut self, stack: &mut Stack) {
        let rate = stack.pop();
        let bits = stack.pop();
        let input = stack.pop();
        let mut output = [0.0; CHANNELS];

        for (out, held, acc, &x, &bits, &rate) in izip!(
            &mut output,
            &mut self.held,
            &mut self.accumulator,
            &input,
            &bits,
            &rate
        ) {
            let x = quantize(x, bits);
            if rate <= 0.0 || rate >= self.sample_rate || !rate.is_finite() {
                *held = x;
                *acc = 1.0;
            } else {
                if *acc >= 1.0 {
                    *held = x;
                    *acc -= 1.0;
                }
                *acc += rate / self.sample_rate;
            }
            *out = *held;
        }

        stack.push(&output);
    }

    fn migrate(&mut self, other: &mut dyn Op) {
        if let Some(other) = other.downcast_mut::<Self>() {
            self.held = other.held;
            self.accumulator = other.accumulator;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn perform(op: &mut dyn Op, input: Frame, bits: Sample, rate: Sample) -> Frame {
        let mut stack = Stack::new();
        stack.push(&input);
        stack.push(&[bits; CHANNELS]);
        stack.push(&[rate; CHANNELS]);
        op.perform(&mut stack);
        stack.pop()
    }

    #[test]
    fn one_bit_quantization_has_few_levels() {
        let mut op = Crush::new(100);
        let mut levels = BTreeSet::new();
        for i in 0..32 {
            let x = (i as Sample * 0.2).sin();
            levels.insert((perform(&mut op, [x; CHANNELS], 1.0, 100.0)[0] * 1000.0) as i64);
        }
        assert!((2..=3).contains(&levels.len()), "levels: {levels:?}");
    }

    #[test]
    fn rate_reduction_holds_for_four_samples() {
        let mut op = Crush::new(8);
        let ys: Vec<_> = (0..8)
            .map(|i| perform(&mut op, [i as Sample; CHANNELS], 32.0, 2.0)[0])
            .collect();
        assert_eq!(&ys[0..4], &[0.0, 0.0, 0.0, 0.0]);
        assert_eq!(&ys[4..8], &[4.0, 4.0, 4.0, 4.0]);
    }

    #[test]
    fn extreme_args_remain_finite() {
        let mut op = Crush::new(48_000);
        for frame in [
            perform(&mut op, [Sample::INFINITY; CHANNELS], -10.0, -1.0),
            perform(&mut op, [1.0; CHANNELS], 100.0, Sample::INFINITY),
        ] {
            assert!(frame.iter().all(|x| x.is_finite()));
        }
    }
}
