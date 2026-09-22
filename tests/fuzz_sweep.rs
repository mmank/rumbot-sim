//! Replay the whole 1-chip fuzz **sweep**: hundreds of runs, one line each.
//!
//! `tools/gen_fuzz_fixture.py --sweep` drives a seeded chooser through
//! `legal_actions()` on every deck and every stake 1..8, forcing every blind's
//! target to 1 chip so any play beats it and the run races through shops,
//! packs, boss blinds, antes and the finisher. That much is the same harness as
//! `tests/fuzz_fixture.rs`; what changes is the *size*.
//!
//! A per-step fixture for 100k+ steps is ~84 MB (42 key digests, two legal
//! digests, the RNG signature and the chosen action per step), so the sweep
//! stores one line per seed instead:
//!
//!   * a running **FNV-1a-64** over the concatenation of every step's canonical
//!     bytes -- `state_render.leaves_bytes` of
//!     `{action, legal_order, legal_sorted, rng, state}`, the same leaves the
//!     detailed fixture's `digests`/`legal` columns hash. FNV-1a is
//!     incremental, so folding one step at a time equals hashing the run at
//!     once (`state::fnv1a64_update_is_incremental` pins that; this test
//!     recomputes it the streaming way);
//!   * **checkpoints every 25 steps**, so a mismatch localises to a 25-step
//!     block and names its range, not the whole run;
//!   * the one-shot digest of the post-run `state_dict`.
//!
//! The line carries no action sequence, so this test reproduces the sweep's
//! chooser: a `RunRng` keyed `fuzz-sweep:<seed>:<deck>:<stake>` on pool
//! `"choose"` -- the game's own generator, already bit-exact in `rng.rs`, not
//! Python's unported `random.Random`. (The committed detail seeds still use
//! `random.Random` and emit their sequence verbatim.)
//!
//! On a mismatch the test prints the seed, the checkpoint block, the step
//! range, both digests and the exact `--detail` command that turns it into a
//! runnable `tests/fuzz_fixture.rs` comparison. Every run stays under ante 16
//! by construction (`blinds` reports Python's unbounded int, which the Rust
//! i64 `ante_base_chips` saturates at ante 16; see `rust/PORTING.md`).

use std::collections::BTreeMap;
use std::path::PathBuf;

use jimbot_sim::game::{Action, ActionType, GameState};
use jimbot_sim::rng::RunRng;
use jimbot_sim::state::{
    flatten_state, fnv1a64, fnv1a64_update, leaves_to_bytes, state_dict, StateValue,
    FNV_OFFSET_BASIS,
};

/// The pool the sweep chooser advances, matching the generator.
const SWEEP_CHOOSER_POOL: &str = "choose";
/// Checkpoint spacing, matching the generator's `CHECKPOINT_EVERY`.
const CHECKPOINT_EVERY: usize = 25;
/// Antes at or past this are out of scope (i64 `ante_base_chips` ceiling).
const ANTE_CAP: i32 = 16;

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

// ----------------------------------------------------------------------
// the per-step canonical bytes, identical to the generator's `step_bytes`
// ----------------------------------------------------------------------

fn rng_signature(game: &GameState) -> String {
    let state = game.rng.state();
    let mut keys: Vec<&String> = state.keys().collect();
    keys.sort();
    let parts: Vec<String> = keys
        .iter()
        .map(|key| format!("{}:{}", key, state[*key].to_bits()))
        .collect();
    parts.join("|")
}

/// The chosen action, structured exactly as the generator writes it.
fn action_field(action: &Action) -> String {
    let cards = action
        .cards
        .iter()
        .map(|c| c.to_string())
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "type={};index={};cards={}",
        action.r#type.as_str(),
        action.index,
        cards
    )
}

/// `state_render.leaves_bytes` of `{action, legal_order, legal_sorted, rng,
/// state}`: one step's canonical bytes.
fn step_bytes(
    game: &GameState,
    action: &Action,
    actions: &[Action],
    selection: &[usize],
) -> Vec<u8> {
    let ordered: Vec<StateValue> = actions
        .iter()
        .map(|a| StateValue::Str(a.to_string()))
        .collect();
    let mut sorted_actions: Vec<Action> = actions.to_vec();
    sorted_actions.sort_by(|a, b| a.to_string().cmp(&b.to_string()));
    let sorted: Vec<StateValue> = sorted_actions
        .iter()
        .map(|a| StateValue::Str(a.to_string()))
        .collect();

    let mut tree = BTreeMap::new();
    tree.insert("action".to_string(), StateValue::Str(action_field(action)));
    tree.insert("legal_order".to_string(), StateValue::List(ordered));
    tree.insert("legal_sorted".to_string(), StateValue::List(sorted));
    tree.insert("rng".to_string(), StateValue::Str(rng_signature(game)));
    tree.insert("state".to_string(), state_dict(game, selection, 0, 0));

    let mut leaves = Vec::new();
    flatten_state(&StateValue::Map(tree), "", &mut leaves);
    leaves_to_bytes(&leaves)
}

