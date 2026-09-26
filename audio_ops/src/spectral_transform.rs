//! # Spectral transform
//!
//! Windowed FFT transforms with Hann analysis/synthesis windows and
//! overlap-add resynthesis. Output is delayed by roughly one analysis window.
//!
//! Source to connect: input, preceded by zero or more control signals.
use audio_vm::{CHANNELS, Frame, Op, Sample, Stack};
use itertools::izip;
use rand::{SeedableRng, rngs::SmallRng, seq::SliceRandom};
use rustfft::num_complex::Complex;
use rustfft::num_traits::Zero;
use rustfft::{Fft, FftPlanner};
use std::collections::VecDeque;
use std::sync::Arc;

pub type TransformFn =
    Box<dyn FnMut(usize, usize, &mut [Complex<Sample>], &[Sample], &[bool]) + Send>;

pub const DEFAULT_WINDOW_SIZE: usize = 2048;
pub const DEFAULT_HOP: usize = 64;

pub fn reverse_half_spectrum(
    _channel: usize,
    frame_number: usize,
    freqs: &mut [Complex<Sample>],
    _values: &[Sample],
    _fired: &[bool],
) {
    let nyquist = freqs.len() - 1;
    if nyquist <= 1 {
        return;
    }

    // Bin k takes bin nyquist - k, so swap mirrored pairs in place (this runs on
    // the audio thread and must not allocate).
    let n = nyquist * 2;
    let rotation = |k: usize, src: usize| {
        let phase = std::f64::consts::TAU * (k as Sample - src as Sample) * frame_number as Sample
            / n as Sample;
        Complex::from_polar(1.0, phase)
    };
    for k in 1..=nyquist / 2 {
        let src = nyquist - k;
        let (low, high) = (freqs[k], freqs[src]);
        freqs[k] = high * rotation(k, src);
        freqs[src] = low * rotation(src, k);
    }
}

pub struct SpectralTransform {
    input_buffers: Vec<VecDeque<Complex<Sample>>>,
    ola_buffers: Vec<Vec<Sample>>,
    scratch: Vec<Complex<Sample>>,
    freq_buffer: Vec<Complex<Sample>>,
    fft: Arc<dyn Fft<Sample>>,
    ifft: Arc<dyn Fft<Sample>>,
    period_mask: usize,
    ola_pos: usize,
    window_size: usize,
    half_len: usize,
    window: Vec<Sample>,
    scale: Sample,
    frame_number: usize,
    n_controls: usize,
    control_fired: Vec<bool>,
    /// Preallocated per-frame/per-hop control storage so perform() never allocates.
    controls: Vec<Frame>,
    control_values: Vec<Sample>,
    hop_fired: Vec<bool>,
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

        // The planner picks SIMD (NEON/AVX) implementations; Radix4 used directly is scalar.
        let mut planner = FftPlanner::new();
        let fft = planner.plan_fft_forward(window_size);
        let ifft = planner.plan_fft_inverse(window_size);
        let scratch_len = fft
            .get_inplace_scratch_len()
            .max(ifft.get_inplace_scratch_len());

