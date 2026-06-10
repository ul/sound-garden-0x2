# Performance benchmarks

Sound Garden uses Criterion for repeatable microbenchmarks of critical paths.

Benchmark groups:

- `compile_program/*`: text-operation compilation into an executable audio VM program.
- `vm_next_frame/*`: single-frame audio-thread generation through `VM::next_frame`.
- `stack/*`: hot `Stack` push/pop/peek operations.
- `program_lifecycle/*`: `VM::load_program` migration paths with matching and non-matching statement ids.
- `vm_state_paths/*`: monitor, active crossfade, and pause-fade paths.
- `vm_render_block/*`: 128-frame block rendering throughput.

The `microstructure` bench (`cargo bench --bench microstructure`) validates eyeballed data-structure choices:

- `inline_capacity_perform/*`: statement-chain execution with `Vec` vs `SmallVec` inline capacities 8/64/128.
- `program_move/*`: `mem::swap` cost of program containers (the `load_program` garbage path).
- `migration_strategy/*`: vm.rs's sorted-index migration vs a naive linear scan.
- `poly_voice_storage/*`: per-frame run and construction of 8 voice sub-programs in `SmallVec<[_;64]>` vs `Vec` vs `Box<[Stmt]>`.

## Microstructure findings (2026-06, Apple Silicon)

- **`FAST_PROGRAM_SIZE` inline storage buys nothing on the hot path.** Perform times are identical across `Vec` and `SmallVec` 8/64/128 at program lengths 8 and 96 (≈52 ns / ≈685 ns). Statements are `{u64, Box<dyn Op>}` — the op state is heap-boxed regardless, so inline placement of the statement array doesn't change locality where it matters.
- **Inline storage actively costs on moves:** swapping programs is ≈2 ns for `Vec` vs ≈41 ns (`SmallVec<64>`, 1552 B inline) and ≈81 ns (`SmallVec<128>`, 3088 B). Only hit once per reload, so absolute cost is trivial — but it is pure waste, and it bloats every `Program` value (and the `VM` struct) by ≈1.5 KB.
- **`MIGRATION_INDEX_SIZE` indexed migration is justified.** Crossover vs linear scan is ≈len 32 (16: 180 ns vs 89 ns; 32: 249 vs 256; 64: 518 vs 928; 128: 1.15 µs vs 3.53 µs; 256: 1.81 µs vs 12.9 µs, reversed-id worst case). The ≈90 ns loss for tiny programs once per reload doesn't merit a small-program shortcut.
- **Poly voice sub-programs should be `Box<[Statement]>` (or `Vec`), not `Program`.** Frame and construction times are identical across storages (≈692 ns frame, ≈1.8 µs construct for 8 voices × 12 ops); `SmallVec<[_;64]>` would add ≈1.5 KB inline per voice for zero benefit.

Run all benchmarks:

```sh
cargo bench --bench performance
```

For a quicker smoke run while iterating:

```sh
cargo bench --bench performance -- --sample-size 10
```

Criterion writes reports under `target/criterion/`.
