//! Replay random legal actions through the Rust engine, and compare the
//! **legal action list itself** at every step.
//!
//! `tools/gen_fuzz_fixture.py` forces every blind's target to 1 chip (so any
//! play beats the blind) and then drives the Python `jimbot_sim` with a seeded
//! `random.Random` choosing uniformly from `legal_actions()`. Runs progress
//! deep and fast -- through shops, packs, boss blinds, antes and the finisher
//! -- through states neither the scripted flow policy nor the 15 human
//! recordings ever reached, with *random* rather than policy actions.
//!
//! The action sequence is emitted by the generator and applied here verbatim,
//! so the chooser never has to be mirrored; what is compared is the engine.
//! After every step this test computes and compares:
//!
//!   * the **legal-action digests**: `legal_order` over `legal_actions()` in
//!     list order and `legal_sorted` over the same list sorted by the action's
//!     canonical rendering. A mismatch in `legal_sorted` is a *missing or
//!     extra* action; a mismatch only in `legal_order` is a wrong *order*.
//!     This validates the whole action mask -- it knows Death takes two cards,
//!     that an eternal joker cannot be sold, that a boss cannot be skipped,
//!     that a full row cannot buy a sixth -- which no fixture here has ever
//!     checked;
//!   * all 42 per-key `state_dict` digests;
//!   * the raw RNG pool signature.
//!
//! Both engines force `blind.target = 1` at the same point, before the record.
//! A mismatch stops at the first disagreement with the seed, step, kind, both
//! digests and the generator `--dump` command that names the differing action.

use std::collections::HashMap;
use std::path::PathBuf;

use jimbot_sim::cards::{self, CardRef};
use jimbot_sim::game::{Action, ActionType, GameState, PackChoice};
use jimbot_sim::state::{
    digest_leaves, flatten_state, key_digests, state_dict, top_key, StateValue,
};

/// The seeds whose full per-step traces are committed: the original 12. Any
/// extra `fuzz_<SEED>.txt` -- `gen_fuzz_fixture.py --detail <SEED>` writes one
/// so a sweep mismatch becomes a runnable comparison -- is picked up too.
fn detail_seeds() -> Vec<String> {
    let dir = fixture_dir();
    let mut seeds: Vec<String> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("cannot list {}: {}", dir.display(), e))
        .filter_map(|entry| {
            let name = entry.ok()?.file_name().into_string().ok()?;
            let seed = name.strip_prefix("fuzz_")?.strip_suffix(".txt")?;
            // `fuzz_sweep.txt` is the rolling-digest fixture, a different shape.
            if seed.is_empty() || seed == "sweep" {
                return None;
            }
            Some(seed.to_string())
        })
        .collect();
    seeds.sort();
    seeds
}

/// The `state_dict` keys the fixture carries: the number `state_dict` returns.
const TOP_LEVEL_KEYS: usize = 42;

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

// ----------------------------------------------------------------------
// the fields, rendered exactly as `replay_fixture.rs` renders them
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

fn card_label(card: &CardRef) -> String {
    format!(
        "{}{}",
        cards::rank_of(card).short(),
        cards::suit_of(card).as_str()
    )
}

fn hand_label(game: &GameState) -> String {
    let mut labels: Vec<String> = game.hand.iter().map(card_label).collect();
    labels.sort();
    labels.join(",")
}

fn joker_label(game: &GameState) -> String {
    game.jokers
        .iter()
        .map(|j| j.borrow().name().to_string())
        .collect::<Vec<_>>()
        .join(",")
}

fn consumable_label(game: &GameState) -> String {
    game.consumables
        .iter()
        .map(|c| c.borrow().spec.name.to_string())
        .collect::<Vec<_>>()
        .join(",")
}

