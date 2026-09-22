//! The first hand at every stake, against Python.
//!
//! A teammate reported a stake-dependent deal divergence (`GameState(seed,
//! deck, stake)` + a round start deals a different hand in Rust than in Python
//! at stakes 4 and 8). The fuzz sweep already covers every deck and stakes 1..8
//! with exact per-seed digests and passes, so the report looked like a harness
//! artefact; this asks the narrow question directly and permanently.
//!
//!     /home/marcin/balatro_bot/.venv/bin/python tools/gen_stake_deal_fixture.py \
//!         > rust/jimbot_sim/tests/fixtures/stake_deal.txt
//!     cargo test -p jimbot_sim --offline --test stake_deal_fixture

use jimbot_sim::game::{Action, ActionType, GameState};

#[derive(Debug)]
struct Case {
    seed: String,
    deck: String,
    stake: i32,
    hand: String,
    rng: String,
}

fn rng_signature(game: &GameState) -> String {
    let state = game.rng.state();
    let mut keys: Vec<&String> = state.keys().collect();
    keys.sort();
    keys.iter()
        .map(|key| format!("{}:{}", key, state[*key].to_bits()))
        .collect::<Vec<String>>()
        .join("|")
}

fn load() -> Vec<Case> {
    let text = include_str!("fixtures/stake_deal.txt");
    let mut cases: Vec<Case> = Vec::new();
    let mut current: Option<Case> = None;
    for line in text.lines() {
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        let mut parts = line.split('\t');
        match parts.next() {
            Some("run") => {
                if let Some(case) = current.take() {
                    cases.push(case);
                }
                current = Some(Case {
                    seed: parts.next().expect("seed").to_string(),
                    deck: parts.next().expect("deck").to_string(),
                    stake: parts.next().expect("stake").parse().unwrap(),
                    hand: String::new(),
                    rng: String::new(),
                });
            }
            Some("hand") => {
                current.as_mut().expect("a run before hand").hand =
                    parts.next().unwrap_or("").to_string();
            }
            Some("rng") => {
                current.as_mut().expect("a run before rng").rng =
                    parts.next().unwrap_or("").to_string();
            }
            other => panic!("unknown fixture tag {:?}", other),
        }
    }
    if let Some(case) = current.take() {
        cases.push(case);
    }
    assert!(!cases.is_empty(), "the fixture is empty");
    cases
}

#[test]
fn the_first_hand_is_the_same_at_every_stake() {
    for case in load() {
        let mut game = GameState::new(&case.seed, &case.deck, case.stake);
        game.step(&Action::new(ActionType::SelectBlind));
        let hand = game
            .hand
            .iter()
            .map(|card| {
                let c = card.borrow();
                format!("{:?}{:?}", c.rank, c.suit)
            })
            .collect::<Vec<String>>()
            .join(" ");
        assert_eq!(
            hand, case.hand,
            "{} / {} stake {}: the dealt hand differs",
            case.seed, case.deck, case.stake
        );
        assert_eq!(
            rng_signature(&game),
            case.rng,
            "{} / {} stake {}: the RNG differs",
            case.seed,
            case.deck,
            case.stake
        );
    }
}