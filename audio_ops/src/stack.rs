use audio_vm::{Op, Stack};

#[derive(Clone)]
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

    fn fork(&self) -> Box<dyn Op> {
        Box::new(self.clone())
    }
}

#[derive(Clone)]
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

    fn fork(&self) -> Box<dyn Op> {
        Box::new(self.clone())
    }
}

#[derive(Clone)]
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

    fn fork(&self) -> Box<dyn Op> {
        Box::new(self.clone())
    }
}

#[derive(Clone)]
pub struct Pop;

impl Pop {
    pub fn new() -> Self {
        Pop {}
    }
}

impl Op for Pop {
    fn perform(&mut self, stack: &mut Stack) {
        stack.pop();
    }

    fn fork(&self) -> Box<dyn Op> {
        Box::new(self.clone())
    }
}
