//! Replay the Python simulator's flow trace.
//!
//! `tools/gen_flow_fixture.py` drives the Python `jimbot_sim` with the same
//! deterministic policy the Rust tests use and writes one line per step: phase,
//! action, money, hands, discards, the hand as sorted card labels, jokers by name,
//! chips scored and `rng.state()`. This test replays it and stops at the first
//! disagreement, reporting how many steps matched.

use std::path::PathBuf;

use rumbot_sim::game::{Action, ActionType, GameState, PackChoice, Phase};

const SEEDS: [&str; 32] = [
    "FLOW0001", "FLOW0002", "FLOW0003", "FLOW0004", "FLOW0005", "FLOW0006", "FLOW0007", "FLOW0008",
    "FLOW0009", "FLOW0010", "FLOW0011", "FLOW0012", "FLOW0013", "FLOW0014", "FLOW0015", "FLOW0016",
    "FLOW0017", "FLOW0018", "FLOW0019", "FLOW0020", "FLOW0021", "FLOW0022", "FLOW0023", "FLOW0024",
    "FLOW0025", "FLOW0026", "FLOW0027", "FLOW0028", "FLOW0029", "FLOW0030", "FLOW0031", "FLOW0032",
];

/// The one seed whose policy skips blinds, so the skip-tag path stays covered.
const SKIP_SEEDS: [&str; 1] = ["FLOW0001"];

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn rng_signature(game: &GameState) -> String {
    let state = game.rng.state();
    // Less the pools only the game's Lua draws, which this Python-recorded
    // fixture cannot know: see `rng::LUA_ONLY_POOLS`.
    let mut keys: Vec<&String> = state
        .keys()
        .filter(|key| !rumbot_sim::rng::LUA_ONLY_POOLS.contains(&key.as_str()))
        .collect();
    keys.sort();
    let parts: Vec<String> = keys
        .iter()
        .map(|key| format!("{}:{}", key, state[*key].to_bits()))
        .collect();
    parts.join("|")
}

/// The mirrored policy. `tools/gen_flow_fixture.py::Policy` is the same code in
/// Python; the two are compared action-for-action by `replay`. Keep them in
/// lockstep -- every branch, every tie-break and the `bought`/`rerolled` flags
/// are part of the policy, and a difference here shows up as a policy artefact
/// (not an engine bug) the moment the first differing action is chosen.
///
/// Where a choice is made from `legal_actions()`, the *list order* decides:
/// `first` is the first in list order, `min` is by `(price, index)`. The play
/// loop breaks ties by taking the earlier action (strict `>`), the same rule as
/// Python.
struct Policy {
    last_phase: Option<Phase>,
    bought: bool,
    rerolled: bool,
}

impl Policy {
    fn new() -> Self {
        Policy {
            last_phase: None,
            bought: false,
            rerolled: false,
        }
    }

