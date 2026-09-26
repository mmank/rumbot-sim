//! Face-down draws, against the game's own Lua.
//!
//! Every other differential fixture was recorded off the Python simulator,
//! which never turned a card over, so none of them can say which cards The
//! House, The Wheel, The Mark and The Fish deal face down -- or that The
//! Wheel's roll is a draw on a pool of its own, one per card dealt. This one
//! was recorded off the real game, booted headless by
//! `scripts/gen_facedown_fixture.py`: a face-down boss on the opening blind, a
//! fixed script of discards and plays, and after every step the hand in order
//! with the face-down cards starred, the joker row the same way (Amber Acorn),
//! and the `wheel` pool's state, compared here by its bits.
//!
//! The rows cover Pareidolia under The Mark (every card is a face card), Oops!
//! All 6s under The Wheel (one and two of them: 2 in 7, 4 in 7), Chicot
//! (nothing turns over and the Wheel never rolls), and Amber Acorn on rows of
//! one, two and five -- and beside Chicot, which leaves the row face up but
//! shuffled all the same.

use jimbot_sim::blinds::{boss_by_name, make_blind, BlindKind};
use jimbot_sim::cards::CardRef;
use jimbot_sim::game::{Action, ActionType, GameState};
use jimbot_sim::jokers::{make_ref, spec_or_panic, JokerInstance};

const FIXTURE: &str = include_str!("fixtures/facedown.txt");

fn joker_name(key: &str) -> &'static str {
    match key {
        "j_pareidolia" => "Pareidolia",
        "j_oops" => "Oops! All 6s",
        "j_chicot" => "Chicot",
        "j_joker" => "Joker",
        "j_greedy_joker" => "Greedy Joker",
        "j_lusty_joker" => "Lusty Joker",
        "j_wrathful_joker" => "Wrathful Joker",
        "j_gluttenous_joker" => "Gluttonous Joker",
        other => panic!("no name for {}", other),
    }
}

fn card_label(card: &CardRef) -> String {
    let c = card.borrow();
    format!(
        "{}{}{}",
        c.suit.as_str(),
        c.rank.short(),
        if c.face_down { "*" } else { "" }
    )
}

fn observe(game: &GameState) -> String {
    let hand: Vec<String> = game.hand.iter().map(card_label).collect();
    let jokers: Vec<String> = game
        .jokers
        .iter()
        .map(|j| {
            let j = j.borrow();
            format!(
                "{}{}",
                j.name().replace(' ', "_"),
                if j.face_down { "*" } else { "" }
            )
        })
        .collect();
    let jokers = if jokers.is_empty() {
        "-".to_string()
    } else {
        jokers.join(" ")
    };
    let wheel = match game.rng.state().get("wheel") {
        Some(state) => state.to_bits().to_string(),
        None => "-".to_string(),
    };
    format!("{} | {} | {}", hand.join(" "), jokers, wheel)
}

/// The recorded line, with the pool's `%.17g` read back to its bits.
fn expected(line: &str) -> String {
    let parts: Vec<&str> = line.splitn(3, " | ").collect();
    let wheel = match parts[2].trim() {
        "-" => "-".to_string(),
        text => text.parse::<f64>().unwrap().to_bits().to_string(),
    };
    format!("{} | {} | {}", parts[0].trim(), parts[1].trim(), wheel)
}

struct Case {
    header: String,
    steps: Vec<String>,
}

fn cases() -> Vec<Case> {
    let mut out: Vec<Case> = Vec::new();
    for line in FIXTURE.lines() {
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix("case ") {
            out.push(Case {
                header: rest.to_string(),
                steps: Vec::new(),
            });
        } else if let Some(rest) = line.strip_prefix("step ") {
            out.last_mut().unwrap().steps.push(rest.to_string());
        } else {
            panic!("unreadable fixture line: {}", line);
        }
    }
    out
}

fn start(header: &str) -> GameState {
    let f: Vec<&str> = header.split(' ').collect();
    let (seed, deck, stake, boss, jokers) = (f[0], f[1].replace('_', " "), f[2], f[4], f[5]);
    let mut game = GameState::new(seed, &deck, stake.parse().unwrap());
    if jokers != "-" {
        for key in jokers.split(',') {
            let joker = make_ref(JokerInstance::new(spec_or_panic(joker_name(key))));
            game.jokers.push(joker);
        }
    }
    // The harness puts the boss on the opening blind, out of reach.
    let effect = boss_by_name(&boss.replace('_', " ")).expect("a boss");
    let mut blind = make_blind(
        BlindKind::Boss,
        1,
        Some(effect),
        game.deck_config().ante_scaling,
        game.blind_scaling(),
        false,
    );
    blind.target = 999_999_999;
    blind.on_deck = true;
    game.blind = Some(blind);
    game.step(&Action::new(ActionType::SelectBlind));
    game
}

fn action(text: &str) -> Action {
    let (kind, picks) = text.split_once(' ').unwrap();
    let cards: Vec<usize> = picks
        .split(',')
        .map(|p| p.parse::<usize>().unwrap() - 1)
        .collect();
    match kind {
        "discard" => Action::with_cards(ActionType::Discard, cards),
        "play" => Action::with_cards(ActionType::Play, cards),
        other => panic!("no action {}", other),
    }
}

#[test]
fn face_down_draws_match_the_game() {
    let cases = cases();
    let mut steps = 0;
    let mut turned = 0;
    for case in &cases {
        let mut game = start(&case.header);
        for step in &case.steps {
            let (what, recorded) = step.split_once(" | ").unwrap();
            if what != "deal" {
                let act = action(what);
                assert!(
                    game.is_legal(&act),
                    "{}: {} is not legal here",
                    case.header,
                    what
                );
                game.step(&act);
            }
            let want = expected(recorded);
            let got = observe(&game);
            assert_eq!(
                got, want,
                "\ncase {}\nafter {}\n  game: {}\n  sim:  {}",
                case.header, what, want, got
            );
            turned += game.hand.iter().filter(|c| c.borrow().face_down).count();
            steps += 1;
        }
    }
    // The fixture's own totals, not a floor: 87 cases, and the recording
    // does turn cards over.
    assert_eq!(cases.len(), 87);
    assert_eq!(
        steps,
        FIXTURE.lines().filter(|l| l.starts_with("step ")).count()
    );
    assert!(turned > 0);
}
