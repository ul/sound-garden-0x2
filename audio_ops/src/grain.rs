//! # Granular synthesis
//!
//! Two ops share one grain engine; see docs/adr/0004-granular-synthesis.md.
//!
//! - `grain:NAME:N` — `(position, duration, rate, trig)`: grains read a table
//!   (`wt:`/`ft:`), `position` in seconds from its start, wrapping around it.
//! - `granulate:SECONDS:N` — `(x, back, duration, rate, trig)`: grains read a
//!   rolling buffer of the op's own input, starting `back` seconds ago.
//!
//! Each rising edge of `trig` (any channel) starts one grain in a free slot, or
//! steals the oldest when all N are busy. Position, duration, rate and the
//! trigger amplitude are latched per channel at that moment, so stereo inputs
//! (e.g. a little `noise` on the position) spread grains across the field.
//! A grain plays `duration` seconds of source at `rate` (negative runs
//! backwards) through a Hann window. The op pushes the sum of all grains.
use audio_vm::{AtomicFrame, CHANNELS, Frame, Op, Sample, Stack};
use std::sync::{Arc, atomic::Ordering};

/// Shorter grains are all window and no sound; also avoids dividing by ~0.
const MIN_GRAIN_FRAMES: Sample = 2.0;

#[derive(Clone, Copy, Default)]
struct Grain {
    /// Source position of the first frame, per channel (table frames, or
    /// absolute input frame index for `granulate`).
    start: Frame,
    rate: Frame,
    /// Length in output frames, per channel; a channel is silent past it.
    length: Frame,
    amplitude: Frame,
    /// Output frames played so far; the grain is free once past every length.
    age: Sample,
    /// Hann window by recurrence: (cos, sin) of `TAU * age / length`, rotated
    /// one step per frame instead of calling `cos` per grain per sample.
    phase_cos: Frame,
    phase_sin: Frame,
    step_cos: Frame,
    step_sin: Frame,
    active: bool,
    /// Allocation order, for stealing the oldest grain.
    order: u64,
}

/// Fixed pool of grain slots. Sized once at construction; never allocates.
struct GrainPool {
    grains: Vec<Grain>,
    previous_trigger: Frame,
    order: u64,
}

impl GrainPool {
    fn new(size: usize) -> Self {
        GrainPool {
            grains: vec![Grain::default(); size],
            previous_trigger: [0.0; CHANNELS],
            order: 0,
        }
    }

    /// On a rising edge of `trigger` on any channel, claim a slot (free, else
    /// oldest) stamped with the trigger amplitude and return its index for the
    /// caller to fill in.
    fn trigger(&mut self, trigger: &Frame) -> Option<usize> {
        let rising = self
            .previous_trigger
            .iter()
            .zip(trigger)
            .any(|(&previous, &current)| previous <= 0.0 && current > 0.0);
        self.previous_trigger = *trigger;
        if !rising || self.grains.is_empty() {
            return None;
        }
        let index = self
            .grains
            .iter()
            .position(|grain| !grain.active)
            .or_else(|| {
                self.grains
                    .iter()
                    .enumerate()
                    .min_by_key(|(_, grain)| grain.order)
                    .map(|(index, _)| index)
            })?;
        let order = self.order;
        self.order = self.order.wrapping_add(1);
        self.grains[index] = Grain {
            amplitude: trigger.map(|x| x.max(0.0)),
            active: true,
            order,
            ..Grain::default()
        };
        Some(index)
    }

