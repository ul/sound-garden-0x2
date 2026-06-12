use audio_vm::{CHANNELS, Frame, Op, Sample, Stack};
use itertools::izip;

/// Exponential segments use this one-pole constant so the envelope travels
/// 99.9% of the way to its target over the requested segment time.
const EXP_SEGMENT_TAU: Sample = 6.91;
const SNAP_EPSILON: Sample = 0.001;

pub struct ADSR {
    frame: u64,
    gate_frame_on: [u64; CHANNELS],
    gate_frame_off: [u64; CHANNELS],
    last_gate: Frame,
    sample_period: Sample,
    current_level: Frame,
    release_start_level: Frame,
}

impl ADSR {
    pub fn new(sample_rate: u32) -> Self {
        ADSR {
            frame: 0,
            gate_frame_on: [u64::MAX; CHANNELS],
            gate_frame_off: [0; CHANNELS],
            last_gate: [0.0; CHANNELS],
            sample_period: Sample::from(sample_rate).recip(),
            current_level: [0.0; CHANNELS],
            release_start_level: [0.0; CHANNELS],
        }
    }

    fn decay_level(delta: Sample, d: Sample, s: Sample) -> Sample {
        if d <= 0.0 {
            return s;
        }
        let distance = 1.0 - s;
        let level = s + distance * (-EXP_SEGMENT_TAU * delta / d).exp();
        if (level - s).abs() <= distance.abs() * SNAP_EPSILON {
            s
        } else {
            level
        }
    }

    fn release_level(delta: Sample, r: Sample, start: Sample) -> Sample {
        if r <= 0.0 {
            return 0.0;
        }
        let level = start * (-EXP_SEGMENT_TAU * delta / r).exp();
        if level.abs() <= start.abs() * SNAP_EPSILON {
            0.0
        } else {
            level
        }
    }
}

impl Op for ADSR {
    fn perform(&mut self, stack: &mut Stack) {
        let mut frame = [0.0; CHANNELS];
        let r = stack.pop();
        let s = stack.pop();
        let d = stack.pop();
        let a = stack.pop();
        let gate = stack.pop();
        let now = self.frame as Sample * self.sample_period;
        for (
            output,
            &gate,
            &a,
            &d,
            &s,
            &r,
            last_gate,
            gate_frame_on,
            gate_frame_off,
            current_level,
            release_start_level,
        ) in izip!(
            &mut frame,
            &gate,
            &a,
            &d,
            &s,
            &r,
            &mut self.last_gate,
            &mut self.gate_frame_on,
            &mut self.gate_frame_off,
            &mut self.current_level,
            &mut self.release_start_level
        ) {
            if *last_gate <= 0.0 && gate > 0.0 {
                *gate_frame_on = self.frame;
            }
            if *last_gate > 0.0 && gate <= 0.0 {
                *gate_frame_off = self.frame;
                *release_start_level = *current_level;
            }
            *last_gate = gate;

            let mut level = 0.0;
            if self.frame >= *gate_frame_on {
                if gate > 0.0 {
                    let on = *gate_frame_on as Sample * self.sample_period;
                    let delta = now - on;

                    if a > 0.0 && delta < a {
                        level = delta / a;
                    } else {
                        let delta = (delta - a.max(0.0)).max(0.0);
                        level = Self::decay_level(delta, d, s);
                    }
                } else if *gate_frame_off >= *gate_frame_on {
                    let off = *gate_frame_off as Sample * self.sample_period;
                    let delta = (now - off).max(0.0);
                    level = Self::release_level(delta, r, *release_start_level);
                }
            }

            *current_level = if level.is_finite() { level } else { 0.0 };
            *output = *current_level;
        }
        self.frame += 1;
        stack.push(&frame);
    }

    fn migrate(&mut self, other: &mut dyn Op) {
        if let Some(other) = other.downcast_mut::<Self>() {
            self.frame = other.frame;
            self.gate_frame_on = other.gate_frame_on;
            self.gate_frame_off = other.gate_frame_off;
            self.last_gate = other.last_gate;
            self.current_level = other.current_level;
            self.release_start_level = other.release_start_level;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn perform(op: &mut ADSR, gate: Sample, a: Sample, d: Sample, s: Sample, r: Sample) -> Sample {
        let mut stack = Stack::new();
        stack.push(&[gate; CHANNELS]);
        stack.push(&[a; CHANNELS]);
        stack.push(&[d; CHANNELS]);
        stack.push(&[s; CHANNELS]);
        stack.push(&[r; CHANNELS]);
        op.perform(&mut stack);
        stack.pop()[0]
    }

    #[test]
    fn decay_is_convex_and_below_linear_midpoint() {
        let sr = 1000;
        let mut op = ADSR::new(sr);
        let a = 0.001;
        let d = 0.1;
        let s = 0.25;
        let mut mid = 0.0;
        for n in 0..80 {
            let y = perform(&mut op, 1.0, a, d, s, 0.1);
            assert!(y.is_finite());
            if n == 51 {
                mid = y;
            }
        }
        let linear_midpoint = 1.0 - (1.0 - s) * 0.5;
        assert!(
            mid < linear_midpoint,
            "{mid} should be below {linear_midpoint}"
        );
    }

    #[test]
    fn release_reaches_near_zero_by_release_time() {
        let sr = 1000;
        let mut op = ADSR::new(sr);
        for _ in 0..200 {
            perform(&mut op, 1.0, 0.001, 0.01, 0.8, 0.05);
        }
        let mut y = 1.0;
        for _ in 0..=50 {
            y = perform(&mut op, 0.0, 0.001, 0.01, 0.8, 0.05);
            assert!(y.is_finite());
        }
        assert!(y.abs() < 0.002, "release ended at {y}");
    }

    #[test]
    fn retrigger_mid_release_restarts_attack() {
        let sr = 1000;
        let mut op = ADSR::new(sr);
        for _ in 0..100 {
            perform(&mut op, 1.0, 0.05, 0.05, 0.5, 0.2);
        }
        for _ in 0..20 {
            perform(&mut op, 0.0, 0.05, 0.05, 0.5, 0.2);
        }
        let first = perform(&mut op, 1.0, 0.05, 0.05, 0.5, 0.2);
        let later = perform(&mut op, 1.0, 0.05, 0.05, 0.5, 0.2);
        assert!(first.is_finite() && later.is_finite());
        assert!(later >= first);
    }
}
