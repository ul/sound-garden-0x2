use crate::op::Op;
use crate::sample::{AtomicFrame, Frame, Sample};
use crate::stack::Stack;
#[cfg(feature = "allocation-checks")]
use alloc_counter::no_alloc;
use smallvec::SmallVec;
use std::sync::{atomic::Ordering, Arc};

// Totally unscientific attempt to improve performance of small programs by using SmallVec.
/// FAST_PROGRAM_SIZE determines how large we expect program to be before it would incur extra indirection.
pub const FAST_PROGRAM_SIZE: usize = 64;
const MIGRATION_INDEX_SIZE: usize = 128;
/// Default program reload declick duration in frames (~5 ms at 48 kHz).
const DECLICK_DURATION: usize = 256;
/// Residual level the declick correction decays to over its duration (-100 dB).
const DECLICK_RESIDUAL: Sample = 1e-5;
/// Reload steps below this level are inaudible; skip declicking to keep output bit-exact.
const DECLICK_THRESHOLD: Sample = 1e-6;

pub struct Statement {
    pub id: u64,
    pub op: Box<dyn Op>,
}

pub type Program = SmallVec<[Statement; FAST_PROGRAM_SIZE]>;

pub struct VM {
    /// Program to generate audio.
    active_program: Program,
    /// Reused stack for the active program hot path.
    active_stack: Stack,
    /// Total duration of play/pause fade in frames.
    xfade_duration: usize,
    /// Reciprocal of fade duration, cached to avoid per-frame division.
    xfade_duration_recip: Sample,
    /// Crossfade duration left on play/pause toggle.
    pause_countdown: usize,
    status: Status,
    /// For oscilloscope-like feedback to the client.
    monitor: Arc<AtomicFrame>,
    /// What Statement output we want to monitor.
    /// 0 has a special meaning of the last Statement.
    monitor_id: u64,
    /// Last output frame, used to measure the step introduced by a program reload.
    last_frame: Frame,
    /// Exponentially decaying correction that cancels the program reload step.
    declick_offset: Frame,
    /// Frames of declick correction left.
    declick_countdown: usize,
    /// Total declick duration in frames.
    declick_duration: usize,
    /// Per-frame decay factor of the declick correction.
    declick_decay: Sample,
    /// Set by load_program while audible; the next frame captures the reload step.
    declick_pending: bool,
}

impl Default for VM {
    fn default() -> Self {
        VM::new()
    }
}

impl VM {
    pub fn new() -> Self {
        Self {
            active_program: Default::default(),
            active_stack: Stack::new(),
            xfade_duration: 8192,
            xfade_duration_recip: 1.0 / 8192.0,
            pause_countdown: 0,
            status: Status::Pause,
            monitor: Default::default(),
            monitor_id: 0,
            last_frame: Default::default(),
            declick_offset: Default::default(),
            declick_countdown: 0,
            declick_duration: DECLICK_DURATION,
            declick_decay: declick_decay(DECLICK_DURATION),
            declick_pending: false,
        }
    }

    pub fn toggle_play(&mut self) {
        match self.status {
            Status::Play => self.pause(),
            Status::Pause => self.play(),
        }
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
        self.pause_countdown = 0;
        self.status = Status::Pause;
    }

    /// Set play/pause fade duration in frames. Program reload crossfade is disabled.
    pub fn set_xfade_duration(&mut self, frames: Sample) {
        self.xfade_duration = frames.max(0.0) as usize;
        self.xfade_duration_recip = if self.xfade_duration > 0 {
            (self.xfade_duration as Sample).recip()
        } else {
            0.0
        };
    }

    /// Set program reload declick duration in frames. 0 disables declicking.
    pub fn set_declick_duration(&mut self, frames: Sample) {
        self.declick_duration = frames.max(0.0) as usize;
        self.declick_decay = declick_decay(self.declick_duration);
        self.declick_countdown = self.declick_countdown.min(self.declick_duration);
    }

