use alloc_counter::{AllocCounterSystem, count_alloc};
use audio_program::{Context, TextOp, compile_program};
use audio_vm::VM;
use std::time::Instant;

#[global_allocator]
static A: AllocCounterSystem = AllocCounterSystem;

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
    eprintln!("simple reload: allocations={:?}, elapsed={:?}", simple.0, simple.1);

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
