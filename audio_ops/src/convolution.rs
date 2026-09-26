//! # Convolution
//!
//! Convolve two signals by making dot-product of a N-sample sliding window on both.
//!
//! Sources to connect: input and kernel, but roles are vague in this case.
use crate::buffer::Buffer;
use audio_vm::{CHANNELS, Frame, Op, Stack};
use itertools::izip;

pub struct Convolution {
    window: Buffer<Frame>,
    /// Running sum of `window`, updated in O(1) per frame.
    sum: Frame,
    /// Frames left until `sum` is recomputed exactly, bounding rounding drift.
    frames_until_resum: usize,
    window_size: usize,
}

impl Convolution {
    pub fn new(window_size: usize) -> Self {
        Convolution {
            window: Buffer::new([0.0; CHANNELS], window_size),
            sum: [0.0; CHANNELS],
            frames_until_resum: window_size,
            window_size,
        }
    }

    fn resum(&mut self) {
        self.sum = [0.0; CHANNELS];
        for xs in self.window.iter() {
            for (sample, x) in izip!(&mut self.sum, xs) {
                *sample += x;
            }
        }
    }
}

impl Op for Convolution {
    fn perform(&mut self, stack: &mut Stack) {
        let mut frame = [0.0; CHANNELS];
        let kernel = stack.pop();
        let input = stack.pop();
        for (sample, &x, &y) in izip!(&mut frame, &input, &kernel) {
            *sample = x * y;
        }
        // Index 0 is the oldest frame, the one push_back overwrites.
        let evicted = self.window[0];
        self.window.push_back(frame);

        self.frames_until_resum -= 1;
        if self.frames_until_resum == 0 || self.sum.iter().any(|x| !x.is_finite()) {
            // Also resum while non-finite: inf - inf would otherwise stick as NaN
            // after the offending frame leaves the window.
            self.resum();
            self.frames_until_resum = self.window_size;
        } else {
            for (sample, &x, &old) in izip!(&mut self.sum, &frame, &evicted) {
                *sample += x - old;
            }
        }
        stack.push(&self.sum);
    }

    fn migrate(&mut self, other: &mut dyn Op) {
        if let Some(other) = other.downcast_mut::<Self>()
            && self.window.steal_same_size(&mut other.window)
        {
            self.sum = other.sum;
            self.frames_until_resum = other.frames_until_resum;
        }
    }
}

pub struct ConvolutionM {
    window: Buffer<Frame>,
    kernel: Vec<Frame>,
}

impl ConvolutionM {
    pub fn new(window_size: usize) -> Self {
        let zero = [0.0; CHANNELS];
        ConvolutionM {
            window: Buffer::new(zero, window_size),
            kernel: vec![zero; window_size],
        }
    }
}

impl Op for ConvolutionM {
    fn perform(&mut self, stack: &mut Stack) {
        for kernel in self.kernel.iter_mut().rev() {
            *kernel = stack.pop();
        }
        self.window.push_back(stack.pop());

        // Walk the ring as two contiguous runs so the inner loop has no wrapping.
        let (older, newer) = self.window.as_slices();
        let (older_kernel, newer_kernel) = self.kernel.split_at(older.len());
        let mut frame = [0.0; CHANNELS];
        for (input, kernel) in izip!(older, older_kernel).chain(izip!(newer, newer_kernel)) {
            for (sample, &x, &y) in izip!(&mut frame, input, kernel) {
                *sample += x * y
            }
        }
        stack.push(&frame);
    }

    fn migrate(&mut self, other: &mut dyn Op) {
        if let Some(other) = other.downcast_mut::<Self>() {
            self.window.steal_same_size(&mut other.window);
        }
        // No need to copy kernel.
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use audio_vm::Sample;

    #[test]
    fn running_sum_matches_full_window_sum() {
        let window_size = 7;
        let mut op = Convolution::new(window_size);
        let mut history: Vec<Frame> = Vec::new();
        let mut x: u64 = 3;
        for n in 0..500 {
            x = x
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let a = (x >> 11) as Sample / (1u64 << 53) as Sample * 2.0 - 1.0;
            let b = if n == 100 {
                Sample::NAN
            } else if n == 200 {
                Sample::INFINITY
            } else {
                0.5
            };
            let mut stack = Stack::new();
            stack.push(&[a, -a]);
            stack.push(&[b, b]);
            op.perform(&mut stack);
            let output = stack.pop();

            history.push([a * b, -a * b]);
            let start = history.len().saturating_sub(window_size);
            for channel in 0..CHANNELS {
                let expected: Sample = history[start..].iter().map(|f| f[channel]).sum();
                if expected.is_finite() {
                    assert!((output[channel] - expected).abs() < 1e-12, "frame {n}");
                } else {
                    assert!(!output[channel].is_finite(), "frame {n}");
                }
            }
        }
    }
}
