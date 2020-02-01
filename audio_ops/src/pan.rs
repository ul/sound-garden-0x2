//! # Stereo panner
//!
//! Sources to connect: left, right, position.
use crate::pure;
use audio_vm::{Op, Stack, CHANNELS};
use itertools::izip;

#[derive(Clone)]
pub struct Pan1 {}

/// Pan left and right channels of input signal.
/// Left channel of position signal is used as position value for both.
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

    fn fork(&self) -> Box<dyn Op> {
        Box::new(self.clone())
    }
}

/// Pan left channel of one signal with left channel of another.
/// Left channel of position signal is used as position value for both.
#[derive(Clone)]
pub struct Pan2 {}

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

    fn fork(&self) -> Box<dyn Op> {
        Box::new(self.clone())
    }
}

/// Pan left and right channels of inputs as two pairs of left and right
/// and then output left channel of lefts' pan as left, and right channel
/// of rights' pan as right.
#[derive(Clone)]
pub struct Pan3 {}

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

    fn fork(&self) -> Box<dyn Op> {
        Box::new(self.clone())
    }
}
