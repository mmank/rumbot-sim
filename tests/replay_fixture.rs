//! Replay real recorded human games through the Rust engine.
//!
//! `tools/gen_replay_fixture.py` drives the Python `jimbot_sim` through each
//! `external/jimbot-sim/recordings/*.json` -- 15 real human runs, 4,140 merged
//! actions, reaching ante 8-9 -- and writes one trace per recording. Each line
//! carries the state the player was looking at, the resolved action as
//! structured data, and the hand/joker order the driver arranged before the
//! move. This test rebuilds the identical `GameState`, applies the recorded
//! arrange and then the recorded action, and compares every field after every
//! step, stopping at the first disagreement.
//!
//! Unlike `flow_fixture.rs` the actions are not chosen by a policy: they are
//! what a human did, so the shops, packs, boss blinds and finisher bosses are
//! the ones a real run met rather than the ones a scripted driver reaches.
//!
//! The thirteen fields are not the whole observation: `state_dict` returns 42
//! top-level keys (~1,900 leaves), so each step also carries one digest per
//! key and the test computes and compares those too (42 x 4,140 key-steps).
//! The digest is the canonical one `state.rs` shares with
//! `tools/state_render.py`; a mismatch reports the key, both digests and the
//! generator `--dump` command that names the differing leaf. The picking
//! `state_dict` takes is reconstructed as the generator did -- the action's
//! own card indices for a PLAY or DISCARD, empty otherwise -- and the fixture
//! header says so. `state_dict`'s `toggles_used`/`joker_swaps_used` arguments
//! are varied per step too (they were passed 0 at every recorded step before,
//! so neither was ever exercised); the generator's `ui_counters` and this test
//! hand the identical pair to both engines.
//!
//! Four observation keys (`in_run`, `highlight_limit`, `sorted_rank`,
//! `sorted_suit`) are literals in `state.py` (lines 471, 499, 504, 505) and
//! are constants by construction. Their digests are pinned on purpose; do not
//! "fix" them to vary. `won` and `bankrupt_at` are constant in these recordings
//! only because no recorded run was won and none ever held Credit Card -- both
//! are covered by the synthetic `state.txt` cases.

use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;

use jimbot_sim::cards::{self, CardRef};
use jimbot_sim::game::{Action, ActionType, GameState, PackChoice};
use jimbot_sim::jokers::JokerRef;
use jimbot_sim::shop_pool::key_by_joker_name;
use jimbot_sim::state::{digest_leaves, flatten_state, key_digests, state_dict, StateValue};

/// The recording stems. `13` does not exist (the raw files are 1..16 minus 13).
const RECORDINGS: [&str; 15] = [
    "1", "2", "3", "4", "5", "6", "7", "8", "9", "10", "11", "12", "14", "15", "16",
];

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

// ----------------------------------------------------------------------
// the fields, rendered exactly as `flow_fixture.rs` renders them
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

