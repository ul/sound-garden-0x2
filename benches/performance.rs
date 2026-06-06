use audio_program::{Context, TextOp, compile_program};
use audio_vm::VM;
use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use std::hint::black_box;

const SAMPLE_RATE: u32 = 48_000;

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
        ops.push(text_op((i + 2) as u64, (1.0 / window_size as f64).to_string()));
    }

    ops.push(text_op((window_size + 2) as u64, format!("convm:{window_size}")));
    ops
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

    let ops = filtered_synth_ops(8);
    group.bench_function("filtered_synth_8_lpf_stages", |b| {
        b.iter_batched(
            || (ops.clone(), Context::new()),
            |(ops, mut ctx)| black_box(compile_program(black_box(&ops), SAMPLE_RATE, &mut ctx)),
            BatchSize::SmallInput,
        );
    });

    let ops = convolution_ops(64);
    group.bench_function("convolution_m_64_taps", |b| {
        b.iter_batched(
            || (ops.clone(), Context::new()),
            |(ops, mut ctx)| black_box(compile_program(black_box(&ops), SAMPLE_RATE, &mut ctx)),
            BatchSize::SmallInput,
        );
    });

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

fn audio_frame_benchmarks(c: &mut Criterion) {
    let mut group = c.benchmark_group("vm_next_frame");

    for voices in [1, 4, 16] {
        let ops = poly_synth_ops(voices);
        group.bench_function(format!("poly_synth_{voices}_voices"), |b| {
            let mut vm = vm_from_ops(&ops);
            b.iter(|| black_box(vm.next_frame()));
        });
    }

    let ops = filtered_synth_ops(8);
    group.bench_function("filtered_synth_8_lpf_stages", |b| {
        let mut vm = vm_from_ops(&ops);
        b.iter(|| black_box(vm.next_frame()));
    });

    let ops = convolution_ops(64);
    group.bench_function("convolution_m_64_taps", |b| {
        let mut vm = vm_from_ops(&ops);
        b.iter(|| black_box(vm.next_frame()));
    });

    group.finish();
}

criterion_group!(benches, compile_benchmarks, audio_frame_benchmarks);
criterion_main!(benches);
