# CLAUDE.md

`rumbot_sim`: Balatro as a Rust library, translated from the Python simulator
in `jimbot-sim`. Its one user is `rumbot` (the handcrafted policy), which
carries this repository as the submodule `external/rumbot-sim`. It lived in
`rumbot` as `rust/jimbot_sim` until 2026-10-04; the history came with it.

## FIXED RULE: ask before anything that runs longer than 5 minutes

**Never start a screen, comparison, sweep, test suite, build or any other job
expected to take more than 5 minutes -- on the laptop or the Linux box --
without asking Marcin first and getting a yes.** Say what it runs, on how many
runs, where, and how long you expect it to take. If unsure how long it takes,
time a small slice first or ask. Marcin, 2026-09-28: *"do not, under any
circumstances, run a screen, test, whatever, that takes more than 5 minutes,
without asking me first."* (The full suite here is about 2.5 minutes.)

## FIXED RULE: every session works in its own worktree

Start each session by creating a git worktree for it (`EnterWorktree`, or
`git worktree add .claude/worktrees/<name>`), and do all edits, builds and
commits there -- never in the main checkout, which other sessions share. When
working through `rumbot`'s submodule, the same holds: branch in the submodule,
not on its detached checkout. Marcin, 2026-09-28.

## "Ship it" means all the way to main

When Marcin says **"ship it"**: commit on the worktree branch, put it on
`main`, push `main` to `origin`, and fast-forward the main checkout to it.
A change here is not live for the policy until `rumbot` bumps its submodule
pin (`git -C external/rumbot-sim checkout <sha>` and commit the pointer), so
ship the pin bump in `rumbot` with it. Marcin, 2026-09-30.

## Read first

[PORTING.md](PORTING.md) is the specification: the frozen interface, the
module map, how each differential fixture works and what it caught. Treat its
*interface* and *lessons* sections as current and its tallies as dated -- it is
layered history, with two "where the port stands" sections written at
different times. When you change behaviour, update it.

**Everything worth knowing goes in the repo, not in an assistant's memory.**

## A change here is measured in rumbot

The tests here say the simulator still matches the Python and the game. They
cannot say whether a rules fix changes how the policy plays: that is
`rumbot`'s eval mix (`scripts/eval/compare_policies.py`, see its CLAUDE.md).
A fix that makes the simulator closer to the game is shipped on the fixtures
and a recording or seed that shows the difference; then the policy's
`knowledge/learned.rs` may want regenerating, which is `rumbot`'s business.

## Commands

```bash
cargo test --profile quick      # 633 tests, ~2.5 min
cargo test --profile quick --test rng_fixture --test hands_fixture \
    --test blinds_fixture --test state_fixture      # the fast ones
cargo run --release --bin sim_bench -- 1200
```

The suite is dominated by two differential fixtures -- `fuzz_sweep` (~90 s)
and `replay_fixture` (~36 s). Neither is `#[ignore]`d on purpose. Use
`--profile quick` (release minus `lto` and `codegen-units = 1`) for tests and
iteration; `--release` only for anything measured for speed.

## Traps

- **`fork` must deep clone.** `GameState` holds `Rc<RefCell<Card>>` shared
  across four piles, so `clone()` aliases the fork back into the run. Go
  through `GameState::deep_clone` (`src/game_clone.rs`) and nothing else. A
  shallow clone makes every number wrong while every test passes.
- **`rng.rs` is LuaJIT's TW223 plus the game's `pseudohash`/`pseudoseed`**,
  bit-exact, with a named pool per decision. (The CPython Mersenne Twister and
  set order, `pyrandom.rs`/`pyset.rs`, are the policy's, in `rumbot`.)
- **The card sort-id counter (`cards::sort_id_now`/`set_sort_id`) is
  `thread_local!`** and decides tie-breaks: anything that creates cards on a
  copy and does not put it back moves unrelated seeds downstream.
- **The fixture generators are not here.** PORTING.md refers throughout to
  `tools/gen_*.py`; they live in `balatro_bot`. The fixtures under
  `tests/fixtures/` are committed and checked, but regenerating one means
  going back there. The exception is `facedown.txt`, recorded off the game's
  own Lua by `rumbot`'s `scripts/gen_facedown_fixture.py`.
- **Face-down cards** are marked (`Card::face_down`,
  `JokerInstance::face_down`) and play as themselves; hiding them from a
  decision is the caller's job. The Python-recorded fixtures compare the RNG
  pools less `rng::LUA_ONLY_POOLS` (The Wheel's `wheel`), which the Python
  never drew.
- **`jimbot_sim.*` in comments is the Python package**, not this crate: those
  names say where a function was translated from. The crate is `rumbot_sim`.

## Conventions

- Formatting: a `PostToolUse` hook (`.claude/hooks/format_edited.py`) runs
  `rustfmt` on every file Claude edits, and `.githooks/pre-commit` on staged
  files. Enable the latter once per clone with
  `git config core.hooksPath .githooks`.
- LF line endings everywhere, pinned in `.gitattributes` and `.editorconfig`.
- Commit messages are a short declarative sentence about the behaviour that
  changed, not a summary of the diff -- see `git log`.
- Comments argue from named seeds and recorded divergences, because that is
  how the bugs were found. Keep that style.