fn shop_label(game: &GameState) -> String {
    let shop = match &game.shop {
        Some(shop) => shop,
        None => return String::new(),
    };
    let mut parts: Vec<String> = Vec::new();
    for slot in &shop.slots {
        if slot.kind == "joker" {
            let name = slot
                .joker
                .as_ref()
                .map(|j| j.borrow().name().to_string())
                .unwrap_or_else(|| "?".to_string());
            parts.push(format!("j:{}", name));
        } else if slot.kind == "consumable" {
            let name = slot
                .consumable
                .as_ref()
                .map(|s| s.name.to_string())
                .unwrap_or_else(|| "?".to_string());
            parts.push(format!("c:{}", name));
        } else {
            let name = slot
                .card
                .as_ref()
                .map(card_label)
                .unwrap_or_else(|| "?".to_string());
            parts.push(format!("p:{}", name));
        }
    }
    for pack in &shop.packs {
        parts.push(format!("pk:{}", pack.key));
    }
    for voucher in shop.vouchers_on_offer() {
        parts.push(format!("v:{}", voucher.key));
    }
    parts.join("|")
}

fn pack_label(game: &GameState) -> String {
    game.pack_options
        .iter()
        .map(|option| match option {
            PackChoice::Joker(joker) => format!("j:{}", joker.borrow().name()),
            PackChoice::Card(card) => format!("p:{}", card_label(card)),
            PackChoice::Consumable(spec) => format!("c:{}", spec.name),
        })
        .collect::<Vec<_>>()
        .join("|")
}

// ----------------------------------------------------------------------
// the legal-action digests: the point of this fixture
// ----------------------------------------------------------------------

/// FNV-1a-64 over the canonical leaves of `[<action rendering>, ...]`, laid out
/// exactly as the generator lays it out for `tools/state_render.py`.
fn legal_digest(actions: &[Action]) -> u64 {
    let tree = StateValue::List(
        actions
            .iter()
            .map(|action| StateValue::Str(action.to_string()))
            .collect(),
    );
    let mut leaves = Vec::new();
    flatten_state(&tree, "", &mut leaves);
    digest_leaves(&leaves)
}

/// `(order, sorted)` digests for the position the game is in.
fn legal_digests(game: &GameState) -> (u64, u64) {
    let actions = game.legal_actions();
    let ordered = legal_digest(&actions);
    let mut sorted = actions.clone();
    sorted.sort_by(|a, b| a.to_string().cmp(&b.to_string()));
    (ordered, legal_digest(&sorted))
}

/// The canonical (action-sorted) renderings of every legal action: what
/// `--dump <SEED> <STEP>` prints, so a mismatch names the differing action.
fn legal_renderings(game: &GameState) -> Vec<String> {
    let mut actions: Vec<String> = game
        .legal_actions()
        .into_iter()
        .map(|action| action.to_string())
        .collect();
    actions.sort();
    actions
}

// ----------------------------------------------------------------------
// the trace
// ----------------------------------------------------------------------

struct Step {
    phase: String,
    ante: i32,
    action: Action,
    action_text: String,
    money: i32,
    hands: i32,
    discards: i32,
    hand: String,
    jokers: String,
    consumables: String,
    shop: String,
    pack: String,
    chips: i64,
    rng: String,
    /// The picking the generator handed `state_dict`, emitted rather than
    /// re-derived, so both engines get the identical one.
    selection: Vec<usize>,
    /// One observation digest per top-level `state_dict` key.
    digests: HashMap<String, u64>,
    /// `legal_order` and `legal_sorted`.
    legal: HashMap<String, u64>,
}

/// `key=<hex>` pairs separated by `;`, as the generator writes them.
fn parse_digests(field: &str) -> HashMap<String, u64> {
    let mut out = HashMap::new();
    if field.is_empty() {
        return out;
    }
    for pair in field.split(';') {
        let (key, value) = pair
            .split_once('=')
            .unwrap_or_else(|| panic!("malformed digest pair {:?}", pair));
        let digest = u64::from_str_radix(value, 16)
            .unwrap_or_else(|_| panic!("malformed digest {:?}", pair));
        out.insert(key.to_string(), digest);
    }
    out
}

/// The selection column: comma-separated hand indices, or `-` for empty.
fn parse_selection(field: &str) -> Vec<usize> {
    if field == "-" || field.is_empty() {
        return Vec::new();
    }
    field
        .split(',')
        .map(|index| {
            index
                .parse()
                .unwrap_or_else(|_| panic!("malformed selection index {:?}", index))
        })
        .collect()
}