/// The one-shot digest of the post-run observation, as the generator computes
/// it (empty selection: there is no action to hold).
fn final_digest(game: &GameState) -> u64 {
    let mut leaves = Vec::new();
    flatten_state(&state_dict(game, &[], 0, 0), "", &mut leaves);
    fnv1a64(&leaves_to_bytes(&leaves))
}

// ----------------------------------------------------------------------
// the fixture
// ----------------------------------------------------------------------

struct Row {
    seed: String,
    deck: String,
    stake: i32,
    steps: usize,
    over: bool,
    deepest: i32,
    final_digest: Option<u64>,
    /// `(step, running digest)` at every checkpoint, in order.
    checkpoints: Vec<(usize, u64)>,
}

fn parse_hex(text: &str, what: &str) -> u64 {
    u64::from_str_radix(text, 16)
        .unwrap_or_else(|_| panic!("malformed hex {:?} for {}", text, what))
}

fn parse_row(line: &str) -> Row {
    let fields: Vec<&str> = line.split('\t').collect();
    assert_eq!(fields.len(), 9, "malformed sweep row: {:?}", line);
    let checkpoints = fields[8]
        .split(';')
        .filter(|p| !p.is_empty())
        .map(|pair| {
            let (step, digest) = pair
                .split_once('=')
                .unwrap_or_else(|| panic!("malformed checkpoint {:?}", pair));
            (
                step.parse()
                    .unwrap_or_else(|_| panic!("bad checkpoint step {:?}", step)),
                parse_hex(digest, "checkpoint"),
            )
        })
        .collect();
    Row {
        seed: fields[0].to_string(),
        deck: fields[1].to_string(),
        stake: fields[2].parse().unwrap(),
        steps: fields[3].parse().unwrap(),
        over: fields[4] == "1",
        deepest: fields[6].parse().unwrap(),
        final_digest: if fields[7] == "-" {
            None
        } else {
            Some(parse_hex(fields[7], "final digest"))
        },
        checkpoints,
    }
}

fn load_rows() -> Vec<Row> {
    let path = fixture_dir().join("fuzz_sweep.txt");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read {}: {}", path.display(), e));
    text.lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(parse_row)
        .collect()
}


