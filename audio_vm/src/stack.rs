use crate::sample::{Frame, Sample, CHANNELS};

pub const STACK_SIZE: usize = 16;
const STACK_CAPACITY: usize = CHANNELS * STACK_SIZE;

pub struct Stack {
    data: [Sample; STACK_CAPACITY],
    top: usize,
}

impl Stack {
    pub fn new() -> Self {
        Stack {
            data: [0.0; STACK_CAPACITY],
            /// Index of the top of the stack (in Samples, not Frames).
            top: 0,
        }
    }

    #[inline]
    pub fn reset(&mut self) {
        self.top = 0;
    }

    #[inline]
    pub fn peek(&self) -> Frame {
        let mut frame = [0.0; CHANNELS];
        if self.top < CHANNELS {
            return frame;
        }
        frame.copy_from_slice(&self.data[(self.top - CHANNELS)..self.top]);
        frame
    }

    #[inline]
    pub fn push(&mut self, frame: &Frame) {
        let new_top = self.top + CHANNELS;
        if new_top > STACK_CAPACITY {
            return;
        }
        self.data[self.top..new_top].copy_from_slice(frame);
        self.top = new_top;
    }

    #[inline]
    pub fn pop(&mut self) -> Frame {
        let mut frame = [0.0; CHANNELS];
        if self.top < CHANNELS {
            return frame;
        }
        let new_top = self.top - CHANNELS;
        frame.copy_from_slice(&self.data[new_top..self.top]);
        self.top = new_top;
        frame
    }
}