    /// Advance every active grain by one frame and return their sum.
    /// `read(channel, position)` samples the source.
    fn render(&mut self, mut read: impl FnMut(usize, Sample) -> Sample) -> Frame {
        let mut output = [0.0; CHANNELS];
        for grain in self.grains.iter_mut().filter(|grain| grain.active) {
            if grain.age == 0.0 {
                // Lengths are filled in by the caller after `trigger`.
                for channel in 0..CHANNELS {
                    let (sin, cos) = (std::f64::consts::TAU / grain.length[channel]).sin_cos();
                    grain.phase_cos[channel] = 1.0;
                    grain.phase_sin[channel] = 0.0;
                    grain.step_cos[channel] = cos;
                    grain.step_sin[channel] = sin;
                }
            }
            for (channel, out) in output.iter_mut().enumerate() {
                let length = grain.length[channel];
                if grain.age < length && grain.amplitude[channel] > 0.0 {
                    let window = 0.5 - 0.5 * grain.phase_cos[channel];
                    let position = grain.start[channel] + grain.rate[channel] * grain.age;
                    *out += grain.amplitude[channel] * window * read(channel, position);
                }
                let (c, s) = (grain.phase_cos[channel], grain.phase_sin[channel]);
                let (dc, ds) = (grain.step_cos[channel], grain.step_sin[channel]);
                grain.phase_cos[channel] = c * dc - s * ds;
                grain.phase_sin[channel] = c * ds + s * dc;
            }
            grain.age += 1.0;
            if grain.length.iter().all(|&length| grain.age >= length) {
                grain.active = false;
            }
        }
        output
    }

    /// Keep sounding grains and edge state across a live edit. Slots beyond
    /// the new size are dropped (the VM's reload declick covers the step).
    fn migrate_from(&mut self, other: &GrainPool) {
        for (grain, other) in self.grains.iter_mut().zip(&other.grains) {
            *grain = *other;
        }
        self.previous_trigger = other.previous_trigger;
        self.order = other.order;
    }
}

fn grain_frames(duration: Sample, sample_rate: Sample) -> Sample {
    let frames = duration * sample_rate;
    if frames.is_finite() {
        frames.max(MIN_GRAIN_FRAMES)
    } else {
        MIN_GRAIN_FRAMES
    }
}

fn finite_or(x: Sample, default: Sample) -> Sample {
    if x.is_finite() { x } else { default }
}

/// Grains over a table recorded with `wt:` or loaded with `ft:`.
pub struct TableGrains {
    pool: GrainPool,
    table: Option<Arc<Vec<AtomicFrame>>>,
    sample_rate: Sample,
}

impl TableGrains {
    pub fn new(sample_rate: u32, table: Arc<Vec<AtomicFrame>>, grains: usize) -> Self {
        TableGrains {
            pool: GrainPool::new(grains),
            table: Some(table),
            sample_rate: Sample::from(sample_rate),
        }
    }

    /// Forgiving zero-output op for a missing table or bad argument: consumes
    /// the four inputs and pushes silence.
    pub fn silent() -> Self {
        TableGrains {
            pool: GrainPool::new(0),
            table: None,
            sample_rate: 0.0,
        }
    }
}

impl Op for TableGrains {
    fn perform(&mut self, stack: &mut Stack) {
        let trigger = stack.pop();
        let rate = stack.pop();
        let duration = stack.pop();
        let position = stack.pop();
        let table = match &self.table {
            Some(table) if !table.is_empty() => table,
            _ => {
                stack.push(&[0.0; CHANNELS]);
                return;
            }
        };
        let sample_rate = self.sample_rate;
        if let Some(index) = self.pool.trigger(&trigger) {
            let grain = &mut self.pool.grains[index];
            for channel in 0..CHANNELS {
                grain.start[channel] = finite_or(position[channel], 0.0) * sample_rate;
                grain.rate[channel] = finite_or(rate[channel], 1.0);
                grain.length[channel] = grain_frames(duration[channel], sample_rate);
            }
        }
        let len = table.len();
        let output = self.pool.render(|channel, position| {
            let floor = position.floor();
            let fraction = position - floor;
            // Grains wrap around the table in both directions.
            let i = (floor as i64).rem_euclid(len as i64) as usize;
            let j = if i + 1 == len { 0 } else { i + 1 };
            let a = Sample::from_bits(table[i][channel].load(Ordering::Relaxed));
            let b = Sample::from_bits(table[j][channel].load(Ordering::Relaxed));
            a + (b - a) * fraction
        });
        stack.push(&output);
    }

    fn migrate(&mut self, other: &mut dyn Op) {
        if let Some(other) = other.downcast_mut::<Self>() {
            self.pool.migrate_from(&other.pool);
        }
    }
}

/// Grains over a rolling buffer of the op's own input.
pub struct LiveGrains {
    pool: GrainPool,
    buffer: Vec<Frame>,
    /// Frames written so far; the newest frame has absolute index `written - 1`.
    written: u64,
    sample_rate: Sample,
}