    /// Load the new program and steal/migrate state from the previous active program.
    /// Returns the old program so it can be deallocated somewhere else.
    pub fn load_program(&mut self, program: Program) -> Program {
        let mut garbage = std::mem::replace(&mut self.active_program, program);
        migrate_program_state(&mut self.active_program, &mut garbage);
        // Arm the declicker only when the VM is audible; a silent VM cannot click.
        self.declick_pending = self.declick_duration > 0
            && (matches!(self.status, Status::Play) || self.pause_countdown > 0);
        garbage
    }

    #[cfg_attr(feature = "allocation-checks", no_alloc)]
    pub fn next_frame(&mut self) -> Frame {
        let frame = match self.status {
            Status::Play => {
                let (frame, monitor_frame) = if self.monitor_id == 0 {
                    let frame = perform(&mut self.active_program, &mut self.active_stack);
                    (frame, frame)
                } else {
                    perform_and_monitor(
                        &mut self.active_program,
                        &mut self.active_stack,
                        self.monitor_id,
                    )
                };

                for (a, &x) in self.monitor.iter().zip(&monitor_frame) {
                    a.store(x.to_bits(), Ordering::Relaxed);
                }

                let frame = self.declick(frame);
                self.play_xfade(frame)
            }
            Status::Pause => {
                if self.pause_countdown > 0 {
                    let frame = perform(&mut self.active_program, &mut self.active_stack);
                    let frame = self.declick(frame);
                    self.pause_xfade(frame)
                } else {
                    // Fully silent: nothing to declick against.
                    self.declick_pending = false;
                    self.declick_countdown = 0;
                    Default::default()
                }
            }
        };
        self.last_frame = frame;
        frame
    }

    pub fn monitor(&self) -> Arc<AtomicFrame> {
        Arc::clone(&self.monitor)
    }

    pub fn set_monitor_id(&mut self, id: u64) {
        self.monitor_id = id;
    }

    /// Cancel the step discontinuity introduced by a program reload: on the first
    /// frame after the reload, capture the step against the last heard frame, then
    /// add it back to the output while it decays exponentially to silence.
    fn declick(&mut self, mut frame: Frame) -> Frame {
        if self.declick_pending {
            self.declick_pending = false;
            let mut step: Sample = 0.0;
            for (offset, (&last, &new)) in self
                .declick_offset
                .iter_mut()
                .zip(self.last_frame.iter().zip(&frame))
            {
                *offset = last - new;
                step = step.max(offset.abs());
            }
            self.declick_countdown = if step > DECLICK_THRESHOLD {
                self.declick_duration
            } else {
                0
            };
        }

        if self.declick_countdown > 0 {
            self.declick_countdown -= 1;
            for (x, offset) in frame.iter_mut().zip(self.declick_offset.iter_mut()) {
                *x += *offset;
                *offset *= self.declick_decay;
            }
        }

        frame
    }

    fn play_xfade(&mut self, mut frame: Frame) -> Frame {
        if self.pause_countdown > 0 {
            let progress = 1.0 - (self.pause_countdown as Sample * self.xfade_duration_recip);
            self.pause_countdown -= 1;
            for x in frame.iter_mut() {
                *x *= progress;
            }
        }

        frame
    }

    fn pause_xfade(&mut self, mut frame: Frame) -> Frame {
        let progress = self.pause_countdown as Sample * self.xfade_duration_recip;
        self.pause_countdown -= 1;
        for x in frame.iter_mut() {
            *x *= progress;
        }

        frame
    }
}

fn declick_decay(duration: usize) -> Sample {
    if duration > 0 {
        DECLICK_RESIDUAL.powf((duration as Sample).recip())
    } else {
        0.0
    }
}