    fn choose(&mut self, game: &mut GameState) -> Action {
        use ActionType::*;

        let phase = game.phase;
        // Per-shop flags: reset on the first step taken in a shop.
        if phase == Phase::Shop && self.last_phase != Some(Phase::Shop) {
            self.bought = false;
            self.rerolled = false;
        }
        self.last_phase = Some(phase);

        let acts = game.legal_actions();
        let first = |kind: ActionType| acts.iter().find(|a| a.r#type == kind).cloned();

        match phase {
            Phase::RoundEval => first(CashOut).expect("cash out is always offered"),

            Phase::BlindSelect => {
                if SKIP_SEEDS.contains(&game.seed.as_str()) {
                    if let Some(skip) = first(SkipBlind) {
                        return skip;
                    }
                }
                first(SelectBlind).expect("select blind is always offered")
            }

            Phase::Playing => {
                // Use a consumable first, so the tarot/spectral/planet paths are
                // walked; the first in legal order is the lowest consumable slot.
                if let Some(use_it) = first(UseConsumable) {
                    return use_it;
                }
                let plays: Vec<Action> =
                    acts.iter().filter(|a| a.r#type == Play).cloned().collect();
                if !plays.is_empty() {
                    let mut best = plays[0].clone();
                    let mut best_score = game.preview_score(&best.cards, "roll");
                    for action in &plays[1..] {
                        let score = game.preview_score(&action.cards, "roll");
                        if score > best_score {
                            best_score = score;
                            best = action.clone();
                        }
                    }
                    // If the best play cannot clear the blind, mulligan: discard
                    // the first legal subset that keeps every card the play
                    // uses. Mirrors the Python comment; see `Policy`.
                    let short = game
                        .blind
                        .as_ref()
                        .is_some_and(|b| game.chips_scored + best_score < b.target);
                    if short && game.discards_left > 0 {
                        let keep: Vec<usize> = best.cards.clone();
                        if let Some(discard) = acts.iter().find(|a| {
                            a.r#type == Discard && a.cards.iter().all(|i| !keep.contains(i))
                        }) {
                            return discard.clone();
                        }
                    }
                    return best;
                }
                if let Some(discard) = first(Discard) {
                    return discard;
                }
                acts[0].clone()
            }

            Phase::Shop => {
                let shop = game.shop.as_ref().expect("shop phase without a shop");
                // Cheapest affordable joker, then cheapest affordable consumable,
                // both keyed by `(price, index)`.
                let mut best_joker: Option<((i32, i32), Action)> = None;
                let mut best_cons: Option<((i32, i32), Action)> = None;
                for action in acts.iter().filter(|a| a.r#type == Buy) {
                    let slot = &shop.slots[action.index as usize];
                    let key = (game.slot_price(slot), action.index);
                    if slot.kind == "joker" {
                        if best_joker.as_ref().map_or(true, |(k, _)| key < *k) {
                            best_joker = Some((key, action.clone()));
                        }
                    } else if slot.kind == "consumable"
                        && best_cons.as_ref().map_or(true, |(k, _)| key < *k)
                    {
                        best_cons = Some((key, action.clone()));
                    }
                }
                if let Some((_, action)) = best_joker {
                    self.bought = true;
                    return action;
                }
                if let Some((_, action)) = best_cons {
                    self.bought = true;
                    return action;
                }
                let packs: Vec<Action> = acts
                    .iter()
                    .filter(|a| a.r#type == BuyPack)
                    .cloned()
                    .collect();
                if !packs.is_empty() {
                    self.bought = true;
                    let mut best = packs[0].clone();
                    let mut best_key = (
                        game.pack_price(&shop.packs[best.index as usize]),
                        best.index,
                    );
                    for action in &packs[1..] {
                        let key = (
                            game.pack_price(&shop.packs[action.index as usize]),
                            action.index,
                        );
                        if key < best_key {
                            best_key = key;
                            best = action.clone();
                        }
                    }
                    return best;
                }
                if !self.bought && !self.rerolled {
                    if let Some(reroll) = first(Reroll) {
                        self.rerolled = true;
                        return reroll;
                    }
                }
                first(LeaveShop).expect("leave shop is always offered")
            }

            Phase::Pack => {
                // Take the first option; the pack closes itself once its picks are
                // spent, so this picks as many as the pack allows, then skips.
                if let Some(pick) = first(PickPack) {
                    return pick;
                }
                first(SkipPack).expect("skip pack is always offered")
            }

            _ => acts[0].clone(),
        }
    }
}

fn hand_label(game: &GameState) -> String {
    let mut labels: Vec<String> = game
        .hand
        .iter()
        .map(|c| {
            format!(
                "{}{}",
                rumbot_sim::cards::rank_of(c).short(),
                rumbot_sim::cards::suit_of(c).as_str()
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

fn card_label(card: &rumbot_sim::cards::CardRef) -> String {
    format!(
        "{}{}",
        rumbot_sim::cards::rank_of(card).short(),
        rumbot_sim::cards::suit_of(card).as_str()
    )
}

/// The shop's whole row: what is on sale, not just what was bought. Mirrors the
/// Python `shop_label`.
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
                .map(|c| card_label(c))
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

/// An open booster's options, by name, whether or not any is taken.
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
}

fn parse(path: &PathBuf) -> (String, String, i32, Vec<Step>) {
    let text = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("cannot read {}: {}", path.display(), e));
    let mut seed = String::new();
    let mut deck = String::new();
    let mut stake = 1;
    let mut steps = Vec::new();
    for line in text.lines() {
        if line.starts_with("seed|") {
            seed = line[5..].to_string();
        } else if line.starts_with("deck|") {
            deck = line[5..].to_string();
        } else if line.starts_with("stake|") {
            stake = line[6..].parse().unwrap();
        } else if line.starts_with('#') || line.is_empty() {
            continue;
        } else {
            let f: Vec<&str> = line.split('\t').collect();
            assert_eq!(f.len(), 13, "malformed step line: {:?}", line);
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
            });
        }
    }
    (seed, deck, stake, steps)
}

/// Replay one trace; panics with the first disagreement, and how many steps
/// matched before it.
fn replay(seed: &str, deck: &str, stake: i32, steps: &[Step]) -> usize {
    let mut game = GameState::new(seed, deck, stake);
    let mut policy = Policy::new();
    for (i, step) in steps.iter().enumerate() {
        let action = policy.choose(&mut game);
        let got = [
            ("phase", game.phase.as_str().to_string(), step.phase.clone()),
            ("ante", game.ante.to_string(), step.ante.to_string()),
            ("action", action.to_string(), step.action.clone()),
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
                    "seed {} diverged at step {} ({} steps matched) on {}:\n  python: {}\n  rust:   {}",
                    seed, i, i, field, expected, actual
                );
            }
        }
        game.step(&action);
    }
    steps.len()
}

#[test]
fn the_flow_trace_matches_the_python_simulator() {
    let dir = fixture_dir();
    let mut matched_total = 0usize;
    let mut deepest_ante = 0i32;
    for seed in SEEDS {
        let path = dir.join(format!("flow_{}.txt", seed));
        let (seed, deck, stake, steps) = parse(&path);
        let matched = replay(&seed, &deck, stake, &steps);
        // Not silently zero: a trace that matched nothing is not evidence.
        assert!(
            matched > 0,
            "seed {} matched zero steps -- the fixture is not being exercised",
            seed
        );
        assert_eq!(
            matched,
            steps.len(),
            "seed {} stopped early rather than replaying its whole trace",
            seed
        );
        let seed_ante = steps.iter().map(|s| s.ante).max().unwrap_or(0);
        deepest_ante = deepest_ante.max(seed_ante);
        eprintln!(
            "flow fixture: seed {} matched {} steps, deepest ante {}",
            seed, matched, seed_ante
        );
        matched_total += matched;
    }
    eprintln!(
        "flow fixture: {} steps matched across {} seeds, deepest ante {}",
        matched_total,
        SEEDS.len(),
        deepest_ante
    );
    // The weak policy this replaced matched only 82 steps across 6 seeds, dying
    // in ante 1-2. A floor near the measured 2166 is what "the run went deep
    // enough to reach the shop, the packs and the consumables" looks like.
    assert!(
        matched_total >= 2000,
        "only {} steps matched across {} seeds -- the policy stopped exercising \
         the run (was 82 across 6 before the policy was strengthened)",
        matched_total,
        SEEDS.len()
    );
    assert!(
        deepest_ante >= 4,
        "deepest ante reached was {} -- no run got past the early blinds",
        deepest_ante
    );
}