        SpectralTransform {
            input_buffers: vec![
                std::iter::repeat_n(Complex::zero(), window_size).collect();
                CHANNELS
            ],
            ola_buffers: vec![vec![0.0; window_size]; CHANNELS],
            scratch: vec![Complex::zero(); scratch_len],
            freq_buffer: vec![Complex::zero(); window_size],
            fft,
            ifft,
            period_mask: period - 1,
            ola_pos: 0,
            window_size,
            half_len: window_size / 2 + 1,
            window,
            scale,
            frame_number: 0,
            n_controls,
            control_fired: vec![false; n_controls],
            controls: vec![[0.0; CHANNELS]; n_controls],
            control_values: vec![0.0; n_controls],
            hop_fired: vec![false; n_controls],
            transform,
        }
    }

    pub fn shuffle(locality: usize, seed: Option<u64>) -> Self {
        let mut rng = seed.map_or_else(rand::make_rng::<SmallRng>, SmallRng::seed_from_u64);
        // All scratch is sized up front: the closure runs on the audio thread.
        let bins_len = DEFAULT_WINDOW_SIZE / 2 + 1;
        let mut permutation = Vec::<usize>::with_capacity(bins_len);
        let mut bins = Vec::<usize>::with_capacity(bins_len);
        let mut destinations = Vec::<usize>::with_capacity(bins_len);
        let mut input = Vec::<Complex<Sample>>::with_capacity(bins_len);
        let mut initialized = false;
        Self::new(
            DEFAULT_WINDOW_SIZE,
            DEFAULT_HOP,
            2,
            Box::new(move |channel, _, freqs, values, fired| {
                let nyquist = freqs.len() - 1;
                if channel == 0 && (!initialized || fired.get(1).copied().unwrap_or(false)) {
                    initialized = true;
                    permutation.clear();
                    permutation.extend(0..freqs.len());
                    let amount = values.first().copied().unwrap_or(0.0).clamp(0.0, 1.0);
                    if amount > 0.0 && nyquist > 1 {
                        let total = nyquist - 1;
                        let block_size = if locality == 0 {
                            total
                        } else {
                            locality.max(1)
                        };
                        let mut block_start = 1;
                        while block_start < nyquist {
                            let block_end = (block_start + block_size).min(nyquist);
                            let block_len = block_end - block_start;
                            let selected = (amount * block_len as Sample).round() as usize;
                            if selected > 1 {
                                bins.clear();
                                bins.extend(block_start..block_end);
                                bins.shuffle(&mut rng);
                                bins.truncate(selected);
                                destinations.clear();
                                destinations.extend_from_slice(&bins);
                                destinations.shuffle(&mut rng);
                                for (&src, &dst) in bins.iter().zip(&destinations) {
                                    permutation[src] = dst;
                                }
                            }
                            block_start = block_end;
                        }
                    }
                }

                if permutation.len() == freqs.len() {
                    input.clear();
                    input.extend_from_slice(freqs);
                    for k in 1..nyquist {
                        freqs[permutation[k]] = input[k];
                    }
                }
            }),
        )
    }

    pub fn freeze(seed: Option<u64>) -> Self {
        let _ = seed;
        let mut previous_gate = false;
        let mut rising_hop = false;
        // Sized up front: the closure runs on the audio thread.
        let bins_len = DEFAULT_WINDOW_SIZE / 2 + 1;
        let mut captured_magnitudes = vec![vec![0.0; bins_len]; CHANNELS];
        let mut captured_phases = vec![vec![0.0; bins_len]; CHANNELS];
        let mut has_capture = [false; CHANNELS];
        let mut capture_frames = [0usize; CHANNELS];
        Self::new(
            DEFAULT_WINDOW_SIZE,
            DEFAULT_HOP,
            1,
            Box::new(move |channel, frame_number, freqs, values, fired| {
                let nyquist = freqs.len() - 1;
                let gate_high = values.first().copied().unwrap_or(0.0) > 0.0;
                if channel == 0 {
                    rising_hop =
                        fired.first().copied().unwrap_or(false) && !previous_gate && gate_high;
                    previous_gate = gate_high;
                }

                let captured_is_empty = !has_capture[channel]
                    || captured_magnitudes[channel]
                        .iter()
                        .all(|&mag| mag <= Sample::EPSILON);
                let should_capture = gate_high
                    && frame_number >= DEFAULT_WINDOW_SIZE
                    && (rising_hop || captured_is_empty);
                if should_capture {
                    for ((magnitude, phase), bin) in captured_magnitudes[channel]
                        .iter_mut()
                        .zip(captured_phases[channel].iter_mut())
                        .zip(freqs.iter())
                    {
                        *magnitude = bin.norm();
                        *phase = bin.arg();
                    }
                    has_capture[channel] = true;
                    capture_frames[channel] = frame_number;
                    captured_magnitudes[channel][0] = 0.0;
                    captured_magnitudes[channel][nyquist] = 0.0;
                }

                if gate_high && has_capture[channel] {
                    let n = nyquist * 2;
                    let frame_offset = frame_number.wrapping_sub(capture_frames[channel]) as Sample;
                    freqs[0] = Complex::zero();
                    freqs[nyquist] = Complex::zero();
                    for k in 1..nyquist {
                        let phase = captured_phases[channel][k]
                            + std::f64::consts::TAU * k as Sample * frame_offset / n as Sample;
                        freqs[k] = Complex::from_polar(captured_magnitudes[channel][k], phase);
                    }
                }
            }),
        )
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

            (self.transform)(
                channel,
                self.frame_number,
                &mut self.freq_buffer[..half],
                values,
                fired,
            );

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
        for control in self.controls.iter_mut().rev() {
            *control = stack.pop();
        }
        let input_frame = stack.pop();

        for (fired, frame) in self.control_fired.iter_mut().zip(&self.controls) {
            *fired |= frame.iter().any(|&x| x > 0.0);
        }

        let index = self.frame_number & self.period_mask;
        if index == 0 {
            for (value, frame) in self.control_values.iter_mut().zip(&self.controls) {
                *value = frame[0];
            }
            self.hop_fired.copy_from_slice(&self.control_fired);
            // Move the snapshots out so process_hop can borrow self mutably; these
            // are moves of existing allocations, not new ones.
            let values = std::mem::take(&mut self.control_values);
            let fired = std::mem::take(&mut self.hop_fired);
            self.process_hop(&values, &fired);
            self.control_values = values;
            self.hop_fired = fired;
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
        if let Some(other) = other.downcast_mut::<Self>()
            && self.window_size == other.window_size
            && self.n_controls == other.n_controls
        {
            // Steal rather than clone: migrate runs on the audio thread.
            std::mem::swap(&mut self.input_buffers, &mut other.input_buffers);
            std::mem::swap(&mut self.ola_buffers, &mut other.ola_buffers);
            self.ola_pos = other.ola_pos;
            self.frame_number = other.frame_number;
            self.control_fired.copy_from_slice(&other.control_fired);
            // The transform closure owns op-specific state (permutations,
            // freeze captures, RNG streams) and is intentionally not migrated.
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
        let controls = vec![vec![0.0; input.len()]; op.n_controls];
        run_op_controls(op, input, &controls)
    }

    fn run_op_controls(
        op: &mut SpectralTransform,
        input: &[Sample],
        controls: &[Vec<Sample>],
    ) -> Vec<Sample> {
        let mut stack = Stack::new();
        let mut out = Vec::with_capacity(input.len());
        for (i, &x) in input.iter().enumerate() {
            stack.reset();
            stack.push(&[x, x]);
            for control in controls {
                stack.push(&[control[i], control[i]]);
            }
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

    fn fft_magnitudes(input: &[Sample]) -> Vec<Sample> {
        let fft = FftPlanner::new().plan_fft_forward(input.len());
        let mut scratch = vec![Complex::zero(); fft.get_inplace_scratch_len()];
        let mut buffer = input.iter().copied().map(Complex::from).collect::<Vec<_>>();
        fft.process_with_scratch(&mut buffer, &mut scratch);
        buffer[..(input.len() / 2 + 1)]
            .iter()
            .map(|x| x.norm())
            .collect()
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
    // The reference deliberately mirrors the original out-of-place loop.
    #[allow(clippy::needless_range_loop)]
    fn in_place_reverse_matches_copying_reference() {
        for bins in [2, 3, 4, 5, 1025] {
            let nyquist = bins - 1;
            let original = (0..bins)
                .map(|k| Complex::new(k as Sample + 0.5, -(k as Sample) * 0.25))
                .collect::<Vec<_>>();
            for frame_number in [0, 64, 12_345] {
                let mut expected = original.clone();
                let n = nyquist * 2;
                for k in 1..nyquist {
                    let src = nyquist - k;
                    let phase =
                        TAU * (k as Sample - src as Sample) * frame_number as Sample / n as Sample;
                    expected[k] = original[src] * Complex::from_polar(1.0, phase);
                }
                let mut actual = original.clone();
                reverse_half_spectrum(0, frame_number, &mut actual, &[], &[]);
                assert_eq!(actual, expected, "bins {bins}, frame {frame_number}");
            }
        }
    }

    #[test]
    fn identity_reconstructs_sine_after_warmup() {
        let input = sine(440.0, N * 5);
        let mut op = SpectralTransform::new(N, H, 0, Box::new(|_, _, _, _, _| {}));
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
        let mut op = SpectralTransform::new(N, H, 0, Box::new(|_, _, _, _, _| {}));
        let out = run_op(&mut op, &input);
        assert!(out.iter().all(|x| x.is_finite()));
        let mean = out[N * 3..].iter().sum::<Sample>() / (out.len() - N * 3) as Sample;
        assert!((mean - 0.25).abs() < 1e-3, "mean {mean}");
    }

    #[test]
    fn spectral_shuffle_amount_zero_matches_identity() {
        let input = sine(440.0, N * 5);
        let controls = vec![vec![0.0; input.len()], vec![1.0; input.len()]];
        let mut identity = SpectralTransform::new(N, H, 0, Box::new(|_, _, _, _, _| {}));
        let mut shuffle = SpectralTransform::shuffle(0, Some(42));
        let a = run_op(&mut identity, &input);
        let b = run_op_controls(&mut shuffle, &input, &controls);
        let diff = a.iter().zip(&b).map(|(x, y)| (x - y).abs()).sum::<Sample>();
        assert!(diff < 1e-9, "diff {diff}");
    }

    #[test]
    fn spectral_shuffle_is_seeded_and_stable_without_trigger() {
        let input = sine(440.0, N * 5);
        let controls = vec![vec![1.0; input.len()], vec![0.0; input.len()]];
        let mut a = SpectralTransform::shuffle(0, Some(42));
        let mut b = SpectralTransform::shuffle(0, Some(42));
        let mut c = SpectralTransform::shuffle(0, Some(43));
        let out_a = run_op_controls(&mut a, &input, &controls);
        let out_b = run_op_controls(&mut b, &input, &controls);
        let out_c = run_op_controls(&mut c, &input, &controls);
        assert_eq!(out_a, out_b);
        let diff = out_a[N * 3..]
            .iter()
            .zip(&out_c[N * 3..])
            .map(|(x, y)| (x - y).abs())
            .sum::<Sample>();
        assert!(diff > 1e-3, "diff {diff}");
    }

    #[test]
    fn spectral_shuffle_locality_limits_bin_motion() {
        let k = 100usize;
        let input = sine(k as Sample * SR / N as Sample, N * 6);
        let controls = vec![vec![1.0; input.len()], vec![0.0; input.len()]];
        let mut shuffle = SpectralTransform::shuffle(8, Some(7));
        let out = run_op_controls(&mut shuffle, &input, &controls);
        let mags = fft_magnitudes(&out[N * 4..N * 5]);
        let max_bin = (1..(mags.len() - 1))
            .max_by(|&a, &b| mags[a].total_cmp(&mags[b]))
            .unwrap();
        assert!(max_bin.abs_diff(k) <= 8, "max_bin {max_bin}");
    }

    #[test]
    fn spectral_shuffle_catches_one_sample_trigger_between_hops() {
        let input = sine(440.0, N * 7);
        let amount = vec![1.0; input.len()];
        let mut trig = vec![0.0; input.len()];
        trig[N * 3 + 1] = 1.0;
        let controls_trigger = vec![amount.clone(), trig];
        let controls_none = vec![amount, vec![0.0; input.len()]];
        let mut a = SpectralTransform::shuffle(0, Some(42));
        let mut b = SpectralTransform::shuffle(0, Some(42));
        let out_trigger = run_op_controls(&mut a, &input, &controls_trigger);
        let out_none = run_op_controls(&mut b, &input, &controls_none);
        let start = N * 4;
        let diff = out_trigger[start..]
            .iter()
            .zip(&out_none[start..])
            .map(|(x, y)| (x - y).abs())
            .sum::<Sample>();
        assert!(diff > 1e-3, "diff {diff}");
    }

    #[test]
    fn spectral_freeze_gate_low_matches_identity() {
        let input = sine(440.0, N * 5);
        let controls = vec![vec![0.0; input.len()]];
        let mut identity = SpectralTransform::new(N, H, 0, Box::new(|_, _, _, _, _| {}));
        let mut freeze = SpectralTransform::freeze(Some(42));
        let a = run_op(&mut identity, &input);
        let b = run_op_controls(&mut freeze, &input, &controls);
        let diff = a.iter().zip(&b).map(|(x, y)| (x - y).abs()).sum::<Sample>();
        assert!(diff < 1e-9, "diff {diff}");
    }

    #[test]
    fn spectral_freeze_sustains_captured_sine_after_input_goes_silent() {
        let capture = N * 3;
        let len = capture + N + 48_000 + N;
        let mut input = sine(440.0, len);
        for x in &mut input[(capture + H)..] {
            *x = 0.0;
        }
        let mut gate = vec![0.0; len];
        gate[capture..].fill(1.0);
        let mut freeze = SpectralTransform::freeze(Some(9));
        let out = run_op_controls(&mut freeze, &input, &[gate]);
        assert!(out.iter().all(|x| x.is_finite()));
        let captured = rms(&out[(capture + N)..(capture + N + 4096)]);
        let sustained = rms(&out[(capture + N + 24_000)..(capture + N + 48_000)]);
        let db = 20.0 * (sustained / captured).log10();
        assert!(
            db.abs() <= 6.0,
            "captured {captured}, sustained {sustained}, {db} dB"
        );
    }

    #[test]
    fn spectral_freeze_seed_reproducible() {
        let input = sine(440.0, N * 5);
        let controls = vec![vec![1.0; input.len()]];
        let mut a = SpectralTransform::freeze(Some(42));
        let mut b = SpectralTransform::freeze(Some(42));
        assert_eq!(
            run_op_controls(&mut a, &input, &controls),
            run_op_controls(&mut b, &input, &controls)
        );
    }

    #[test]
    fn reverse_and_st1_are_finite_with_plausible_energy() {
        let input = sine(440.0, N * 4);
        let mut reverse = SpectralTransform::new(N, H, 0, Box::new(reverse_half_spectrum));
        let mut st1 = SpectralTransform::new(
            N,
            H,
            0,
            Box::new(|_, _, freqs, _, _| {
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