fn action_type(name: &str) -> ActionType {
    match name {
        "cash_out" => ActionType::CashOut,
        "select_blind" => ActionType::SelectBlind,
        "skip_blind" => ActionType::SkipBlind,
        "play" => ActionType::Play,
        "discard" => ActionType::Discard,
        "use_consumable" => ActionType::UseConsumable,
        "sell_joker" => ActionType::SellJoker,
        "swap_joker_left" => ActionType::SwapJokerLeft,
        "sell_consumable" => ActionType::SellConsumable,
        "buy" => ActionType::Buy,
        "buy_and_use" => ActionType::BuyAndUse,
        "reroll_boss" => ActionType::RerollBoss,
        "buy_voucher" => ActionType::BuyVoucher,
        "reroll" => ActionType::Reroll,
        "buy_pack" => ActionType::BuyPack,
        "pick_pack" => ActionType::PickPack,
        "skip_pack" => ActionType::SkipPack,
        "leave_shop" => ActionType::LeaveShop,
        other => panic!("unknown action type {:?}", other),
    }
}

/// The structured action column (`type=..;index=..;cards=..`), parsed.
fn parse_action(field: &str) -> Action {
    let mut kind = "";
    let mut index = -1i32;
    let mut card_text = "";
    for part in field.split(';') {
        if let Some(value) = part.strip_prefix("type=") {
            kind = value;
        } else if let Some(value) = part.strip_prefix("index=") {
            index = value.parse().unwrap();
        } else if let Some(value) = part.strip_prefix("cards=") {
            card_text = value;
        }
    }
    let cards: Vec<usize> = if card_text.is_empty() {
        Vec::new()
    } else {
        card_text.split(',').map(|c| c.parse().unwrap()).collect()
    };
    Action {
        r#type: action_type(kind),
        index,
        cards,
    }
}

/// The same string the generator wrote for the chosen `Action`, so the test can
/// check the Rust action equals the Python one, not merely that it played.
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

/// Parse one trace: `(seed, deck, stake, endless, steps, footer)`.
fn parse(path: &PathBuf) -> (String, String, i32, bool, Vec<Step>, String) {
    let text = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("cannot read {}: {}", path.display(), e));
    let mut seed = String::new();
    let mut deck = String::new();
    let mut stake = 1;
    let mut endless = false;
    let mut footer = String::new();
    let mut steps = Vec::new();
    for line in text.lines() {
        if line.starts_with("seed|") {
            seed = line[5..].to_string();
        } else if line.starts_with("deck|") {
            deck = line[5..].to_string();
        } else if line.starts_with("stake|") {
            stake = line[6..].parse().unwrap();
        } else if line.starts_with("endless|") {
            endless = &line[8..] == "1";
        } else if line.starts_with('#') {
            footer = line.to_string();
        } else if line.is_empty() {
            continue;
        } else {
            let f: Vec<&str> = line.split('\t').collect();
            assert_eq!(f.len(), 16, "malformed step line: {:?}", line);
            steps.push(Step {
                phase: f[0].to_string(),
                ante: f[1].parse().unwrap(),
                action: parse_action(f[2]),
                action_text: f[2].to_string(),
                money: f[3].parse().unwrap(),
                hands: f[4].parse().unwrap(),
                discards: f[5].parse().unwrap(),
                hand: f[6].to_string(),
                jokers: f[7].to_string(),
                consumables: f[8].to_string(),
                shop: f[9].to_string(),
                pack: f[10].to_string(),
                chips: f[11].parse().unwrap(),
                rng: f[12].to_string(),
                selection: parse_selection(f[13]),
                digests: parse_digests(f[14]),
                legal: parse_digests(f[15]),
            });
        }
    }
    (seed, deck, stake, endless, steps, footer)
}

