//! Microbenchmarks for eyeballed data-structure choices in the audio hot path:
//! Program container inline capacity (the former FAST_PROGRAM_SIZE SmallVec),
//! MIGRATION_INDEX_SIZE (indexed vs linear state migration), and sub-program
//! storage for the upcoming Poly container op (see
//! docs/adr/0002-polyphony-container-op.md). Findings in BENCHMARKS.md.
//!
//! These replicate the VM's Statement/perform/migrate shapes generically so
//! container choices can be compared head-to-head, which the public types
//! (with baked-in capacities) cannot express.

use audio_vm::{Frame, Op, Sample, Stack};
use criterion::{Criterion, criterion_group, criterion_main};
use smallvec::SmallVec;
use std::hint::black_box;

struct Stmt {
    id: u64,
    op: Box<dyn Op>,
}

struct Constant(Sample);

impl Op for Constant {
    fn perform(&mut self, stack: &mut Stack) {
        stack.push(&[self.0, self.0]);
    }
}

/// Stateful one-pole smoother: consumes top frame, pushes smoothed frame.
/// Realistic per-op work that the optimizer cannot fold away, with state
/// worth migrating.
struct Smooth {
    y: Frame,
}

impl Op for Smooth {
    fn perform(&mut self, stack: &mut Stack) {
        let x = stack.pop();
        for (y, x) in self.y.iter_mut().zip(&x) {
            *y += 0.1 * (*x - *y);
        }
        let y = self.y;
        stack.push(&y);
    }

    fn migrate(&mut self, other: &mut dyn Op) {
        if let Some(other) = other.downcast_mut::<Self>() {
            self.y = other.y;
        }
    }
}

/// `Constant` followed by a chain of `Smooth`s; stack depth stays at 1.
fn chain(len: usize) -> impl Iterator<Item = Stmt> {
    (0..len).map(|i| Stmt {
        id: i as u64 + 1,
        op: if i == 0 {
            Box::new(Constant(110.0)) as Box<dyn Op>
        } else {
            Box::new(Smooth { y: [0.0, 0.0] })
        },
    })
}

/// Chain of `Smooth`s only (a poly voice body operating on the seeded sub-stack).
fn body(len: usize) -> impl Iterator<Item = Stmt> {
    (0..len).map(|i| Stmt {
        id: i as u64 + 1,
        op: Box::new(Smooth { y: [0.0, 0.0] }) as Box<dyn Op>,
    })
}

fn perform_slice(program: &mut [Stmt], stack: &mut Stack) -> Frame {
    stack.reset();
    for stmt in program {
        stmt.op.perform(stack);
    }
    stack.peek()
}

// --- 1. Inline capacity vs perform locality -------------------------------

fn inline_capacity_benchmarks(c: &mut Criterion) {
    eprintln!(
        "sizes: Stmt={}B, SmallVec<[Stmt;8]>={}B, SmallVec<[Stmt;64]>={}B, SmallVec<[Stmt;128]>={}B, Vec<Stmt>={}B",
        size_of::<Stmt>(),
        size_of::<SmallVec<[Stmt; 8]>>(),
        size_of::<SmallVec<[Stmt; 64]>>(),
        size_of::<SmallVec<[Stmt; 128]>>(),
        size_of::<Vec<Stmt>>(),
    );

    let mut group = c.benchmark_group("inline_capacity_perform");
    macro_rules! bench_storage {
        ($name:literal, $ty:ty, $len:expr) => {{
            let mut program: $ty = chain($len).collect();
            let mut stack = Stack::new();
            group.bench_function(format!("{}_len{}", $name, $len), |b| {
                b.iter(|| black_box(perform_slice(black_box(&mut program), &mut stack)))
            });
        }};
    }
    for len in [8, 96] {
        bench_storage!("vec", Vec<Stmt>, len);
        bench_storage!("smallvec8", SmallVec<[Stmt; 8]>, len);
        bench_storage!("smallvec64", SmallVec<[Stmt; 64]>, len);
        bench_storage!("smallvec128", SmallVec<[Stmt; 128]>, len);
    }
    group.finish();
}

// --- 2. Program move cost (load_program garbage swap path) ----------------

