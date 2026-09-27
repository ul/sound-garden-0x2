//! Performance feedback from the audio callback: buffer size, DSP load,
//! dropouts and output levels.
//!
//! The audio thread writes plain atomics once per callback (no locks, no
//! allocation); the monitor thread takes a [`Meters`] snapshot every poll,
//! which resets the windowed values (load peak, levels, clip count).
use audio_vm::{CHANNELS, Frame, Sample};
use std::{
    sync::atomic::{AtomicU32, AtomicU64, Ordering},
    time::Instant,
};

/// A callback that starts this many buffer periods after the previous one
/// means at least one buffer was never delivered.
const DROPOUT_GAP_PERIODS: f64 = 1.75;

/// What the GUI shows about the audio engine. Windowed fields cover the time
/// since the previous snapshot.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Meters {
    pub sample_rate: u32,
    /// Frames in the most recent callback (what the device actually uses).
    pub buffer_frames: u32,
    /// Peak share of a buffer's duration spent computing it (1.0 = no headroom).
    pub load: Sample,
    /// Dropouts since the engine started.
    pub dropouts: u64,
    /// Peak |sample| before the output clip, per channel.
    pub peak: Frame,
    /// RMS level, per channel.
    pub rms: Frame,
    /// Samples that exceeded ±1.0 and were clipped.
    pub clipped: u64,
}

#[derive(Default)]
pub struct Telemetry {
    sample_rate: AtomicU32,
    buffer_frames: AtomicU32,
    load_peak: AtomicU64,
    dropouts: AtomicU64,
    peak: [AtomicU64; CHANNELS],
    sum_squares: [AtomicU64; CHANNELS],
    frames: AtomicU64,
    clipped: AtomicU64,
}

/// `fetch_max` on the bits is correct for non-negative floats, whose bit
/// patterns order like their values.
fn max_non_negative(atomic: &AtomicU64, x: Sample) {
    atomic.fetch_max(x.max(0.0).to_bits(), Ordering::Relaxed);
}

fn add(atomic: &AtomicU64, x: Sample) {
    atomic
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |bits| {
            Some((Sample::from_bits(bits) + x).to_bits())
        })
        .ok();
}

fn take(atomic: &AtomicU64) -> Sample {
    Sample::from_bits(atomic.swap(0, Ordering::Relaxed))
}

impl Telemetry {
    pub fn new() -> Self {
        Self::default()
    }

    pub(crate) fn set_sample_rate(&self, sample_rate: u32) {
        self.sample_rate.store(sample_rate, Ordering::Relaxed);
    }

    pub(crate) fn record_callback(&self, frames: u32, load: Sample, dropout: bool) {
        self.buffer_frames.store(frames, Ordering::Relaxed);
        max_non_negative(&self.load_peak, load);
        if dropout {
            self.dropouts.fetch_add(1, Ordering::Relaxed);
        }
    }

    pub(crate) fn record_output(&self, levels: &OutputLevels) {
        for channel in 0..CHANNELS {
            max_non_negative(&self.peak[channel], levels.peak[channel]);
            add(&self.sum_squares[channel], levels.sum_squares[channel]);
        }
        self.frames.fetch_add(levels.frames, Ordering::Relaxed);
        self.clipped.fetch_add(levels.clipped, Ordering::Relaxed);
    }

    /// Read everything and reset the windowed values.
    pub fn snapshot(&self) -> Meters {
        let frames = self.frames.swap(0, Ordering::Relaxed);
        let mut peak = [0.0; CHANNELS];
        let mut rms = [0.0; CHANNELS];
        for channel in 0..CHANNELS {
            peak[channel] = take(&self.peak[channel]);
            let sum_squares = take(&self.sum_squares[channel]);
            rms[channel] = if frames > 0 {
                (sum_squares / frames as Sample).sqrt()
            } else {
                0.0
            };
        }
        Meters {
            sample_rate: self.sample_rate.load(Ordering::Relaxed),
            buffer_frames: self.buffer_frames.load(Ordering::Relaxed),
            load: take(&self.load_peak),
            dropouts: self.dropouts.load(Ordering::Relaxed),
            peak,
            rms,
            clipped: self.clipped.swap(0, Ordering::Relaxed),
        }
    }
}

/// Output levels accumulated locally over one callback, then published once.
#[derive(Default)]
pub(crate) struct OutputLevels {
    peak: Frame,
    sum_squares: Frame,
    frames: u64,
    clipped: u64,
}

