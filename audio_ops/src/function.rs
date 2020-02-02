use audio_vm::{Op, Sample, Stack, CHANNELS};
use itertools::izip;

pub struct Fn1 {
    f: fn(Sample) -> Sample,
}

impl Fn1 {
    pub fn new(f: fn(Sample) -> Sample) -> Self {
        Fn1 { f }
    }
}

impl Op for Fn1 {
    fn perform(&mut self, stack: &mut Stack) {
        let mut frame = [0.0; CHANNELS];
        for (y, &x) in frame.iter_mut().zip(&stack.pop()) {
            *y = (self.f)(x);
        }
        stack.push(&frame);
    }
}

pub struct Fn2 {
    f: fn(Sample, Sample) -> Sample,
}

impl Fn2 {
    pub fn new(f: fn(Sample, Sample) -> Sample) -> Self {
        Fn2 { f }
    }
}

impl Op for Fn2 {
    fn perform(&mut self, stack: &mut Stack) {
        let b = stack.pop();
        let a = stack.pop();
        let mut frame = [0.0; CHANNELS];
        for (y, &a, &b) in izip!(&mut frame, &a, &b) {
            *y = (self.f)(a, b);
        }
        stack.push(&frame);
    }
}

pub struct Fn3 {
    f: fn(Sample, Sample, Sample) -> Sample,
}

impl Fn3 {
    pub fn new(f: fn(Sample, Sample, Sample) -> Sample) -> Self {
        Fn3 { f }
    }
}

impl Op for Fn3 {
    fn perform(&mut self, stack: &mut Stack) {
        let c = stack.pop();
        let b = stack.pop();
        let a = stack.pop();
        let mut frame = [0.0; CHANNELS];
        for (y, &a, &b, &c) in izip!(&mut frame, &a, &b, &c) {
            *y = (self.f)(a, b, c);
        }
        stack.push(&frame);
    }
}
