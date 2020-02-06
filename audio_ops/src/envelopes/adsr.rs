use audio_vm::{Frame, Op, Sample, Stack, CHANNELS};
use itertools::izip;

pub struct ADSR {
    frame: usize,
    gate_frame_on: [usize; CHANNELS],
    gate_frame_off: [usize; CHANNELS],
    last_gate: Frame,
    sample_period: Sample,
}

impl ADSR {
    pub fn new(sample_rate: u32) -> Self {
        ADSR {
            frame: 0,
            gate_frame_on: [0; CHANNELS],
            gate_frame_off: [0; CHANNELS],
            last_gate: [0.0; CHANNELS],
            sample_period: Sample::from(sample_rate).recip(),
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
        for (output, &gate, &a, &d, &s, &r, last_gate, gate_frame_on, gate_frame_off) in izip!(
            &mut frame,
            &gate,
            &a,
            &d,
            &s,
            &r,
            &mut self.last_gate,
            &mut self.gate_frame_on,
            &mut self.gate_frame_off
        ) {
            if *last_gate <= 0.0 && gate > 0.0 {
                *gate_frame_on = self.frame;
            }
            if *last_gate > 0.0 && gate <= 0.0 {
                *gate_frame_off = self.frame;
            }
            *last_gate = gate;

            let on = *gate_frame_on as Sample * self.sample_period;
            let delta = now - on;

            if delta <= a {
                *output = delta / a;
                continue;
            }

            let delta = delta - a;

            if delta <= d {
                *output = 1.0 - (1.0 - s) * delta / d;
                continue;
            }

            if gate > 0.0 {
                *output = s;
                continue;
            }

            let off = *gate_frame_off as Sample * self.sample_period;
            let delta = now - off.max(on + a + d);

            if delta <= r {
                *output = s * (1.0 - delta / r);
            }
        }
        self.frame += 1;
        stack.push(&frame);
    }

    fn migrate(&mut self, other: &Box<dyn Op>) {
        if let Some(other) = other.downcast_ref::<Self>() {
            self.frame = other.frame;
            self.gate_frame_on = other.gate_frame_on;
            self.gate_frame_off = other.gate_frame_off;
            self.last_gate = other.last_gate;
        }
    }
}