impl LiveGrains {
    pub fn new(sample_rate: u32, seconds: Sample, grains: usize) -> Self {
        let sample_rate = Sample::from(sample_rate);
        // Grains interpolate between neighbouring frames, so keep a few spare.
        let len = ((seconds * sample_rate) as usize).max(4);
        LiveGrains {
            pool: GrainPool::new(grains),
            buffer: vec![[0.0; CHANNELS]; len],
            written: 0,
            sample_rate,
        }
    }

    /// Forgiving zero-output op for bad arguments: consumes the five inputs
    /// and pushes silence.
    pub fn silent() -> Self {
        LiveGrains {
            pool: GrainPool::new(0),
            buffer: Vec::new(),
            written: 0,
            sample_rate: 0.0,
        }
    }

    /// Clamp a grain so every frame it reads is already written and not yet
    /// overwritten, for the whole of its life. A grain faster than 1x would
    /// otherwise overtake the write head, and a slower or reversed one would
    /// fall behind the buffer's tail.
    fn place(&self, back: Sample, duration: Sample, rate: Sample) -> (Sample, Sample) {
        let len = self.buffer.len() as Sample;
        let newest = self.written as Sample - 1.0;
        let rate = finite_or(rate, 1.0);
        let mut length = grain_frames(duration, self.sample_rate);
        // The read head moves (rate - 1) frames per frame relative to the
        // write head. Keeping a frame of margin at each end (interpolation
        // reads the next frame), it may drift at most len - 3 frames.
        let drift = (rate - 1.0).abs();
        if drift > 0.0 {
            length = length.min(((len - 3.0) / drift).max(MIN_GRAIN_FRAMES));
        }
        let latest_start = newest - 1.0 - (rate - 1.0).max(0.0) * length;
        let earliest_start = newest - (len - 2.0) + (1.0 - rate).max(0.0) * length;
        let wanted = newest - finite_or(back, 0.0) * self.sample_rate;
        let start = wanted.min(latest_start).max(earliest_start);
        (start, length)
    }
}

impl Op for LiveGrains {
    fn perform(&mut self, stack: &mut Stack) {
        let trigger = stack.pop();
        let rate = stack.pop();
        let duration = stack.pop();
        let back = stack.pop();
        let input = stack.pop();
        if self.buffer.is_empty() {
            stack.push(&[0.0; CHANNELS]);
            return;
        }
        let len = self.buffer.len();
        self.buffer[(self.written % len as u64) as usize] = input;
        self.written += 1;

        if let Some(index) = self.pool.trigger(&trigger) {
            for channel in 0..CHANNELS {
                let (start, length) = self.place(back[channel], duration[channel], rate[channel]);
                let grain = &mut self.pool.grains[index];
                grain.start[channel] = start;
                grain.length[channel] = length;
                grain.rate[channel] = finite_or(rate[channel], 1.0);
            }
        }
        let buffer = &self.buffer;
        let output = self.pool.render(|channel, position| {
            let floor = position.floor();
            let fraction = position - floor;
            // Before the buffer has filled, early grains may point before the
            // first frame; those slots still hold silence.
            let i = (floor as i64).rem_euclid(len as i64) as usize;
            let j = if i + 1 == len { 0 } else { i + 1 };
            let a = buffer[i][channel];
            let b = buffer[j][channel];
            a + (b - a) * fraction
        });
        stack.push(&output);
    }

