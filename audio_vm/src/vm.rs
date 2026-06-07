use crate::op::Op;
use crate::sample::{AtomicFrame, Frame, Sample};
use crate::stack::Stack;
#[cfg(feature = "allocation-checks")]
use alloc_counter::no_alloc;
use smallvec::SmallVec;
use std::sync::{Arc, atomic::Ordering};

// Totally unscientific attempt to improve performance of small programs by using SmallVec.
/// FAST_PROGRAM_SIZE determines how large we expect program to be before it would incur extra indirection.
pub const FAST_PROGRAM_SIZE: usize = 64;
const MIGRATION_INDEX_SIZE: usize = 128;

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
        Self {
            active_program: Default::default(),
            active_stack: Stack::new(),
            status: Status::Pause,
            monitor: Default::default(),
            monitor_id: 0,
        }
    }

    pub fn toggle_play(&mut self) {
        self.status = match self.status {
            Status::Play => Status::Pause,
            Status::Pause => Status::Play,
        };
    }

    pub fn play(&mut self) {
        self.status = Status::Play;
    }

    pub fn pause(&mut self) {
        self.status = Status::Pause;
    }

    pub fn stop(&mut self) {
        self.status = Status::Pause;
    }

    /// Kept for API compatibility. Program/play/pause crossfades are disabled.
    pub fn set_xfade_duration(&mut self, _frames: Sample) {}

    /// Load the new program and steal/migrate state from the previous active program.
    /// Returns the old program so it can be deallocated somewhere else.
    pub fn load_program(&mut self, program: Program) -> Program {
        let mut garbage = std::mem::replace(&mut self.active_program, program);
        migrate_program_state(&mut self.active_program, &mut garbage);
        garbage
    }

    #[cfg_attr(feature = "allocation-checks", no_alloc)]
    pub fn next_frame(&mut self) -> Frame {
        match self.status {
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

                frame
            }
            Status::Pause => Default::default(),
        }
    }

    pub fn monitor(&self) -> Arc<AtomicFrame> {
        Arc::clone(&self.monitor)
    }

    pub fn set_monitor_id(&mut self, id: u64) {
        self.monitor_id = id;
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
            stmt.op.migrate(previous_program[previous_index].op.as_mut());
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
        vm.load_program(smallvec![statement(1, PushFrame([0.0, 0.0]))]);
        vm.play();
        vm.next_frame();
        vm.next_frame();

        vm.load_program(smallvec![statement(1, PushFrame([10.0, 20.0]))]);

        assert_eq!(vm.next_frame(), [10.0, 20.0]);
    }
}