/// Replay one seed. Panics on the first divergence with the seed, the block,
/// the step range, both digests and the `--detail` command. Returns
/// `(steps matched, deepest ante, checkpoint comparisons)`.
fn replay(row: &Row) -> (usize, i32, usize) {
    let mut game = GameState::new(&row.seed, &row.deck, row.stake);
    game.endless = true;
    let mut chooser = RunRng::new(format!(
        "fuzz-sweep:{}:{}:{}",
        row.seed, row.deck, row.stake
    ));

    let mut deepest = game.ante;
    let mut rolling = FNV_OFFSET_BASIS;
    let mut got: Vec<(usize, u64)> = Vec::new();
    let mut steps = 0usize;

    for _ in 0..row.steps {
        assert!(
            !game.is_over(),
            "seed {}: the sweep says {} steps but the engine ended after {}",
            row.seed,
            row.steps,
            steps
        );
        assert!(
            game.ante < ANTE_CAP,
            "seed {} step {}: ante {} is past the i64 ceiling the sweep excludes",
            row.seed,
            steps + 1,
            game.ante
        );
        // Both engines force this at the same point, before the record.
        if let Some(blind) = game.blind.as_mut() {
            blind.target = 1;
        }
        let actions = game.legal_actions();
        assert!(
            !actions.is_empty(),
            "seed {} step {}: no legal actions in phase {:?}",
            row.seed,
            steps + 1,
            game.phase
        );
        let action = chooser.choice(SWEEP_CHOOSER_POOL, &actions);
        let selection: Vec<usize> = match action.r#type {
            ActionType::Play | ActionType::Discard => action.cards.clone(),
            _ => Vec::new(),
        };

        rolling = fnv1a64_update(rolling, &step_bytes(&game, &action, &actions, &selection));
        steps += 1;
        if steps % CHECKPOINT_EVERY == 0 {
            got.push((steps, rolling));
        }
        game.step(&action);
        deepest = deepest.max(game.ante);
    }

    // The generator anchors the final step too, even when it is a partial block.
    if steps > 0 && got.last().map(|(s, _)| *s) != Some(steps) {
        got.push((steps, rolling));
    }

    assert_eq!(
        steps, row.steps,
        "seed {}: replayed {} steps, the sweep records {}",
        row.seed, steps, row.steps
    );
    assert_eq!(
        deepest, row.deepest,
        "seed {}: replayed to ante {}, the sweep records {}",
        row.seed, deepest, row.deepest
    );
    assert_eq!(
        got.len(),
        row.checkpoints.len(),
        "seed {}: recomputed {} checkpoints, the sweep records {}",
        row.seed,
        got.len(),
        row.checkpoints.len()
    );

    let mut comparisons = 0usize;
    for (i, (want_step, want)) in row.checkpoints.iter().enumerate() {
        let (got_step, got_digest) = got[i];
        assert_eq!(
            got_step, *want_step,
            "seed {}: checkpoint {} is step {} in the sweep, step {} here",
            row.seed, i, want_step, got_step
        );
        if got_digest != *want {
            // The block starts just after the previous checkpoint.
            let start = if i == 0 { 1 } else { row.checkpoints[i - 1].0 + 1 };
            panic!(
                "seed {} diverged in checkpoint block {} (steps {}-{}, {}):\n  \
                 python {:016x}\n  rust   {:016x}\n  \
                 replay every step with\n    \
                 python tools/gen_fuzz_fixture.py --detail {}\n  \
                 then cargo test --offline --release --test fuzz_fixture",
                row.seed,
                i,
                start,
                want_step,
                *want_step - start + 1,
                want,
                got_digest,
                row.seed
            );
        }
        comparisons += 1;
    }

    if let Some(want) = row.final_digest {
        let actual = final_digest(&game);
        assert_eq!(
            actual, want,
            "seed {}: the post-run state_dict digest diverges:\n  \
             python {:016x}\n  rust   {:016x}\n  \
             replay every step with:\n    \
             python tools/gen_fuzz_fixture.py --detail {}",
            row.seed, want, actual, row.seed
        );
    }

    (steps, deepest, comparisons)
}


#[test]
fn the_sweep_rolling_digests_replay() {
    let rows = load_rows();

    // The fixture declares its own totals -- each row carries its step count and
    // its checkpoint list -- so the assertions below are exact rather than
    // guessed, and cannot drift from the fixture or flatter it.
    //
    // A floor was tried first and it lied: `MIN_STEPS = 100_000` failed against a
    // 95,437-step fixture that replayed perfectly, which is a test misreporting
    // its own coverage rather than finding a bug. Only the seed count stays a
    // floor, because that is what a truncated fixture would break and there is no
    // independent total to compare it against.
    const MIN_SEEDS: usize = 500;
    let fixture_steps: usize = rows.iter().map(|r| r.steps).sum();
    let fixture_checkpoints: usize = rows.iter().map(|r| r.checkpoints.len()).sum();
    let fixture_deepest: i32 = rows.iter().map(|r| r.deepest).max().unwrap_or(0);

    assert!(
        rows.len() >= MIN_SEEDS,
        "the sweep carries {} seeds, expected at least {}",
        rows.len(),
        MIN_SEEDS
    );

    let mut seeds = 0usize;
    let mut total_steps = 0usize;
    let mut total_checkpoints = 0usize;
    let mut deepest_ante = 0i32;
    let mut ended_by_themselves = 0usize;
    for row in &rows {
        let (steps, deepest, comparisons) = replay(row);
        seeds += 1;
        total_steps += steps;
        total_checkpoints += comparisons;
        deepest_ante = deepest_ante.max(deepest);
        if row.over {
            ended_by_themselves += 1;
        }
    }

    assert!(
        deepest_ante < ANTE_CAP,
        "the sweep reached ante {}, past the documented i64 ceiling",
        deepest_ante
    );
    assert_eq!(
        total_steps, fixture_steps,
        "replayed {} steps but the fixture declares {}",
        total_steps, fixture_steps
    );
    assert_eq!(
        total_checkpoints, fixture_checkpoints,
        "made {} checkpoint comparisons but the fixture declares {}",
        total_checkpoints, fixture_checkpoints
    );
    assert_eq!(
        deepest_ante, fixture_deepest,
        "reached ante {} but the fixture declares {}",
        deepest_ante, fixture_deepest
    );

    eprintln!(
        "fuzz sweep: {} seeds, {} steps, {} checkpoint comparisons, deepest ante {}, \
         {} runs ended by themselves",
        seeds, total_steps, total_checkpoints, deepest_ante, ended_by_themselves
    );
}

