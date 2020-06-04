use crate::op::Op;
use crate::sample::{AtomicFrame, Frame, Sample};
use crate::stack::Stack;
use alloc_counter::no_alloc;
use smallvec::SmallVec;
use std::sync::{atomic::Ordering, Arc};

// Totally unscientific attempt to improve performance of small programs by using SmallVec.
/// FAST_PROGRAM_SIZE determines how large we expect program to be before it would incur exetra indirection.
pub const FAST_PROGRAM_SIZE: usize = 64;

pub struct Statement {
    pub id: u64,
    pub op: Box<dyn Op>,
}

pub type Program = SmallVec<[Statement; FAST_PROGRAM_SIZE]>;

pub struct VM {
    /// Program to generate audio.
    active_program: Program,
    /// Previous program stored to provide crossfade.
    previous_program: Program,
    /// How many frames left before crossfade end.
    xfade_countdown: Sample,
    /// Total duration of crossfade in frames.
    xfade_duration: Sample,
    /// Crossfade duration left on pause toggle.
    pause_countdown: Sample,
    /// |> / ||
    status: Status,
    /// For oscilloscope-like feedback to the client.
    monitor: Arc<AtomicFrame>,
    /// What Statement output we want to monitor.
    /// 0 has a special meaning of the last Statement.
    monitor_id: u64,
}

impl Default for VM {
    fn default() -> Self {
        VM::new()
    }
}

impl VM {
    pub fn new() -> Self {
        VM {
            active_program: Default::default(),
            previous_program: Default::default(),
            xfade_countdown: 0.0,
            xfade_duration: 2048.0,
            pause_countdown: 0.0,
            status: Status::Pause,
            monitor: Default::default(),
            monitor_id: 0,
        }
    }

    pub fn toggle_play(&mut self) {
        self.pause_countdown = self.xfade_duration;
        self.status = match self.status {
            Status::Play => Status::Pause,
            Status::Pause => Status::Play,
        };
    }

    pub fn play(&mut self) {
        self.pause_countdown = self.xfade_duration;
        self.status = Status::Play;
    }

    pub fn pause(&mut self) {
        self.pause_countdown = self.xfade_duration;
        self.status = Status::Pause;
    }

    pub fn stop(&mut self) {
        self.status = Status::Pause;
    }

    pub fn set_xfade_duration(&mut self, frames: Sample) {
        self.xfade_duration = frames;
    }

    /// Load the new program and crossfade to it from the previous one.
    /// Returns previous value of previous program so it could be deallocated
    /// somewhere else.
    pub fn load_program(&mut self, program: Program) -> Program {
        let garbage = std::mem::replace(
            &mut self.previous_program,
            std::mem::replace(&mut self.active_program, program),
        );
        for stmt in &mut self.active_program {
            if let Some(prev_stmt) = self
                .previous_program
                .iter()
                .find(|prev_stmt| prev_stmt.id == stmt.id)
            {
                stmt.op.migrate(&prev_stmt.op);
            }
        }
        self.xfade_countdown = self.xfade_duration;
        garbage
    }

    #[no_alloc]
    pub fn next_frame(&mut self) -> Frame {
        match self.status {
            Status::Play => {
                let (frame, monitor_frame) =
                    perform_and_monitor(&mut self.active_program, self.monitor_id);
                for (a, &x) in self.monitor.iter().zip(&monitor_frame) {
                    a.store(x.to_bits(), Ordering::Relaxed);
                }
                self.xfade(frame);
                self.play_xfade(frame)
            }
            Status::Pause => {
                if self.pause_countdown > 0.0 {
                    let frame = perform(&mut self.active_program);
                    self.xfade(frame);
                    self.pause_xfade(frame)
                } else {
                    Default::default()
                }
            }
        }
    }

    pub fn monitor(&self) -> Arc<AtomicFrame> {
        Arc::clone(&self.monitor)
    }

    pub fn set_monitor_id(&mut self, id: u64) {
        self.monitor_id = id;
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

    #[inline]
    fn play_xfade(&mut self, mut frame: Frame) -> Frame {
        if self.pause_countdown > 0.0 {
            let progress = 1.0 - (self.pause_countdown / self.xfade_duration);
            self.pause_countdown -= 1.0;
            for x in frame.iter_mut() {
                *x *= progress;
            }
        }
        frame
    }

    #[inline]
    fn pause_xfade(&mut self, mut frame: Frame) -> Frame {
        let progress = self.pause_countdown / self.xfade_duration;
        self.pause_countdown -= 1.0;
        for x in frame.iter_mut() {
            *x *= progress;
        }
        frame
    }
}

#[inline]
fn perform(program: &mut Program) -> Frame {
    let mut stack = Stack::new();
    for stmt in program {
        stmt.op.perform(&mut stack);
    }
    stack.peek()
}

#[inline]
fn perform_and_monitor(program: &mut Program, scope_id: u64) -> (Frame, Frame) {
    let mut scope = Default::default();
    let mut stack = Stack::new();
    for stmt in program {
        stmt.op.perform(&mut stack);
        if scope_id == stmt.id {
            scope = stack.peek();
        }
    }
    let frame = stack.peek();
    if scope_id == 0 {
        scope = frame;
    }
    (frame, scope)
}

enum Status {
    Play,
    Pause,
}
