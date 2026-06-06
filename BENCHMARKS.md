# Performance benchmarks

Sound Garden uses Criterion for repeatable microbenchmarks of the current critical paths:

- `compile_program/*`: text-operation compilation into an executable audio VM program.
- `vm_next_frame/*`: audio-thread frame generation through `VM::next_frame`.

Run all benchmarks:

```sh
cargo bench --bench performance
```

For a quicker smoke run while iterating:

```sh
cargo bench --bench performance -- --sample-size 10
```

Criterion writes reports under `target/criterion/`.