fn migrate_program_state(active_program: &mut Program, previous_program: &mut Program) {
    if active_program.is_empty() || previous_program.is_empty() {
        return;
    }

    // Keep this allocation-free: load_program() runs on the realtime thread in
    // plugin/server callbacks. Index the common case in fixed stack storage, and
    // fall back to a direct scan only for programs beyond MIGRATION_INDEX_SIZE.
    // Store indices rather than references so migration can steal mutable state
    // from the previous program.
    let indexed_len = previous_program.len().min(MIGRATION_INDEX_SIZE);
    let mut previous_by_id: SmallVec<[(u64, usize); MIGRATION_INDEX_SIZE]> = previous_program
        .iter()
        .take(indexed_len)
        .enumerate()
        .map(|(index, stmt)| (stmt.id, index))
        .collect();
    previous_by_id.sort_unstable_by_key(|(id, _)| *id);

    for stmt in active_program {
        if let Ok(index) = previous_by_id.binary_search_by_key(&stmt.id, |(id, _)| *id) {
            let previous_index = previous_by_id[index].1;
            stmt.op
                .migrate(previous_program[previous_index].op.as_mut());
        } else if let Some(previous_stmt) = previous_program
            .iter_mut()
            .skip(indexed_len)
            .find(|previous_stmt| previous_stmt.id == stmt.id)
        {
            stmt.op.migrate(previous_stmt.op.as_mut());
        }
    }
}

#[inline]
fn perform(program: &mut Program, stack: &mut Stack) -> Frame {
    stack.reset();
    for stmt in program {
        stmt.op.perform(stack);
    }
    stack.peek()
}

#[inline]
fn perform_and_monitor(program: &mut Program, stack: &mut Stack, scope_id: u64) -> (Frame, Frame) {
    debug_assert_ne!(scope_id, 0);

    let mut scope = Default::default();
    stack.reset();
    for stmt in program {
        stmt.op.perform(stack);
        if scope_id == stmt.id {
            scope = stack.peek();
        }
    }

    (stack.peek(), scope)
}

enum Status {
    Pause,
    Play,
}

#[cfg(test)]
mod tests {
    use super::*;
    use smallvec::smallvec;

    struct PushFrame(Frame);

    impl Op for PushFrame {
        fn perform(&mut self, stack: &mut Stack) {
            stack.push(&self.0);
        }
    }

    struct AddTopTwo;

    impl Op for AddTopTwo {
        fn perform(&mut self, stack: &mut Stack) {
            let b = stack.pop();
            let a = stack.pop();
            stack.push(&[a[0] + b[0], a[1] + b[1]]);
        }
    }

    struct Counter {
        count: Sample,
    }

    impl Counter {
        fn new() -> Self {
            Self { count: 0.0 }
        }
    }

    impl Op for Counter {
        fn perform(&mut self, stack: &mut Stack) {
            self.count += 1.0;
            stack.push(&[self.count; 2]);
        }

        fn migrate(&mut self, other: &mut dyn Op) {
            if let Some(other) = other.downcast_mut::<Self>() {
                self.count = other.count;
            }
        }
    }

    fn statement(id: u64, op: impl Op + 'static) -> Statement {
        Statement {
            id,
            op: Box::new(op),
        }
    }

    #[test]
    fn paused_vm_outputs_silence_until_playing() {
        let mut vm = VM::new();
        vm.set_xfade_duration(0.0);
        vm.load_program(smallvec![statement(1, PushFrame([1.0, -1.0]))]);

        assert_eq!(vm.next_frame(), [0.0, 0.0]);

        vm.play();
        assert_eq!(vm.next_frame(), [1.0, -1.0]);
    }

    #[test]
    fn play_and_pause_fade_active_program() {
        let mut vm = VM::new();
        vm.set_xfade_duration(2.0);
        vm.load_program(smallvec![statement(1, PushFrame([10.0, 20.0]))]);

        vm.play();
        assert_eq!(vm.next_frame(), [0.0, 0.0]);
        assert_eq!(vm.next_frame(), [5.0, 10.0]);
        assert_eq!(vm.next_frame(), [10.0, 20.0]);

        vm.pause();
        assert_eq!(vm.next_frame(), [10.0, 20.0]);
        assert_eq!(vm.next_frame(), [5.0, 10.0]);
        assert_eq!(vm.next_frame(), [0.0, 0.0]);
    }

