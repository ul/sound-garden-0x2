use crate::glide::Glide;
use audio_vm::{CHANNELS, Op, Sample, Stack};
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

// Binary ops with a constant operand, specialised by the compiler from `x 0.2 *` and friends.
// The constant glides when a live edit changes it (see glide.rs), so number edits don't click.
macro_rules! const_op {
    ($name:ident, |$x:ident, $value:ident| $body:expr) => {
        pub struct $name {
            value: Glide,
        }

        impl $name {
            pub fn new(value: Sample) -> Self {
                Self {
                    value: Glide::new(value),
                }
            }
        }

        impl $name {
            #[inline(always)]
            fn apply(&self, stack: &mut Stack) {
                let mut frame = stack.pop();
                for (sample, &$value) in frame.iter_mut().zip(self.value.current()) {
                    let $x = *sample;
                    *sample = $body;
                }
                stack.push(&frame);
            }

            #[cold]
            #[inline(never)]
            fn perform_gliding(&mut self, stack: &mut Stack) {
                self.value.step();
                self.apply(stack);
            }
        }

        impl Op for $name {
            // A glide runs in a cold tail call, so the common case stays a leaf (see glide.rs).
            fn perform(&mut self, stack: &mut Stack) {
                if self.value.is_gliding() {
                    return self.perform_gliding(stack);
                }
                self.apply(stack);
            }

            fn migrate(&mut self, other: &mut dyn Op) {
                if let Some(other) = other.downcast_mut::<Self>() {
                    self.value.migrate(&other.value);
                }
            }
        }
    };
}

const_op!(AddConst, |x, value| x + value);
const_op!(MulConst, |x, value| x * value);
const_op!(SubConst, |x, value| x - value);
const_op!(RSubConst, |x, value| value - x);
const_op!(DivConst, |x, value| if value != 0.0 {
    x / value
} else {
    0.0
});
const_op!(RDivConst, |x, value| if x != 0.0 { value / x } else { 0.0 });

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