impl OutputLevels {
    /// Record one frame before it is clipped to ±1.
    #[inline]
    pub(crate) fn add(&mut self, frame: &Frame) {
        for (channel, &x) in frame.iter().enumerate() {
            let x = if x.is_finite() { x } else { Sample::INFINITY };
            self.peak[channel] = self.peak[channel].max(x.abs());
            if x.is_finite() {
                self.sum_squares[channel] += x * x;
            }
            if x.abs() > 1.0 {
                self.clipped += 1;
            }
        }
        self.frames += 1;
    }
}

/// Measures how long each callback takes relative to the audio it produces,
/// and spots gaps between callbacks.
#[derive(Default)]
pub(crate) struct CallbackTiming {
    previous_start: Option<Instant>,
    previous_period: Sample,
}

impl CallbackTiming {
    /// Returns the callback's load (compute time / buffer duration) and
    /// whether a dropout happened: either this callback overran its buffer,
    /// or it started so long after the previous one that a buffer was missed.
    pub(crate) fn record(
        &mut self,
        start: Instant,
        end: Instant,
        frames: usize,
        sample_rate: u32,
    ) -> (Sample, bool) {
        let period = frames as Sample / Sample::from(sample_rate.max(1));
        let load = if period > 0.0 {
            end.saturating_duration_since(start).as_secs_f64() / period
        } else {
            0.0
        };
        let late = self.previous_start.is_some_and(|previous| {
            start.saturating_duration_since(previous).as_secs_f64()
                > DROPOUT_GAP_PERIODS * self.previous_period
        });
        self.previous_start = Some(start);
        self.previous_period = period;
        (load, late || load > 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn load_is_compute_time_over_buffer_duration() {
        let mut timing = CallbackTiming::default();
        let t0 = Instant::now();
        // 480 frames at 48 kHz is 10 ms; 2.5 ms of work is 25% load.
        let (load, dropout) = timing.record(t0, t0 + Duration::from_micros(2500), 480, 48_000);
        assert!((load - 0.25).abs() < 1e-9, "{load}");
        assert!(!dropout);
    }

    #[test]
    fn overruns_and_missed_buffers_count_as_dropouts() {
        let mut timing = CallbackTiming::default();
        let t0 = Instant::now();
        let ms = Duration::from_millis;
        assert!(!timing.record(t0, t0 + ms(1), 480, 48_000).1);
        // On time.
        assert!(!timing.record(t0 + ms(10), t0 + ms(11), 480, 48_000).1);
        // Took longer than the buffer lasts.
        assert!(timing.record(t0 + ms(20), t0 + ms(32), 480, 48_000).1);
        // Started two periods after the previous one: a buffer was missed.
        assert!(timing.record(t0 + ms(40), t0 + ms(41), 480, 48_000).1);
    }

    #[test]
    fn snapshot_reports_levels_and_resets_the_window() {
        let telemetry = Telemetry::new();
        telemetry.set_sample_rate(48_000);
        let mut levels = OutputLevels::default();
        levels.add(&[0.5, -0.25]);
        levels.add(&[-0.5, 1.5]);
        telemetry.record_output(&levels);
        telemetry.record_callback(2, 0.3, false);
        telemetry.record_callback(2, 0.1, true);

        let meters = telemetry.snapshot();
        assert_eq!(meters.sample_rate, 48_000);
        assert_eq!(meters.buffer_frames, 2);
        assert_eq!(meters.load, 0.3, "peak, not latest");
        assert_eq!(meters.dropouts, 1);
        assert_eq!(meters.peak, [0.5, 1.5]);
        assert!((meters.rms[0] - 0.5).abs() < 1e-12);
        assert!((meters.rms[1] - ((0.0625 + 2.25) / 2.0f64).sqrt()).abs() < 1e-12);
        assert_eq!(meters.clipped, 1);

        let empty = telemetry.snapshot();
        assert_eq!(
            (empty.load, empty.peak, empty.rms, empty.clipped),
            (0.0, [0.0; 2], [0.0; 2], 0)
        );
        assert_eq!(empty.dropouts, 1, "dropouts are a running total");
    }

    #[test]
    fn non_finite_output_reads_as_clipping_not_silence() {
        let mut levels = OutputLevels::default();
        levels.add(&[Sample::NAN, 0.0]);
        let telemetry = Telemetry::new();
        telemetry.record_output(&levels);
        let meters = telemetry.snapshot();
        assert_eq!(meters.peak[0], Sample::INFINITY);
        assert_eq!(meters.clipped, 1);
    }
}