    #[test]
    fn monitor_tracks_selected_statement_or_final_output() {
        let mut vm = VM::new();
        vm.set_xfade_duration(0.0);
        vm.load_program(smallvec![
            statement(10, PushFrame([2.0, 3.0])),
            statement(20, PushFrame([5.0, 7.0])),
            statement(30, AddTopTwo),
        ]);
        vm.play();

        vm.set_monitor_id(10);
        assert_eq!(vm.next_frame(), [7.0, 10.0]);
        let monitor = vm.monitor();
        assert_eq!(
            [
                Sample::from_bits(monitor[0].load(Ordering::Relaxed)),
                Sample::from_bits(monitor[1].load(Ordering::Relaxed)),
            ],
            [2.0, 3.0]
        );

        vm.set_monitor_id(0);
        assert_eq!(vm.next_frame(), [7.0, 10.0]);
        let monitor = vm.monitor();
        assert_eq!(
            [
                Sample::from_bits(monitor[0].load(Ordering::Relaxed)),
                Sample::from_bits(monitor[1].load(Ordering::Relaxed)),
            ],
            [7.0, 10.0]
        );
    }

    #[test]
    fn load_program_migrates_matching_statement_state() {
        let mut vm = VM::new();
        vm.set_xfade_duration(0.0);
        vm.set_declick_duration(0.0);
        vm.load_program(smallvec![statement(42, Counter::new())]);
        vm.play();
        assert_eq!(vm.next_frame(), [1.0, 1.0]);

        vm.load_program(smallvec![statement(42, Counter::new())]);
        assert_eq!(vm.next_frame(), [2.0, 2.0]);
    }

    #[test]
    fn load_program_switches_to_new_program_immediately() {
        let mut vm = VM::new();
        vm.set_xfade_duration(2.0);
        vm.set_declick_duration(0.0);
        vm.load_program(smallvec![statement(1, PushFrame([0.0, 0.0]))]);
        vm.play();
        vm.next_frame();
        vm.next_frame();

        vm.load_program(smallvec![statement(1, PushFrame([10.0, 20.0]))]);

        assert_eq!(vm.next_frame(), [10.0, 20.0]);
    }

    #[test]
    fn load_program_declicks_step_discontinuity() {
        let mut vm = VM::new();
        vm.set_xfade_duration(0.0);
        vm.set_declick_duration(2.0);
        vm.load_program(smallvec![statement(1, PushFrame([1.0, -1.0]))]);
        vm.play();
        assert_eq!(vm.next_frame(), [1.0, -1.0]);

        vm.load_program(smallvec![statement(1, PushFrame([0.0, 0.0]))]);

        // First frame after reload is continuous with the last heard frame.
        assert_eq!(vm.next_frame(), [1.0, -1.0]);
        // Then the correction decays exponentially...
        let decay = DECLICK_RESIDUAL.powf(0.5);
        let frame = vm.next_frame();
        assert!((frame[0] - decay).abs() < 1e-12);
        assert!((frame[1] + decay).abs() < 1e-12);
        // ...and is dropped entirely after the declick duration.
        assert_eq!(vm.next_frame(), [0.0, 0.0]);
    }

    #[test]
    fn load_program_with_continuous_output_skips_declick() {
        let mut vm = VM::new();
        vm.set_xfade_duration(0.0);
        vm.load_program(smallvec![statement(1, PushFrame([0.5, 0.5]))]);
        vm.play();
        assert_eq!(vm.next_frame(), [0.5, 0.5]);

        vm.load_program(smallvec![statement(1, PushFrame([0.5, 0.5]))]);

        // No audible step: declick stays disarmed and output is bit-exact.
        assert_eq!(vm.next_frame(), [0.5, 0.5]);
        assert_eq!(vm.next_frame(), [0.5, 0.5]);
    }

    #[test]
    fn load_program_while_silent_does_not_arm_declick() {
        let mut vm = VM::new();
        vm.set_xfade_duration(0.0);
        vm.load_program(smallvec![statement(1, PushFrame([1.0, 1.0]))]);
        assert_eq!(vm.next_frame(), [0.0, 0.0]);

        // A silent VM cannot click, so the reload must not smear the first
        // audible frame after play().
        vm.play();
        assert_eq!(vm.next_frame(), [1.0, 1.0]);
    }
}
