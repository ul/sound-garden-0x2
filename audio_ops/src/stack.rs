use audio_vm::{Op, Stack};

pub struct Dup;

impl Dup {
    pub fn new() -> Self {
        Dup {}
    }
}

impl Op for Dup {
    fn perform(&mut self, stack: &mut Stack) {
        stack.push(&stack.peek());
    }
}

pub struct Swap;

impl Swap {
    pub fn new() -> Self {
        Swap {}
    }
}

impl Op for Swap {
    fn perform(&mut self, stack: &mut Stack) {
        let a = stack.pop();
        let b = stack.pop();
        stack.push(&a);
        stack.push(&b);
    }
}

pub struct Rot;

impl Rot {
    pub fn new() -> Self {
        Rot {}
    }
}

impl Op for Rot {
    fn perform(&mut self, stack: &mut Stack) {
        let a = stack.pop();
        let b = stack.pop();
        let c = stack.pop();
        stack.push(&b);
        stack.push(&a);
        stack.push(&c);
    }
}
