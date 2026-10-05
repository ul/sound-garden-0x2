use crate::glide::Glide;
use audio_vm::{Op, Sample, Stack};

pub struct Constant {
    value: Glide,
}

impl Constant {
    pub fn new(sample_rate: u32, x: Sample) -> Self {
        Constant {
            value: Glide::new(sample_rate, x),
        }
    }
}

impl Constant {
    #[cold]
    #[inline(never)]
    fn perform_gliding(&mut self, stack: &mut Stack) {
        self.value.step();
        stack.push(self.value.current());
    }
}

impl Op for Constant {
    fn perform(&mut self, stack: &mut Stack) {
        // A glide runs in a cold tail call, so the common case stays a leaf (see glide.rs).
        if self.value.is_gliding() {
            return self.perform_gliding(stack);
        }
        stack.push(self.value.current());
    }

    /// An edited number glides to its new value, except when it turns on: a plain constant may
    /// be a gate or trigger, and ops that latch amplitude on a rising edge (`adsr`, `impulse`,
    /// `poly`, `grain`) must see the new value at once, not the first step of a glide.
    fn migrate(&mut self, other: &mut dyn Op) {
        if let Some(other) = other.downcast_mut::<Self>() {
            self.value
                .migrate_unless(&other.value, |old, new| old <= 0.0 && new > 0.0);
        }
    }
}