/// Replay one trace; panics with the first disagreement. Returns
/// `(steps matched, deepest ante, key comparisons, legal comparisons)`.
fn replay(
    seed: &str,
    deck: &str,
    stake: i32,
    endless: bool,
    steps: &[Step],
) -> (usize, i32, usize, usize) {
    let mut game = GameState::new(seed, deck, stake);
    game.endless = endless;

    let mut deepest = game.ante;
    let mut key_comparisons = 0usize;
    let mut legal_comparisons = 0usize;
    for (i, step) in steps.iter().enumerate() {
        let n = i + 1;
        assert!(
            !game.is_over(),
            "seed {} step {}: the trace still has steps but the engine is over",
            seed,
            n
        );
        // Both engines force this at the same point, before the record.
        if let Some(blind) = game.blind.as_mut() {
            blind.target = 1;
        }

        // The chosen action, checked against the one the generator emitted.
        assert_eq!(
            action_field(&step.action),
            step.action_text,
            "seed {} step {}: parsed action {:?} does not render as the recorded {:?}",
            seed,
            n,
            action_field(&step.action),
            step.action_text
        );

        let got: [(&str, String, String); 13] = [
            ("phase", game.phase.as_str().to_string(), step.phase.clone()),
            ("ante", game.ante.to_string(), step.ante.to_string()),
            ("action", action_field(&step.action), step.action_text.clone()),
            ("money", game.money.to_string(), step.money.to_string()),
            ("hands", game.hands_left.to_string(), step.hands.to_string()),
            (
                "discards",
                game.discards_left.to_string(),
                step.discards.to_string(),
            ),
            ("hand", hand_label(&game), step.hand.clone()),
            ("jokers", joker_label(&game), step.jokers.clone()),
            (
                "consumables",
                consumable_label(&game),
                step.consumables.clone(),
            ),
            ("shop", shop_label(&game), step.shop.clone()),
            ("pack", pack_label(&game), step.pack.clone()),
            ("chips", game.chips_scored.to_string(), step.chips.to_string()),
            ("rng", rng_signature(&game), step.rng.clone()),
        ];
        for (field, actual, expected) in got {
            if actual != expected {
                panic!(
                    "seed {} diverged at step {} of {} ({} steps matched) on {}:\n  \
                     python: {}\n  rust:   {}",
                    seed, n, steps.len(), i, field, expected, actual
                );
            }
        }

        // The whole observation, not just the thirteen fields above. The
        // picking is the one the generator emitted and handed `state_dict`.
        let got_digests = key_digests(&state_dict(&game, &step.selection, 0, 0));
        assert_eq!(
            got_digests.len(),
            TOP_LEVEL_KEYS,
            "seed {} step {}: state_dict returned {} top-level keys, expected {}",
            seed,
            n,
            got_digests.len(),
            TOP_LEVEL_KEYS
        );
        assert_eq!(
            step.digests.len(),
            TOP_LEVEL_KEYS,
            "seed {} step {}: the fixture carries {} digests, expected {}",
            seed,
            n,
            step.digests.len(),
            TOP_LEVEL_KEYS
        );
        for (key, want) in &step.digests {
            match got_digests.get(key) {
                Some(actual) if actual == want => {}
                Some(actual) => {
                    // Name the differing leaf, not just the digest, so the
                    // `--dump <SEED> <STEP> <KEY>` command is one step away.
                    let state = state_dict(&game, &step.selection, 0, 0);
                    let mut leaves = Vec::new();
                    flatten_state(&state, "", &mut leaves);
                    for (path, value) in leaves.iter().filter(|(p, _)| top_key(p) == key) {
                        eprintln!("  rust {} = {}", path, value);
                    }
                    panic!(
                        "seed {} diverged at step {} of {} on state key {}:\n  \
                         python: {:016x}\n  rust:   {:016x}\n  \
                         (python tools/gen_fuzz_fixture.py --dump {} {} {})",
                        seed, n, steps.len(), key, want, actual, seed, n, key
                    );
                }
                None => panic!(
                    "seed {} step {}: the fixture has state key {:?} but state_dict does not",
                    seed, n, key
                ),
            }
        }
        for key in got_digests.keys() {
            assert!(
                step.digests.contains_key(key),
                "seed {} step {}: state_dict key {:?} is missing from the fixture",
                seed,
                n,
                key
            );
        }
        key_comparisons += got_digests.len();

        // The legal action list itself. `legal_sorted` matching while
        // `legal_order` differs is a wrong ORDER; a `legal_sorted` mismatch is
        // a MISSING OR EXTRA action. Both are compared, because the
        // distinction is the point.
        let (order, sorted) = legal_digests(&game);
        let want_order = step.legal["legal_order"];
        let want_sorted = step.legal["legal_sorted"];
        if order != want_order || sorted != want_sorted {
            let verdict = if sorted == want_sorted {
                "the ORDER differs (same action set)"
            } else {
                "the action SET differs (a missing or extra action)"
            };
            let mut listing = String::new();
            for rendering in legal_renderings(&game) {
                listing.push_str(&format!("  rust {}\n", rendering));
            }
            panic!(
                "seed {} diverged at step {} of {} on the legal actions ({}):\n  \
                 order:  python {:016x}  rust {:016x}\n  \
                 sorted: python {:016x}  rust {:016x}\n  \
                 the engine's list, canonical order:\n{}  \
                 (python tools/gen_fuzz_fixture.py --dump {} {})",
                seed, n, steps.len(), verdict, want_order, order, want_sorted, sorted,
                listing, seed, n
            );
        }
        legal_comparisons += 2;

        deepest = deepest.max(game.ante);
        game.step(&step.action);
    }
    (steps.len(), deepest, key_comparisons, legal_comparisons)
}

