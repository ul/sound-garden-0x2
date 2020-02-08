use audio_vm::{Frame, Op, Stack};

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
}

pub struct Dig {
    t: Vec<Frame>,
}

impl Dig {
    pub fn new(depth: usize) -> Self {
        Dig {
            t: vec![Default::default(); depth],
        }
    }
}

impl Op for Dig {
    fn perform(&mut self, stack: &mut Stack) {
        for x in self.t.iter_mut() {
            *x = stack.pop();
        }
        for x in self.t.iter_mut().rev().skip(1) {
            stack.push(&std::mem::replace(x, Default::default()));
        }
        let depth = self.t.len();
        stack.push(&std::mem::replace(
            &mut self.t[depth - 1],
            Default::default(),
        ));
    }
}

pub struct Bury {
    t: Vec<Frame>,
}

impl Bury {
    pub fn new(depth: usize) -> Self {
        Bury {
            t: vec![Default::default(); depth],
        }
    }
}

impl Op for Bury {
    fn perform(&mut self, stack: &mut Stack) {
        for x in self.t.iter_mut() {
            *x = stack.pop();
        }
        stack.push(&std::mem::replace(&mut self.t[0], Default::default()));
        for x in self.t[1..].iter_mut().rev() {
            stack.push(&std::mem::replace(x, Default::default()));
        }
    }
}
