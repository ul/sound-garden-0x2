//! Glides for edited numbers.
//!
//! When a live edit changes a number (the same word or node, now a different value), the op that
//! carries it starts from the value its predecessor was producing and eases to the new value over
//! [`GLIDE_FRAMES`] with a raised-cosine curve. An instant change to a running signal clicks no
//! matter how the output is patched afterwards (the reload declick only removes the jump in value,
//! not the kink in slope); easing the number itself makes the edit inaudible as a transition.
//! Rationale in docs/adr/0009-number-edits-glide.md.

use audio_vm::{CHANNELS, Frame, Sample};

/// ~10 ms at 48 kHz: too short to hear as a slide, long enough not to click.
pub const GLIDE_FRAMES: u32 = 480;

#[derive(Clone, Copy, Debug)]
pub struct Glide {
    target: Frame,
    current: Frame,
    from: Frame,
    remaining: u32,
}

impl Glide {
    pub fn new(value: Sample) -> Self {
        let value = [value; CHANNELS];
        Glide {
            target: value,
            current: value,
            from: value,
            remaining: 0,
        }
    }

    /// The value for this frame. Ops on a hot path call [`Glide::is_gliding`] first and advance
    /// in a cold function instead, so their common case stays a leaf without a stack frame.
    #[inline(always)]
    pub fn next(&mut self) -> &Frame {
        if self.remaining > 0 {
            self.step();
        }
        &self.current
    }

    #[inline(always)]
    pub fn is_gliding(&self) -> bool {
        self.remaining > 0
    }

    /// The value while not gliding.
    #[inline(always)]
    pub fn current(&self) -> &Frame {
        &self.current
    }

    /// Advance a glide by one frame; only call while [`Glide::is_gliding`].
    pub fn step(&mut self) {
        self.remaining -= 1;
        let progress = (GLIDE_FRAMES - self.remaining) as Sample / GLIDE_FRAMES as Sample;
        let weight = 0.5 + 0.5 * (std::f64::consts::PI as Sample * progress).cos();
        for ((current, &from), &target) in self.current.iter_mut().zip(&self.from).zip(&self.target)
        {
            *current = target + (from - target) * weight;
        }
        if self.remaining == 0 {
            self.current = self.target;
        }
    }

    /// Continue from the op this one replaces: glide from what it was producing (even mid-glide)
    /// to this op's value. Unchanged numbers stay bit-exact.
    pub fn migrate(&mut self, previous: &Glide) {
        self.migrate_unless(previous, |_, _| false);
    }

    /// Like [`Glide::migrate`], but channels where `jump(old, new)` holds switch at once.
    pub fn migrate_unless(&mut self, previous: &Glide, jump: impl Fn(Sample, Sample) -> bool) {
        let mut glides = false;
        for c in 0..CHANNELS {
            let (old, new) = (previous.current[c], self.target[c]);
            if old != new && !jump(old, new) {
                self.from[c] = old;
                self.current[c] = old;
                glides = true;
            }
        }
        if glides {
            self.remaining = GLIDE_FRAMES;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(glide: &mut Glide, frames: u32) -> Vec<Sample> {
        (0..frames).map(|_| glide.next()[0]).collect()
    }

    #[test]
    fn unchanged_numbers_stay_exact() {
        let mut old = Glide::new(0.2);
        let mut new = Glide::new(0.2);
        new.migrate(&old);
        assert!(run(&mut new, 10).iter().all(|&x| x == 0.2));
        assert!(run(&mut old, 1)[0] == 0.2);
    }

    #[test]
    fn changed_numbers_ease_monotonically_and_land_exactly() {
        let old = Glide::new(0.2);
        let mut new = Glide::new(0.1);
        new.migrate(&old);
        let values = run(&mut new, GLIDE_FRAMES + 10);
        assert!(
            (values[0] - 0.2).abs() < 1e-5,
            "starts where the old value was"
        );
        assert!(values.windows(2).all(|w| w[1] <= w[0]), "never overshoots");
        assert_eq!(values[GLIDE_FRAMES as usize - 1], 0.1);
        assert_eq!(*values.last().unwrap(), 0.1);
    }

    #[test]
    fn an_edit_mid_glide_continues_from_where_it_is() {
        let old = Glide::new(0.0);
        let mut middle = Glide::new(1.0);
        middle.migrate(&old);
        let reached = run(&mut middle, GLIDE_FRAMES / 2)[GLIDE_FRAMES as usize / 2 - 1];
        let mut new = Glide::new(0.0);
        new.migrate(&middle);
        assert!((new.next()[0] - reached).abs() < 1e-4);
    }

    #[test]
    fn jumps_switch_at_once() {
        let old = Glide::new(0.0);
        let mut new = Glide::new(1.0);
        new.migrate_unless(&old, |old, new| old <= 0.0 && new > 0.0);
        assert_eq!(new.next()[0], 1.0);
    }
}
