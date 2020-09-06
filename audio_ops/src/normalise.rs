//! # Normalise
//!
//! Normalise signal to [-1, 1] based on min/max in the given window.
//!
//! Sources to connect: input.

use crate::buffer::Buffer;
use audio_vm::{Frame, Op, Stack, CHANNELS};
use itertools::izip;

pub struct Normalise {
    window: Buffer<Frame>,
}

impl Normalise {
    pub fn new(window_size: usize) -> Self {
        Normalise {
            window: Buffer::new([0.0; CHANNELS], window_size),
        }
    }
}

impl Op for Normalise {
    fn perform(&mut self, stack: &mut Stack) {
        let input = stack.pop();
        self.window.push_back(input);

        let (min, max) =
            self.window
                .iter()
                .fold((f64::INFINITY, f64::NEG_INFINITY), |(min, max), frame| {
                    (
                        frame.iter().fold(min, |min, &x| min.min(x)),
                        frame.iter().fold(max, |max, &x| max.max(x)),
                    )
                });

        let mut frame = [0.0; CHANNELS];
        for (y, &x) in izip!(&mut frame, &input) {
            *y = crate::pure::linlin(x, min, max, -1.0, 1.0);
        }
        stack.push(&frame);
    }

    fn migrate(&mut self, other: &Box<dyn Op>) {
        if let Some(other) = other.downcast_ref::<Self>() {
            self.window.copy_backward(&other.window);
        }
    }
}
