use audio_program::{Context, TextOp, compile_program};
use audio_vm::{Stack, VM, set_pattern_monitor_ids};
use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use std::hint::black_box;

const SAMPLE_RATE: u32 = 48_000;
const BLOCK_FRAMES: usize = 128;

fn text_op(id: u64, op: impl Into<String>) -> TextOp {
    TextOp { id, op: op.into() }
}

fn poly_synth_ops(voices: usize) -> Vec<TextOp> {
    let mut ops = Vec::new();
    let mut id = 1;

    for voice in 0..voices {
        let frequency = 110.0 * (voice + 1) as f64;
        ops.push(text_op(id, frequency.to_string()));
        id += 1;
        ops.push(text_op(id, "s'"));
        id += 1;
        ops.push(text_op(id, (1.0 / voices as f64).to_string()));
        id += 1;
        ops.push(text_op(id, "*"));
        id += 1;

        if voice > 0 {
            ops.push(text_op(id, "+"));
            id += 1;
        }
    }

    ops
}

fn filtered_synth_ops(stages: usize) -> Vec<TextOp> {
    let mut ops = vec![text_op(1, "110"), text_op(2, "s'")];

    for stage in 0..stages {
        let id = 3 + (stage as u64 * 2);
        ops.push(text_op(id, (600 + stage * 200).to_string()));
        ops.push(text_op(id + 1, "lpf"));
    }

    ops
}

fn convolution_ops(window_size: usize) -> Vec<TextOp> {
    let mut ops = vec![text_op(1, "0.25")];

    for i in 0..window_size {
        ops.push(text_op(
            (i + 2) as u64,
            (1.0 / window_size as f64).to_string(),
        ));
    }

    ops.push(text_op(
        (window_size + 2) as u64,
        format!("convm:{window_size}"),
    ));
    ops
}

fn delay_ops() -> Vec<TextOp> {
    vec![
        text_op(1, "0.25"),
        text_op(2, "0.125"),
        text_op(3, "delay:1"),
    ]
}

fn biquad_lpf_ops() -> Vec<TextOp> {
    vec![
        text_op(1, "110"),
        text_op(2, "s'"),
        text_op(3, "1000"),
        text_op(4, "l"),
    ]
}

fn constant_arithmetic_ops(terms: usize) -> Vec<TextOp> {
    let mut ops = vec![text_op(1, "1")];

    for i in 0..terms {
        let id = 2 + (i as u64 * 2);
        ops.push(text_op(id, (i + 2).to_string()));
        ops.push(text_op(id + 1, "+"));
    }

    ops
}

fn pitch_detection_ops() -> Vec<TextOp> {
    vec![text_op(1, "110"), text_op(2, "s'"), text_op(3, "pitch")]
}

/// Canonical poly patch: pattern-driven pitch + trigger into 8 voices of
/// sine * exponential impulse.
fn poly_voices_ops() -> Vec<TextOp> {
    [
        "1",
        "cycle",
        "pat:60,64,67,72",
        "m2f",
        "1",
        "cycle",
        "trig:x.xx",
        "[",
        "swap",
        "s",
        "swap",
        "0.01",
        "impulse",
        "*",
        "]",
        "poly:8",
        "0.2",
        "*",
    ]
    .iter()
    .enumerate()
    .map(|(i, op)| text_op(i as u64 + 1, *op))
    .collect()
}

fn words(source: &str) -> Vec<TextOp> {
    source
        .split_whitespace()
        .enumerate()
        .map(|(i, op)| text_op(i as u64 + 1, op))
        .collect()
}

/// Non-constant binary/unary arithmetic, exercising the generic Fn ops.
fn fm_arithmetic_ops() -> Vec<TextOp> {
    words("110 s 220 s * 330 s + 440 s - 2 * tanh 550 s max")
}

/// Band-limited oscillators, exercising phase wrapping.
fn oscillator_bank_ops() -> Vec<TextOp> {
    words("110 w 220 t + 330 0.5 p + 440 0 saw + 550 0 tri + 0.2 *")
}

/// Long value/gate/trigger patterns, exercising per-sample cell lookup.
fn long_pattern_ops() -> Vec<TextOp> {
    let values = (0..32)
        .map(|i| (48 + i).to_string())
        .collect::<Vec<_>>()
        .join(",");
    words(&format!(
        "1 cycle pat:{values} 1 cycle gate:{gates} + 1 cycle trig:{gates} + 1 cpat:{values} +",
        gates = "x.".repeat(16),
    ))
}

/// Random choices, which compile to a variant per cycle of the random period.
fn random_choice_pattern_ops() -> Vec<TextOp> {
    words("1 cycle pat:60|64|67,[62|65]*4,<70;72|74> 1 cycle gate:x|.,x(3,8)|x(5,8),[x.]|[.x] +")
}

