# rumbot-sim

Balatro as a Rust library: the crate `rumbot_sim`, a module-for-module
translation of the pure-Python simulator in
[jimbot-sim](https://github.com/mmank/jimbot-sim), checked against it (and,
for face-down cards and recorded human games, against the game itself) by
differential fixtures. Deterministic given a seed, no I/O, no dependencies;
the run is a phase-based state machine:

```rust
let mut game = rumbot_sim::game::GameState::new("SEED0000", "Red Deck", 1);
while !game.is_over() {
    game.step(&game.legal_actions()[0]);
}
```

Its user is [rumbot](https://github.com/mmank/rumbot), the handcrafted policy
that plays it, which carries this repository as the submodule
`external/rumbot-sim` and depends on it by path.

- [PORTING.md](PORTING.md) -- the frozen interface (shared `Rc<RefCell<_>>`
  handles, `fn`-pointer hooks, the module map), how the differential fixtures
  work, and the bugs each one caught.
- `src/bin/sim_bench.rs` -- the speed benchmark, against the Python's
  `tools/bench_python.py`.

```bash
cargo test --profile quick      # 633 tests, ~2.5 min
cargo test --profile quick --test rng_fixture --test hands_fixture \
    --test blinds_fixture --test state_fixture      # the fast ones
cargo run --release --bin sim_bench -- 1200
```

`--profile quick` is release without `lto` and `codegen-units = 1`: a third of
the build time and identical answers. Use `--release` only to measure speed.
