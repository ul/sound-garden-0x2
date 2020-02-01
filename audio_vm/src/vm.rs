use crate::op::Op;
use crate::sample::{Frame, Sample};
use crate::stack::Stack;
use smallvec::SmallVec;

// Totally unscientific attempt to improve performance of small programs by using SmallVec.
/// FAST_PROGRAM_SIZE determines how large we expect program to be before it would incur exetra indirection.
pub const FAST_PROGRAM_SIZE: usize = 64;

pub type Program = SmallVec<[Box<dyn Op>; FAST_PROGRAM_SIZE]>;

pub struct VM {
    /// Output zero frames disregard of program.
    pub pause: bool,
    /// We store active and previous program for the sake of crossfade.
    programs: [Program; 2],
    /// How many frames left before crossfade end.
    xfade_countdown: Sample,
    /// Total duration of crossfade in frames.
    xfade_duration: Sample,
}

impl VM {
    pub fn new() -> Self {
        VM {
            programs: Default::default(),
            xfade_countdown: 0.0,
            xfade_duration: 2048.0,
            pause: false,
        }
    }

    pub fn set_xfade_duration(&mut self, frames: Sample) {
        self.xfade_duration = frames;
    }

    pub fn load_program(&mut self, program: Program) {
        self.programs[PREVIOUS_PROGRAM] =
            std::mem::replace(&mut self.programs[ACTIVE_PROGRAM], program);
        self.xfade_countdown = self.xfade_duration;
    }

    // TODO This is really bad as fork may allocate.
    // But it's a tough task, we want reuse ops, have crossfade
    // and don't want to call ops twice at the same time.
    pub fn load_program_reuse(&mut self, program: Program, reuse: &[(usize, usize)]) {
        self.programs[PREVIOUS_PROGRAM] =
            std::mem::replace(&mut self.programs[ACTIVE_PROGRAM], program);
        for &ix in reuse {
            self.programs[ACTIVE_PROGRAM][ix.1] = self.programs[PREVIOUS_PROGRAM][ix.0].fork()
        }
        self.xfade_countdown = self.xfade_duration;
    }

    pub fn next_frame(&mut self) -> Frame {
        if self.pause {
            return Default::default();
        }
        let frame = self.perform(ACTIVE_PROGRAM);
        self.xfade(frame)
    }

    #[inline]
    fn xfade(&mut self, mut frame: Frame) -> Frame {
        if self.xfade_countdown > 0.0 {
            let progress = self.xfade_countdown / self.xfade_duration;
            self.xfade_countdown -= 1.0;
            for (x, &p) in frame.iter_mut().zip(self.perform(PREVIOUS_PROGRAM).iter()) {
                *x *= 1.0 - progress;
                *x += p * progress;
            }
        }
        frame
    }

    #[inline]
    fn perform(&mut self, ix: usize) -> Frame {
        let mut stack = Stack::new();
        for op in &mut self.programs[ix] {
            op.perform(&mut stack);
        }
        stack.peek()
    }
}

const ACTIVE_PROGRAM: usize = 0;
const PREVIOUS_PROGRAM: usize = 1;