    fn migrate(&mut self, other: &mut dyn Op) {
        if let Some(other) = other.downcast_mut::<Self>()
            && self.buffer.len() == other.buffer.len()
        {
            // Steal the recorded past and keep grains reading it.
            std::mem::swap(&mut self.buffer, &mut other.buffer);
            self.written = other.written;
            self.pool.migrate_from(&other.pool);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: u32 = 1000;

    fn table(values: &[Sample]) -> Arc<Vec<AtomicFrame>> {
        Arc::new(
            values
                .iter()
                .map(|&x| [x, -x].map(|x| std::sync::atomic::AtomicU64::new(x.to_bits())))
                .collect(),
        )
    }

    fn table_frame(
        op: &mut TableGrains,
        position: Sample,
        duration: Sample,
        rate: Sample,
        trig: Sample,
    ) -> Frame {
        let mut stack = Stack::new();
        stack.push(&[position; CHANNELS]);
        stack.push(&[duration; CHANNELS]);
        stack.push(&[rate; CHANNELS]);
        stack.push(&[trig; CHANNELS]);
        op.perform(&mut stack);
        stack.pop()
    }

    fn live_frame(
        op: &mut LiveGrains,
        x: Sample,
        back: Sample,
        duration: Sample,
        rate: Sample,
        trig: Sample,
    ) -> Frame {
        let mut stack = Stack::new();
        stack.push(&[x; CHANNELS]);
        stack.push(&[back; CHANNELS]);
        stack.push(&[duration; CHANNELS]);
        stack.push(&[rate; CHANNELS]);
        stack.push(&[trig; CHANNELS]);
        op.perform(&mut stack);
        stack.pop()
    }

    #[test]
    fn table_grain_is_hann_windowed_source_and_ends() {
        // Constant source: the output is exactly the window, scaled by the
        // trigger amplitude, on each channel's own table column.
        let mut op = TableGrains::new(SR, table(&[0.5; 100]), 4);
        let length = 10;
        let mut outputs = Vec::new();
        for n in 0..(length + 5) {
            let trig = if n == 0 { 0.8 } else { 0.0 };
            outputs.push(table_frame(&mut op, 0.0, 0.01, 1.0, trig));
        }
        for (n, frame) in outputs.iter().enumerate().take(length) {
            let window = 0.5 - 0.5 * (std::f64::consts::TAU * n as Sample / length as Sample).cos();
            assert!((frame[0] - 0.8 * 0.5 * window).abs() < 1e-12, "frame {n}");
            assert!((frame[1] + 0.8 * 0.5 * window).abs() < 1e-12, "frame {n}");
        }
        assert!(outputs[length..].iter().all(|f| *f == [0.0; CHANNELS]));
        assert!(op.pool.grains.iter().all(|g| !g.active));
    }

    #[test]
    fn table_grains_read_at_rate_and_wrap_both_ways() {
        let ramp = (0..8).map(|i| i as Sample).collect::<Vec<_>>();
        // A one-second grain's window is tiny but non-zero over its first
        // frames; dividing it out recovers the table value being read.
        for (rate, start, expected) in [
            (1.0, 0.006, [7.0, 0.0, 1.0, 2.0]),
            (2.0, 0.0, [2.0, 4.0, 6.0, 0.0]),
            (-1.0, 0.001, [0.0, 7.0, 6.0, 5.0]),
            (0.5, 0.0, [0.5, 1.0, 1.5, 2.0]),
        ] {
            let mut op = TableGrains::new(SR, table(&ramp), 1);
            table_frame(&mut op, start, 1.0, rate, 1.0);
            for (age, &value) in (1..).zip(&expected) {
                let out = table_frame(&mut op, start, 1.0, rate, 0.0);
                let window = 0.5 - 0.5 * (std::f64::consts::TAU * age as Sample / 1000.0).cos();
                assert!(
                    (out[0] / window - value).abs() < 1e-6,
                    "rate {rate} age {age}: {}",
                    out[0] / window
                );
                assert!(
                    (out[1] / window + value).abs() < 1e-6,
                    "rate {rate} age {age}"
                );
            }
        }
    }

    #[test]
    fn full_pool_steals_the_oldest_grain() {
        let orders = |op: &TableGrains| op.pool.grains.iter().map(|g| g.order).collect::<Vec<_>>();
        let mut op = TableGrains::new(SR, table(&[1.0; 16]), 2);
        for trig in [1.0, 0.0, 1.0, 0.0] {
            table_frame(&mut op, 0.0, 1.0, 1.0, trig);
        }
        assert_eq!(orders(&op), [0, 1]);
        table_frame(&mut op, 0.0, 1.0, 1.0, 1.0);
        assert_eq!(orders(&op), [2, 1]);
    }

    #[test]
    fn missing_table_and_empty_pool_preserve_stack_shape() {
        let mut op = TableGrains::silent();
        let mut stack = Stack::new();
        stack.push(&[9.0; CHANNELS]);
        for _ in 0..4 {
            stack.push(&[1.0; CHANNELS]);
        }
        op.perform(&mut stack);
        assert_eq!(stack.pop(), [0.0; CHANNELS]);
        assert_eq!(stack.pop(), [9.0; CHANNELS]);

        let mut op = LiveGrains::silent();
        stack.push(&[9.0; CHANNELS]);
        for _ in 0..5 {
            stack.push(&[1.0; CHANNELS]);
        }
        op.perform(&mut stack);
        assert_eq!(stack.pop(), [0.0; CHANNELS]);
        assert_eq!(stack.pop(), [9.0; CHANNELS]);
    }

    #[test]
    fn live_grain_replays_the_past() {
        // Input is the frame counter, so the grain's source position shows
        // up directly (divided by the window).
        let mut op = LiveGrains::new(SR, 1.0, 1);
        for n in 0..500 {
            live_frame(&mut op, n as Sample, 0.0, 0.0, 1.0, 0.0);
        }
        // Frame 500: start 0.1 s = 100 frames back, i.e. at input frame 400.
        let length = 20;
        for t in 0..length {
            let n = 500 + t;
            let trig = if t == 0 { 1.0 } else { 0.0 };
            let out = live_frame(&mut op, n as Sample, 0.1, 0.02, 1.0, trig);
            let window = 0.5 - 0.5 * (std::f64::consts::TAU * t as Sample / length as Sample).cos();
            if window > 1e-9 {
                assert!(
                    (out[0] / window - (400 + t) as Sample).abs() < 1e-9,
                    "t {t}"
                );
            }
        }
    }

    #[test]
    fn live_grains_never_read_unwritten_or_overwritten_frames() {
        // Input is the frame counter. Any read outside the valid window would
        // show up as a value from the wrong era; check every grain frame's
        // source position against the window at the time it is read.
        let seconds = 0.1; // 100-frame buffer
        for (back, duration, rate) in [
            (0.0, 0.05, 3.0),  // fast grain starting at the write head
            (0.09, 0.2, -2.0), // reversed long grain near the tail
            (0.05, 1.0, 0.25), // very slow, longer than the buffer allows
            (-1.0, 0.01, 1.0), // "from the future"
            (5.0, 0.01, 1.0),  // older than the buffer
        ] {
            let mut op = LiveGrains::new(SR, seconds, 1);
            for n in 0..300 {
                live_frame(&mut op, n as Sample, 0.0, 0.0, 1.0, 0.0);
            }
            for t in 0..400 {
                let n = 300 + t;
                let trig = if t == 0 { 1.0 } else { 0.0 };
                live_frame(&mut op, n as Sample, back, duration, rate, trig);
                let grain = op.pool.grains[0];
                if !grain.active && t > 0 {
                    break;
                }
                let position = grain.start[0] + grain.rate[0] * (grain.age - 1.0);
                let newest = n as Sample;
                let oldest = newest - 99.0;
                assert!(
                    position >= oldest && position + 1.0 <= newest,
                    "back {back} duration {duration} rate {rate}: frame {n} reads {position}, valid {oldest}..{newest}"
                );
            }
        }
    }

    #[test]
    fn migrate_keeps_live_buffer_and_sounding_grains() {
        let mut old = LiveGrains::new(SR, 1.0, 2);
        for n in 0..300 {
            let trig = if n == 250 { 1.0 } else { 0.0 };
            live_frame(&mut old, n as Sample, 0.1, 0.1, 1.0, trig);
        }
        // Reference: what the old op would have played next without a reload.
        let mut reference = clone_for_test(&old);
        let mut new = LiveGrains::new(SR, 1.0, 2);
        new.migrate(&mut old);
        for n in 300..340 {
            let a = live_frame(&mut new, n as Sample, 0.1, 0.1, 1.0, 0.0);
            let b = live_frame(&mut reference, n as Sample, 0.1, 0.1, 1.0, 0.0);
            assert_eq!(a, b, "frame {n}");
            assert!(a[0] > 0.0, "grain stopped at frame {n}");
        }
    }

    fn clone_for_test(op: &LiveGrains) -> LiveGrains {
        LiveGrains {
            pool: GrainPool {
                grains: op.pool.grains.clone(),
                previous_trigger: op.pool.previous_trigger,
                order: op.pool.order,
            },
            buffer: op.buffer.clone(),
            written: op.written,
            sample_rate: op.sample_rate,
        }
    }
}
