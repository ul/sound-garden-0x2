//! # Normalise
//!
//! Normalise signal to [-1, 1] based on min/max in the given window.
//!
//! Sources to connect: input.

use crate::buffer::Buffer;
use audio_vm::{Frame, Op, Sample, Stack, CHANNELS};
use itertools::izip;

pub struct Normalise {
    window: Buffer<Frame>,
    min: Sample,
    max: Sample,
    q: Sample,
}

impl Normalise {
    pub fn new(window_size: usize) -> Self {
        Normalise {
            window: Buffer::new([0.0; CHANNELS], window_size),
            min: 0.0,
            max: 0.0,
            q: (window_size as Sample).recip(),
        }
    }
}

impl Op for Normalise {
    fn perform(&mut self, stack: &mut Stack) {
        let input = stack.pop();
        self.window.push_back(input);

        let mut min = f64::INFINITY;
        let mut max = f64::NEG_INFINITY;
        for frame in self.window.iter() {
            for &x in frame.iter() {
                min = min.min(x);
                max = max.max(x);
            }
        }

        self.min += self.q * (min - self.min);
        self.max += self.q * (max - self.max);

        let mut frame = [0.0; CHANNELS];
        for (y, &x) in izip!(&mut frame, &input) {
            *y = crate::pure::linlin(x, self.min, self.max, -1.0, 1.0);
        }
        stack.push(&frame);
    }

    fn migrate(&mut self, other: &Box<dyn Op>) {
        if let Some(other) = other.downcast_ref::<Self>() {
            self.window.copy_backward(&other.window);
            self.min = other.min;
            self.max = other.max;
        }
    }
}
