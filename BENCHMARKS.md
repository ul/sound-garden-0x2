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

## Op hot-path pass (2026-09, Apple Silicon)

`vm_next_frame`, before → after:

| bench | before | after | change | cause |
|---|---|---|---|---|
| `pitch_detection_yin` | 10.5 µs | 1.07 µs | −90% | contiguous window copy + 8 independent accumulators (the serial add chain was the bottleneck) |
| `sliding_convolution_256` | 255 ns | 38 ns | −85% | O(1) running sum, exact resum every N frames |
| `long_patterns_32_cells` | 165 ns | 79 ns | −51% | cell lookup starts from the previous cell; floor instead of fmod |
| `spectral_shuffle` | 884 ns | 590 ns | −33% | `FftPlanner` (SIMD) instead of scalar `Radix4`; no per-frame `Vec` |
| `spectral_reverse` | 1.08 µs | 791 ns | −27% | same, plus in-place bin reversal |
| `poly_synth_16_voices` | 277 ns | 243 ns | −12% | `Fn1`..`Fn5` generic over the function, so it inlines |
| `fm_arithmetic` | 128 ns | 116 ns | −10% | same |
| `oscillator_bank` | 120 ns | 112 ns | −6% | floor-based `wrap_phase` |

`tests/hot_path_load.rs::realtime_ops_do_not_allocate_while_rendering_or_reloading` guards the
audio-thread no-allocation rule for these ops.

## Typical-piece pass (2026-10, Apple Silicon)

Profiled with `sample` on two real pieces (a granular/poly piece and a slow
sine-strata piece), rendering speed before → after, output bit-identical at
16 bits (seeded):

| piece | before | after | main causes |
|---|---|---|---|
| sine strata (~20 sines, much `^`) | 28× | 39.5× | `pi`/`tau`/`golden` fold as literals (so `0.01 golden * s` is a `FixedOsc`); `x 8 ^` is `PowConst` (powi); `2 x ^` is `exp2`; oscillators shape a mono phase once instead of per channel |
| granulate + `poly:32` + `verb` | 5.7× | 6.9× | Hann window by rotation instead of `cos` per grain-sample; `impulse` skips `exp` past 40 apexes; `verb` caches line gains; mono oscillators |

`vm_next_frame/fm_arithmetic` 105 → 77 ns (−27%) from the mono-oscillator change.

The largest remaining cost in poly pieces is idle voices still running every
frame; sleeping them changes state (oscillator phase, in-voice patterns), so
it was left out for now.

Run all benchmarks:

```sh
cargo bench --bench performance
```

For a quicker smoke run while iterating:

```sh
cargo bench --bench performance -- --sample-size 10
```

Criterion writes reports under `target/criterion/`.
