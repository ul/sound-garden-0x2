use audio_vm::{CHANNELS, Frame, Op, Sample, Stack};

const LINES: usize = 8;
const BASE_DELAYS_44K: [usize; LINES] = [1117, 1361, 1423, 1619, 1931, 2269, 2633, 3023];
const RIGHT_SIGNS: [Sample; LINES] = [1.0, -1.0, 1.0, -1.0, -1.0, 1.0, -1.0, 1.0];

fn is_prime(n: usize) -> bool {
    if n < 2 {
        return false;
    }
    if n.is_multiple_of(2) {
        return n == 2;
    }
    let mut d = 3;
    while d * d <= n {
        if n.is_multiple_of(d) {
            return false;
        }
        d += 2;
    }
    true
}

fn next_prime(mut n: usize) -> usize {
    n = n.max(2);
    while !is_prime(n) {
        n += 1;
    }
    n
}

struct DelayLine {
    buffer: Vec<Frame>,
    cursor: usize,
}

impl DelayLine {
    fn new(len: usize) -> Self {
        Self {
            buffer: vec![[0.0; CHANNELS]; len],
            cursor: 0,
        }
    }

    #[inline]
    fn read(&self) -> Frame {
        self.buffer[self.cursor]
    }

    #[inline]
    fn write_and_advance(&mut self, frame: Frame) {
        self.buffer[self.cursor] = frame;
        self.cursor += 1;
        if self.cursor == self.buffer.len() {
            self.cursor = 0;
        }
    }

    fn steal_same_size(&mut self, other: &mut Self) {
        if self.buffer.len() == other.buffer.len() {
            std::mem::swap(self, other);
        }
    }
}

pub struct Reverb {
    lines: [DelayLine; LINES],
    delay_seconds: [Sample; LINES],
    lowpass: [Frame; LINES],
}

impl Reverb {
    pub fn new(sample_rate: u32) -> Self {
        let sample_rate = sample_rate as Sample;
        let lengths = BASE_DELAYS_44K.map(|delay| {
            next_prime(((delay as Sample * sample_rate / 44_100.0).round() as usize).max(2))
        });
        Self {
            lines: lengths.map(DelayLine::new),
            delay_seconds: lengths.map(|len| len as Sample / sample_rate),
            lowpass: [[0.0; CHANNELS]; LINES],
        }
    }

    fn migrate_same(&mut self, other: &mut Self) {
        for (line, other_line) in self.lines.iter_mut().zip(&mut other.lines) {
            line.steal_same_size(other_line);
        }
        self.lowpass = other.lowpass;
    }
}

impl Op for Reverb {
    fn perform(&mut self, stack: &mut Stack) {
        let damp = stack.pop();
        let time = stack.pop();
        let input = stack.pop();

        let time = time.map(|time| time.clamp(0.01, 60.0));
        let lp_alpha = damp.map(|damp| (1.0 - damp.clamp(0.0, 1.0)).max(0.05));
        let mut delayed = [[0.0; CHANNELS]; LINES];
        let mut matrix_in = [[0.0; CHANNELS]; LINES];
        let mut sums = [0.0; CHANNELS];
        let mut output = [0.0; CHANNELS];
        let mix = (LINES as Sample).sqrt().recip();

        for i in 0..LINES {
            delayed[i] = self.lines[i].read();
            for channel in 0..CHANNELS {
                self.lowpass[i][channel] +=
                    lp_alpha[channel] * (delayed[i][channel] - self.lowpass[i][channel]);
                matrix_in[i][channel] = self.lowpass[i][channel];
                sums[channel] += matrix_in[i][channel];
            }
            output[0] += delayed[i][0] * mix;
            output[1] += delayed[i][1] * RIGHT_SIGNS[i] * mix;
        }

        for i in 0..LINES {
            let mut write = [0.0; CHANNELS];
            for channel in 0..CHANNELS {
                let reflected = matrix_in[i][channel] - (2.0 / LINES as Sample) * sums[channel];
                let gain = 10.0f64.powf(-3.0 * self.delay_seconds[i] / time[channel]);
                write[channel] = input[channel] + gain * reflected;
            }
            self.lines[i].write_and_advance(write);
        }

        stack.push(&output);
    }

    fn migrate(&mut self, other: &mut dyn Op) {
        if let Some(other) = other.downcast_mut::<Self>() {
            self.migrate_same(other);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn perform(op: &mut Reverb, input: Frame, time: Sample, damp: Sample) -> Frame {
        let mut stack = Stack::new();
        stack.push(&input);
        stack.push(&[time; CHANNELS]);
        stack.push(&[damp; CHANNELS]);
        op.perform(&mut stack);
        stack.pop()
    }

    fn impulse_response(time: Sample) -> Vec<Frame> {
        let mut verb = Reverb::new(44_100);
        let mut frames = Vec::new();
        frames.push(perform(&mut verb, [1.0, 1.0], time, 0.5));
        for _ in 1..20_000 {
            frames.push(perform(&mut verb, [0.0, 0.0], time, 0.5));
        }
        frames
    }

    fn rms(frames: &[Frame]) -> Sample {
        (frames.iter().flatten().map(|x| x * x).sum::<Sample>()
            / (frames.len() * CHANNELS) as Sample)
            .sqrt()
    }

    #[test]
    fn impulse_energy_decays() {
        let frames = impulse_response(0.3);
        let early = rms(&frames[1_100..5_000]);
        let late = rms(&frames[14_000..19_000]);
        assert!(late < early, "late {late} early {early}");
    }

    #[test]
    fn longer_time_decays_more_slowly() {
        let short = impulse_response(0.2);
        let long = impulse_response(1.5);
        assert!(rms(&long[12_000..18_000]) > rms(&short[12_000..18_000]));
    }

    #[test]
    fn extreme_args_stay_finite() {
        let mut verb = Reverb::new(48_000);
        for i in 0..4096 {
            let frame = perform(
                &mut verb,
                [if i == 0 { 10.0 } else { 0.0 }, -10.0],
                if i % 2 == 0 { -100.0 } else { 1.0e9 },
                if i % 2 == 0 { -1.0 } else { 2.0 },
            );
            assert!(frame.iter().all(|x| x.is_finite()), "{frame:?}");
        }
    }
}
