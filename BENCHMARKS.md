# Performance benchmarks

Sound Garden uses Criterion for repeatable microbenchmarks of critical paths.

Benchmark groups:

- `compile_program/*`: text-operation compilation into an executable audio VM program.
- `vm_next_frame/*`: single-frame audio-thread generation through `VM::next_frame`.
- `stack/*`: hot `Stack` push/pop/peek operations.
- `program_lifecycle/*`: `VM::load_program` migration paths with matching and non-matching statement ids.
- `vm_state_paths/*`: monitor, active crossfade, and pause-fade paths.
- `vm_render_block/*`: 128-frame block rendering throughput.

Run all benchmarks:

```sh
cargo bench --bench performance
```

For a quicker smoke run while iterating:

```sh
cargo bench --bench performance -- --sample-size 10
```

Criterion writes reports under `target/criterion/`.
