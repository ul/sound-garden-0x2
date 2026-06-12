//! # Normalise
//!
//! Normalise signal to [-1, 1] based on min/max in the given window.
//!
//! Sources to connect: input.

use crate::buffer::Buffer;
use audio_vm::{CHANNELS, Frame, Op, Sample, Stack};
use itertools::izip;
use std::collections::VecDeque;

pub struct Normalise {
    window: Buffer<Frame>,
    min: Sample,
    max: Sample,
    q: Sample,
    index: usize,
    window_size: usize,
    min_deque: VecDeque<(usize, Sample)>,
    max_deque: VecDeque<(usize, Sample)>,
}

impl Normalise {
    pub fn new(window_size: usize) -> Self {
        let mut normalise = Normalise {
            window: Buffer::new([0.0; CHANNELS], window_size),
            min: 0.0,
            max: 0.0,
            q: (window_size as Sample).recip(),
            index: window_size,
            window_size,
            min_deque: VecDeque::with_capacity(window_size * CHANNELS),
            max_deque: VecDeque::with_capacity(window_size * CHANNELS),
        };
        normalise.rebuild_deques();
        normalise
    }

    fn rebuild_deques(&mut self) {
        self.min_deque.clear();
        self.max_deque.clear();
        let start = self.index.saturating_sub(self.window_size);
        let frames: Vec<_> = self.window.iter().copied().collect();
        for (offset, frame) in frames.into_iter().enumerate() {
            self.push_deque_values(start + offset, &frame);
        }
    }

    fn push_deque_values(&mut self, index: usize, frame: &Frame) {
        while self
            .min_deque
            .front()
            .is_some_and(|(i, _)| *i + self.window_size <= index)
        {
            self.min_deque.pop_front();
        }
        while self
            .max_deque
            .front()
            .is_some_and(|(i, _)| *i + self.window_size <= index)
        {
            self.max_deque.pop_front();
        }

        for &x in frame {
            while self.min_deque.back().is_some_and(|(_, v)| *v > x) {
                self.min_deque.pop_back();
            }
            self.min_deque.push_back((index, x));

            while self.max_deque.back().is_some_and(|(_, v)| *v < x) {
                self.max_deque.pop_back();
            }
            self.max_deque.push_back((index, x));
        }
    }
}

impl Op for Normalise {
    fn perform(&mut self, stack: &mut Stack) {
        let input = stack.pop();
        self.window.push_back(input);
        self.push_deque_values(self.index, &input);
        self.index = self.index.wrapping_add(1);

        let min = self.min_deque.front().map(|(_, x)| *x).unwrap_or(0.0);
        let max = self.max_deque.front().map(|(_, x)| *x).unwrap_or(0.0);

        self.min += self.q * (min - self.min);
        self.max += self.q * (max - self.max);

        let mut frame = [0.0; CHANNELS];
        if min != max {
            for (y, &x) in izip!(&mut frame, &input) {
                *y = crate::pure::linlin(x, self.min, self.max, -1.0, 1.0);
            }
        }
        stack.push(&frame);
    }

    fn migrate(&mut self, other: &mut dyn Op) {
        if let Some(other) = other.downcast_mut::<Self>() {
            self.window.steal_same_size(&mut other.window);
            self.min = other.min;
            self.max = other.max;
            self.index = other.index;
            self.rebuild_deques();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn brute_force_step(
        history: &mut Vec<Frame>,
        input: Frame,
        window_size: usize,
        min_state: &mut Sample,
        max_state: &mut Sample,
    ) -> Frame {
        history.push(input);
        if history.len() > window_size {
            history.remove(0);
        }
        let mut min = Sample::INFINITY;
        let mut max = Sample::NEG_INFINITY;
        for frame in history
            .iter()
            .chain(std::iter::repeat(&[0.0; CHANNELS]))
            .take(window_size)
        {
            for &x in frame {
                min = min.min(x);
                max = max.max(x);
            }
        }
        let q = (window_size as Sample).recip();
        *min_state += q * (min - *min_state);
        *max_state += q * (max - *max_state);
        let mut out = [0.0; CHANNELS];
        if min != max {
            for (y, &x) in izip!(&mut out, &input) {
                *y = crate::pure::linlin(x, *min_state, *max_state, -1.0, 1.0);
            }
        }
        out
    }

    fn perform(op: &mut Normalise, input: Frame) -> Frame {
        let mut stack = Stack::new();
        stack.push(&input);
        op.perform(&mut stack);
        stack.pop()
    }

    #[test]
    fn matches_brute_force_windowed_min_max() {
        let window_size = 8;
        let mut op = Normalise::new(window_size);
        let mut history = Vec::new();
        let mut min_state = 0.0;
        let mut max_state = 0.0;
        let mut seed = 0x1234_5678_u64;

        for _ in 0..128 {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            let a = ((seed >> 33) as Sample / u32::MAX as Sample) * 2.0 - 1.0;
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            let b = ((seed >> 33) as Sample / u32::MAX as Sample) * 2.0 - 1.0;
            let input = [a, b];
            let expected = brute_force_step(
                &mut history,
                input,
                window_size,
                &mut min_state,
                &mut max_state,
            );
            let actual = perform(&mut op, input);
            assert!((actual[0] - expected[0]).abs() < 1e-12);
            assert!((actual[1] - expected[1]).abs() < 1e-12);
        }
    }

    #[test]
    fn flat_input_outputs_zero_after_window_is_flat() {
        let mut op = Normalise::new(4);
        let mut out = [0.0; CHANNELS];
        for _ in 0..8 {
            out = perform(&mut op, [0.5, 0.5]);
        }
        assert_eq!(out, [0.0; CHANNELS]);
    }
}
