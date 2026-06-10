//! # Polyphonic voice container
//!
//! `<value> <ctl> poly:N` — runs N copies of a compiled voice body (a
//! quotation) and pushes the sum of their outputs. See
//! docs/adr/0002-polyphony-container-op.md for the full design.
//!
//! The op is gate-transparent: it does not synthesize impulses. On a rising
//! edge of the control signal (previous sample ≤ 0, current > 0 on any
//! channel) it allocates the next voice (least-recently-released policy,
//! which under a single serialized control input is plain round-robin) and
//! latches the current value into it. Each sample, the most recently
//! allocated voice receives the live control signal as-is (so trig bodies see
//! a one-sample impulse and gate bodies see the full gate including its fall;
//! amplitude passes through, so gates can carry velocity), and all other
//! voices receive 0. Each voice's sub-program runs against a sub-stack
//! initialized to `[latched_value, routed_ctl]`.
use audio_vm::{CHANNELS, Frame, Op, Stack, Statement, migrate_program_state};

const SILENCE: Frame = [0.0; CHANNELS];

struct Voice {
    program: Box<[Statement]>,
    latched: Frame,
}

pub struct Poly {
    voices: Vec<Voice>,
    /// Most recently allocated voice; receives the live control signal.
    current: Option<usize>,
    /// Previous control frame for rising-edge detection.
    previous_ctl: Frame,
    /// Reused sub-stack for voice bodies.
    stack: Stack,
}

impl Poly {
    pub fn new(bodies: Vec<Box<[Statement]>>) -> Self {
        Poly {
            voices: bodies
                .into_iter()
                .map(|program| Voice {
                    program,
                    latched: SILENCE,
                })
                .collect(),
            current: None,
            previous_ctl: SILENCE,
            stack: Stack::new(),
        }
    }

    /// Forgiving zero-voice op for invalid quotations/arguments: preserves
    /// stack shape (consumes value and ctl, pushes silence).
    pub fn empty() -> Self {
        Poly::new(Vec::new())
    }
}

impl Op for Poly {
    fn perform(&mut self, stack: &mut Stack) {
        let ctl = stack.pop();
        let value = stack.pop();
        let rising = self
            .previous_ctl
            .iter()
            .zip(&ctl)
            .any(|(&previous, &current)| previous <= 0.0 && current > 0.0);
        self.previous_ctl = ctl;
        if rising && !self.voices.is_empty() {
            let next = self
                .current
                .map_or(0, |current| (current + 1) % self.voices.len());
            self.voices[next].latched = value;
            self.current = Some(next);
        }
        let mut sum = SILENCE;
        for (index, voice) in self.voices.iter_mut().enumerate() {
            self.stack.reset();
            self.stack.push(&voice.latched);
            self.stack.push(if Some(index) == self.current {
                &ctl
            } else {
                &SILENCE
            });
            for stmt in voice.program.iter_mut() {
                stmt.op.perform(&mut self.stack);
            }
            let frame = self.stack.peek();
            for (sum, x) in sum.iter_mut().zip(&frame) {
                *sum += x;
            }
        }
        stack.push(&sum);
    }

