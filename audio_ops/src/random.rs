use audio_vm::{CHANNELS, Frame, Op, Stack};
use itertools::izip;
use rand::{RngExt, SeedableRng, rngs::SmallRng};

pub struct Rnd {
    rng: SmallRng,
    held: Frame,
    previous_trigger: Frame,
}

impl Rnd {
    pub fn new() -> Self {
        Self::with_seed(None)
    }

    pub fn with_seed(seed: Option<u64>) -> Self {
        Self {
            rng: seed.map_or_else(rand::make_rng, SmallRng::seed_from_u64),
            held: [0.0; CHANNELS],
            previous_trigger: [0.0; CHANNELS],
        }
    }
}

impl Default for Rnd {
    fn default() -> Self {
        Self::new()
    }
}

impl Op for Rnd {
    fn perform(&mut self, stack: &mut Stack) {
        let trigger = stack.pop();
        for (held, previous, &trig) in izip!(&mut self.held, &mut self.previous_trigger, &trigger) {
            if *previous <= 0.0 && trig > 0.0 {
                *held = self.rng.random_range(0.0..1.0);
            }
            *previous = trig;
        }
        stack.push(&self.held);
    }

    fn migrate(&mut self, other: &mut dyn Op) {
        if let Some(other) = other.downcast_mut::<Self>() {
            self.held = other.held;
            self.previous_trigger = other.previous_trigger;
            self.rng = other.rng.clone();
        }
    }
}

pub struct Chance {
    rng: SmallRng,
    previous_trigger: Frame,
    passing: [bool; CHANNELS],
}

impl Chance {
    pub fn new() -> Self {
        Self::with_seed(None)
    }

    pub fn with_seed(seed: Option<u64>) -> Self {
        Self {
            rng: seed.map_or_else(rand::make_rng, SmallRng::seed_from_u64),
            previous_trigger: [0.0; CHANNELS],
            passing: [false; CHANNELS],
        }
    }
}

impl Default for Chance {
    fn default() -> Self {
        Self::new()
    }
}

impl Op for Chance {
    fn perform(&mut self, stack: &mut Stack) {
        let probability = stack.pop();
        let trigger = stack.pop();
        let mut output = [0.0; CHANNELS];

        for (out, previous, passing, &trig, &prob) in izip!(
            &mut output,
            &mut self.previous_trigger,
            &mut self.passing,
            &trigger,
            &probability
        ) {
            if *previous <= 0.0 && trig > 0.0 {
                let prob = if prob.is_finite() {
                    prob.clamp(0.0, 1.0)
                } else {
                    0.0
                };
                *passing = self.rng.random_range(0.0..1.0) < prob;
            } else if trig <= 0.0 {
                *passing = false;
            }
            *out = if trig > 0.0 && *passing { trig } else { 0.0 };
            *previous = trig;
        }

        stack.push(&output);
    }

    fn migrate(&mut self, other: &mut dyn Op) {
        if let Some(other) = other.downcast_mut::<Self>() {
            self.previous_trigger = other.previous_trigger;
            self.passing = other.passing;
            self.rng = other.rng.clone();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rnd(op: &mut dyn Op, trigger: f64) -> Frame {
        let mut stack = Stack::new();
        stack.push(&[trigger; CHANNELS]);
        op.perform(&mut stack);
        stack.pop()
    }

    fn chance(op: &mut dyn Op, trigger: f64, probability: f64) -> Frame {
        let mut stack = Stack::new();
        stack.push(&[trigger; CHANNELS]);
        stack.push(&[probability; CHANNELS]);
        op.perform(&mut stack);
        stack.pop()
    }

    #[test]
    fn rnd_holds_between_triggers_and_changes_on_rising_edge() {
        let mut op = Rnd::with_seed(Some(1));
        let first = rnd(&mut op, 1.0);
        assert_eq!(rnd(&mut op, 1.0), first);
        assert_eq!(rnd(&mut op, 0.0), first);
        let second = rnd(&mut op, 1.0);
        assert_ne!(second, first);
        assert!(second.iter().all(|x| (0.0..1.0).contains(x)));
    }

    #[test]
    fn rnd_seed_is_deterministic() {
        let mut a = Rnd::with_seed(Some(42));
        let mut b = Rnd::with_seed(Some(42));
        let triggers = [1.0, 0.0, 1.0, 0.0, 1.0];
        for trigger in triggers {
            assert_eq!(rnd(&mut a, trigger), rnd(&mut b, trigger));
        }
    }

    #[test]
    fn chance_extreme_probabilities_and_sustained_gates() {
        let mut pass = Chance::with_seed(Some(3));
        assert_eq!(chance(&mut pass, 0.8, 1.0), [0.8; CHANNELS]);
        assert_eq!(chance(&mut pass, 0.6, 0.0), [0.6; CHANNELS]);
        assert_eq!(chance(&mut pass, 0.0, 0.0), [0.0; CHANNELS]);

        let mut suppress = Chance::with_seed(Some(3));
        assert_eq!(chance(&mut suppress, 1.0, 0.0), [0.0; CHANNELS]);
        assert_eq!(chance(&mut suppress, 1.0, 1.0), [0.0; CHANNELS]);
    }
}
