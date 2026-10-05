use audio_program::{Context, TextOp, compile_program};
use audio_vm::VM;
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::time::Instant;

/// Counts this thread's (allocations, reallocations, deallocations), so tests
/// running in parallel don't see each other's.
struct CountingAllocator;

thread_local! {
    static COUNTS: Cell<(usize, usize, usize)> = const { Cell::new((0, 0, 0)) };
}

fn bump(f: impl FnOnce(&mut (usize, usize, usize))) {
    // try_with: the allocator also runs while thread-locals are being torn down.
    let _ = COUNTS.try_with(|counts| {
        let mut c = counts.get();
        f(&mut c);
        counts.set(c);
    });
}

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        bump(|c| c.0 += 1);
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        bump(|c| c.0 += 1);
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        bump(|c| c.1 += 1);
        unsafe { System.realloc(ptr, layout, new_size) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        bump(|c| c.2 += 1);
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static A: CountingAllocator = CountingAllocator;

/// Runs `f`, returning what it allocated, reallocated and deallocated on this thread.
fn count_alloc<T>(f: impl FnOnce() -> T) -> ((usize, usize, usize), T) {
    let before = COUNTS.with(Cell::get);
    let value = f();
    let after = COUNTS.with(Cell::get);
    let counts = (after.0 - before.0, after.1 - before.1, after.2 - before.2);
    (counts, value)
}

const SAMPLE_RATE: u32 = 48_000;

fn ops(source: &str) -> Vec<TextOp> {
    source
        .split_whitespace()
        .enumerate()
        .map(|(index, op)| TextOp {
            id: (index + 1) as u64,
            op: op.to_owned(),
        })
        .collect()
}

fn time_load(old_source: &str, new_source: &str) -> ((usize, usize, usize), std::time::Duration) {
    let mut ctx = Context::new();
    let mut vm = VM::new();
    vm.set_xfade_duration(0.0);
    vm.load_program(compile_program(&ops(old_source), SAMPLE_RATE, &mut ctx));
    vm.play();
    for _ in 0..1024 {
        let _ = vm.next_frame();
    }

    // Compile is intentionally outside the measured section: audio_server does this off the
    // callback thread before sending Command::LoadProgram to audio.rs.
    let program = compile_program(&ops(new_source), SAMPLE_RATE, &mut ctx);

    let start = Instant::now();
    let (counts, garbage) = count_alloc(|| vm.load_program(program));
    let elapsed = start.elapsed();

    // Match the audio callback's no-drop-on-hot-path policy as closely as possible for this
    // allocation counter. audio_server normally pushes this to garbage_tx; if full, it forgets it.
    std::mem::forget(garbage);

    (counts, elapsed)
}

#[test]
fn allocation_counter_sees_allocations() {
    let (counts, v) = count_alloc(|| {
        let mut v = std::hint::black_box(Vec::<u8>::with_capacity(1));
        v.extend_from_slice(&[0; 64]);
        v
    });
    drop(v);
    assert_eq!(counts, (1, 1, 0));
}

#[test]
fn simple_oscillator_reload_does_not_allocate_on_load_program() {
    let (counts, _elapsed) = time_load("110 s", "220 s");
    assert_eq!(counts, (0, 0, 0));
}

#[test]
fn pattern_feedback_reload_does_not_allocate_on_load_program() {
    let old = "1 cy >ph .0625 .5 p <ph pat:110,220 s * dup .5 t 0.0625 5 range 0.5 fb swap .125 t 0.0625 5 range 0.5 fb + .1 * 110 s";
    let new = "1 cy >ph .0625 .5 p <ph pat:110,220 s * dup .5 t 0.0625 5 range 0.5 fb swap .125 t 0.0625 5 range 0.5 fb + .1 * 220 s";
    let (counts, _elapsed) = time_load(old, new);
    assert_eq!(counts, (0, 0, 0));
}

#[test]
#[ignore = "diagnostic timing test; run with --ignored --nocapture"]
fn print_load_program_timing_simple_vs_pattern_feedback() {
    let simple = time_load("110 s", "220 s");
    eprintln!(
        "simple reload: allocations={:?}, elapsed={:?}",
        simple.0, simple.1
    );

    let old = "1 cy >ph .0625 .5 p <ph pat:110,220 s * dup .5 t 0.0625 5 range 0.5 fb swap .125 t 0.0625 5 range 0.5 fb + .1 * 110 s";
    let new = "1 cy >ph .0625 .5 p <ph pat:110,220 s * dup .5 t 0.0625 5 range 0.5 fb swap .125 t 0.0625 5 range 0.5 fb + .1 * 220 s";
    let complex = time_load(old, new);
    eprintln!(
        "pattern+feedback reload: allocations={:?}, elapsed={:?}",
        complex.0, complex.1
    );

    let old_bounded = old.replace(" fb", " fb:5");
    let new_bounded = new.replace(" fb", " fb:5");
    let bounded = time_load(&old_bounded, &new_bounded);
    eprintln!(
        "pattern+feedback(fb:5) reload: allocations={:?}, elapsed={:?}",
        bounded.0, bounded.1
    );
}

/// Programs covering ops with internal buffers, scratch space or sub-programs.
/// Every op must render and reload without touching the allocator, because both
/// happen on the audio thread.
const REALTIME_PROGRAMS: &[&str] = &[
    "110 s 220 s * 330 s + tanh",
    "110 w 220 t + 330 0.5 p + 440 0 saw + -110 w +",
    "1 cycle pat:60,[64,67],<72;74>,60|67 1 cycle gate:x(3,8) + 1 cycle trig:x.x[xx] + 2 cpat:1,2,3 +",
    "110 s 220 s conv:64",
    "110 s 110 s 0.5 0.25 convm:3",
    "110 s norm:64",
    "110 s pitch",
    "0.25 0.125 delay:1 110 s 0.1 0.5 fb +",
    "110 s 0.5 0.5 rev",
    "110 s spectral_reverse",
    "110 s st1",
    "110 s 0.7 4 cycle trig:x. spectral_shuffle",
    "110 s 0.7 4 cycle trig:x. spectral_shuffle:8",
    "110 s 1 cycle gate:xxx. spectral_freeze",
    "110 s 1 wt:loop:0.1 0 w rt:loop +",
    "1 cycle pat:60,64,67,72 1 cycle trig:x.xx [ swap m2f s swap 0.01 impulse * ] poly:4",
    "[ swap m2f s swap 0.01 0.1 0.7 0.3 adsr * ] mpoly:4",
    "110 s 1 wt:loop:1 pop noise 0.05 * 0.2 + 0.08 1 200 metro grain:loop:4",
    "110 s 0.3 0.05 -1.5 200 metro granulate:1:4",
    "cc:74:0.5 200 4000 uniexp 110 0 saw swap 0.7 l 60 bend 2 * + m2f s cc':1 * +",
];

#[test]
fn realtime_ops_do_not_allocate_while_rendering_or_reloading() {
    for source in REALTIME_PROGRAMS {
        let mut ctx = Context::new();
        let mut vm = VM::new();
        vm.set_xfade_duration(0.0);
        vm.load_program(compile_program(&ops(source), SAMPLE_RATE, &mut ctx));
        vm.play();

        // Long enough for spectral ops to pass several hops and a full window
        // (spectral_freeze only captures after 2048 frames).
        let (counts, _) = count_alloc(|| {
            for _ in 0..8192 {
                std::hint::black_box(vm.next_frame());
            }
        });
        assert_eq!(counts, (0, 0, 0), "rendering allocated: {source}");

        let program = compile_program(&ops(source), SAMPLE_RATE, &mut ctx);
        let (counts, garbage) = count_alloc(|| vm.load_program(program));
        std::mem::forget(garbage);
        assert_eq!(counts, (0, 0, 0), "reloading allocated: {source}");

        let (counts, _) = count_alloc(|| {
            for _ in 0..4096 {
                std::hint::black_box(vm.next_frame());
            }
        });
        assert_eq!(
            counts,
            (0, 0, 0),
            "rendering after reload allocated: {source}"
        );
    }
}
