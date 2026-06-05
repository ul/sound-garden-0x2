use crate::sample::{Frame, Sample, CHANNELS};

const STACK_SIZE: usize = 16;
const STACK_CAPACITY: usize = CHANNELS * STACK_SIZE;

/// Simple fixed capacity stack tolerant to {over,under}flows.
pub struct Stack {
    /// Index of the top of the stack (in Samples, not Frames).
    top: usize,
    data: [Sample; STACK_CAPACITY],
}

impl Stack {
    pub fn new() -> Self {
        Stack {
            data: [0.0; STACK_CAPACITY],
            // Index of the top of the stack (in Samples, not Frames).
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
        if self.top >= CHANNELS {
            frame.copy_from_slice(&self.data[(self.top - CHANNELS)..self.top]);
        }
        frame
    }

    #[inline]
    pub fn pop(&mut self) -> Frame {
        let mut frame = [0.0; CHANNELS];
        if self.top >= CHANNELS {
            let new_top = self.top - CHANNELS;
            frame.copy_from_slice(&self.data[new_top..self.top]);
            self.top = new_top;
        }
        frame
    }

    #[inline]
    pub fn push(&mut self, frame: &Frame) {
        let new_top = self.top + CHANNELS;
        if new_top <= STACK_CAPACITY {
            self.data[self.top..new_top].copy_from_slice(frame);
            self.top = new_top;
        }
    }
}

impl Default for Stack {
    fn default() -> Self {
        Stack::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_stack_peeks_and_pops_silence() {
        let mut stack = Stack::new();

        assert_eq!(stack.peek(), [0.0, 0.0]);
        assert_eq!(stack.pop(), [0.0, 0.0]);
        assert_eq!(stack.peek(), [0.0, 0.0]);
    }

    #[test]
    fn push_peek_and_pop_are_lifo_by_frame() {
        let mut stack = Stack::new();

        stack.push(&[1.0, 2.0]);
        stack.push(&[3.0, 4.0]);

        assert_eq!(stack.peek(), [3.0, 4.0]);
        assert_eq!(stack.pop(), [3.0, 4.0]);
        assert_eq!(stack.pop(), [1.0, 2.0]);
        assert_eq!(stack.pop(), [0.0, 0.0]);
    }

    #[test]
    fn reset_discards_frames() {
        let mut stack = Stack::new();
        stack.push(&[1.0, 2.0]);

        stack.reset();

        assert_eq!(stack.peek(), [0.0, 0.0]);
    }

    #[test]
    fn push_ignores_frames_after_capacity() {
        let mut stack = Stack::new();

        for i in 0..STACK_SIZE {
            let x = i as Sample;
            stack.push(&[x, -x]);
        }
        stack.push(&[99.0, 99.0]);

        let top = (STACK_SIZE - 1) as Sample;
        assert_eq!(stack.peek(), [top, -top]);
    }
}