fn program_move_benchmarks(c: &mut Criterion) {
    let mut group = c.benchmark_group("program_move");
    macro_rules! bench_swap {
        ($name:literal, $ty:ty, $len:expr) => {{
            let mut a: $ty = chain($len).collect();
            let mut b_prog: $ty = chain($len).collect();
            group.bench_function(format!("{}_len{}", $name, $len), |b| {
                b.iter(|| {
                    std::mem::swap(black_box(&mut a), black_box(&mut b_prog));
                })
            });
        }};
    }
    for len in [8, 96] {
        bench_swap!("vec", Vec<Stmt>, len);
        bench_swap!("smallvec64", SmallVec<[Stmt; 64]>, len);
        bench_swap!("smallvec128", SmallVec<[Stmt; 128]>, len);
    }
    group.finish();
}

// --- 3. Migration: sorted index + binary search vs linear scan -------------

const MIGRATION_INDEX_SIZE: usize = 128;

/// Replica of vm.rs migrate_program_state's indexed strategy.
fn migrate_indexed(active: &mut [Stmt], previous: &mut [Stmt]) {
    let indexed_len = previous.len().min(MIGRATION_INDEX_SIZE);
    let mut previous_by_id: SmallVec<[(u64, usize); MIGRATION_INDEX_SIZE]> = previous
        .iter()
        .take(indexed_len)
        .enumerate()
        .map(|(index, stmt)| (stmt.id, index))
        .collect();
    previous_by_id.sort_unstable_by_key(|(id, _)| *id);
    for stmt in active {
        if let Ok(index) = previous_by_id.binary_search_by_key(&stmt.id, |(id, _)| *id) {
            let previous_index = previous_by_id[index].1;
            stmt.op.migrate(previous[previous_index].op.as_mut());
        }
    }
}

fn migrate_linear(active: &mut [Stmt], previous: &mut [Stmt]) {
    for stmt in active {
        if let Some(previous_stmt) = previous
            .iter_mut()
            .find(|previous_stmt| previous_stmt.id == stmt.id)
        {
            stmt.op.migrate(previous_stmt.op.as_mut());
        }
    }
}

fn migration_benchmarks(c: &mut Criterion) {
    let mut group = c.benchmark_group("migration_strategy");
    for len in [16, 32, 64, 128, 256] {
        // Reversed previous ids: worst case for the linear scan, unordered
        // input for the sort.
        let mut active: Vec<Stmt> = chain(len).collect();
        let mut previous: Vec<Stmt> = chain(len).collect();
        previous.reverse();
        group.bench_function(format!("indexed_len{len}"), |b| {
            b.iter(|| migrate_indexed(black_box(&mut active), black_box(&mut previous)))
        });
        group.bench_function(format!("linear_len{len}"), |b| {
            b.iter(|| migrate_linear(black_box(&mut active), black_box(&mut previous)))
        });
    }
    group.finish();
}

// --- 4. Poly-shaped workload: voice sub-program storage --------------------

const VOICES: usize = 8;
const BODY_LEN: usize = 12;

fn poly_frame<P: std::ops::DerefMut<Target = [Stmt]>>(
    voices: &mut [P],
    stack: &mut Stack,
) -> Frame {
    let mut sum = [0.0, 0.0];
    for voice in voices.iter_mut() {
        stack.reset();
        stack.push(&[60.0, 60.0]); // latched value
        stack.push(&[1.0, 1.0]); // routed ctl
        for stmt in voice.iter_mut() {
            stmt.op.perform(stack);
        }
        let frame = stack.peek();
        sum[0] += frame[0];
        sum[1] += frame[1];
    }
    sum
}

fn poly_storage_benchmarks(c: &mut Criterion) {
    let mut group = c.benchmark_group("poly_voice_storage");
    macro_rules! bench_poly {
        ($name:literal, $ty:ty) => {{
            let mut voices: Vec<$ty> = (0..VOICES).map(|_| body(BODY_LEN).collect()).collect();
            let mut stack = Stack::new();
            group.bench_function(format!("{}_frame_{VOICES}v_{BODY_LEN}ops", $name), |b| {
                b.iter(|| black_box(poly_frame(black_box(&mut voices), &mut stack)))
            });
            group.bench_function(format!("{}_construct_{VOICES}v_{BODY_LEN}ops", $name), |b| {
                b.iter(|| {
                    let voices: Vec<$ty> =
                        (0..VOICES).map(|_| body(BODY_LEN).collect()).collect();
                    black_box(voices)
                })
            });
        }};
    }
    bench_poly!("smallvec64", SmallVec<[Stmt; 64]>);
    bench_poly!("vec", Vec<Stmt>);
    bench_poly!("boxed_slice", Box<[Stmt]>);
    group.finish();
}

criterion_group!(
    benches,
    inline_capacity_benchmarks,
    program_move_benchmarks,
    migration_benchmarks,
    poly_storage_benchmarks,
);
criterion_main!(benches);
