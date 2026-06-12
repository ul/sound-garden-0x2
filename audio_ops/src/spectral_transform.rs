//! # Spectral transform
//!
//! Windowed FFT transforms with Hann analysis/synthesis windows and
//! overlap-add resynthesis. Output is delayed by roughly one analysis window.
//!
//! Source to connect: input, preceded by zero or more control signals.
use audio_vm::{CHANNELS, Op, Sample, Stack};
use itertools::izip;
use rustfft::num_complex::Complex;
use rustfft::num_traits::Zero;
use rustfft::{
    Fft,
    FftDirection::{Forward, Inverse},
    algorithm::Radix4,
};
use std::collections::VecDeque;

pub type TransformFn = Box<dyn FnMut(usize, &mut [Complex<Sample>], &[Sample], &[bool]) + Send>;

pub struct SpectralTransform {
    input_buffers: Vec<VecDeque<Complex<Sample>>>,
    ola_buffers: Vec<Vec<Sample>>,
    scratch: Vec<Complex<Sample>>,
    freq_buffer: Vec<Complex<Sample>>,
    fft: Radix4<Sample>,
    ifft: Radix4<Sample>,
    period_mask: usize,
    ola_pos: usize,
    window_size: usize,
    half_len: usize,
    window: Vec<Sample>,
    scale: Sample,
    frame_number: usize,
    n_controls: usize,
    control_fired: Vec<bool>,
    transform: TransformFn,
}

impl SpectralTransform {
    // window_size = 2048
    // period = 64
    pub fn new(
        // Must be power of two!
        window_size: usize,
        // Must be power of two!
        period: usize,
        n_controls: usize,
        transform: TransformFn,
    ) -> Self {
        let window = apodize::hanning_iter(window_size).collect::<Vec<Sample>>();
        let cola = (0..window_size)
            .step_by(period)
            .map(|i| window[i] * window[i])
            .sum::<Sample>();
        let scale = if cola > 0.0 {
            1.0 / (window_size as Sample * cola)
        } else {
            1.0 / window_size as Sample
        };

        SpectralTransform {
            input_buffers: vec![
                std::iter::repeat_n(Complex::zero(), window_size).collect();
                CHANNELS
            ],
            ola_buffers: vec![vec![0.0; window_size]; CHANNELS],
            scratch: vec![Complex::zero(); window_size],
            freq_buffer: vec![Complex::zero(); window_size],
            fft: Radix4::new(window_size, Forward),
            ifft: Radix4::new(window_size, Inverse),
            period_mask: period - 1,
            ola_pos: 0,
            window_size,
            half_len: window_size / 2 + 1,
            window,
            scale,
            frame_number: 0,
            n_controls,
            control_fired: vec![false; n_controls],
            transform,
        }
    }

    fn process_hop(&mut self, values: &[Sample], fired: &[bool]) {
        let n = self.window_size;
        let half = self.half_len;
        for channel in 0..CHANNELS {
            let input_slices = self.input_buffers[channel].as_slices();
            let first_len = input_slices.0.len();
            self.freq_buffer[..first_len].copy_from_slice(input_slices.0);
            self.freq_buffer[first_len..].copy_from_slice(input_slices.1);
            for (x, &a) in self.freq_buffer.iter_mut().zip(&self.window) {
                *x *= a;
            }
            self.fft
                .process_with_scratch(&mut self.freq_buffer, &mut self.scratch);

            (self.transform)(channel, &mut self.freq_buffer[..half], values, fired);

            self.freq_buffer[0].im = 0.0;
            self.freq_buffer[half - 1].im = 0.0;
            for k in 1..(half - 1) {
                self.freq_buffer[n - k] = self.freq_buffer[k].conj();
            }

            self.ifft
                .process_with_scratch(&mut self.freq_buffer, &mut self.scratch);

            for i in 0..n {
                let pos = (self.ola_pos + i) & (n - 1);
                self.ola_buffers[channel][pos] +=
                    self.freq_buffer[i].re * self.window[i] * self.scale;
            }
        }
    }
}

impl Op for SpectralTransform {
    fn perform(&mut self, stack: &mut Stack) {
        let mut popped_controls = Vec::with_capacity(self.n_controls);
        for _ in 0..self.n_controls {
            popped_controls.push(stack.pop());
        }
        popped_controls.reverse();
        let input_frame = stack.pop();

        for (fired, frame) in self.control_fired.iter_mut().zip(&popped_controls) {
            *fired |= frame.iter().any(|&x| x > 0.0);
        }

        let index = self.frame_number & self.period_mask;
        if index == 0 {
            let values = popped_controls
                .iter()
                .map(|frame| frame[0])
                .collect::<Vec<_>>();
            let fired = self.control_fired.clone();
            self.process_hop(&values, &fired);
            self.control_fired.fill(false);
        }

        let mut output = [0.0; CHANNELS];
        for (y, ola, input, input_buffer) in izip!(
            &mut output,
            &mut self.ola_buffers,
            &input_frame,
            &mut self.input_buffers,
        ) {
            *y = ola[self.ola_pos];
            ola[self.ola_pos] = 0.0;
            input_buffer.pop_front();
            input_buffer.push_back(Complex::from(input));
        }

        self.ola_pos = (self.ola_pos + 1) & (self.window_size - 1);
        self.frame_number += 1;
        stack.push(&output);
    }

