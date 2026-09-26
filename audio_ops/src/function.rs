use audio_vm::{CHANNELS, Frame, Op, Sample, Stack};
use itertools::izip;

// Fn1..Fn5 are generic over the function so that `FnN::new(pure::add)` is
// monomorphised over the zero-sized function item type and inlined, instead of
// paying an indirect call per channel per frame. The default type parameter keeps
// the plain function-pointer form available where the function is chosen at runtime.

pub struct Fn1<F = fn(Sample) -> Sample> {
    f: F,
}

impl<F: Fn(Sample) -> Sample> Fn1<F> {
    pub fn new(f: F) -> Self {
        Fn1 { f }
    }
}

impl<F: Fn(Sample) -> Sample + Send + 'static> Op for Fn1<F> {
    fn perform(&mut self, stack: &mut Stack) {
        let mut frame = [0.0; CHANNELS];
        for (y, &x) in frame.iter_mut().zip(&stack.pop()) {
            *y = (self.f)(x);
        }
        stack.push(&frame);
    }
}

pub struct Fn2<F = fn(Sample, Sample) -> Sample> {
    f: F,
}

impl<F: Fn(Sample, Sample) -> Sample> Fn2<F> {
    pub fn new(f: F) -> Self {
        Fn2 { f }
    }
}

impl<F: Fn(Sample, Sample) -> Sample + Send + 'static> Op for Fn2<F> {
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

pub struct AddConst {
    value: Frame,
}

impl AddConst {
    pub fn new(value: Sample) -> Self {
        Self {
            value: [value; CHANNELS],
        }
    }
}

impl Op for AddConst {
    fn perform(&mut self, stack: &mut Stack) {
        let mut frame = stack.pop();
        for (sample, &value) in frame.iter_mut().zip(&self.value) {
            *sample += value;
        }
        stack.push(&frame);
    }
}

pub struct MulConst {
    value: Frame,
}

impl MulConst {
    pub fn new(value: Sample) -> Self {
        Self {
            value: [value; CHANNELS],
        }
    }
}

impl Op for MulConst {
    fn perform(&mut self, stack: &mut Stack) {
        let mut frame = stack.pop();
        for (sample, &value) in frame.iter_mut().zip(&self.value) {
            *sample *= value;
        }
        stack.push(&frame);
    }
}

pub struct SubConst {
    value: Frame,
}

impl SubConst {
    pub fn new(value: Sample) -> Self {
        Self {
            value: [value; CHANNELS],
        }
    }
}

impl Op for SubConst {
    fn perform(&mut self, stack: &mut Stack) {
        let mut frame = stack.pop();
        for (sample, &value) in frame.iter_mut().zip(&self.value) {
            *sample -= value;
        }
        stack.push(&frame);
    }
}

pub struct RSubConst {
    value: Frame,
}

impl RSubConst {
    pub fn new(value: Sample) -> Self {
        Self {
            value: [value; CHANNELS],
        }
    }
}

impl Op for RSubConst {
    fn perform(&mut self, stack: &mut Stack) {
        let mut frame = stack.pop();
        for (sample, &value) in frame.iter_mut().zip(&self.value) {
            *sample = value - *sample;
        }
        stack.push(&frame);
    }
}

pub struct DivConst {
    value: Frame,
}

impl DivConst {
    pub fn new(value: Sample) -> Self {
        Self {
            value: [value; CHANNELS],
        }
    }
}

impl Op for DivConst {
    fn perform(&mut self, stack: &mut Stack) {
        let mut frame = stack.pop();
        for (sample, &value) in frame.iter_mut().zip(&self.value) {
            *sample = if value != 0.0 { *sample / value } else { 0.0 };
        }
        stack.push(&frame);
    }
}

pub struct RDivConst {
    value: Frame,
}

impl RDivConst {
    pub fn new(value: Sample) -> Self {
        Self {
            value: [value; CHANNELS],
        }
    }
}

impl Op for RDivConst {
    fn perform(&mut self, stack: &mut Stack) {
        let mut frame = stack.pop();
        for (sample, &value) in frame.iter_mut().zip(&self.value) {
            *sample = if *sample != 0.0 { value / *sample } else { 0.0 };
        }
        stack.push(&frame);
    }
}

pub struct Fn3<F = fn(Sample, Sample, Sample) -> Sample> {
    f: F,
}

impl<F: Fn(Sample, Sample, Sample) -> Sample> Fn3<F> {
    pub fn new(f: F) -> Self {
        Fn3 { f }
    }
}

impl<F: Fn(Sample, Sample, Sample) -> Sample + Send + 'static> Op for Fn3<F> {
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

pub struct Fn4<F = fn(Sample, Sample, Sample, Sample) -> Sample> {
    f: F,
}

impl<F: Fn(Sample, Sample, Sample, Sample) -> Sample> Fn4<F> {
    pub fn new(f: F) -> Self {
        Fn4 { f }
    }
}

impl<F: Fn(Sample, Sample, Sample, Sample) -> Sample + Send + 'static> Op for Fn4<F> {
    fn perform(&mut self, stack: &mut Stack) {
        let d = stack.pop();
        let c = stack.pop();
        let b = stack.pop();
        let a = stack.pop();
        let mut frame = [0.0; CHANNELS];
        for (y, &a, &b, &c, &d) in izip!(&mut frame, &a, &b, &c, &d) {
            *y = (self.f)(a, b, c, d);
        }
        stack.push(&frame);
    }
}

pub struct Fn5<F = fn(Sample, Sample, Sample, Sample, Sample) -> Sample> {
    f: F,
}

impl<F: Fn(Sample, Sample, Sample, Sample, Sample) -> Sample> Fn5<F> {
    pub fn new(f: F) -> Self {
        Fn5 { f }
    }
}

impl<F: Fn(Sample, Sample, Sample, Sample, Sample) -> Sample + Send + 'static> Op for Fn5<F> {
    fn perform(&mut self, stack: &mut Stack) {
        let e = stack.pop();
        let d = stack.pop();
        let c = stack.pop();
        let b = stack.pop();
        let a = stack.pop();
        let mut frame = [0.0; CHANNELS];
        for (y, &a, &b, &c, &d, &e) in izip!(&mut frame, &a, &b, &c, &d, &e) {
            *y = (self.f)(a, b, c, d, e);
        }
        stack.push(&frame);
    }
}
