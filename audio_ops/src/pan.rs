//! # Stereo panner
//!
//! Sources to connect: left, right, position.
use crate::pure;
use audio_vm::{CHANNELS, Op, Stack};
use itertools::izip;

pub struct Width;

impl Default for Width {
    fn default() -> Self {
        Self::new()
    }
}

impl Width {
    pub fn new() -> Self {
        Width {}
    }
}

impl Op for Width {
    fn perform(&mut self, stack: &mut Stack) {
        let width = stack.pop()[0];
        let input = stack.pop();
        let mid = (input[0] + input[1]) * 0.5;
        let side = (input[0] - input[1]) * 0.5;
        stack.push(&[mid + width * side, mid - width * side]);
    }
}

pub struct Pan1;

/// Pan left and right channels of input signal.
/// Left channel of position signal is used as position value for both.
impl Default for Pan1 {
    fn default() -> Self {
        Self::new()
    }
}

impl Pan1 {
    pub fn new() -> Self {
        Pan1 {}
    }
}

impl Op for Pan1 {
    fn perform(&mut self, stack: &mut Stack) {
        let position = stack.pop();
        let input = stack.pop();
        let (l, r) = pure::pan(input[0], input[1], position[0]);
        stack.push(&[l, r]);
    }
}

/// Pan left channel of one signal with left channel of another.
/// Left channel of position signal is used as position value for both.
pub struct Pan2;

impl Default for Pan2 {
    fn default() -> Self {
        Self::new()
    }
}

impl Pan2 {
    pub fn new() -> Self {
        Pan2 {}
    }
}

impl Op for Pan2 {
    fn perform(&mut self, stack: &mut Stack) {
        let c = stack.pop()[0]; // left of the position
        let r = stack.pop()[0]; // left of the second input
        let l = stack.pop()[0]; // left of the first input
        let (l, r) = pure::pan(l, r, c);
        stack.push(&[l, r]);
    }
}

/// Pan left and right channels of inputs as two pairs of left and right
/// and then output left channel of lefts' pan as left, and right channel
/// of rights' pan as right.
pub struct Pan3;

impl Default for Pan3 {
    fn default() -> Self {
        Self::new()
    }
}

impl Pan3 {
    pub fn new() -> Self {
        Pan3 {}
    }
}

impl Op for Pan3 {
    fn perform(&mut self, stack: &mut Stack) {
        let mut frame = [0.0; CHANNELS];
        let position = stack.pop();
        let right = stack.pop();
        let left = stack.pop();
        for (channel, (output, &l, &r, &c)) in
            izip!(&mut frame, &left, &right, &position).enumerate()
        {
            // Left output is left of pan of left inputs, right output right is pan of right inputs.
            // I don't know who need this.
            *output = match channel {
                0 => 1.0_f64.min(1.0 - c).sqrt() * l + 0.0_f64.max(-c).sqrt() * r,
                1 => 0.0_f64.max(c).sqrt() * l + 1.0_f64.min(1.0 + c).sqrt() * r,
                _ => 0.0,
            }
        }
        stack.push(&frame);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn perform_width(input: [f64; CHANNELS], width: f64) -> [f64; CHANNELS] {
        let mut op = Width::new();
        let mut stack = Stack::new();
        stack.push(&input);
        stack.push(&[width; CHANNELS]);
        op.perform(&mut stack);
        stack.pop()
    }

    #[test]
    fn width_zero_makes_mono() {
        let output = perform_width([0.75, -0.25], 0.0);
        assert_eq!(output[0], output[1]);
        assert_eq!(output, [0.25, 0.25]);
    }

    #[test]
    fn width_one_is_identity() {
        assert_eq!(perform_width([0.75, -0.25], 1.0), [0.75, -0.25]);
    }
}
