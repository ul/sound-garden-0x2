//! # Convolution
//!
//! Convolve two signals by making dot-product of a N-sample sliding window on both.
//!
//! Sources to connect: input and kernel, but roles are vague in this case.
use crate::buffer::Buffer;
use audio_vm::{Frame, Op, Stack, CHANNELS};
use itertools::izip;

pub struct Convolution {
    window: Buffer<Frame>,
}

impl Convolution {
    pub fn new(window_size: usize) -> Self {
        Convolution {
            window: Buffer::new([0.0; CHANNELS], window_size),
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
        self.window.push_back(frame);

        let mut frame = [0.0; CHANNELS];
        for xs in self.window.iter() {
            for (sample, x) in izip!(&mut frame, xs) {
                *sample += x;
            }
        }
        stack.push(&frame);
    }

    fn migrate(&mut self, other: &Box<dyn Op>) {
        if let Some(other) = other.downcast_ref::<Self>() {
            self.window.copy_backward(&other.window);
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

        let mut frame = [0.0; CHANNELS];
        for (input, kernel) in izip!(self.window.iter(), &self.kernel) {
            for (sample, &x, &y) in izip!(&mut frame, input, kernel) {
                *sample += x * y
            }
        }
        stack.push(&frame);
    }

    fn migrate(&mut self, other: &Box<dyn Op>) {
        if let Some(other) = other.downcast_ref::<Self>() {
            self.window.copy_backward(&other.window);
        }
        // No need to copy kernel.
    }
}
