use crate::op::Op;
use crate::sample::{Frame, Sample};
use crate::stack::Stack;
use smallvec::SmallVec;

// Totally unscientific attempt to improve performance of small programs by using SmallVec.
/// FAST_PROGRAM_SIZE determines how large we expect program to be before it would incur exetra indirection.
pub const FAST_PROGRAM_SIZE: usize = 64;

pub type Program = SmallVec<[Box<dyn Op>; FAST_PROGRAM_SIZE]>;

pub struct VM {
    /// Output zero frames disregard of program when true.
    pub pause: bool,
    /// Program to generate audio.
    active_program: Program,
    /// Previous program stored to provide crossfade.
    previous_program: Program,
    /// How many frames left before crossfade end.
    xfade_countdown: Sample,
    /// Total duration of crossfade in frames.
    xfade_duration: Sample,
}

impl VM {
    pub fn new() -> Self {
        VM {
            active_program: Default::default(),
            previous_program: Default::default(),
            xfade_countdown: 0.0,
            xfade_duration: 2048.0,
            pause: false,
        }
    }

    pub fn set_xfade_duration(&mut self, frames: Sample) {
        self.xfade_duration = frames;
    }

    pub fn load_program(&mut self, program: Program) {
        self.previous_program = std::mem::replace(&mut self.active_program, program);
        self.xfade_countdown = self.xfade_duration;
    }

    pub fn migrate_program(&mut self, program: Program, migrate: &[(usize, usize)]) {
        self.load_program(program);
        for &ix in migrate {
            self.active_program[ix.1].migrate(&self.previous_program[ix.0]);
        }
    }

    pub fn next_frame(&mut self) -> Frame {
        if self.pause {
            return Default::default();
        }
        let frame = perform(&mut self.active_program);
        self.xfade(frame)
    }

    #[inline]
    fn xfade(&mut self, mut frame: Frame) -> Frame {
        if self.xfade_countdown > 0.0 {
            let progress = self.xfade_countdown / self.xfade_duration;
            self.xfade_countdown -= 1.0;
            for (x, &p) in frame
                .iter_mut()
                .zip(perform(&mut self.previous_program).iter())
            {
                *x *= 1.0 - progress;
                *x += p * progress;
            }
        }
        frame
    }
}

#[inline]
fn perform(program: &mut Program) -> Frame {
    let mut stack = Stack::new();
    for op in program {
        op.perform(&mut stack);
    }
    stack.peek()
}