#[test]
fn the_random_legal_actions_replay_step_by_step() {
    let dir = fixture_dir();
    // Sum of the steps across the runs: 1,547. This is the floor -- a trace
    // that stopped early, or a seed skipped, drops it.
    const EXPECTED_STEPS: usize = 1547;
    // 42 key digests and 2 legal digests per step.
    const EXPECTED_KEY_STEPS: usize = 1547 * 42;
    const EXPECTED_LEGAL_DIGESTS: usize = 1547 * 2;
    let mut matched_total = 0usize;
    let mut deepest_ante = 0i32;
    let mut keys_total = 0usize;
    let mut legal_total = 0usize;
    let seeds = detail_seeds();
    assert!(
        seeds.len() >= 12,
        "only {} detail traces found; the committed 12 are missing",
        seeds.len()
    );
    for seed in &seeds {
        let path = dir.join(format!("fuzz_{}.txt", seed));
        let (file_seed, deck, stake, endless, steps, footer) = parse(&path);
        assert_eq!(file_seed, *seed, "fixture {} names seed {}", seed, file_seed);
        let (matched, deepest, keys, legal) = replay(&file_seed, &deck, stake, endless, &steps);
        assert_eq!(
            matched,
            steps.len(),
            "seed {} stopped early rather than replaying its whole trace",
            seed
        );
        eprintln!(
            "fuzz fixture: seed {} ({}, stake {}) matched {} steps / {} key-digests / {} legal-digests, deepest ante {} [{}]",
            seed, deck, stake, matched, keys, legal, deepest, footer
        );
        matched_total += matched;
        keys_total += keys;
        legal_total += legal;
        deepest_ante = deepest_ante.max(deepest);
    }
    eprintln!(
        "fuzz fixture: {} steps, {} key-digest comparisons, {} legal-action digest comparisons across {} seeds; deepest ante {}",
        matched_total,
        keys_total,
        legal_total,
        seeds.len(),
        deepest_ante
    );
    assert!(
        matched_total >= EXPECTED_STEPS,
        "only {} steps matched across {} seeds -- {} were expected",
        matched_total,
        seeds.len(),
        EXPECTED_STEPS
    );
    assert!(
        keys_total >= EXPECTED_KEY_STEPS,
        "only {} key-digests were compared -- {} were expected",
        keys_total,
        EXPECTED_KEY_STEPS
    );
    assert!(
        legal_total >= EXPECTED_LEGAL_DIGESTS,
        "only {} legal-action digests were compared -- {} were expected",
        legal_total,
        EXPECTED_LEGAL_DIGESTS
    );
    assert!(
        deepest_ante >= 8,
        "deepest ante reached was {} -- no run got to the finisher blinds",
        deepest_ante
    );
}
