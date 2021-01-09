//! # Spectral transform
//!
//! Do a Fft of the input signal, transform bins, and produce an output signal with IFft.
//!
//! Source to connect: input.
use audio_vm::{Op, Sample, Stack, CHANNELS};
use itertools::izip;
use rustfft::num_complex::Complex;
use rustfft::num_traits::Zero;
use rustfft::Fft;
use rustfft::{
    algorithm::Radix4,
    FftDirection::{Forward, Inverse},
};
use std::collections::VecDeque;

pub struct SpectralTransform {
    input_buffers: Vec<VecDeque<Complex<Sample>>>,
    scratch: Vec<Complex<Sample>>,
    freq_buffer: Vec<Complex<Sample>>,
    fft: Radix4<Sample>,
    ifft: Radix4<Sample>,
    period_mask: usize,
    period_offset: usize,
    window: Vec<Complex<Sample>>,
    frame_number: usize,
    transform: Box<dyn FnMut(&mut Vec<Complex<Sample>>) + Send>,
}

impl SpectralTransform {
    // window_size = 2048
    // period = 64
    pub fn new(
        // Must be power of two!
        window_size: usize,
        // Must be power of two!
        period: usize,
        transform: Box<dyn FnMut(&mut Vec<Complex<Sample>>) + Send>,
    ) -> Self {
        SpectralTransform {
            input_buffers: vec![
                std::iter::repeat(Complex::zero())
                    .take(window_size)
                    .collect();
                CHANNELS
            ],
            scratch: vec![Complex::zero(); window_size],
            freq_buffer: vec![Complex::zero(); window_size],
            fft: Radix4::new(window_size, Forward),
            ifft: Radix4::new(window_size, Inverse),
            period_mask: period - 1,
            period_offset: window_size - period,
            frame_number: 0,
            window: apodize::hanning_iter(window_size)
                .map(Complex::from)
                .collect(),
            transform,
        }
    }
}

impl Op for SpectralTransform {
    fn perform(&mut self, stack: &mut Stack) {
        let mut frame = [0.0; CHANNELS];
        let index = self.frame_number & self.period_mask;
        for (output, input, input_buffer) in
            izip!(&mut frame, &stack.pop(), &mut self.input_buffers,)
        {
            if index == 0 {
                let input_slices = input_buffer.as_slices();
                let n = input_slices.0.len();
                self.freq_buffer[..n].copy_from_slice(input_slices.0);
                self.freq_buffer[n..].copy_from_slice(input_slices.1);
                for (x, a) in self.freq_buffer.iter_mut().zip(&self.window) {
                    *x *= a;
                }
                self.fft
                    .process_with_scratch(&mut self.freq_buffer, &mut self.scratch);
                (self.transform)(&mut self.freq_buffer);
                self.ifft
                    .process_with_scratch(&mut self.freq_buffer, &mut self.scratch);
            }
            *output = self.freq_buffer[self.period_offset + index].re;
            input_buffer.pop_front();
            input_buffer.push_back(Complex::from(input));
        }
        self.frame_number += 1;
        stack.push(&frame);
    }
}
