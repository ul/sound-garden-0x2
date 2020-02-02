//! # Spectral transform
//!
//! Do a FFT of the input signal, transform bins, and produce an output signal with IFFT.
//!
//! Source to connect: input.
use audio_vm::{Op, Sample, Stack, CHANNELS};
use itertools::izip;
use rustfft::algorithm::Radix4;
use rustfft::num_complex::Complex;
use rustfft::num_traits::Zero;
use rustfft::FFT;
use std::collections::VecDeque;

pub struct SpectralTransform {
    input_buffers: Vec<VecDeque<Complex<Sample>>>,
    input_scratch: Vec<Complex<Sample>>,
    freq_buffer: Vec<Complex<Sample>>,
    output_buffers: Vec<Vec<Complex<Sample>>>,
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
            input_scratch: vec![Complex::zero(); window_size],
            freq_buffer: vec![Complex::zero(); window_size],
            output_buffers: vec![vec![Complex::zero(); window_size]; CHANNELS],
            fft: Radix4::new(window_size, false),
            ifft: Radix4::new(window_size, true),
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
        for (output, input, input_buffer, output_buffer) in izip!(
            &mut frame,
            &stack.pop(),
            &mut self.input_buffers,
            &mut self.output_buffers
        ) {
            if index == 0 {
                let mut scratch = &mut self.input_scratch;
                let freq_buffer = &mut self.freq_buffer;
                let input_slices = input_buffer.as_slices();
                let n = input_slices.0.len();
                scratch[..n].clone_from_slice(input_slices.0);
                scratch[n..].clone_from_slice(input_slices.1);
                for (x, a) in scratch.iter_mut().zip(&self.window) {
                    *x *= a;
                }
                self.fft.process(&mut scratch, freq_buffer);
                (self.transform)(freq_buffer);
                self.ifft.process(freq_buffer, output_buffer);
            }
            *output = output_buffer[self.period_offset + index].re;
            input_buffer.pop_front();
            input_buffer.push_back(Complex::from(input));
        }
        self.frame_number += 1;
        stack.push(&frame);
    }
}
