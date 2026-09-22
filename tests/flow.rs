//! End-to-end flow tests for the run state machine.
//!
//! A deterministic policy drives whole runs, so the state machine is exercised
//! the way the environment uses it: enumerating `legal_actions()`, taking one,
//! and stepping. The preview test proves the previews are read-only, which the
//! environment depends on and which a single un-restored counter would silently
//! break.

use jimbot_sim::game::{Action, ActionType, GameState, Phase};

/// Always play the highest-scoring subset, else the first legal action.
pub fn choose(game: &mut GameState) -> Action {
    let acts = game.legal_actions();
    let plays: Vec<Action> = acts
        .iter()
        .filter(|a| a.r#type == ActionType::Play)
        .cloned()
        .collect();
    if plays.is_empty() {
        // Skip where the run can, so the trace reaches the tags and the packs
        // rather than falling at the first blind it cannot beat.
        if game.phase == Phase::BlindSelect {
            if let Some(skip) = acts.iter().find(|a| a.r#type == ActionType::SkipBlind) {
                return skip.clone();
            }
        }
        return acts[0].clone();
    }
    let mut best = plays[0].clone();
    let mut best_score = game.preview_score(&best.cards, "roll");
    for action in &plays[1..] {
        let score = game.preview_score(&action.cards, "roll");
        if score > best_score {
            best_score = score;
            best = action.clone();
        }
    }
    best
}

pub fn card_labels(game: &GameState) -> Vec<String> {
    let mut labels: Vec<String> = game
        .hand
        .iter()
        .map(|c| {
            format!(
                "{}{}",
                jimbot_sim::cards::rank_of(c).short(),
                jimbot_sim::cards::suit_of(c).as_str()
            )
        })
        .collect();
    labels.sort();
    labels
}

/// A fingerprint of everything a preview must not move.
pub fn fingerprint(game: &GameState) -> String {
    let mut out = String::new();
    out.push_str(&format!("money={}\n", game.money));
    out.push_str(&format!("chips={}\n", game.chips_scored));
    out.push_str(&format!("ante={}\n", game.ante));
    out.push_str(&format!("blind_index={}\n", game.blind_index));
    out.push_str(&format!("cards_created={}\n", game.cards_created));
    out.push_str(&format!("tarots={}\n", game.tarots_used));
    out.push_str(&format!("planets={}\n", game.planets_used));
    out.push_str(&format!("preview_expected={:?}\n", game.preview_expected));
    out.push_str(&format!("hand={:?}\n", card_labels(game)));
    out.push_str(&format!(
        "blind_triggered={}\n",
        game.blind.as_ref().map(|b| b.triggered).unwrap_or(false)
    ));
    let mut planets: Vec<String> = game.unique_planets.iter().cloned().collect();
    planets.sort();
    out.push_str(&format!("unique_planets={:?}\n", planets));
    let mut levels: Vec<(String, i32)> = game
        .hand_levels
        .levels
        .iter()
        .map(|(h, l)| (h.label().to_string(), *l))
        .collect();
    levels.sort();
    for (hand, level) in levels {
        out.push_str(&format!("level {}={}\n", hand, level));
    }
    let mut plays: Vec<(String, i32)> = game
        .hand_levels
        .plays
        .iter()
        .map(|(h, p)| (h.label().to_string(), *p))
        .collect();
    plays.sort();
    for (hand, n) in plays {
        out.push_str(&format!("plays {}={}\n", hand, n));
    }
    out.push_str(&format!("consumables={}\n", game.consumables.len()));
    for (i, joker) in game.jokers.iter().enumerate() {
        let j = joker.borrow();
        out.push_str(&format!(
            "joker{}={} uid={} counter={:?} secondary={:?} debuffed={} named={:?} extra={:?} tally={} hands_at_create={} edition={:?}\n",
            i,
            j.name(),
            j.uid,
            j.counter,
            j.secondary,
            j.debuffed,
            j.named_hand,
            j.extra_sell_value,
            j.perish_tally,
            j.hands_at_create,
            j.edition
        ));
    }
    out.push_str("rng=");
    let state = game.rng.state();
    let mut keys: Vec<&String> = state.keys().collect();
    keys.sort();
    for key in keys {
        out.push_str(&format!("{}:{}|", key, state[key].to_bits()));
    }
    out.push('\n');
    out
}

/// Push the run into a round with cards in hand.
pub fn drive_to_playing(game: &mut GameState) {
    let mut steps = 0;
    while game.phase != Phase::Playing && !game.is_over() && steps < 64 {
        let action = choose(game);
        assert!(game.is_legal(&action), "policy offered an illegal action");
        game.step(&action);
        steps += 1;
    }
    assert_eq!(game.phase, Phase::Playing, "never reached the playing phase");
    assert!(!game.hand.is_empty(), "no cards dealt");
}

#[test]
fn a_full_run_terminates_and_advances() {
    let mut resolved_a_round = false;
    for seed in ["FLOW0001", "FLOW0002", "FLOW0003", "FLOW0004", "FLOW0005"] {
        let mut game = GameState::new(seed, "Red Deck", 1);
        let mut phases = std::collections::HashSet::new();
        let mut steps = 0;
        while !game.is_over() && steps < 4000 {
            let action = choose(&mut game);
            assert!(game.is_legal(&action), "chosen action was illegal");
            phases.insert(game.phase);
            game.step(&action);
            steps += 1;
        }
        assert!(game.is_over(), "seed {} did not terminate", seed);
        assert!(steps > 5, "seed {} ended after {} steps", seed, steps);
        assert!(
            phases.contains(&Phase::Playing),
            "seed {} never played a hand",
            seed
        );
        resolved_a_round |= phases.contains(&Phase::RoundEval) || phases.contains(&Phase::Shop);
    }
    assert!(resolved_a_round, "no seed ever resolved a round");
}

#[test]
fn legal_actions_are_never_illegal() {
    for seed in [
        "MASK0001", "MASK0002", "MASK0003", "MASK0004", "MASK0005", "MASK0006",
    ] {
        let mut game = GameState::new(seed, "Blue Deck", 3);
        let mut steps = 0;
        while !game.is_over() && steps < 400 {
            let actions = game.legal_actions();
            assert!(
                !actions.is_empty(),
                "seed {} offered no actions in phase {}",
                seed,
                game.phase.as_str()
            );
            for action in &actions {
                assert!(
                    game.is_legal(action),
                    "seed {} step {}: legal_actions offered {:?} which is_legal rejects",
                    seed,
                    steps,
                    action
                );
            }
            let action = choose(&mut game);
            assert!(game.is_legal(&action), "chosen action was illegal");
            game.step(&action);
            steps += 1;
        }
    }
}

#[test]
fn previews_are_read_only() {
    for seed in ["PREV0001", "PREV0002", "PREV0003"] {
        let mut game = GameState::new(seed, "Red Deck", 1);
        drive_to_playing(&mut game);
        // Put scaling jokers in the row so the previews run counters that a
        // careless restore would leave moved: Runner and Square Joker grow after a
        // hand, and both carry a counter the fingerprint reads.
        for name in ["Runner", "Square Joker", "Joker"] {
            let joker = jimbot_sim::jokers::make(name);
            game.gain_joker(&joker);
        }

        let before = fingerprint(&game);
        let n = game.hand.len();
        let mut sets: Vec<Vec<usize>> = vec![vec![0]];
        if n >= 2 {
            sets.push(vec![0, 1]);
        }
        if n >= 3 {
            sets.push(vec![0, 1, 2]);
        }

        for indices in &sets {
            // Every preview called twice: once is a coincidence, twice is a rule.
            for mode in ["roll", "pessimistic", "expected"] {
                let _ = game.preview_score(indices, mode);
                let _ = game.preview_score(indices, mode);
                let _ = game.preview_play(indices, mode);
                let _ = game.preview_play(indices, mode);
                let _ = game.preview_value(indices, mode);
                let _ = game.preview_value(indices, mode);
            }
            let _ = game.preview_outcome(indices);
            let _ = game.preview_outcome(indices);
            let _ = game.preview_money(indices);
            let _ = game.preview_money(indices);
        }

        let after = fingerprint(&game);
        assert_eq!(
            before, after,
            "seed {}: a preview mutated the run (see the differing lines)",
            seed
        );
    }
}