    fn migrate(&mut self, other: &mut dyn Op) {
        if let Some(other) = other.downcast_mut::<Self>() {
            self.previous_ctl = other.previous_ctl;
            if !self.voices.is_empty() {
                self.current = other.current.map(|current| current % self.voices.len());
            }
            for (voice, other_voice) in self.voices.iter_mut().zip(other.voices.iter_mut()) {
                voice.latched = other_voice.latched;
                migrate_program_state(&mut voice.program, &mut other_voice.program);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use audio_vm::Sample;

    /// Pops ctl and value, pushes `latched_value + ctl * 10` so tests can
    /// observe both routing and latching per voice.
    struct Probe;

    impl Op for Probe {
        fn perform(&mut self, stack: &mut Stack) {
            let ctl = stack.pop();
            let value = stack.pop();
            stack.push(&[value[0] + ctl[0] * 10.0, value[1] + ctl[1] * 10.0]);
        }
    }

    /// Stateful counter to observe per-voice state migration.
    struct Count {
        count: Sample,
    }

    impl Op for Count {
        fn perform(&mut self, stack: &mut Stack) {
            stack.pop();
            stack.pop();
            self.count += 1.0;
            stack.push(&[self.count; CHANNELS]);
        }

        fn migrate(&mut self, other: &mut dyn Op) {
            if let Some(other) = other.downcast_mut::<Self>() {
                self.count = other.count;
            }
        }
    }

    fn probe_poly(voices: usize) -> Poly {
        Poly::new(
            (0..voices)
                .map(|_| {
                    vec![Statement {
                        id: 1,
                        op: Box::new(Probe) as Box<dyn Op>,
                    }]
                    .into_boxed_slice()
                })
                .collect(),
        )
    }

    fn frame(poly: &mut Poly, value: Sample, ctl: Sample) -> Frame {
        let mut stack = Stack::new();
        stack.push(&[value; CHANNELS]);
        stack.push(&[ctl; CHANNELS]);
        poly.perform(&mut stack);
        stack.peek()
    }

    #[test]
    fn allocates_round_robin_and_latches_value_on_rising_edge() {
        let mut poly = probe_poly(2);
        // Trigger voice 0 with value 60: voice 0 = 60 + 10, voice 1 silent.
        assert_eq!(frame(&mut poly, 60.0, 1.0), [70.0, 70.0]);
        // Ctl back to 0: both voices keep latched values, no routed ctl.
        assert_eq!(frame(&mut poly, 99.0, 0.0), [60.0, 60.0]);
        // Next trigger goes to voice 1 and latches 64: 60 + (64 + 10).
        assert_eq!(frame(&mut poly, 64.0, 1.0), [134.0, 134.0]);
        // Release, then the third trigger steals voice 0 (round-robin):
        // (72 + 10) + 64.
        assert_eq!(frame(&mut poly, 64.0, 0.0), [124.0, 124.0]);
        assert_eq!(frame(&mut poly, 72.0, 1.0), [146.0, 146.0]);
    }

    #[test]
    fn is_gate_transparent_and_passes_amplitude_through() {
        let mut poly = probe_poly(2);
        // Gate rises at 0.7 (velocity) and holds: only the rising edge
        // allocates, the held gate keeps routing to the same voice.
        assert_eq!(frame(&mut poly, 60.0, 0.7), [67.0, 67.0]);
        assert_eq!(frame(&mut poly, 60.0, 0.7), [67.0, 67.0]);
        // Gate falls: voice 0 sees ctl 0 (release), keeps latched value.
        assert_eq!(frame(&mut poly, 60.0, 0.0), [60.0, 60.0]);
        // New gate allocates voice 1; voice 0 unaffected.
        assert_eq!(frame(&mut poly, 64.0, 1.0), [134.0, 134.0]);
    }

    #[test]
    fn held_gate_does_not_retrigger() {
        let mut poly = probe_poly(4);
        frame(&mut poly, 60.0, 1.0);
        // Rising ctl while already positive is not an edge.
        assert_eq!(frame(&mut poly, 61.0, 2.0), [80.0, 80.0]);
    }

    #[test]
    fn empty_poly_consumes_inputs_and_pushes_silence() {
        let mut poly = Poly::empty();
        let mut stack = Stack::new();
        stack.push(&[5.0; CHANNELS]);
        stack.push(&[60.0; CHANNELS]);
        stack.push(&[1.0; CHANNELS]);
        poly.perform(&mut stack);
        assert_eq!(stack.pop(), [0.0, 0.0]);
        // Stack shape preserved: the frame below the consumed pair survives.
        assert_eq!(stack.pop(), [5.0, 5.0]);
    }

    #[test]
    fn migrate_steals_allocator_and_per_voice_state() {
        let count_poly = |voices: usize| {
            Poly::new(
                (0..voices)
                    .map(|_| {
                        vec![Statement {
                            id: 7,
                            op: Box::new(Count { count: 0.0 }) as Box<dyn Op>,
                        }]
                        .into_boxed_slice()
                    })
                    .collect(),
            )
        };
        let mut old = count_poly(2);
        frame(&mut old, 60.0, 1.0); // counts: [1, 1], current = 0, prev_ctl = 1
        let mut new = count_poly(2);
        new.migrate(&mut old);
        // prev_ctl migrated: a held ctl is not a new edge, so current voice
        // allocation is preserved and counters continue from stolen state.
        assert_eq!(frame(&mut new, 60.0, 1.0), [4.0, 4.0]); // counts: [2, 2]
    }

    #[test]
    fn migrate_wraps_current_voice_when_shrinking() {
        let mut old = probe_poly(3);
        frame(&mut old, 60.0, 1.0);
        frame(&mut old, 60.0, 0.0);
        frame(&mut old, 64.0, 1.0);
        frame(&mut old, 64.0, 0.0);
        frame(&mut old, 67.0, 1.0); // current = 2
        frame(&mut old, 67.0, 0.0); // release so the next edge can rise
        let mut new = probe_poly(2);
        new.migrate(&mut old);
        // Latched values for surviving voices: [60, 64]; current 2 wraps to 0
        // in the 2-voice op, so the next trigger steals voice 1:
        // voice 0 = 60, voice 1 = 72 + 10.
        assert_eq!(frame(&mut new, 72.0, 1.0), [142.0, 142.0]);
    }
}
