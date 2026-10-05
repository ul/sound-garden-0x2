use crate::op::Op;
use crate::sample::{AtomicFrame, CHANNELS, Frame, Sample};
use crate::stack::Stack;
use smallvec::SmallVec;
use std::sync::{Arc, Mutex, atomic::Ordering};

// Benchmarks (benches/microstructure.rs) showed SmallVec inline storage for Program
// buys nothing on the hot path (op state is boxed anyway) while bloating every
// Program value and move; a plain Vec is used instead. SmallVec remains only for
// the allocation-free migration index below.
const MIGRATION_INDEX_SIZE: usize = 128;
/// Monitors are published once per this many frames. Readers poll every
/// ~10 ms (~480 frames), so publishing every frame is wasted work.
const MONITOR_INTERVAL: usize = 64;
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

pub type Program = Vec<Statement>;

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
    /// Statement outputs used for GUI pattern highlighting.
    pattern_monitor: Arc<Mutex<Vec<(u64, Frame)>>>,
    /// Frames until monitors are published next; 0 publishes on the next frame.
    monitor_countdown: usize,
    /// This frame's output of the monitored statement (or the program output
    /// for id 0), updated every frame for per-sample capture.
    scope: Frame,
    /// Last three output frames: the last two predict the frame a reload replaces, and all
    /// three show how much the signal moves by itself.
    last_frame: Frame,
    previous_frame: Frame,
    earlier_frame: Frame,
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
        let pattern_monitor: Arc<Mutex<Vec<(u64, Frame)>>> = Default::default();
        // Some platforms (e.g. pthread-backed std Mutex on macOS) allocate the OS
        // lock lazily on first use. Lock once here so that allocation happens on
        // the constructing thread, not on the audio thread's first try_lock.
        drop(pattern_monitor.lock());
        Self {
            active_program: Default::default(),
            active_stack: Stack::new(),
            xfade_duration: 8192,
            xfade_duration_recip: 1.0 / 8192.0,
            pause_countdown: 0,
            status: Status::Pause,
            monitor: Default::default(),
            monitor_id: 0,
            pattern_monitor,
            monitor_countdown: 0,
            scope: Default::default(),
            last_frame: Default::default(),
            previous_frame: Default::default(),
            earlier_frame: Default::default(),
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

    /// Fade in. Playing while already playing changes nothing (hosts may call it on every
    /// program commit), and reversing a fade-out midway fades back in from the level reached.
    pub fn play(&mut self) {
        if !matches!(self.status, Status::Play) {
            self.pause_countdown = self.reversed_fade();
            self.status = Status::Play;
        }
    }

    /// Fade out; like `play`, idempotent and continuous when reversing a fade midway.
    pub fn pause(&mut self) {
        if !matches!(self.status, Status::Pause) {
            self.pause_countdown = self.reversed_fade();
            self.status = Status::Pause;
        }
    }

    /// The countdown that continues the current fade level in the other direction: a fade in
    /// has gain `1 - countdown / duration`, a fade out `countdown / duration`.
    fn reversed_fade(&self) -> usize {
        self.xfade_duration - self.pause_countdown.min(self.xfade_duration)
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

    pub fn next_frame(&mut self) -> Frame {
        let frame = match self.status {
            Status::Play if self.monitor_countdown > 0 => {
                self.monitor_countdown -= 1;
                let (frame, scope) = perform_scoped(
                    &mut self.active_program,
                    &mut self.active_stack,
                    self.monitor_id,
                );
                self.scope = scope;
                let frame = self.declick(frame);
                self.play_xfade(frame)
            }
            Status::Play => {
                self.monitor_countdown = MONITOR_INTERVAL - 1;
                let mut pattern_monitor = self.pattern_monitor.try_lock().ok();
                let (frame, monitor_frame) = perform_and_monitor(
                    &mut self.active_program,
                    &mut self.active_stack,
                    self.monitor_id,
                    pattern_monitor
                        .as_mut()
                        .map(|monitor| monitor.as_mut_slice()),
                );

                for (a, &x) in self.monitor.iter().zip(&monitor_frame) {
                    a.store(x.to_bits(), Ordering::Relaxed);
                }
                drop(pattern_monitor);
                self.scope = monitor_frame;

                let frame = self.declick(frame);
                self.play_xfade(frame)
            }
            Status::Pause => {
                if self.pause_countdown > 0 {
                    let (frame, scope) = perform_scoped(
                        &mut self.active_program,
                        &mut self.active_stack,
                        self.monitor_id,
                    );
                    self.scope = scope;
                    let frame = self.declick(frame);
                    self.pause_xfade(frame)
                } else {
                    // Fully silent: nothing to declick against.
                    self.scope = Default::default();
                    self.declick_pending = false;
                    self.declick_countdown = 0;
                    Default::default()
                }
            }
        };
        self.earlier_frame = self.previous_frame;
        self.previous_frame = self.last_frame;
        self.last_frame = frame;
        frame
    }

    pub fn monitor(&self) -> Arc<AtomicFrame> {
        Arc::clone(&self.monitor)
    }

    pub fn pattern_monitor(&self) -> Arc<Mutex<Vec<(u64, Frame)>>> {
        Arc::clone(&self.pattern_monitor)
    }

    /// This frame's output of the monitored statement (the program output
    /// when monitoring id 0), for per-sample capture by the host.
    pub fn scope(&self) -> Frame {
        self.scope
    }

    pub fn set_monitor_id(&mut self, id: u64) {
        self.monitor_id = id;
        // Publish the newly selected statement on the next frame.
        self.monitor_countdown = 0;
    }

    /// Cancel the step discontinuity introduced by a program reload: on the first
    /// frame after the reload, measure the step against where the signal was heading
    /// (a linear prediction from the last two heard frames), then add it back to the
    /// output while it decays exponentially to silence.
    ///
    /// A step no larger than twice the signal's own motion (the larger of its last two
    /// first differences, or its second difference, from the last three frames) is left alone: a
    /// moving signal differs from its last frame by itself, and "correcting" that
    /// injected a click into every reload, even of an unchanged program. Edits that
    /// change a number glide and oscillator swaps morph (see `audio_ops::glide` and
    /// `audio_ops::waveform`), so their steps stay within the motion and exact. Motion is
    /// judged only at reload time, so playing costs nothing but remembering a frame; the
    /// price is that a reload landing exactly on a saw or pulse wrap softens that edge.
    // Five per-channel arrays are read by channel; indexing reads clearer than zipping them.
    #[allow(clippy::needless_range_loop)]
    fn declick(&mut self, mut frame: Frame) -> Frame {
        if self.declick_pending {
            self.declick_pending = false;
            let mut step: Sample = 0.0;
            for c in 0..CHANNELS {
                let (last, previous, earlier) = (
                    self.last_frame[c],
                    self.previous_frame[c],
                    self.earlier_frame[c],
                );
                // First differences and the second difference (discrete second derivative).
                let motion = (last - previous)
                    .abs()
                    .max((previous - earlier).abs())
                    .max((last - 2.0 * previous + earlier).abs());
                let error = (2.0 * last - previous) - frame[c];
                let offset = &mut self.declick_offset[c];
                *offset = if error.abs() > 2.0 * motion {
                    error
                } else {
                    0.0
                };
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

/// Replace the statement ids whose outputs are published to `monitor` (see
/// `VM::pattern_monitor`). This takes the shared monitor rather than the VM
/// because it blocks on the lock and allocates, so it must be called off the
/// audio thread; `next_frame` only ever `try_lock`s and skips a frame if busy.
pub fn set_pattern_monitor_ids(monitor: &Mutex<Vec<(u64, Frame)>>, ids: &[u64]) {
    let entries = ids.iter().map(|&id| (id, Frame::default())).collect();
    if let Ok(mut monitor) = monitor.lock() {
        // Swap under the lock and drop the old Vec after releasing it.
        let _old = std::mem::replace(&mut *monitor, entries);
    }
}

fn declick_decay(duration: usize) -> Sample {
    if duration > 0 {
        DECLICK_RESIDUAL.powf((duration as Sample).recip())
    } else {
        0.0
    }
}

/// Migrate state between statement lists pairing by statement id.
/// Public so container ops (e.g. Poly) can reuse it for their sub-programs.
/// Allocation-free: safe to call on the audio thread during `Op::migrate`.
pub fn migrate_program_state(active_program: &mut [Statement], previous_program: &mut [Statement]) {
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

/// Run the program and also return the monitored statement's output: the
/// program output for id 0, silence if no statement has the id. Costs one id
/// comparison per statement.
#[inline]
fn perform_scoped(program: &mut Program, stack: &mut Stack, scope_id: u64) -> (Frame, Frame) {
    stack.reset();
    let mut scope = Frame::default();
    for stmt in program {
        stmt.op.perform(stack);
        if stmt.id == scope_id {
            scope = stack.peek();
        }
    }
    let frame = stack.peek();
    (frame, if scope_id == 0 { frame } else { scope })
}

#[inline]
fn perform_and_monitor(
    program: &mut Program,
    stack: &mut Stack,
    scope_id: u64,
    mut pattern_monitor: Option<&mut [(u64, Frame)]>,
) -> (Frame, Frame) {
    let mut scope = Default::default();
    stack.reset();
    for stmt in program {
        stmt.op.perform(stack);
        let frame = stack.peek();
        if scope_id == stmt.id {
            scope = frame;
        }
        if let Some(pattern_monitor) = &mut pattern_monitor
            && let Some((_, pattern_frame)) =
                pattern_monitor.iter_mut().find(|(id, _)| *id == stmt.id)
        {
            *pattern_frame = frame;
        }
    }

    let frame = stack.peek();
    if scope_id == 0 {
        scope = frame;
    }
    (frame, scope)
}

enum Status {
    Pause,
    Play,
}

#[cfg(test)]
mod tests {
    use super::*;

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
        vm.load_program(vec![statement(1, PushFrame([1.0, -1.0]))]);

        assert_eq!(vm.next_frame(), [0.0, 0.0]);

        vm.play();
        assert_eq!(vm.next_frame(), [1.0, -1.0]);
    }

    #[test]
    fn play_and_pause_fade_active_program() {
        let mut vm = VM::new();
        vm.set_xfade_duration(2.0);
        vm.load_program(vec![statement(1, PushFrame([10.0, 20.0]))]);

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
        vm.load_program(vec![
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
    fn monitor_is_published_once_per_interval() {
        let mut vm = VM::new();
        vm.set_xfade_duration(0.0);
        vm.load_program(vec![statement(1, Counter::new())]);
        vm.play();
        let monitor = vm.monitor();
        let published = || Sample::from_bits(monitor[0].load(Ordering::Relaxed));

        vm.next_frame();
        assert_eq!(published(), 1.0);
        for _ in 1..MONITOR_INTERVAL {
            vm.next_frame();
        }
        assert_eq!(published(), 1.0);
        vm.next_frame();
        assert_eq!(published(), (MONITOR_INTERVAL + 1) as Sample);
    }

    #[test]
    fn scope_follows_the_monitored_statement_every_frame() {
        let mut vm = VM::new();
        vm.set_xfade_duration(0.0);
        vm.load_program(vec![
            statement(10, Counter::new()),
            statement(20, PushFrame([0.5, 0.5])),
            statement(30, AddTopTwo),
        ]);
        vm.play();
        vm.set_monitor_id(10);
        // Unlike the published monitor, the scope updates on every frame,
        // including the ones between monitor publications.
        for n in 1..=(MONITOR_INTERVAL + 3) {
            let frame = vm.next_frame();
            assert_eq!(vm.scope(), [n as Sample; 2]);
            assert_eq!(frame, [n as Sample + 0.5; 2]);
        }
        vm.set_monitor_id(0);
        let frame = vm.next_frame();
        assert_eq!(vm.scope(), frame, "id 0 is the program output");
        vm.set_monitor_id(999);
        vm.next_frame();
        vm.next_frame();
        assert_eq!(vm.scope(), [0.0; 2], "unknown id is silence");
    }

    #[test]
    fn load_program_migrates_matching_statement_state() {
        let mut vm = VM::new();
        vm.set_xfade_duration(0.0);
        vm.set_declick_duration(0.0);
        vm.load_program(vec![statement(42, Counter::new())]);
        vm.play();
        assert_eq!(vm.next_frame(), [1.0, 1.0]);

        vm.load_program(vec![statement(42, Counter::new())]);
        assert_eq!(vm.next_frame(), [2.0, 2.0]);
    }

    #[test]
    fn play_while_playing_does_not_restart_the_fade_in() {
        // Hosts call play() on every commit; it must not dip to silence and fade back in.
        let mut vm = VM::new();
        vm.set_xfade_duration(4.0);
        vm.load_program(vec![statement(1, PushFrame([1.0, 1.0]))]);
        vm.play();
        for _ in 0..10 {
            vm.next_frame();
        }
        vm.play();
        assert_eq!(vm.next_frame(), [1.0, 1.0]);
    }

    #[test]
    fn reversing_a_fade_midway_continues_from_its_level() {
        let mut vm = VM::new();
        vm.set_xfade_duration(4.0);
        vm.load_program(vec![statement(1, PushFrame([1.0, 1.0]))]);
        vm.play();
        for _ in 0..10 {
            vm.next_frame();
        }
        vm.pause();
        assert_eq!(vm.next_frame(), [1.0, 1.0]);
        assert_eq!(vm.next_frame(), [0.75, 0.75]);
        vm.play();
        // The fade out reached 0.5; the fade in continues from there, not from silence.
        assert_eq!(vm.next_frame(), [0.5, 0.5]);
        assert_eq!(vm.next_frame(), [0.75, 0.75]);
    }

    #[test]
    fn load_program_switches_to_new_program_immediately() {
        let mut vm = VM::new();
        vm.set_xfade_duration(2.0);
        vm.set_declick_duration(0.0);
        vm.load_program(vec![statement(1, PushFrame([0.0, 0.0]))]);
        vm.play();
        vm.next_frame();
        vm.next_frame();

        vm.load_program(vec![statement(1, PushFrame([10.0, 20.0]))]);

        assert_eq!(vm.next_frame(), [10.0, 20.0]);
    }

    #[test]
    fn load_program_declicks_step_discontinuity() {
        let mut vm = VM::new();
        vm.set_xfade_duration(0.0);
        vm.set_declick_duration(2.0);
        vm.load_program(vec![statement(1, PushFrame([1.0, -1.0]))]);
        vm.play();
        // Hold steady for a few frames so the onset is out of the three-frame history.
        for _ in 0..3 {
            assert_eq!(vm.next_frame(), [1.0, -1.0]);
        }

        vm.load_program(vec![statement(1, PushFrame([0.0, 0.0]))]);

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
    fn reload_of_a_moving_signal_is_left_exact() {
        // A ramp moves by itself; a reload that continues it must not be "corrected" towards
        // the last heard frame (which once injected a click into every reload).
        struct Ramp(Sample);
        impl Op for Ramp {
            fn perform(&mut self, stack: &mut Stack) {
                self.0 += 0.01;
                stack.push(&[self.0; crate::sample::CHANNELS]);
            }
            fn migrate(&mut self, other: &mut dyn Op) {
                if let Some(other) = other.downcast_mut::<Self>() {
                    self.0 = other.0;
                }
            }
        }
        let mut vm = VM::new();
        vm.set_xfade_duration(0.0);
        vm.load_program(vec![statement(1, Ramp(0.0))]);
        vm.play();
        for _ in 0..100 {
            vm.next_frame();
        }
        let before = vm.next_frame()[0];
        vm.load_program(vec![statement(1, Ramp(0.0))]);
        assert!((vm.next_frame()[0] - (before + 0.01)).abs() < 1e-9);
    }

    #[test]
    fn load_program_with_continuous_output_skips_declick() {
        let mut vm = VM::new();
        vm.set_xfade_duration(0.0);
        vm.load_program(vec![statement(1, PushFrame([0.5, 0.5]))]);
        vm.play();
        assert_eq!(vm.next_frame(), [0.5, 0.5]);

        vm.load_program(vec![statement(1, PushFrame([0.5, 0.5]))]);

        // No audible step: declick stays disarmed and output is bit-exact.
        assert_eq!(vm.next_frame(), [0.5, 0.5]);
        assert_eq!(vm.next_frame(), [0.5, 0.5]);
    }

    #[test]
    fn load_program_while_silent_does_not_arm_declick() {
        let mut vm = VM::new();
        vm.set_xfade_duration(0.0);
        vm.load_program(vec![statement(1, PushFrame([1.0, 1.0]))]);
        assert_eq!(vm.next_frame(), [0.0, 0.0]);

        // A silent VM cannot click, so the reload must not smear the first
        // audible frame after play().
        vm.play();
        assert_eq!(vm.next_frame(), [1.0, 1.0]);
    }
}