fn sliding_convolution_ops() -> Vec<TextOp> {
    words("110 s 220 s conv:256")
}

fn spectral_shuffle_ops() -> Vec<TextOp> {
    words("110 s 0.5 0 spectral_shuffle")
}

fn spectral_reverse_ops() -> Vec<TextOp> {
    words("110 s spectral_reverse")
}

/// A dense grain cloud: 50 grains/s of 0.3 s each keep ~15 of 32 slots busy.
fn grain_cloud_ops() -> Vec<TextOp> {
    words("110 0 saw 1 wt:src:2 pop noise 0.2 * 1 + 0.3 1 50 metro grain:src:32")
}

fn live_granulate_ops() -> Vec<TextOp> {
    words("110 0 saw 0.5 0.3 0.5 50 metro granulate:2:32")
}

fn compile_benchmarks(c: &mut Criterion) {
    let mut group = c.benchmark_group("compile_program");

    for voices in [1, 4, 16] {
        let ops = poly_synth_ops(voices);
        group.bench_function(format!("poly_synth_{voices}_voices"), |b| {
            b.iter_batched(
                || (ops.clone(), Context::new()),
                |(ops, mut ctx)| black_box(compile_program(black_box(&ops), SAMPLE_RATE, &mut ctx)),
                BatchSize::SmallInput,
            );
        });
    }

    for (name, ops) in [
        ("filtered_synth_8_lpf_stages", filtered_synth_ops(8)),
        ("convolution_m_64_taps", convolution_ops(64)),
        ("delay_1_second", delay_ops()),
        ("biquad_lpf", biquad_lpf_ops()),
        ("constant_arithmetic_64_terms", constant_arithmetic_ops(64)),
        ("pitch_detection_yin", pitch_detection_ops()),
        ("poly_8_voices", poly_voices_ops()),
        ("long_patterns_32_cells", long_pattern_ops()),
        ("random_choice_patterns", random_choice_pattern_ops()),
    ] {
        group.bench_function(name, |b| {
            b.iter_batched(
                || (ops.clone(), Context::new()),
                |(ops, mut ctx)| black_box(compile_program(black_box(&ops), SAMPLE_RATE, &mut ctx)),
                BatchSize::SmallInput,
            );
        });
    }

    group.finish();
}

fn vm_from_ops(ops: &[TextOp]) -> VM {
    let mut ctx = Context::new();
    let mut vm = VM::new();
    vm.set_xfade_duration(0.0);
    vm.load_program(compile_program(ops, SAMPLE_RATE, &mut ctx));
    vm.play();
    vm
}

fn render_block(vm: &mut VM, frames: usize) -> [f64; 2] {
    let mut sum = [0.0, 0.0];
    for _ in 0..frames {
        let frame = vm.next_frame();
        sum[0] += frame[0];
        sum[1] += frame[1];
    }
    sum
}

fn audio_frame_benchmarks(c: &mut Criterion) {
    let mut group = c.benchmark_group("vm_next_frame");

    for voices in [1, 4, 16] {
        let ops = poly_synth_ops(voices);
        group.bench_function(format!("poly_synth_{voices}_voices"), |b| {
            let mut vm = vm_from_ops(&ops);
            b.iter(|| black_box(vm.next_frame()));
        });
    }

    for (name, ops) in [
        ("filtered_synth_8_lpf_stages", filtered_synth_ops(8)),
        ("convolution_m_64_taps", convolution_ops(64)),
        ("delay_1_second", delay_ops()),
        ("biquad_lpf", biquad_lpf_ops()),
        ("constant_arithmetic_64_terms", constant_arithmetic_ops(64)),
        ("pitch_detection_yin", pitch_detection_ops()),
        ("poly_8_voices", poly_voices_ops()),
        ("fm_arithmetic", fm_arithmetic_ops()),
        ("oscillator_bank", oscillator_bank_ops()),
        ("long_patterns_32_cells", long_pattern_ops()),
        ("sliding_convolution_256", sliding_convolution_ops()),
        ("spectral_shuffle", spectral_shuffle_ops()),
        ("spectral_reverse", spectral_reverse_ops()),
        ("grain_cloud_32", grain_cloud_ops()),
        ("live_granulate_32", live_granulate_ops()),
    ] {
        group.bench_function(name, |b| {
            let mut vm = vm_from_ops(&ops);
            b.iter(|| black_box(vm.next_frame()));
        });
    }

    group.finish();
}