    fn migrate(&mut self, other: &mut dyn Op) {
        if let Some(other) = other.downcast_mut::<Self>() {
            if self.window_size == other.window_size && self.n_controls == other.n_controls {
                self.input_buffers = other.input_buffers.clone();
                self.ola_buffers = other.ola_buffers.clone();
                self.ola_pos = other.ola_pos;
                self.frame_number = other.frame_number;
                self.control_fired = other.control_fired.clone();
                // The transform closure owns op-specific state (permutations,
                // freeze captures, RNG streams) and is intentionally not migrated.
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::TAU;

    const SR: Sample = 48_000.0;
    const N: usize = 2048;
    const H: usize = 64;

    fn run_op(op: &mut SpectralTransform, input: &[Sample]) -> Vec<Sample> {
        let mut stack = Stack::new();
        let mut out = Vec::with_capacity(input.len());
        for &x in input {
            stack.reset();
            stack.push(&[x, x]);
            op.perform(&mut stack);
            out.push(stack.pop()[0]);
        }
        out
    }

    fn sine(freq: Sample, len: usize) -> Vec<Sample> {
        (0..len)
            .map(|i| (TAU * freq * i as Sample / SR).sin())
            .collect()
    }

    fn rms(xs: &[Sample]) -> Sample {
        (xs.iter().map(|x| x * x).sum::<Sample>() / xs.len() as Sample).sqrt()
    }

    fn corr(a: &[Sample], b: &[Sample]) -> Sample {
        let ma = a.iter().sum::<Sample>() / a.len() as Sample;
        let mb = b.iter().sum::<Sample>() / b.len() as Sample;
        let mut num = 0.0;
        let mut da = 0.0;
        let mut db = 0.0;
        for (&x, &y) in a.iter().zip(b) {
            let x = x - ma;
            let y = y - mb;
            num += x * y;
            da += x * x;
            db += y * y;
        }
        num / (da.sqrt() * db.sqrt())
    }

    #[test]
    fn identity_reconstructs_sine_after_warmup() {
        let input = sine(440.0, N * 5);
        let mut op = SpectralTransform::new(N, H, 0, Box::new(|_, _, _, _| {}));
        let out = run_op(&mut op, &input);
        assert!(out.iter().all(|x| x.is_finite()));
        let start = N * 2;
        let a = &input[start..(input.len() - N)];
        let b = &out[(start + N)..];
        let db = 20.0 * (rms(b) / rms(a)).log10();
        assert!(db.abs() < 0.5, "rms delta {db} dB");
        assert!(corr(a, b) > 0.995);
    }

    #[test]
    fn identity_preserves_dc_after_warmup() {
        let input = vec![0.25; N * 4];
        let mut op = SpectralTransform::new(N, H, 0, Box::new(|_, _, _, _| {}));
        let out = run_op(&mut op, &input);
        assert!(out.iter().all(|x| x.is_finite()));
        let mean = out[N * 3..].iter().sum::<Sample>() / (out.len() - N * 3) as Sample;
        assert!((mean - 0.25).abs() < 1e-3, "mean {mean}");
    }

    #[test]
    fn reverse_and_st1_are_finite_with_plausible_energy() {
        let input = sine(440.0, N * 4);
        let mut reverse =
            SpectralTransform::new(N, H, 0, Box::new(|_, freqs, _, _| freqs[1..].reverse()));
        let mut st1 = SpectralTransform::new(
            N,
            H,
            0,
            Box::new(|_, freqs, _, _| {
                let nyq = freqs.len() - 1;
                let max_idx = (1..nyq)
                    .max_by(|&a, &b| freqs[a].norm_sqr().total_cmp(&freqs[b].norm_sqr()))
                    .unwrap();
                for (i, bin) in freqs.iter_mut().enumerate() {
                    if i != max_idx {
                        *bin = Complex::zero();
                    }
                }
            }),
        );
        for op in [&mut reverse, &mut st1] {
            let out = run_op(op, &input);
            assert!(out.iter().all(|x| x.is_finite()));
            let e = rms(&out[N * 2..]);
            assert!(e > 1e-4 && e < 2.0, "energy {e}");
        }
    }
}