fn hand_label(game: &GameState) -> String {
    let mut labels: Vec<String> = game
        .hand
        .iter()
        .map(|c| {
            format!(
                "{}{}",
                cards::rank_of(c).short(),
                cards::suit_of(c).as_str()
            )
        })
        .collect();
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

fn card_label(card: &CardRef) -> String {
    format!(
        "{}{}",
        cards::rank_of(card).short(),
        cards::suit_of(card).as_str()
    )
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
// the arrange, ported from `jimbot_sim.run`
// ----------------------------------------------------------------------

/// Mirror of `jimbot_sim.run.match_hand_order`: hold the hand in the order the
/// recording shows it, by position in `full_deck` for the cards the deck was
/// built with and by age for the ones the run made.
fn match_hand_order(game: &mut GameState, ids: &[i64], deck_index: &HashMap<u64, usize>) {
    if ids.is_empty() {
        return;
    }
    let mut by_id: HashMap<usize, Vec<CardRef>> = HashMap::new();
    let mut made: Vec<CardRef> = Vec::new();
    for card in &game.hand {
        match deck_index.get(&cards::uid_of(card)) {
            Some(&index) => by_id.entry(index).or_default().push(card.clone()),
            None => made.push(card.clone()),
        }
    }
    made.sort_by_key(cards::uid_of);
    let made_ids: Vec<i64> = {
        let mut ids: Vec<i64> = ids
            .iter()
            .copied()
            .filter(|w| *w >= deck_index.len() as i64)
            .collect();
        ids.sort();
        ids
    };
    let mut by_made: HashMap<i64, CardRef> = made_ids.into_iter().zip(made.into_iter()).collect();

    let mut ordered: Vec<CardRef> = Vec::new();
    let mut leftover: Vec<CardRef> = game.hand.clone();
    for &want in ids {
        let card = if let Some(pool) = by_id.get_mut(&(want as usize)) {
            if pool.is_empty() {
                None
            } else {
                Some(pool.remove(0))
            }
        } else {
            None
        };
        let card = card.or_else(|| by_made.remove(&want));
        if let Some(card) = card {
            ordered.push(card.clone());
            leftover.retain(|l| cards::uid_of(l) != cards::uid_of(&card));
        }
    }
    ordered.extend(leftover);
    game.hand = ordered;
}

/// Mirror of `jimbot_sim.run.match_joker_order`: put the joker row into the
/// order the recording shows, matching by the centre key.
fn match_joker_order(game: &mut GameState, recorded_keys: &[String]) {
    if recorded_keys.is_empty() {
        return;
    }
    let mut by_key: HashMap<String, Vec<JokerRef>> = HashMap::new();
    for joker in &game.jokers {
        let name = joker.borrow().name();
        let key = key_by_joker_name(name).unwrap_or(name).to_string();
        by_key.entry(key).or_default().push(joker.clone());
    }
    let mut ordered: Vec<JokerRef> = Vec::new();
    let mut leftover: Vec<JokerRef> = game.jokers.clone();
    for key in recorded_keys {
        if let Some(pool) = by_key.get_mut(key) {
            if !pool.is_empty() {
                let joker = pool.remove(0);
                ordered.push(joker.clone());
                leftover.retain(|j| !Rc::ptr_eq(j, &joker));
            }
        }
    }
    ordered.extend(leftover);
    game.jokers = ordered;
}

// ----------------------------------------------------------------------
// the trace
// ----------------------------------------------------------------------

/// What the driver did for one step: an action, a sort-button press, or nothing
/// (the game acting on its own, a pack tag using the pack it made).
enum Move {
    Action(Action),
    Sort,
    Skip,
}

struct Step {
    phase: String,
    ante: i32,
    action: String,
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
    hand_ids: Vec<i64>,
    joker_keys: Vec<String>,
    sort: Option<String>,
    /// The picking the generator handed `state_dict` for this step. Emitted
    /// rather than re-derived here, so both engines get the identical one.
    selection: Vec<usize>,
    /// One observation digest per top-level `state_dict` key, computed by the
    /// generator with `tools/state_render.py`.
    digests: HashMap<String, u64>,
    /// One preview digest per kind and ordering (`<kind>_order`,
    /// `<kind>_sorted`), over every legal PLAY/DISCARD action's preview value.
    previews: HashMap<String, u64>,
    /// `state_dict`'s own UI counters, varied by `tools/gen_replay_fixture.py`
    /// and handed to `state_dict` unchanged. Not fields of the run.
    toggles_used: i32,
    joker_swaps_used: i32,
}

/// The `state_dict` keys the fixture carries: the number `state_dict` returns.
/// Asserted so a key added on one side and not the other is a failure.
const TOP_LEVEL_KEYS: usize = 42;

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

/// The structured action column, parsed. `type=skip` is a step the driver did
/// nothing for; `type=sort` is a sort-button press, carried by the `sort`
/// column instead.
fn parse_move(field: &str) -> Move {
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
    match kind {
        "skip" => Move::Skip,
        "sort" => Move::Sort,
        other => {
            let cards: Vec<usize> = if card_text.is_empty() {
                Vec::new()
            } else {
                card_text.split(',').map(|c| c.parse().unwrap()).collect()
            };
            Move::Action(Action {
                r#type: action_type(other),
                index,
                cards,
            })
        }
    }
}

/// The same string the generator wrote for a resolved `Action`, so the test can
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

// ----------------------------------------------------------------------
// the preview digests
// ----------------------------------------------------------------------

/// The preview kinds the fixture carries, matching the generator's
/// `PREVIEW_KINDS`. Each is one preview method, so both of `_preview_mode`'s
/// modes (`roll` for `scores`/`value`, `pessimistic`) are compared.
const PREVIEW_KINDS: [&str; 5] = ["scores", "pessimistic", "money", "outcome", "value"];

/// One action's preview value, shaped as the generator shapes it: an int for a
/// score, a list for a tuple, so the canonical renderer puts each part in its
/// own leaf. The tuple order is the Python return order.
fn preview_value(game: &mut GameState, kind: &str, cards: &[usize]) -> StateValue {
    match kind {
        "scores" => StateValue::Int(game.preview_score(cards, "roll")),
        "pessimistic" => StateValue::Int(game.preview_score(cards, "pessimistic")),
        "money" => {
            let (score, expected) = game.preview_money(cards);
            StateValue::List(vec![StateValue::Int(score), StateValue::Float(expected)])
        }
        "outcome" => {
            let (score, dollars, rolls) = game.preview_outcome(cards);
            StateValue::List(vec![
                StateValue::Int(score),
                StateValue::Int(dollars),
                StateValue::Int(rolls as i64),
            ])
        }
        "value" => {
            let (score, dollars) = game.preview_value(cards, "roll");
            StateValue::List(vec![StateValue::Int(score), StateValue::Int(dollars)])
        }
        other => panic!("unknown preview kind {:?}", other),
    }
}

/// `(action rendering, value)` for each action, in the order given.
fn preview_entries(
    game: &mut GameState,
    actions: &[Action],
    kind: &str,
) -> Vec<(String, StateValue)> {
    actions
        .iter()
        .map(|action| {
            let value = preview_value(game, kind, &action.cards);
            (action.to_string(), value)
        })
        .collect()
}

/// FNV-1a-64 over `[{action: value}, ...]`, exactly as the generator lays it
/// out for `tools/state_render.py`: the list keeps the pairs' order, so this
/// digest is order-sensitive.
fn preview_digest(entries: &[(String, StateValue)]) -> u64 {
    let tree = StateValue::List(
        entries
            .iter()
            .map(|(action, value)| {
                let mut map = std::collections::BTreeMap::new();
                map.insert(action.clone(), value.clone());
                StateValue::Map(map)
            })
            .collect(),
    );
    let mut leaves = Vec::new();
    flatten_state(&tree, "", &mut leaves);
    digest_leaves(&leaves)
}

/// The leaves of the same tree, sorted by action: what `--dump <kind>` prints,
/// so a digest mismatch can be turned into the differing action.
fn preview_leaves(entries: &[(String, StateValue)]) -> Vec<(String, String)> {
    let mut sorted = entries.to_vec();
    sorted.sort_by(|a, b| a.0.cmp(&b.0));
    let tree = StateValue::List(
        sorted
            .iter()
            .map(|(action, value)| {
                let mut map = std::collections::BTreeMap::new();
                map.insert(action.clone(), value.clone());
                StateValue::Map(map)
            })
            .collect(),
    );
    let mut leaves = Vec::new();
    flatten_state(&tree, "", &mut leaves);
    leaves
}

/// Every preview digest for the position the game is in: `<kind>_order` in
/// `legal_actions()` order and `<kind>_sorted` sorted by the action rendering.
/// Runs every preview; the caller proves that is read-only.
fn preview_digests(game: &mut GameState) -> (std::collections::BTreeMap<String, u64>, usize) {
    let actions: Vec<Action> = game
        .legal_actions()
        .into_iter()
        .filter(|action| matches!(action.r#type, ActionType::Play | ActionType::Discard))
        .collect();
    let mut out = std::collections::BTreeMap::new();
    for kind in PREVIEW_KINDS {
        let entries = preview_entries(game, &actions, kind);
        out.insert(format!("{}_order", kind), preview_digest(&entries));
        let mut sorted = entries.clone();
        sorted.sort_by(|a, b| a.0.cmp(&b.0));
        out.insert(format!("{}_sorted", kind), preview_digest(&sorted));
    }
    (out, actions.len())
}

fn parse_arrange(field: &str) -> (Vec<i64>, Vec<String>, Option<String>) {
    let mut hand_ids = Vec::new();
    let mut joker_keys = Vec::new();
    let mut sort = None;
    for part in field.split(' ') {
        if let Some(value) = part.strip_prefix("H:") {
            if !value.is_empty() {
                hand_ids = value.split(',').map(|v| v.parse().unwrap()).collect();
            }
        } else if let Some(value) = part.strip_prefix("J:") {
            if !value.is_empty() {
                joker_keys = value.split(',').map(|v| v.to_string()).collect();
            }
        } else if let Some(value) = part.strip_prefix("S:") {
            if value != "-" {
                sort = Some(value.to_string());
            }
        }
    }
    (hand_ids, joker_keys, sort)
}

fn parse(path: &PathBuf) -> (String, String, i32, Option<i32>, Vec<Step>, String) {
    let text = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("cannot read {}: {}", path.display(), e));
    let mut seed = String::new();
    let mut deck = String::new();
    let mut stake = 1;
    let mut money = None;
    let mut footer = String::new();
    let mut steps = Vec::new();
    for line in text.lines() {
        if line.starts_with("seed|") {
            seed = line[5..].to_string();
        } else if line.starts_with("deck|") {
            deck = line[5..].to_string();
        } else if line.starts_with("stake|") {
            stake = line[6..].parse().unwrap();
        } else if line.starts_with("money|") {
            money = Some(line[6..].parse().unwrap());
        } else if line.starts_with('#') {
            footer = line.to_string();
        } else if line.is_empty() {
            continue;
        } else {
            let f: Vec<&str> = line.split('\t').collect();
            // digests + previews + the two UI counters appended by the generator.
            assert_eq!(f.len(), 19, "malformed step line: {:?}", line);
            let (hand_ids, joker_keys, sort) = parse_arrange(f[13]);
            let selection = parse_selection(f[14]);
            let digests = parse_digests(f[15]);
            let previews = parse_digests(f[16]);
            let toggles_used: i32 = f[17].parse().unwrap();
            let joker_swaps_used: i32 = f[18].parse().unwrap();
            steps.push(Step {
                phase: f[0].to_string(),
                ante: f[1].parse().unwrap(),
                action: f[2].to_string(),
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
                hand_ids,
                joker_keys,
                sort,
                selection,
                digests,
                previews,
                toggles_used,
                joker_swaps_used,
            });
        }
    }
    (seed, deck, stake, money, steps, footer)
}

/// Replay one trace; panics with the first disagreement, and how many steps
/// matched before it. Returns (steps matched, deepest ante, key comparisons).
fn replay(
    name: &str,
    seed: &str,
    deck: &str,
    stake: i32,
    money: Option<i32>,
    steps: &[Step],
) -> (usize, i32, usize, usize, usize, usize) {
    let mut game = GameState::new(seed, deck, stake);
    // The recordings reach past ante eight (a player who pressed continue), as
    // `SimRun(endless=True)` does in `ops/sim_replay.py`.
    game.endless = true;
    // A recording made with a bankroll already set, reapplied before anything
    // is compared, exactly as `replay` does.
    if let Some(money) = money {
        game.money = money;
    }
    // The hand's order is aligned by position in the deck as it was built, the
    // same map `SimRun.deck_index` holds; it is not rebuilt as cards are made.
    let deck_index: HashMap<u64, usize> = game
        .full_deck
        .iter()
        .enumerate()
        .map(|(i, c)| (cards::uid_of(c), i))
        .collect();

    let mut deepest = game.ante;
    let mut key_comparisons = 0usize;
    let mut preview_comparisons = 0usize;
    let mut candidate_total = 0usize;
    let mut read_only_positions = 0usize;
    for (i, step) in steps.iter().enumerate() {
        match_hand_order(&mut game, &step.hand_ids, &deck_index);
        match_joker_order(&mut game, &step.joker_keys);
        let mv = parse_move(&step.action);
        // The sort column is the flip side of the action column: a sort move
        // carries one, every other move must not.
        assert_eq!(
            matches!(mv, Move::Sort),
            step.sort.is_some(),
            "{} step {}: sort column {:?} does not match action {:?}",
            name,
            i,
            step.sort,
            step.action
        );
        let got_action = match &mv {
            Move::Action(action) => action_field(action),
            Move::Sort => "type=sort;index=-1;cards=".to_string(),
            Move::Skip => "type=skip;index=-1;cards=".to_string(),
        };

        let got: [(&str, String, String); 13] = [
            ("phase", game.phase.as_str().to_string(), step.phase.clone()),
            ("ante", game.ante.to_string(), step.ante.to_string()),
            ("action", got_action, step.action.clone()),
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
            (
                "chips",
                game.chips_scored.to_string(),
                step.chips.to_string(),
            ),
            ("rng", rng_signature(&game), step.rng.clone()),
        ];
        for (field, actual, expected) in got {
            if actual != expected {
                panic!(
                    "recording {} diverged at step {} ({} steps matched) on {}:\n  \
                     python: {}\n  rust:   {}",
                    name, i, i, field, expected, actual
                );
            }
        }

        // The whole observation, not just the thirteen fields above. The
        // picking is the one the generator emitted and handed `state_dict` --
        // the action's own card indices for a PLAY or DISCARD, empty otherwise
        // (see the fixture header) -- so both engines get the identical one.
        let got_digests = key_digests(&state_dict(
            &game,
            &step.selection,
            step.toggles_used,
            step.joker_swaps_used,
        ));

        // The covered-key count is the point: a digest that covered three keys
        // must not pass, so both sides' key counts are asserted rather than a
        // floor, and a key present on only one side fails here.
        assert_eq!(
            got_digests.len(),
            TOP_LEVEL_KEYS,
            "{} step {}: state_dict returned {} top-level keys, expected {}",
            name,
            i + 1,
            got_digests.len(),
            TOP_LEVEL_KEYS
        );
        assert_eq!(
            step.digests.len(),
            TOP_LEVEL_KEYS,
            "{} step {}: the fixture carries {} digests, expected {}",
            name,
            i + 1,
            step.digests.len(),
            TOP_LEVEL_KEYS
        );
        for (key, want) in &step.digests {
            match got_digests.get(key) {
                Some(actual) if actual == want => {}
                Some(actual) => panic!(
                    "recording {} diverged at step {} on key {}:\n  \
                     python: {:016x}\n  rust:   {:016x}\n  \
                     (python tools/gen_replay_fixture.py --dump {} {} {})",
                    name,
                    i + 1,
                    key,
                    want,
                    actual,
                    name,
                    i + 1,
                    key
                ),
                None => panic!(
                    "recording {} step {}: the fixture has key {:?} but state_dict does not",
                    name,
                    i + 1,
                    key
                ),
            }
        }
        for key in got_digests.keys() {
            assert!(
                step.digests.contains_key(key),
                "recording {} step {}: state_dict key {:?} is missing from the fixture",
                name,
                i + 1,
                key
            );
        }
        key_comparisons += got_digests.len();

        // The read-only proof, at scale. A preview that mutates the run is the
        // worst bug here because it is invisible until a seed diverges hours
        // later, so every preview is run between two reads of the whole
        // observation and the RNG pool, and they must be byte-identical.
        let rng_before = rng_signature(&game);
        let (got_previews, candidates) = preview_digests(&mut game);
        assert_eq!(
            key_digests(&state_dict(
                &game,
                &step.selection,
                step.toggles_used,
                step.joker_swaps_used,
            )),
            got_digests,
            "recording {} step {}: a preview mutated the observation",
            name,
            i + 1
        );
        assert_eq!(
            rng_signature(&game),
            rng_before,
            "recording {} step {}: a preview mutated the RNG pool",
            name,
            i + 1
        );
        read_only_positions += 1;
        candidate_total += candidates;

        // The preview values: every legal PLAY/DISCARD action, in both
        // `legal_actions()` order and sorted by the action rendering. A wrong
        // value moves both; a wrong order moves only `_order`.
        assert_eq!(
            got_previews.len(),
            PREVIEW_KINDS.len() * 2,
            "{} step {}: preview_digests returned {} keys",
            name,
            i + 1,
            got_previews.len()
        );
        assert_eq!(
            step.previews.len(),
            PREVIEW_KINDS.len() * 2,
            "{} step {}: the fixture carries {} preview digests",
            name,
            i + 1,
            step.previews.len()
        );
        preview_comparisons += got_previews.len();
        for (key, want) in &step.previews {
            match got_previews.get(key) {
                Some(actual) if actual == want => {}
                Some(_) => {
                    let kind = key.split('_').next().unwrap_or(key);
                    // Both orderings, whatever this key is: sorted matching
                    // while order differs is a wrong *order*; both differing is
                    // a wrong *value*. That distinction is the point.
                    let order_key = format!("{}_order", kind);
                    let sorted_key = format!("{}_sorted", kind);
                    let pair = |k: &str| {
                        (
                            step.previews.get(k).copied().unwrap_or(0),
                            got_previews.get(k).copied().unwrap_or(0),
                        )
                    };
                    let (p_order, r_order) = pair(&order_key);
                    let (p_sorted, r_sorted) = pair(&sorted_key);
                    let actions: Vec<Action> = game
                        .legal_actions()
                        .into_iter()
                        .filter(|action| {
                            matches!(action.r#type, ActionType::Play | ActionType::Discard)
                        })
                        .collect();
                    let entries = preview_entries(&mut game, &actions, kind);
                    for (path, value) in preview_leaves(&entries) {
                        eprintln!("  rust {} = {}", path, value);
                    }
                    let verdict = if p_sorted == r_sorted {
                        "the ORDER differs (values match)"
                    } else {
                        "the VALUES differ"
                    };
                    panic!(
                        "recording {} diverged at step {} on preview kind {} ({}):\n  \
                         order:  python {:016x}  rust {:016x}\n  \
                         sorted: python {:016x}  rust {:016x}\n  \
                         (python tools/gen_replay_fixture.py --dump {} {} {})",
                        name,
                        i + 1,
                        kind,
                        verdict,
                        p_order,
                        r_order,
                        p_sorted,
                        r_sorted,
                        name,
                        i + 1,
                        kind
                    );
                }
                None => panic!(
                    "recording {} step {}: the fixture has preview {:?} but the \
                     engine does not",
                    name,
                    i + 1,
                    key
                ),
            }
        }
        for key in got_previews.keys() {
            assert!(
                step.previews.contains_key(key),
                "recording {} step {}: preview {:?} is missing from the fixture",
                name,
                i + 1,
                key
            );
        }

        match &mv {
            Move::Action(action) => game.step(action),
            Move::Sort => game.sort_hand(step.sort.as_deref().expect("a sort move has a sort")),
            Move::Skip => {}
        }
        deepest = deepest.max(game.ante);
    }
    (
        steps.len(),
        deepest,
        key_comparisons,
        preview_comparisons,
        candidate_total,
        read_only_positions,
    )
}

#[test]
fn the_recorded_human_games_replay_step_by_step() {
    let dir = fixture_dir();
    // Sum of the merged steps across the 15 recordings: 4,140. This is the
    // floor -- a trace that stopped early, or a recording skipped, drops it.
    const EXPECTED_STEPS: usize = 4140;
    // Every recorded step carries one digest per top-level key, all 42 of
    // them, so the comparison is 42 x 4,140 = 173,880 key-steps. This is the
    // floor the covered-key count has to clear.
    const EXPECTED_KEY_STEPS: usize = 4140 * 42;
    // Every recorded step carries both orderings of the 5 preview kinds, so
    // 10 x 4,140 = 41,400 preview-digest comparisons. Floor, like above.
    const EXPECTED_PREVIEW_DIGEST_STEPS: usize = 4140 * 10;
    // The candidate PLAY/DISCARD subsets the previews score, summed over the
    // recordings: 743,526 actions, each measured by 5 preview calls, so
    // 3,717,630 preview values. Floors: a trace that stopped early or a
    // candidate kind skipped drops them.
    const EXPECTED_CANDIDATES: usize = 743_526;
    const EXPECTED_PREVIEW_VALUES: usize = 743_526 * 5;
    let mut matched_total = 0usize;
    let mut deepest_ante = 0i32;
    let mut keys_total = 0usize;
    let mut previews_total = 0usize;
    let mut candidates_total = 0usize;
    let mut read_only_total = 0usize;
    // The two keys that were compared but never varied before: `state_dict`'s
    // caller-supplied counters, passed 0 at every recorded step. Count the
    // distinct digests the fixture actually carries, so a fixture that pinned
    // them again (or a Rust side that ignored them) fails here rather than
    // passing while proving nothing.
    let mut toggles_variants: std::collections::BTreeSet<u64> = std::collections::BTreeSet::new();
    let mut swaps_variants: std::collections::BTreeSet<u64> = std::collections::BTreeSet::new();
    for name in RECORDINGS {
        let path = dir.join(format!("replay_{}.txt", name));
        let (seed, deck, stake, money, steps, footer) = parse(&path);
        for step in &steps {
            toggles_variants.insert(step.digests["toggles_used"]);
            swaps_variants.insert(step.digests["joker_swaps_used"]);
        }
        let (matched, deepest, keys, previews, candidates, read_only) =
            replay(name, &seed, &deck, stake, money, &steps);
        assert!(
            matched > 0,
            "recording {} matched zero steps -- the fixture is not being exercised",
            name
        );
        assert_eq!(
            matched,
            steps.len(),
            "recording {} stopped early rather than replaying its whole trace",
            name
        );
        eprintln!(
            "replay fixture: recording {} (seed {}, {}, stake {}) matched {} steps / {} key-steps / {} preview-digests, {} candidate plays ({} preview values), {} read-only positions, deepest ante {} [{}]",
            name, seed, deck, stake, matched, keys, previews, candidates,
            candidates * 5, read_only, deepest, footer
        );
        matched_total += matched;
        keys_total += keys;
        previews_total += previews;
        candidates_total += candidates;
        read_only_total += read_only;
        deepest_ante = deepest_ante.max(deepest);
    }
    eprintln!(
        "replay fixture: {} steps, {} key-steps, {} preview-digest comparisons across {} recordings; {} candidate plays scored ({} preview values), {} read-only positions, deepest ante {}",
        matched_total,
        keys_total,
        previews_total,
        RECORDINGS.len(),
        candidates_total,
        candidates_total * 5,
        read_only_total,
        deepest_ante
    );
    assert!(
        matched_total >= EXPECTED_STEPS,
        "only {} steps matched across {} recordings -- {} were expected",
        matched_total,
        RECORDINGS.len(),
        EXPECTED_STEPS
    );
    assert!(
        keys_total >= EXPECTED_KEY_STEPS,
        "only {} key-steps were compared -- {} were expected",
        keys_total,
        EXPECTED_KEY_STEPS
    );
    assert!(
        previews_total >= EXPECTED_PREVIEW_DIGEST_STEPS,
        "only {} preview-digest comparisons were made -- {} were expected",
        previews_total,
        EXPECTED_PREVIEW_DIGEST_STEPS
    );
    assert!(
        candidates_total >= EXPECTED_CANDIDATES,
        "only {} candidate plays were scored -- {} were expected",
        candidates_total,
        EXPECTED_CANDIDATES
    );
    assert!(
        candidates_total * 5 >= EXPECTED_PREVIEW_VALUES,
        "only {} preview values were compared -- {} were expected",
        candidates_total * 5,
        EXPECTED_PREVIEW_VALUES
    );
    assert!(
        read_only_total >= EXPECTED_STEPS,
        "the read-only proof covered only {} positions -- {} were expected",
        read_only_total,
        EXPECTED_STEPS
    );
    assert!(
        deepest_ante >= 8,
        "deepest ante reached was {} -- no recording got to the finisher blinds",
        deepest_ante
    );
    // `ui_counters(step)` gives `toggles_used` the six values 0..5 and
    // `joker_swaps_used` the seven values 0..6 (step*3 mod 7, 3 and 7 coprime),
    // so the fixture must carry exactly that many distinct digests for the two
    // keys. Fewer means the parameters were pinned again and the comparison
    // proves nothing; a Rust side that dropped them would fail earlier, on the
    // digest itself.
    eprintln!(
        "replay fixture: toggles_used {} distinct digests, joker_swaps_used {} distinct digests",
        toggles_variants.len(),
        swaps_variants.len()
    );
    assert_eq!(
        toggles_variants.len(),
        6,
        "toggles_used only took {} distinct digests -- the parameter is not varying",
        toggles_variants.len()
    );
    assert_eq!(
        swaps_variants.len(),
        7,
        "joker_swaps_used only took {} distinct digests -- the parameter is not varying",
        swaps_variants.len()
    );
}
