use crate::op::Op;
use crate::sample::{Frame, Sample};
use crate::stack::Stack;
use smallvec::SmallVec;

pub const FAST_PROGRAM_SIZE: usize = 64;
pub const FADE_FRAMES: Sample = 2048.0;
const FADE_FRAMES_RECIP: Sample = 1.0 / FADE_FRAMES;

pub type Program = SmallVec<[Box<dyn Op + Send>; FAST_PROGRAM_SIZE]>;

pub struct VM {
    stack: Stack,
    ops: Program,
    fade_counter: Sample,
    fade_in: bool,
}

impl VM {
    pub fn new() -> Self {
        VM {
            stack: Stack::new(),
            ops: SmallVec::new(),
            fade_counter: 0.0,
            fade_in: true,
        }
    }

    pub fn load_program(&mut self, ops: Program) {
        self.ops = ops;
    }

    pub fn unload_program(&mut self) -> Program {
        std::mem::replace(&mut self.ops, SmallVec::new())
    }

    pub fn next_frame(&mut self) -> Frame {
        let stack = &mut self.stack;
        stack.reset();
        for op in self.ops.iter_mut() {
            op.perform(stack);
        }
        let mut frame = stack.peek();
        if self.fade_counter > 0.0 {
            let progress = FADE_FRAMES_RECIP * self.fade_counter;
            self.fade_counter -= 1.0;
            let amp = if self.fade_in {
                1.0 - progress
            } else {
                progress
            };
            for x in &mut frame {
                *x *= amp;
            }
        }
        frame
    }

    pub fn fade_in(&mut self) {
        self.fade_counter = FADE_FRAMES;
        self.fade_in = true;
    }

    pub fn fade_out(&mut self) {
        self.fade_counter = FADE_FRAMES;
        self.fade_in = false;
    }
}
