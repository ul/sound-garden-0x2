//! # Yin
//!
//! Yin pitch detection algorithm.
//!
//! Sources to connect: signal to detect pitch of.
//!
//! TODO use FFT and avoid O(n^2)
use crate::buffer::Buffer;
use audio_vm::{Frame, Op, Sample, Stack, CHANNELS};

pub struct Yin {
    buffer: Vec<Sample>,
    period: usize,
    frame_number: usize,
    sample_rate: Sample,
    threshold: Sample,
    window: Buffer<Frame>,
}

impl Yin {
    // window_size = 1024
    // threshold = 0.2
    pub fn new(sample_rate: u32, window_size: usize, period: usize, threshold: Sample) -> Self {
        Yin {
            buffer: vec![0.0; window_size / 2],
            period,
            frame_number: 0,
            sample_rate: Sample::from(sample_rate),
            threshold,
            window: Buffer::new(Default::default(), window_size),
        }
    }

    fn difference(&mut self, channel: usize) {
        let buffer_len = self.buffer.len();
        for (tau, x) in self.buffer.iter_mut().enumerate().skip(1) {
            *x = 0.0;
            for i in 0..buffer_len {
                let delta = self.window[i][channel] - self.window[i + tau][channel];
                *x += delta * delta;
            }
        }
    }

    fn cumulative_mean_normalized_difference(&mut self) {
        let mut running_sum = 0.0;
        self.buffer[0] = 1.0;
        for (tau, x) in self.buffer.iter_mut().enumerate().skip(1) {
            running_sum += *x;
            *x *= tau as f64 / running_sum;
        }
    }

    fn absolute_threshold(&self) -> Option<usize> {
        // Search through the array of cumulative mean values, and look for ones that are below the threshold.
        // The first two positions in yinBuffer are always so start at the third (index 2).
        let mut tau = 2;
        let buffer_len = self.buffer.len();
        while tau < buffer_len && !(self.buffer[tau] < self.threshold) {
            tau += 1;
        }
        while tau + 1 < buffer_len && self.buffer[tau + 1] < self.buffer[tau] {
            tau += 1;
        }
        if tau == buffer_len || self.buffer[tau] >= self.threshold {
            None
        } else {
            Some(tau)
        }
    }

    fn parabolic_interpolation(&self, x1: usize) -> Sample {
        let x0 = x1 - 1;
        let x2 = x1 + 1;
        let s0 = self.buffer[x0];
        let s1 = self.buffer[x1];
        if x2 < self.buffer.len() {
            let s2 = self.buffer[x2];
            let d = 2.0 * s1 - s2 - s0;
            let delta = s2 - s0;
            x1 as Sample + if d != 0.0 { delta / (2.0 * d) } else { 0.0 }
        } else if s0 < s1 {
            x0 as Sample
        } else {
            x1 as Sample
        }
    }
}

impl Op for Yin {
    fn perform(&mut self, stack: &mut Stack) {
        let mut frame = [0.0; CHANNELS];
        let input = stack.pop();
        self.window.push_back(input);
        if self.frame_number % self.period == 0 {
            for channel in 0..CHANNELS {
                self.difference(channel);
                self.cumulative_mean_normalized_difference();
                frame[channel] = match self.absolute_threshold() {
                    Some(tau_estimate) => {
                        self.sample_rate / self.parabolic_interpolation(tau_estimate)
                    }
                    None => 0.0,
                }
            }
        }
        self.frame_number += 1;
        stack.push(&frame);
    }
}
