use crate::op::Op;
use crate::sample::Frame;
use crate::stack::Stack;
use smallvec::SmallVec;

pub const FAST_PROGRAM_SIZE: usize = 64;

pub type Program = SmallVec<[Box<dyn Op + Send>; FAST_PROGRAM_SIZE]>;

pub struct VM {
    stack: Stack,
    ops: Program,
}

impl VM {
    pub fn new() -> Self {
        VM {
            stack: Stack::new(),
            ops: SmallVec::new(),
        }
    }

    pub fn load_program(&mut self, ops: Program) {
        self.ops = ops;
    }

    pub fn next_frame(&mut self) -> Frame {
        let stack = &mut self.stack;
        stack.reset();
        for op in self.ops.iter_mut() {
            op.perform(stack);
        }
        stack.peek()
    }
}