fn stack_benchmarks(c: &mut Criterion) {
    let mut group = c.benchmark_group("stack");

    group.bench_function("push_peek_pop_one_frame", |b| {
        let mut stack = Stack::new();
        let frame = [1.0, -1.0];
        b.iter(|| {
            stack.reset();
            stack.push(black_box(&frame));
            black_box(stack.peek());
            black_box(stack.pop());
        });
    });

    group.bench_function("push_pop_16_frames", |b| {
        let mut stack = Stack::new();
        b.iter(|| {
            stack.reset();
            for i in 0..16 {
                let x = i as f64;
                stack.push(black_box(&[x, -x]));
            }
            for _ in 0..16 {
                black_box(stack.pop());
            }
        });
    });

    group.finish();
}

fn lifecycle_benchmarks(c: &mut Criterion) {
    let mut group = c.benchmark_group("program_lifecycle");

    let old_ops = poly_synth_ops(16);
    let same_ids_ops = poly_synth_ops(16);
    let mut different_ids_ops = poly_synth_ops(16);
    for op in &mut different_ids_ops {
        op.id += 10_000;
    }

    group.bench_function("load_program_migrate_matching_ids", |b| {
        b.iter_batched(
            || {
                let mut ctx = Context::new();
                let mut vm = VM::new();
                vm.set_xfade_duration(0.0);
                vm.load_program(compile_program(&old_ops, SAMPLE_RATE, &mut ctx));
                let program = compile_program(&same_ids_ops, SAMPLE_RATE, &mut ctx);
                (vm, program)
            },
            |(mut vm, program)| black_box(vm.load_program(program)),
            BatchSize::SmallInput,
        );
    });

    group.bench_function("load_program_no_matching_ids", |b| {
        b.iter_batched(
            || {
                let mut ctx = Context::new();
                let mut vm = VM::new();
                vm.set_xfade_duration(0.0);
                vm.load_program(compile_program(&old_ops, SAMPLE_RATE, &mut ctx));
                let program = compile_program(&different_ids_ops, SAMPLE_RATE, &mut ctx);
                (vm, program)
            },
            |(mut vm, program)| black_box(vm.load_program(program)),
            BatchSize::SmallInput,
        );
    });

    group.finish();
}

fn monitor_and_reload_benchmarks(c: &mut Criterion) {
    let mut group = c.benchmark_group("vm_state_paths");
    let ops = poly_synth_ops(16);

    group.bench_function("monitor_final_output_id_0", |b| {
        let mut vm = vm_from_ops(&ops);
        vm.set_monitor_id(0);
        b.iter(|| black_box(vm.next_frame()));
    });

    group.bench_function("monitor_selected_statement", |b| {
        let mut vm = vm_from_ops(&ops);
        vm.set_monitor_id(2);
        b.iter(|| black_box(vm.next_frame()));
    });

    group.bench_function("monitor_4_patterns", |b| {
        let ops = long_pattern_ops();
        let mut vm = vm_from_ops(&ops);
        let ids = ops
            .iter()
            .filter(|op| op.op.contains(':'))
            .map(|op| op.id)
            .collect::<Vec<_>>();
        assert_eq!(ids.len(), 4);
        set_pattern_monitor_ids(&vm.pattern_monitor(), &ids);
        b.iter(|| black_box(vm.next_frame()));
    });

    group.bench_function("active_program_reload", |b| {
        b.iter_batched(
            || {
                let mut ctx = Context::new();
                let mut vm = VM::new();
                vm.set_xfade_duration(4_096.0);
                vm.load_program(compile_program(&poly_synth_ops(4), SAMPLE_RATE, &mut ctx));
                vm.play();
                vm.load_program(compile_program(&ops, SAMPLE_RATE, &mut ctx));
                vm
            },
            |mut vm| black_box(vm.next_frame()),
            BatchSize::SmallInput,
        );
    });

    group.bench_function("pause", |b| {
        b.iter_batched(
            || {
                let mut vm = vm_from_ops(&ops);
                vm.set_xfade_duration(4_096.0);
                vm.pause();
                vm
            },
            |mut vm| black_box(vm.next_frame()),
            BatchSize::SmallInput,
        );
    });

    group.finish();
}

fn block_render_benchmarks(c: &mut Criterion) {
    let mut group = c.benchmark_group("vm_render_block");

    for (name, ops) in [
        ("poly_synth_16_voices", poly_synth_ops(16)),
        ("filtered_synth_8_lpf_stages", filtered_synth_ops(8)),
        ("convolution_m_64_taps", convolution_ops(64)),
        ("pitch_detection_yin", pitch_detection_ops()),
        ("poly_8_voices", poly_voices_ops()),
    ] {
        group.bench_function(format!("{name}_{BLOCK_FRAMES}_frames"), |b| {
            let mut vm = vm_from_ops(&ops);
            b.iter(|| black_box(render_block(&mut vm, BLOCK_FRAMES)));
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    compile_benchmarks,
    audio_frame_benchmarks,
    stack_benchmarks,
    lifecycle_benchmarks,
    monitor_and_reload_benchmarks,
    block_render_benchmarks,
);
criterion_main!(benches);
