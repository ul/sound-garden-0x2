use audio_program::{Context, TextOp, compile_program};
use audio_vm::{Stack, VM};
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

fn monitor_and_crossfade_benchmarks(c: &mut Criterion) {
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

    group.bench_function("active_program_crossfade", |b| {
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

    group.bench_function("pause_fade", |b| {
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
    monitor_and_crossfade_benchmarks,
    block_render_benchmarks,
);
criterion_main!(benches);
