//! Hand-evaluation and scoring tests, ported from the Python suite.
//!
//! Each `#[test]` mirrors one `def test_x()` under
//! `external/jimbot-sim/tests/`, grouped here by the file it came from. The
//! doc comments carry the reasoning the Python test recorded -- these were
//! written over months against real divergences, and the comment is often the
//! only statement of *why* the assertion has the shape it does.
//!
//! The port is "direct": the same position is rebuilt with `GameState` and the
//! same numbers asserted, except where noted. The Python tests that drive a
//! listed probability use the run's own bit-exact stream (the outcome at seed 0
//! is the same in both engines); where Python monkeypatches `rng.chance`, the
//! Rust test instead checks `RunRng::state()`, which names the same pool.
//!
//! `test_id_ranking.py` pins `ranked`/`differences`, which live in the
//! out-of-scope `jimbot_sim.replay` driver, not the engine; they are
//! reimplemented here as test-local helpers so the assertions survive the port.

use std::collections::BTreeMap;
use std::rc::Rc;

use rumbot_sim::blinds::{boss_by_name, make_blind, BlindKind};
use rumbot_sim::cards::{
    label_of, make_card, uid_of, CardRef, Edition, Enhancement, Rank, Seal, Suit,
};
use rumbot_sim::consumables;
use rumbot_sim::effects::ScoreContext;
use rumbot_sim::game::{Action, ActionType, GameState};
use rumbot_sim::hands::{evaluate, EvalFlags, HandLevels, HandType};
use rumbot_sim::jokers::{self, JokerInstance};
use rumbot_sim::scoring::{effective_specs, score_hand};

const S: Suit = Suit::Spades;
const H: Suit = Suit::Hearts;
const D: Suit = Suit::Diamonds;
const C: Suit = Suit::Clubs;

fn card(rank: Rank, suit: Suit) -> CardRef {
    make_card(rank, suit)
}

fn plain_flags() -> EvalFlags {
    EvalFlags::default()
}

/// A Stone card, which the game answers with a random negative id and no suit.
fn stone_card(rank: Rank, suit: Suit) -> CardRef {
    let card = make_card(rank, suit);
    card.borrow_mut().enhancement = Enhancement::Stone;
    card
}

/// `play(...)` from test_scoring.py: a fresh run, these jokers, this hand, and
/// the score of the played cards.
fn play(cards: &[CardRef], joker_names: &[&str], held: &[CardRef], seed: i32) -> ScoreContext {
    let mut game = GameState::new(seed, "Red Deck", 1);
    game.jokers = joker_names.iter().map(|n| jokers::make(n)).collect();
    game.hand = cards.iter().chain(held.iter()).cloned().collect();
    let result = evaluate(cards, plain_flags());
    score_hand(&mut game, &result, cards, held)
}

// ==========================================================================
// tests/test_scoring.py -- "Scoring checks against values a Balatro player
// can verify by hand."
// ==========================================================================

#[test]
fn test_pair_of_kings() {
    // Pair: 10 chips x 2 mult, plus two 10-chip Kings -> 30 x 2 = 60
    let ctx = play(&[card(Rank::King, S), card(Rank::King, H)], &[], &[], 0);
    assert_eq!((ctx.chips, ctx.mult), (30.0, 2.0));
    assert_eq!(ctx.score(), 60);
}

#[test]
fn test_flush_of_low_hearts() {
    // Flush: 35 x 4, cards 2+4+6+8+10 = 30 -> 65 x 4 = 260
    let cards: Vec<CardRef> = [Rank::Two, Rank::Four, Rank::Six, Rank::Eight, Rank::Ten]
        .iter()
        .map(|r| card(*r, H))
        .collect();
    let ctx = play(&cards, &[], &[], 0);
    assert_eq!(ctx.score(), 260);
}

#[test]
fn test_plain_joker_adds_four_mult() {
    let ctx = play(
        &[card(Rank::King, S), card(Rank::King, H)],
        &["Joker"],
        &[],
        0,
    );
    assert_eq!(ctx.score(), 30 * 6);
}

#[test]
fn test_xmult_applies_after_plus_mult_in_slot_order() {
    // XMult is not commutative with +Mult, and the row is walked left to right.
    let cards = [card(Rank::King, S), card(Rank::King, H)];
    let plus_then_x = play(&cards, &["Joker", "The Duo"], &[], 0);
    let x_then_plus = play(&cards, &["The Duo", "Joker"], &[], 0);
    assert_eq!(plus_then_x.score(), 30 * ((2 + 4) * 2)); // 360
    assert_eq!(x_then_plus.score(), 30 * ((2 * 2) + 4)); // 240
}

#[test]
fn test_steel_card_held_in_hand() {
    let steel = card(Rank::Two, D);
    steel.borrow_mut().enhancement = Enhancement::Steel;
    let ctx = play(
        &[card(Rank::King, S), card(Rank::King, H)],
        &[],
        &[steel],
        0,
    );
    assert_eq!(ctx.score(), (30.0 * 2.0 * 1.5) as i64);
}

#[test]
fn test_bonus_and_mult_enhancements() {
    let bonus = card(Rank::King, S);
    bonus.borrow_mut().enhancement = Enhancement::Bonus;
    let mult = card(Rank::King, H);
    mult.borrow_mut().enhancement = Enhancement::Mult;
    let ctx = play(&[bonus, mult], &[], &[], 0);
    assert_eq!((ctx.chips, ctx.mult), (30.0 + 30.0, 2.0 + 4.0));
}

#[test]
fn test_editions_on_cards() {
    let foil = card(Rank::King, S);
    foil.borrow_mut().edition = Edition::Foil;
    let holo = card(Rank::King, H);
    holo.borrow_mut().edition = Edition::Holographic;
    let ctx = play(&[foil, holo], &[], &[], 0);
    assert_eq!((ctx.chips, ctx.mult), (30.0 + 50.0, 2.0 + 10.0));
}

#[test]
fn test_red_seal_retriggers_the_card() {
    let plain = play(&[card(Rank::King, S), card(Rank::King, H)], &[], &[], 0);
    let sealed_king = card(Rank::King, S);
    sealed_king.borrow_mut().seal = Seal::Red;
    let sealed = play(&[sealed_king, card(Rank::King, H)], &[], &[], 0);
    assert_eq!(sealed.chips, plain.chips + 10.0);
}

#[test]
fn test_scored_card_hook_only_sees_scoring_cards() {
    // The 3 does not score in a pair, so Odd Todd must not count it.
    let cards = [
        card(Rank::King, S),
        card(Rank::King, H),
        card(Rank::Three, D),
    ];
    let ctx = play(&cards, &["Odd Todd"], &[], 0);
    assert_eq!(ctx.chips, 30.0);
}

#[test]
fn test_blueprint_copies_the_joker_to_its_right() {
    let jokers = vec![jokers::make("Blueprint"), jokers::make("The Duo")];
    // effective_specs answers both halves: which ability runs, and whose state
    // it runs on. The Blueprint runs The Duo's ability against The Duo itself,
    // which is what the game does -- other_joker:calculate_joker(context).
    let resolved = effective_specs(&jokers);
    let names: Vec<&str> = resolved.iter().map(|(spec, _)| spec.name).collect();
    assert_eq!(names, vec!["The Duo", "The Duo"]);
    let sourced: Vec<bool> = resolved
        .iter()
        .map(|(_, src)| Rc::ptr_eq(src, &jokers[1]))
        .collect();
    assert_eq!(sourced, vec![true, true]);
    let ctx = play(
        &[card(Rank::King, S), card(Rank::King, H)],
        &["Blueprint", "The Duo"],
        &[],
        0,
    );
    assert_eq!(ctx.score(), 30 * 2 * 2 * 2);
}

#[test]
fn test_blueprint_with_nothing_to_the_right_does_nothing() {
    let ctx = play(
        &[card(Rank::King, S), card(Rank::King, H)],
        &["The Duo", "Blueprint"],
        &[],
        0,
    );
    assert_eq!(ctx.score(), 30 * 2 * 2);
}

#[test]
fn test_brainstorm_copies_the_leftmost_joker() {
    let jokers = vec![
        jokers::make("The Duo"),
        jokers::make("Joker"),
        jokers::make("Brainstorm"),
    ];
    let resolved = effective_specs(&jokers);
    let names: Vec<&str> = resolved.iter().map(|(spec, _)| spec.name).collect();
    assert_eq!(names, vec!["The Duo", "Joker", "The Duo"]);
    // And the Brainstorm reads the leftmost joker's state, not its own.
    assert!(Rc::ptr_eq(&resolved[2].1, &jokers[0]));
}

#[test]
fn test_copier_cycle_terminates() {
    let jokers = vec![jokers::make("Brainstorm"), jokers::make("Blueprint")];
    effective_specs(&jokers); // must not hang
}

#[test]
fn test_baron_multiplies_per_held_king() {
    let ctx = play(
        &[card(Rank::Two, S), card(Rank::Two, H)],
        &["Baron"],
        &[card(Rank::King, D), card(Rank::King, C)],
        0,
    );
    assert_eq!(ctx.mult, 2.0 * 1.5 * 1.5);
}

#[test]
fn test_sock_and_buskin_retriggers_faces() {
    let ctx = play(
        &[card(Rank::King, S), card(Rank::King, H)],
        &["Sock and Buskin", "Scary Face"],
        &[],
        0,
    );
    // base 10, then each King triggers twice for 10 card chips + 30 from Scary Face
    assert_eq!(ctx.chips, (10 + 2 * 2 * (10 + 30)) as f64);
}

#[test]
fn test_hand_level_feeds_the_base_values() {
    let mut game = GameState::new(0, "Red Deck", 1);
    let cards = [card(Rank::King, S), card(Rank::King, H)];
    game.hand_levels.level_up(HandType::Pair, 1);
    let result = evaluate(&cards, plain_flags());
    let ctx = score_hand(&mut game, &result, &cards, &[]);
    assert_eq!((ctx.chips, ctx.mult), (25.0 + 20.0, 3.0));
}

#[test]
fn test_blackboard_counts_a_debuffed_spade_as_black() {
    // The game asks its suit question two ways (card.lua:4064).
    //
    // `is_suit(suit)` refuses a debuffed card outright; `is_suit(suit, nil,
    // true)` -- the flush_calc branch Blackboard uses -- reads the printed suit
    // anyway, and only a wild card loses its everything-suit to a debuff. So
    // The Goad debuffs the Queen of Spades held in hand and Blackboard still
    // counts the hand black. The simulator refused it, which took a flush from
    // the game's 6960 to 2320 on the hand that cleared the blind.
    let game = GameState::new("TESTSEED", "Red Deck", 1);
    let queen = card(Rank::Queen, S);
    queen.borrow_mut().debuffed = true;
    assert!(jokers::counts_for_flush(&queen, S, &game));
    assert!(!jokers::counts_for_flush(&queen, H, &game));
    // A debuffed wild card loses its everything-suit; a stone card never had
    // a suit to lose.
    let wild = card(Rank::Five, H);
    wild.borrow_mut().enhancement = Enhancement::Wild;
    assert!(jokers::counts_for_flush(&wild, S, &game));
    wild.borrow_mut().debuffed = true;
    assert!(!jokers::counts_for_flush(&wild, S, &game));
    let stone = stone_card(Rank::Five, S);
    assert!(!jokers::counts_for_flush(&stone, S, &game));
}

// ==========================================================================
// tests/test_hands.py -- hand detection and the level table.
// ==========================================================================

#[test]
fn test_hand_detection() {
    let cases: [(&[(Rank, Suit)], HandType); 8] = [
        (
            &[
                (Rank::Two, S),
                (Rank::Three, S),
                (Rank::Four, S),
                (Rank::Five, S),
                (Rank::Six, S),
            ],
            HandType::StraightFlush,
        ),
        (
            &[
                (Rank::Ace, S),
                (Rank::Two, H),
                (Rank::Three, S),
                (Rank::Four, S),
                (Rank::Five, S),
            ],
            HandType::Straight,
        ),
        (
            &[
                (Rank::Ten, S),
                (Rank::Jack, H),
                (Rank::Queen, S),
                (Rank::King, S),
                (Rank::Ace, S),
            ],
            HandType::Straight,
        ),
        (
            &[
                (Rank::King, S),
                (Rank::King, H),
                (Rank::King, D),
                (Rank::Two, S),
                (Rank::Two, H),
            ],
            HandType::FullHouse,
        ),
        (
            &[
                (Rank::King, S),
                (Rank::King, H),
                (Rank::King, D),
                (Rank::King, C),
                (Rank::Two, H),
            ],
            HandType::FourOfAKind,
        ),
        (
            &[
                (Rank::Nine, S),
                (Rank::Nine, H),
                (Rank::Three, S),
                (Rank::Three, H),
                (Rank::Two, S),
            ],
            HandType::TwoPair,
        ),
        (
            &[
                (Rank::Two, H),
                (Rank::Five, H),
                (Rank::Nine, H),
                (Rank::Jack, H),
                (Rank::King, H),
            ],
            HandType::Flush,
        ),
        (&[(Rank::King, S), (Rank::Queen, H)], HandType::HighCard),
    ];
    for (specs, expected) in cases {
        let cards: Vec<CardRef> = specs.iter().map(|(r, s)| card(*r, *s)).collect();
        assert_eq!(
            evaluate(&cards, plain_flags()).hand,
            expected,
            "on {:?}",
            specs
        );
    }
}

#[test]
fn test_five_of_a_kind_needs_a_duplicate_deck() {
    let cards: Vec<CardRef> = (0..5).map(|_| card(Rank::King, S)).collect();
    assert_eq!(evaluate(&cards, plain_flags()).hand, HandType::FlushFive);
}

#[test]
fn test_wild_card_completes_a_flush() {
    let mut cards = vec![
        card(Rank::Two, H),
        card(Rank::Five, H),
        card(Rank::Nine, H),
        card(Rank::Jack, H),
    ];
    let wild = card(Rank::King, S);
    wild.borrow_mut().enhancement = Enhancement::Wild;
    cards.push(wild);
    assert_eq!(evaluate(&cards, plain_flags()).hand, HandType::Flush);
}

#[test]
fn test_four_fingers_allows_four_card_flush() {
    let cards = vec![
        card(Rank::Two, H),
        card(Rank::Five, H),
        card(Rank::Nine, H),
        card(Rank::Jack, H),
        card(Rank::King, S),
    ];
    assert_eq!(evaluate(&cards, plain_flags()).hand, HandType::HighCard);
    let four = EvalFlags {
        four_fingers: true,
        ..EvalFlags::default()
    };
    assert_eq!(evaluate(&cards, four).hand, HandType::Flush);
}

#[test]
fn test_shortcut_allows_gapped_straight() {
    let cards = vec![
        card(Rank::Two, S),
        card(Rank::Four, H),
        card(Rank::Six, S),
        card(Rank::Eight, D),
        card(Rank::Ten, C),
    ];
    assert_eq!(evaluate(&cards, plain_flags()).hand, HandType::HighCard);
    let shortcut = EvalFlags {
        shortcut: true,
        ..EvalFlags::default()
    };
    assert_eq!(evaluate(&cards, shortcut).hand, HandType::Straight);
}

#[test]
fn test_only_the_pair_scores() {
    let cards = vec![
        card(Rank::Nine, S),
        card(Rank::Nine, H),
        card(Rank::Three, S),
        card(Rank::Four, H),
        card(Rank::Two, S),
    ];
    let result = evaluate(&cards, plain_flags());
    assert_eq!(result.hand, HandType::Pair);
    let ranks: Vec<Rank> = result.scoring.iter().map(|c| c.borrow().rank).collect();
    assert_eq!(ranks, vec![Rank::Nine, Rank::Nine]);
}

#[test]
fn test_stone_cards_always_score() {
    let mut cards = vec![
        card(Rank::Nine, S),
        card(Rank::Nine, H),
        card(Rank::Three, S),
    ];
    cards.push(stone_card(Rank::Two, S));
    let result = evaluate(&cards, plain_flags());
    assert_eq!(result.hand, HandType::Pair);
    assert_eq!(result.scoring.len(), 3);
}

#[test]
fn test_hand_levels_scale() {
    let mut levels = HandLevels::new();
    assert_eq!(levels.values(HandType::Pair), (10, 2));
    levels.level_up(HandType::Pair, 1);
    assert_eq!(levels.values(HandType::Pair), (25, 3));
    levels.level_up(HandType::Pair, 2);
    assert_eq!(levels.values(HandType::Pair), (55, 5));
}

// ==========================================================================
// tests/test_id_ranking.py -- "Card ids are compared by order, not by value."
//
// These pin `ranked`/`differences` from `jimbot_sim.replay`, a driver the port
// explicitly leaves out of scope and so has no Rust home. The helpers below are
// the same pure functions reimplemented as test-local code, so the assertions
// still hold if the driver is ever ported. The reasoning is the Python file's:
//
// A card's id is its place in the run's card counter, and that counter moves
// for things a recording cannot contain -- opening the deck collection screen
// builds fifty-two Card objects behind the deck art, and looking at a menu is
// not a game action. So the same card ends up under a different number. What
// survives is the order: both counters only go up, and the extra cards appear
// at one moment on one side, so a card made earlier keeps a lower id than one
// made later on both sides. Ranking each list against itself drops the offset
// and keeps that. Ranking is not ignoring: a card genuinely in the wrong place
// still moves a rank and is still caught.
// ==========================================================================

/// A comparison field: an id list or a plain string, as `normalise` yields.
#[derive(Clone, PartialEq, Debug)]
enum Field {
    Text(String),
    Ints(Vec<i64>),
}

/// `ranked(ids)`: replace each id by its place in the sorted list of ids.
fn ranked_ids(ids: &[i64]) -> Vec<i64> {
    let mut sorted = ids.to_vec();
    sorted.sort();
    let mut place = BTreeMap::new();
    for (i, value) in sorted.iter().enumerate() {
        place.insert(*value, i as i64);
    }
    ids.iter().map(|value| place[value]).collect()
}

fn rank_field(field: &Field) -> Field {
    match field {
        Field::Ints(ids) => Field::Ints(ranked_ids(ids)),
        other => other.clone(),
    }
}

/// A Python value `ranked` might be handed, which it leaves alone unless every
/// element is an int.
#[derive(Clone, PartialEq, Debug)]
enum Ranked {
    Ints(Vec<i64>),
    Strs(Vec<String>),
    Null,
}

fn ranked(value: &Ranked) -> Ranked {
    match value {
        Ranked::Ints(ids) => Ranked::Ints(ranked_ids(ids)),
        other => other.clone(),
    }
}

const SCORE_STABLE_PHASES: [&str; 3] = ["SELECTING_HAND", "HAND_PLAYED", "ROUND_EVAL"];
const ID_FIELDS: [&str; 2] = ["hand_ids", "joker_ids"];
const COMPARED: [&str; 19] = [
    "phase",
    "dollars",
    "chips",
    "ante",
    "round",
    "hands_left",
    "discards_left",
    "blind",
    "blind_chips",
    "hand_size",
    "jokers",
    "consumables",
    "hand_levels",
    "deck_size",
    "hands_played",
    "last_hand",
    "hand_ids",
    "tags",
    "joker_ids",
];

/// `differences(recorded, snapshot)`: where a backend's snapshot differs from
/// what the player saw, comparing id fields by rank.
fn differences(recorded: &BTreeMap<&str, Field>, engine: &BTreeMap<&str, Field>) -> Vec<String> {
    let mut out = Vec::new();
    let scoring = matches!(
        recorded.get("phase"),
        Some(Field::Text(p)) if SCORE_STABLE_PHASES.contains(&p.as_str())
    );
    for name in COMPARED {
        if name == "chips" && recorded.contains_key("phase") && !scoring {
            continue;
        }
        let (want, got) = match (recorded.get(name), engine.get(name)) {
            (Some(a), Some(b)) => (a.clone(), b.clone()),
            _ => continue,
        };
        let (want, got) = if ID_FIELDS.contains(&name) {
            (rank_field(&want), rank_field(&got))
        } else {
            (want, got)
        };
        if want != got {
            out.push(format!("{name}: recorded {want:?} but replayed {got:?}"));
        }
    }
    out
}

#[test]
fn test_a_constant_offset_disappears() {
    assert_eq!(
        ranked_ids(&[68, 53, 54, 136]),
        ranked_ids(&[68, 53, 54, 83])
    );
}

#[test]
fn test_the_order_is_what_is_kept() {
    assert_eq!(ranked_ids(&[68, 53, 54, 136]), vec![2, 0, 1, 3]);
}

#[test]
fn test_a_card_out_of_place_is_still_caught() {
    // The same ids in a different order must not compare equal.
    assert_ne!(ranked_ids(&[53, 54, 68]), ranked_ids(&[54, 53, 68]));
}

#[test]
fn test_a_missing_card_is_still_caught() {
    assert_ne!(ranked_ids(&[53, 54, 68, 83]), ranked_ids(&[53, 54, 68]));
}

#[test]
fn test_a_swapped_pair_of_new_cards_is_still_caught() {
    // Two cards created during the run are not interchangeable.
    //
    // Blanking drifted ids to a placeholder lost this: both became "new" and a
    // hand holding them the other way round compared equal.
    assert_ne!(
        ranked_ids(&[4, 17, 171, 172]),
        ranked_ids(&[4, 17, 119, 118])
    );
}

#[test]
fn test_non_numeric_lists_are_left_alone() {
    let names = Ranked::Strs(vec!["j_joker".into(), "j_banner".into()]);
    assert_eq!(ranked(&names), names);
    assert_eq!(ranked(&Ranked::Null), Ranked::Null);
    // And the branch that does rank: an int list comes back by order.
    assert_eq!(
        ranked(&Ranked::Ints(vec![68, 53, 54, 136])),
        Ranked::Ints(vec![2, 0, 1, 3])
    );
}

#[test]
fn test_differences_uses_the_ranking() {
    let phase = || Field::Text("SHOP".into());
    let drifted = BTreeMap::from([
        ("phase", phase()),
        ("joker_ids", Field::Ints(vec![68, 53, 54, 136])),
    ]);
    let engine = BTreeMap::from([
        ("phase", phase()),
        ("joker_ids", Field::Ints(vec![68, 53, 54, 83])),
    ]);
    assert_eq!(differences(&drifted, &engine), Vec::<String>::new());

    let reordered = BTreeMap::from([
        ("phase", phase()),
        ("joker_ids", Field::Ints(vec![53, 68, 54, 136])),
    ]);
    assert!(
        !differences(&reordered, &engine).is_empty(),
        "a reordering slipped through"
    );
}

// ==========================================================================
// tests/test_is_versus_contains.py -- "Is a Flush" and "contains a Flush" are
// different questions.
//
// Balatro asks both, in different places, and reading the wrong one is silent:
// the joker simply never fires, or fires on hands it should not. The game keeps
// two separate things after a hand is evaluated --
//
//     context.scoring_name   the single hand the play counts as
//     context.poker_hands    a table of every hand the cards contain
//
// -- and each joker reads exactly one of them. The containment table is not the
// obvious closure of the hand rankings: it is built from groups of an *exact*
// size -- get_X_same(3, hand) skips a rank with four or five cards -- and then a
// cascade is patched on the end: Five of a Kind counts as Four of a Kind, which
// counts as Three of a Kind, which counts as a Pair. Nothing in that chain
// reaches Two Pair.
// ==========================================================================

fn ivc_hand(codes: &str) -> Vec<CardRef> {
    codes
        .split_whitespace()
        .map(|code| {
            let (suit, rank) = code.split_once('_').expect("suit_rank");
            card(
                Rank::from_code(rank).unwrap_or_else(|| panic!("bad rank {code:?}")),
                Suit::from_code(suit).unwrap_or_else(|| panic!("bad suit {code:?}")),
            )
        })
        .collect()
}

/// `{h.label for h in result.contains} | {result.hand.label}`.
fn ivc_contains(codes: &str) -> BTreeMap<String, ()> {
    let result = evaluate(&ivc_hand(codes), plain_flags());
    let mut set: BTreeMap<String, ()> = result
        .contains
        .iter()
        .map(|h| (h.label().to_string(), ()))
        .collect();
    set.insert(result.hand.label().to_string(), ());
    set
}

fn has(set: &BTreeMap<String, ()>, label: &str) -> bool {
    set.contains_key(label)
}

/// Engine, evaluate_poker_hand on the played cards, one row per hand.
const MEASURED: [(&str, &[&str]); 12] = [
    (
        "S_K H_K D_K C_K S_2",
        &["Four of a Kind", "Three of a Kind", "Pair", "High Card"],
    ),
    (
        "S_K H_K D_K C_K H_K",
        &[
            "Five of a Kind",
            "Four of a Kind",
            "Three of a Kind",
            "Pair",
            "High Card",
        ],
    ),
    (
        "S_K H_K D_K C_Q S_Q",
        &[
            "Full House",
            "Two Pair",
            "Three of a Kind",
            "Pair",
            "High Card",
        ],
    ),
    (
        "S_K S_K S_K S_Q S_Q",
        &[
            "Flush House",
            "Full House",
            "Two Pair",
            "Three of a Kind",
            "Pair",
            "Flush",
            "High Card",
        ],
    ),
    (
        "S_K S_K S_K S_K S_K",
        &[
            "Flush Five",
            "Five of a Kind",
            "Four of a Kind",
            "Three of a Kind",
            "Pair",
            "Flush",
            "High Card",
        ],
    ),
    ("S_K H_K D_Q C_Q S_2", &["Two Pair", "Pair", "High Card"]),
    (
        "S_K H_K D_K C_Q S_2",
        &["Three of a Kind", "Pair", "High Card"],
    ),
    (
        "S_9 S_8 S_7 S_6 S_5",
        &["Straight Flush", "Flush", "Straight", "High Card"],
    ),
    ("S_K S_K S_9 S_6 S_3", &["Flush", "Pair", "High Card"]),
    ("S_9 H_8 D_7 C_6 S_5", &["Straight", "High Card"]),
    ("S_K H_K D_9 C_6 S_3", &["Pair", "High Card"]),
    ("S_K H_9 D_7 C_5 S_3", &["High Card"]),
];

#[test]
fn test_the_containment_table_matches_the_engine() {
    for (codes, expected) in MEASURED {
        let got = ivc_contains(codes);
        let want: BTreeMap<String, ()> = expected.iter().map(|l| (l.to_string(), ())).collect();
        assert_eq!(got, want, "on {codes}");
    }
}

#[test]
fn test_four_of_a_kind_contains_a_pair_but_not_two_pair() {
    // The cascade stops one rung short, and the shape of it is the reason.
    // Four cards of a rank make no *pair* group at all -- get_X_same(2) wants
    // exactly two -- so Pair arrives only through the patched chain from Four
    // of a Kind down. Two Pair is not on that chain, and it needs two separate
    // groups, which four of one rank cannot supply.
    let held = ivc_contains("S_K H_K D_K C_K S_2");
    assert!(has(&held, "Pair"));
    assert!(has(&held, "Three of a Kind"));
    assert!(!has(&held, "Two Pair"));
}

#[test]
fn test_five_of_a_kind_is_not_a_full_house() {
    // Reading "at least three" instead of "exactly three" makes it one.
    // Three of the five plus two of the same five looks like a full house to
    // any closure over the rankings, and the game never counts it: both halves
    // would be the same rank. This was the simulator's bug.
    let held = ivc_contains("S_K H_K D_K C_K H_K");
    assert!(!has(&held, "Full House"));
    assert!(!has(&ivc_contains("S_K S_K S_K S_K S_K"), "Flush House"));
}

#[test]
fn test_a_flush_holding_a_pair_contains_a_pair() {
    // The case that a list of top hands cannot express.
    assert!(has(&ivc_contains("S_K S_K S_9 S_6 S_3"), "Pair"));
}

// ------------------------------------------------------------------ the
// jokers that read one table or the other ---------------------------------

fn ivc_score(hand_codes: &str, joker: &str) -> (i64, i64) {
    let mut game = GameState::new("TESTSEED", "Red Deck", 1);
    game._start_round();
    game.hand = ivc_hand(hand_codes);
    let idx: Vec<usize> = (0..game.hand.len()).collect();
    let plain = game.preview_score(&idx, "roll");
    game.gain_joker(&jokers::make(joker));
    let with_joker = game.preview_score(&idx, "roll");
    (plain, with_joker)
}

#[test]
fn test_a_contains_joker_fires_on_a_hand_that_merely_holds_it() {
    let cases = [
        ("Jolly Joker", "S_K S_K S_9 S_6 S_3"), // a Flush holding a pair
        ("Sly Joker", "S_K S_K S_9 S_6 S_3"),
        ("The Duo", "S_K S_K S_9 S_6 S_3"),
        ("Mad Joker", "S_K H_K D_K C_Q S_Q"), // a Full House holds two pair
        ("Clever Joker", "S_K H_K D_K C_Q S_Q"),
    ];
    for (joker, codes) in cases {
        let (plain, with_joker) = ivc_score(codes, joker);
        assert!(
            with_joker > plain,
            "{joker} did not fire on a hand that contains its hand"
        );
    }
}

#[test]
fn test_a_two_pair_joker_does_not_fire_on_four_of_a_kind() {
    // Because a Four of a Kind does not contain Two Pair. See above.
    for joker in ["Mad Joker", "Clever Joker"] {
        let (plain, with_joker) = ivc_score("S_K H_K D_K C_K S_2", joker);
        assert_eq!(with_joker, plain, "{joker} fired on a Four of a Kind");
    }
}

#[test]
fn test_seance_reads_the_containment_table_like_the_rest() {
    // `next(context.poker_hands[...])`, not a test of what the hand is. The two
    // agree on anything vanilla can make -- nothing outranking a Straight Flush
    // contains one -- but the shape has to be right.
    let mut game = GameState::new("TESTSEED", "Red Deck", 1);
    game._start_round();
    game.hand = ivc_hand("S_9 S_8 S_7 S_6 S_5");
    let result = evaluate(&game.hand, plain_flags());
    let mut held = result.contains;
    held.insert(result.hand);
    assert!(held.contains(HandType::StraightFlush));
    assert!(jokers::spec("Séance").unwrap().after_hand.is_some());
}

// ------------------------------------------------------------------
// The Ox, which reads a snapshot rather than a live count
// ------------------------------------------------------------------

#[test]
fn test_the_most_played_hand_starts_at_high_card() {
    // Engine: 'High Card' at run start, and it does not move on plays.
    let game = GameState::new("TESTSEED", "Red Deck", 1);
    assert_eq!(game.most_played_hand, HandType::HighCard);
}

#[test]
fn test_playing_a_hand_does_not_move_the_snapshot() {
    // Engine: twenty-five Pairs played, still 'High Card'.
    //
    // It is rewritten when a boss round ends and nowhere else, so The Ox
    // punishes what the run was doing an ante ago.
    let mut game = GameState::new("TESTSEED", "Red Deck", 1);
    game.hand_levels.plays.insert(HandType::Pair, 25);
    assert_eq!(game.most_played_hand, HandType::HighCard);
}

#[test]
fn test_the_snapshot_is_taken_when_a_boss_falls() {
    let mut game = GameState::new("TESTSEED", "Red Deck", 1);
    game.hand_levels.plays.insert(HandType::Pair, 25);
    game.blind_index = 2;
    game._next_blind();
    game._start_round();
    game.chips_scored = game.blind.as_ref().unwrap().target;
    game._beat_blind(true);
    assert_eq!(game.most_played_hand, HandType::Pair);
}

#[test]
fn test_a_tie_goes_to_the_weaker_hand() {
    // Engine: Pair and Flush both on 25, and it picked Pair.
    //
    // `_order` is initialised to 100 and never assigned, so every tie replaces
    // the incumbent and the last hand `pairs` yields wins. That order runs
    // strongest-first, so the weakest of the tied hands is left standing.
    let mut game = GameState::new("TESTSEED", "Red Deck", 1);
    game.hand_levels.plays.insert(HandType::Pair, 25);
    game.hand_levels.plays.insert(HandType::Flush, 25);
    game._snapshot_most_played();
    assert_eq!(game.most_played_hand, HandType::Pair);

    game.hand_levels.plays.insert(HandType::Flush, 30);
    game._snapshot_most_played();
    assert_eq!(game.most_played_hand, HandType::Flush);
}

#[test]
fn test_the_ox_reads_the_snapshot_not_a_live_tie() {
    // Two hands level on plays used to mean either one zeroed your money.
    let mut game = GameState::new("TESTSEED", "Red Deck", 1);
    game.hand_levels.plays.insert(HandType::Pair, 25);
    game.hand_levels.plays.insert(HandType::Flush, 25);
    game._snapshot_most_played();
    assert!(game._is_most_played(HandType::Pair));
    assert!(!game._is_most_played(HandType::Flush));
}

// ==========================================================================
// tests/test_face_cards.py -- "Which cards are face cards: Card:is_face, as
// the game asks it."
//
// card.lua:964-970:
//
//     function Card:is_face(from_boss)
//         if self.debuff and not from_boss then return end
//         local id = self:get_id()
//         if id == 11 or id == 12 or id == 13 or next(find_joker("Pareidolia")) then
//             return true
//         end
//     end
//
// get_id answers a Stone card with `-math.random(100, 1000000)`, so a Stone
// King is no face card. The Pareidolia test never looks at the id, so with
// Pareidolia held every card is one, Stone included -- and find_joker leaves out
// a debuffed Pareidolia. A debuffed card is no face card, unless it is a boss
// asking. The simulator said a Stone card was never a face card, Pareidolia or
// not; let Midas Mask and Canio count a debuffed King; and had The Plant read
// the printed rank, debuffing a Stone King.
// ==========================================================================

fn face_game(joker_names: &[&str], cards: &[CardRef], boss: Option<&str>) -> GameState {
    let mut game = GameState::new(0, "Red Deck", 1);
    game.jokers = joker_names.iter().map(|n| jokers::make(n)).collect();
    if let Some(boss) = boss {
        game.blind = Some(make_blind(
            BlindKind::Boss,
            1,
            boss_by_name(boss),
            1.0,
            1,
            false,
        ));
    }
    game.hand = cards.to_vec();
    game.full_deck = cards.to_vec();
    game._apply_debuffs();
    game
}

fn face_score(game: &mut GameState, played: &[CardRef], held: &[CardRef]) -> ScoreContext {
    let result = game.evaluate_selection(played);
    score_hand(game, &result, played, held)
}

fn enhancements(cards: &[CardRef]) -> Vec<Enhancement> {
    cards.iter().map(|c| c.borrow().enhancement).collect()
}

#[test]
fn test_scary_face_pays_a_stone_card_only_beside_pareidolia() {
    // card.lua:3136-3137 `context.other_card:is_face()`. A Stone King and a
    // King of Hearts, High Card: 5 + 50 + 10 chips and 30 for each face.
    let played = vec![stone_card(Rank::King, S), card(Rank::King, H)];
    let mut alone = face_game(&["Scary Face"], &played, None);
    assert_eq!(face_score(&mut alone, &played, &[]).score(), 95);
    let mut beside = face_game(&["Pareidolia", "Scary Face"], &played, None);
    assert_eq!(face_score(&mut beside, &played, &[]).score(), 125);
}

#[test]
fn test_sock_and_buskin_retriggers_a_stone_card_beside_pareidolia() {
    // card.lua:3344-3345: the Stone card's 50 chips come twice.
    let played = vec![stone_card(Rank::King, S), card(Rank::Seven, H)];
    let mut game = face_game(&["Pareidolia", "Sock and Buskin"], &played, None);
    assert_eq!(
        face_score(&mut game, &played, &[]).score(),
        5 + 2 * 50 + 2 * 7
    );
}

#[test]
fn test_photograph_takes_a_leading_stone_card_beside_pareidolia() {
    // card.lua:3093-3098: the first scoring card that is_face() -- the Stone
    // card, so the X2 lands before the Mult card's +4.
    let mult_king = card(Rank::King, H);
    mult_king.borrow_mut().enhancement = Enhancement::Mult;
    let played = vec![stone_card(Rank::King, S), mult_king];
    let mut game = face_game(&["Pareidolia", "Photograph"], &played, None);
    assert_eq!(
        face_score(&mut game, &played, &[]).score(),
        (5 + 50 + 10) * (1 * 2 + 4)
    );
}

#[test]
fn test_ride_the_bus_resets_on_a_lone_stone_card_beside_pareidolia() {
    // card.lua:3525-3532: any scoring card that is_face() resets it.
    let played = vec![stone_card(Rank::Two, S)];
    let mut game = face_game(&["Pareidolia", "Ride the Bus"], &played, None);
    let rtb = game
        .jokers
        .iter()
        .find(|j| j.borrow().name() == "Ride the Bus")
        .unwrap()
        .clone();
    rtb.borrow_mut().counter = 3.0;
    let ctx = face_score(&mut game, &played, &[]);
    assert_eq!(rtb.borrow().counter, 0.0);
    assert_eq!(ctx.score(), 5 + 50);
}

#[test]
fn test_midas_mask_gilds_a_stone_card_beside_pareidolia() {
    // card.lua:3443-3448: set_ability(m_gold) on every scoring is_face().
    let played = vec![stone_card(Rank::Two, S), card(Rank::Seven, H)];
    let mut game = face_game(&["Pareidolia", "Midas Mask"], &played, None);
    face_score(&mut game, &played, &[]);
    assert_eq!(
        enhancements(&played),
        vec![Enhancement::Gold, Enhancement::Gold]
    );
}

#[test]
fn test_midas_mask_without_pareidolia_leaves_a_stone_king() {
    let played = vec![stone_card(Rank::King, S), card(Rank::King, H)];
    let mut game = face_game(&["Midas Mask"], &played, None);
    face_score(&mut game, &played, &[]);
    assert_eq!(
        enhancements(&played),
        vec![Enhancement::Stone, Enhancement::Gold]
    );
}

#[test]
fn test_midas_mask_leaves_the_kings_the_plant_debuffed() {
    // card.lua:965: `if self.debuff and not from_boss then return end`, and
    // Midas Mask's is_face() is not from a boss.
    let played = vec![card(Rank::King, S), card(Rank::King, H)];
    let mut game = face_game(&["Midas Mask"], &played, Some("The Plant"));
    assert!(played.iter().all(|c| c.borrow().debuffed));
    face_score(&mut game, &played, &[]);
    assert_eq!(
        enhancements(&played),
        vec![Enhancement::None, Enhancement::None]
    );
}

/// The RNG pools a scoring pass advanced. Python monkeypatches `rng.chance` to
/// record each listed probability it rolls; the Rust stream names the same pool
/// through `RunRng::state()`, so the set of advanced pools is the same record.
fn changed_pools(
    before: &std::collections::HashMap<String, f64>,
    after: &std::collections::HashMap<String, f64>,
) -> std::collections::BTreeSet<String> {
    let mut out = std::collections::BTreeSet::new();
    for (key, value) in after {
        if before.get(key) != Some(value) {
            out.insert(key.clone());
        }
    }
    for key in before.keys() {
        if !after.contains_key(key) {
            out.insert(key.clone());
        }
    }
    out
}

fn stone_keys(names: &[&str]) -> std::collections::BTreeSet<String> {
    names.iter().map(|n| n.to_string()).collect()
}

#[test]
fn test_business_card_rolls_for_a_stone_card_beside_pareidolia() {
    // card.lua:3175-3177: `is_face() and pseudorandom('business') < ...`, so
    // the roll -- and its draw from the stream -- happens only for a face.
    let played = vec![stone_card(Rank::Two, S)];
    let mut game = face_game(&["Pareidolia", "Business Card"], &played, None);
    let before = game.rng.state();
    let ctx = face_score(&mut game, &played, &[]);
    assert_eq!(ctx.money_gained, 2);
    assert_eq!(
        changed_pools(&before, &game.rng.state()),
        stone_keys(&["business"])
    );
}

#[test]
fn test_reserved_parking_rolls_for_a_held_stone_card_beside_pareidolia() {
    // card.lua:3302-3304, the same shape for a card held in hand.
    let played = vec![card(Rank::Two, S)];
    let held = vec![stone_card(Rank::Seven, H)];
    let all: Vec<CardRef> = played.iter().chain(held.iter()).cloned().collect();
    let mut game = face_game(&["Pareidolia", "Reserved Parking"], &all, None);
    let before = game.rng.state();
    let ctx = face_score(&mut game, &played, &held);
    assert_eq!(ctx.money_gained, 1);
    assert_eq!(
        changed_pools(&before, &game.rng.state()),
        stone_keys(&["parking"])
    );
}

// -- The Plant: is_face(true) --------------------------------------------------

fn debuffed_flags(cards: &[CardRef]) -> Vec<bool> {
    cards.iter().map(|c| c.borrow().debuffed).collect()
}

#[test]
fn test_the_plant_does_not_debuff_a_stone_king() {
    // blind.lua:630 `card:is_face(true)`; get_id for Stone, card.lua:958.
    let cards = vec![
        stone_card(Rank::King, S),
        card(Rank::King, H),
        card(Rank::Seven, C),
    ];
    face_game(&[], &cards, Some("The Plant"));
    assert_eq!(debuffed_flags(&cards), vec![false, true, false]);
}

#[test]
fn test_the_plant_beside_pareidolia_debuffs_every_card_stone_included() {
    // card.lua:967: the Pareidolia test does not look at get_id.
    let cards = vec![
        stone_card(Rank::King, S),
        card(Rank::King, H),
        card(Rank::Seven, C),
    ];
    face_game(&["Pareidolia"], &cards, Some("The Plant"));
    assert_eq!(debuffed_flags(&cards), vec![true, true, true]);
}

#[test]
fn test_the_plant_ignores_a_debuffed_pareidolia() {
    // find_joker skips a debuffed joker (misc_functions.lua:907).
    let cards = vec![
        stone_card(Rank::King, S),
        card(Rank::King, H),
        card(Rank::Seven, C),
    ];
    let mut game = GameState::new(0, "Red Deck", 1);
    game.jokers = vec![jokers::make("Pareidolia")];
    let pareidolia = game.jokers[0].clone();
    game.set_joker_debuff(&pareidolia, true);
    game.blind = Some(make_blind(
        BlindKind::Boss,
        1,
        boss_by_name("The Plant"),
        1.0,
        1,
        false,
    ));
    game.full_deck = cards.to_vec();
    game._apply_debuffs();
    assert_eq!(debuffed_flags(&cards), vec![false, true, false]);
}

// -- Canio: is_face() over the cards removed -----------------------------------

fn canio_game(row: &[&str]) -> GameState {
    let mut game = GameState::new("TESTSEED", "Red Deck", 1);
    game.gain_joker(&jokers::make("Canio"));
    for name in row {
        game.gain_joker(&jokers::make(name));
    }
    game._start_round();
    game
}

#[test]
fn test_canio_counts_what_is_face_counts() {
    // card.lua:2673-2679 `if val:is_face() then face_cards = face_cards + 1`.
    let cases: [(&[&str], Rank, bool, bool, f64); 5] = [
        (&[], Rank::King, false, false, 1.0),
        (&[], Rank::King, true, false, 0.0), // card.lua:958-960
        (&[], Rank::King, false, true, 0.0), // card.lua:965
        (&["Pareidolia"], Rank::Two, true, false, 1.0), // card.lua:967
        (&["Pareidolia"], Rank::Two, false, true, 0.0), // card.lua:965 first
    ];
    for (row, rank, stone, debuffed, gain) in cases {
        let mut game = canio_game(row);
        let target = game.hand[0].clone();
        target.borrow_mut().rank = rank;
        target.borrow_mut().debuffed = debuffed;
        if stone {
            target.borrow_mut().enhancement = Enhancement::Stone;
        }
        let canio = game
            .jokers
            .iter()
            .find(|j| j.borrow().name() == "Canio")
            .unwrap()
            .clone();
        let before = canio.borrow().counter;
        game.remove_card(&target, false);
        assert_eq!(
            canio.borrow().counter - before,
            gain,
            "row {row:?} rank {rank:?} stone {stone} debuffed {debuffed}"
        );
    }
}

// ==========================================================================
// tests/test_ordered_play.py -- "A play names its cards in the order they
// score."
//
// The game scores a play left to right on screen: play_cards_from_highlighted
// sorts the selected cards by position (state_events.lua:463), and a player
// drags cards to choose that order. Hanging Chad retriggers the first card
// scored and Photograph pays on the first face card scored, so with both held
// an Ace-high flush that leads with the Ace spends the retriggers on the Ace --
// unless the Queen is dragged in front of it. BotAPI.swap_card_left is that
// drag; here a play in any order of a legal selection is legal, and playing it
// first moves the named cards to the front in that order.
// ==========================================================================

const FLUSH_PLAY: [usize; 5] = [0, 1, 2, 3, 4];
const QUEEN_FIRST: [usize; 5] = [1, 0, 2, 3, 4];

fn op_game(joker_names: &[&str]) -> GameState {
    let mut game = GameState::new("AWEFRTUZ", "Blue Deck", 1);
    for name in joker_names {
        game.gain_joker(&jokers::make(name));
    }
    game._start_round();
    game.hand = vec![
        card(Rank::Ace, H),
        card(Rank::Queen, H),
        card(Rank::Nine, H),
        card(Rank::Six, H),
        card(Rank::Two, H),
        card(Rank::Five, C),
        card(Rank::Four, C),
        card(Rank::Three, C),
    ];
    game
}

fn same_refs(a: &[CardRef], b: &[CardRef]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| Rc::ptr_eq(x, y))
}

#[test]
fn test_any_order_of_a_legal_selection_is_legal() {
    let game = op_game(&[]);
    assert!(game.is_legal(&Action::with_cards(ActionType::Play, vec![1, 0, 2, 3, 4])));
    assert!(!game.is_legal(&Action::with_cards(ActionType::Play, vec![1, 1, 2])));
    assert!(!game.is_legal(&Action::with_cards(ActionType::Play, vec![1, 0, 9])));
}

#[test]
fn test_the_named_cards_go_to_the_front_in_that_order() {
    let mut game = op_game(&[]);
    let ace = game.hand[0].clone();
    let queen = game.hand[1].clone();
    let flush: Vec<CardRef> = game.hand[2..5].to_vec();
    let rest: Vec<CardRef> = game.hand[5..].to_vec();
    assert_eq!(game._arrange_play(&QUEEN_FIRST), vec![0, 1, 2, 3, 4]);
    assert!(Rc::ptr_eq(&game.hand[0], &queen));
    assert!(Rc::ptr_eq(&game.hand[1], &ace));
    assert!(same_refs(&game.hand[2..5], &flush));
    assert!(same_refs(&game.hand[5..], &rest));
}

#[test]
fn test_a_play_in_hand_order_is_left_alone() {
    let mut game = op_game(&[]);
    let before: Vec<CardRef> = game.hand.clone();
    assert_eq!(game._arrange_play(&[0, 2, 4]), vec![0, 2, 4]);
    assert!(same_refs(&game.hand, &before));
}

#[test]
fn test_left_swaps_reach_the_same_arrangement() {
    let mut direct = op_game(&[]);
    let mut swapped = op_game(&[]);
    direct._arrange_play(&[3, 1, 0]);
    for at in [3, 2, 1, 2] {
        // the Six to the front, then the Queen, the Ace
        swapped.swap_card_left(at);
    }
    let a: Vec<String> = direct.hand.iter().map(label_of).collect();
    let b: Vec<String> = swapped.hand.iter().map(label_of).collect();
    assert_eq!(a, b);
}

#[test]
fn test_a_preview_counts_the_money_a_play_earns_while_it_scores() {
    // A Gold Seal is $3 a trigger (card.lua, Card:get_p_dollars), and Hanging
    // Chad retriggers the first card scored twice: $9 at the front, $3 behind,
    // the same chips either way.
    let mut game = GameState::new("AWEFRTUZ", "Blue Deck", 1);
    game.gain_joker(&jokers::make("Hanging Chad"));
    game._start_round();
    let gold = card(Rank::Seven, S);
    gold.borrow_mut().seal = Seal::Gold;
    game.hand = vec![
        gold,
        card(Rank::Seven, H),
        card(Rank::Two, C),
        card(Rank::Three, C),
    ];
    let front = game.preview_value(&[0, 1], "roll");
    let behind = game.preview_value(&[1, 0], "roll");
    assert_eq!(front.1, 9);
    assert_eq!(behind.1, 3);
    assert_eq!(front.0, game.preview_score(&[0, 1], "roll"));
    assert_eq!(game.money, GameState::new("AWEFRTUZ", "Blue Deck", 1).money);
}

#[test]
fn test_the_queen_first_scores_the_photograph_three_times() {
    let mut game = op_game(&["Hanging Chad", "Photograph"]);
    let queen_first = game.preview_score(&QUEEN_FIRST, "roll");
    let flush = game.preview_score(&FLUSH_PLAY, "roll");
    assert!(queen_first > flush);
    // Python deepcopies the run so the step does not disturb the run the
    // preview came from; a second identically-built run says the same thing.
    let mut played = op_game(&["Hanging Chad", "Photograph"]);
    let before = played.chips_scored;
    played.step(&Action::with_cards(ActionType::Play, QUEEN_FIRST.to_vec()));
    assert_eq!(played.chips_scored - before, queen_first);
}

#[test]
fn test_a_consumable_names_its_targets_in_drag_order() {
    // Death converts the left card into the right one, so the order the two
    // targets are named in is which of them is spent.
    //
    // Without the drag the pair had to be named in hand order, and a hand is
    // sorted by rank descending -- so the card spent always outranked the copy
    // and Death could never make a higher card. Here the Three of Clubs, which
    // sits last, is spent on a copy of the Ace of Hearts, which sits first.
    let mut game = op_game(&[]);
    let death = consumables::spec_or_panic("Death");
    let held = game.hold_consumable(death, Edition::None);
    game.consumables.push(held);
    let ace = game.hand[0].clone();
    let three = game.hand[7].clone();
    assert_eq!(
        (three.borrow().rank, three.borrow().suit),
        (Rank::Three, Suit::Clubs)
    );

    // Naming them out of hand order is legal for a consumable, because the
    // order is a choice the player makes by dragging.
    let action = Action {
        r#type: ActionType::UseConsumable,
        index: 0,
        cards: vec![7, 0],
    };
    assert!(game.is_legal(&action));
    assert!(!game.is_legal(&Action {
        r#type: ActionType::UseConsumable,
        index: 0,
        cards: vec![0, 0],
    }));

    game.step(&action);
    // The Three became an Ace of Hearts; the Ace it copied is untouched.
    assert_eq!(
        (three.borrow().rank, three.borrow().suit),
        (Rank::Ace, Suit::Hearts)
    );
    assert_eq!(
        (ace.borrow().rank, ace.borrow().suit),
        (Rank::Ace, Suit::Hearts)
    );
    // And the pair was dragged to the front, spent card first, the way
    // _arrange_play leaves an ordered play.
    assert!(Rc::ptr_eq(&game.hand[0], &three));
    assert!(Rc::ptr_eq(&game.hand[1], &ace));
}

#[test]
fn test_a_consumable_in_hand_order_is_left_alone() {
    let mut game = op_game(&[]);
    let death = consumables::spec_or_panic("Death");
    let held = game.hold_consumable(death, Edition::None);
    game.consumables.push(held);
    let before: Vec<CardRef> = game.hand.clone();
    let ace = game.hand[0].clone();
    let queen = game.hand[1].clone();
    game.step(&Action {
        r#type: ActionType::UseConsumable,
        index: 0,
        cards: vec![0, 1],
    });
    // The left card becomes the right one, and nothing moved.
    assert_eq!(
        (ace.borrow().rank, ace.borrow().suit),
        (Rank::Queen, Suit::Hearts)
    );
    assert_eq!(
        (queen.borrow().rank, queen.borrow().suit),
        (Rank::Queen, Suit::Hearts)
    );
    assert!(same_refs(&game.hand, &before));
}

// ==========================================================================
// tests/test_four_fingers_straight_scores_every_card.py -- "A Four Fingers
// straight scores every card of every rank in the run."
//
// get_straight (functions/misc_functions.lua:548) buckets the hand by rank and,
// for each rank the run passes through, adds *all* of that rank's cards. With
// Four Fingers a five-card straight can hold a pair -- 9 8 7 7 6 -- and both
// sevens score. This kept one card per rank, so the second seven played and
// scored nothing. Two seeds on the headless engine stopped on it:
//
//   64PUKM3K, Erratic Deck, stake 3, decision 78: 9H 8D 7S 7C 6H scored 195 x
//   44 = 8580 in the game, 188 x 44 = 8272 here -- the 7C's seven chips.
//   U1AYP8BC, Yellow Deck, stake 5, decision 53: 7D 6S 6C 5D 4H, level-2
//   Straight, 226 x 14 = 3164 against 220 x 14 = 3080 -- the 6C's six chips.
//
// get_straight reads get_id, which does not look at debuff (card.lua:957), so
// a debuffed card holds its place in a straight the way it holds its suit in a
// flush; it simply scores nothing when evaluate_play reaches it
// (state_events.lua:655). And a Straight Flush is any hand with both parts
// (misc_functions.lua:428): `if next(parts._flush) and next(parts._straight)`,
// with no test that the flush and the straight are the same cards. With Four
// Fingers they need not be: 2S 3S 4S 5H 9S is a four-card straight and a
// four-card flush, and the game calls it a Straight Flush and scores all five.
// ==========================================================================

fn ff_hand(specs: &[(Rank, Suit)]) -> Vec<CardRef> {
    specs.iter().map(|(r, s)| card(*r, *s)).collect()
}

fn four_fingers() -> EvalFlags {
    EvalFlags {
        four_fingers: true,
        ..EvalFlags::default()
    }
}

#[test]
fn test_both_sevens_score_in_a_four_fingers_straight() {
    // 64PUKM3K decision 78: 9 8 7 7 6.
    let cards = ff_hand(&[
        (Rank::Nine, H),
        (Rank::Eight, D),
        (Rank::Seven, S),
        (Rank::Seven, C),
        (Rank::Six, H),
    ]);
    let result = evaluate(&cards, four_fingers());
    assert_eq!(result.hand, HandType::Straight);
    let scored: Vec<u64> = result.scoring.iter().map(uid_of).collect();
    let want: Vec<u64> = cards.iter().map(uid_of).collect();
    assert_eq!(scored, want);
}

#[test]
fn test_both_sixes_score_in_a_four_fingers_straight() {
    // U1AYP8BC decision 53: 7 6 6 5 4, which scores 3164 in the game.
    let cards = ff_hand(&[
        (Rank::Seven, D),
        (Rank::Six, S),
        (Rank::Six, C),
        (Rank::Five, D),
        (Rank::Four, H),
    ]);
    let result = evaluate(&cards, four_fingers());
    assert_eq!(result.hand, HandType::Straight);
    assert_eq!(result.scoring.len(), 5);
}

#[test]
fn test_the_duplicate_adds_its_chips_to_the_score() {
    // Level-1 Straight, no jokers: 30 + 9 + 8 + 7 + 7 + 6 = 67 chips x 4.
    let mut game = GameState::new("TESTSEED", "Red Deck", 1);
    let cards = ff_hand(&[
        (Rank::Nine, H),
        (Rank::Eight, D),
        (Rank::Seven, S),
        (Rank::Seven, C),
        (Rank::Six, H),
    ]);
    let result = evaluate(&cards, four_fingers());
    let ctx = score_hand(&mut game, &result, &cards, &[]);
    assert_eq!((ctx.chips, ctx.mult, ctx.score()), (67.0, 4.0, 268));
}

#[test]
fn test_shortcut_straight_keeps_its_duplicate_too() {
    // 2 4 4 6 8 with Shortcut and Four Fingers: the run is 2 4 6 8.
    let cards = ff_hand(&[
        (Rank::Two, S),
        (Rank::Four, H),
        (Rank::Four, C),
        (Rank::Six, D),
        (Rank::Eight, S),
    ]);
    let flags = EvalFlags {
        four_fingers: true,
        shortcut: true,
        ..EvalFlags::default()
    };
    let result = evaluate(&cards, flags);
    assert_eq!(result.hand, HandType::Straight);
    assert_eq!(result.scoring.len(), 5);
}

#[test]
fn test_a_card_off_the_run_still_does_not_score() {
    // 9 8 7 6 and a 2: the 2 breaks nothing and joins nothing.
    let cards = ff_hand(&[
        (Rank::Nine, H),
        (Rank::Eight, D),
        (Rank::Seven, S),
        (Rank::Six, H),
        (Rank::Two, C),
    ]);
    let result = evaluate(&cards, four_fingers());
    assert_eq!(result.hand, HandType::Straight);
    let ranks: Vec<Rank> = result.scoring.iter().map(|c| c.borrow().rank).collect();
    assert_eq!(ranks, vec![Rank::Nine, Rank::Eight, Rank::Seven, Rank::Six]);
}

#[test]
fn test_a_debuffed_card_holds_its_place_in_a_straight() {
    // get_id ignores debuff: 5 6 7 8 9 with the 7 debuffed is a Straight.
    let cards = ff_hand(&[
        (Rank::Five, S),
        (Rank::Six, H),
        (Rank::Seven, C),
        (Rank::Eight, D),
        (Rank::Nine, S),
    ]);
    cards[2].borrow_mut().debuffed = true;
    let result = evaluate(&cards, plain_flags());
    assert_eq!(result.hand, HandType::Straight);
    assert_eq!(result.scoring.len(), 5);
}

#[test]
fn test_four_fingers_straight_flush_needs_no_overlap() {
    // 2S 3S 4S 5H 9S: a straight of four and a flush of four, not the same
    // four cards, is a Straight Flush in the game and all five score.
    let cards = ff_hand(&[
        (Rank::Two, S),
        (Rank::Three, S),
        (Rank::Four, S),
        (Rank::Five, H),
        (Rank::Nine, S),
    ]);
    let result = evaluate(&cards, four_fingers());
    assert_eq!(result.hand, HandType::StraightFlush);
    assert_eq!(result.scoring.len(), 5);
}

// ==========================================================================
// tests/test_flush_reads_debuffed_suits.py -- "A debuffed card still counts
// towards a flush by its printed suit."
//
// The game asks its suit question two ways (card.lua:4064). The ordinary one
// refuses a debuffed card outright; the one a flush is judged on,
// `is_suit(suit, nil, true)`, reads the printed suit anyway, and only a Wild
// card loses its every-suit to a debuff. Hand detection used the ordinary
// question, so a hand of debuffed cards was never a flush. Seed QWERTYUI, Blue
// Deck, stake 1 found it at decision 101: The Club debuffs Clubs, Smeared Joker
// makes Spades Clubs, and A-Q-Q-9-6 of Spades and Clubs was a level-3 Flush in
// the game (135 x 26 = 3510) and a level-2 Pair here (95 x 21 = 1995).
// ==========================================================================

fn deb_cards(specs: &[(Rank, Suit)], debuffed: bool) -> Vec<CardRef> {
    specs
        .iter()
        .map(|(rank, suit)| {
            let c = card(*rank, *suit);
            c.borrow_mut().debuffed = debuffed;
            c
        })
        .collect()
}

const QWERTYUI: [(Rank, Suit); 5] = [
    (Rank::Ace, S),
    (Rank::Queen, S),
    (Rank::Queen, C),
    (Rank::Nine, S),
    (Rank::Six, S),
];

#[test]
fn test_the_qwertyui_hand_is_a_flush_under_smeared_joker() {
    let flags = EvalFlags {
        smeared: true,
        ..EvalFlags::default()
    };
    assert_eq!(
        evaluate(&deb_cards(&QWERTYUI, true), flags).hand,
        HandType::Flush
    );
}

#[test]
fn test_without_smeared_joker_the_club_breaks_it() {
    assert_eq!(
        evaluate(&deb_cards(&QWERTYUI, true), plain_flags()).hand,
        HandType::Pair
    );
}

#[test]
fn test_five_debuffed_spades_are_a_flush() {
    let spades = [
        (Rank::Ace, S),
        (Rank::Jack, S),
        (Rank::Nine, S),
        (Rank::Six, S),
        (Rank::Three, S),
    ];
    assert_eq!(
        evaluate(&deb_cards(&spades, true), plain_flags()).hand,
        HandType::Flush
    );
}

#[test]
fn test_a_debuffed_wild_card_loses_its_every_suit() {
    let cards = deb_cards(
        &[
            (Rank::Ace, S),
            (Rank::Jack, S),
            (Rank::Nine, S),
            (Rank::Six, S),
            (Rank::Three, H),
        ],
        true,
    );
    cards[4].borrow_mut().enhancement = Enhancement::Wild;
    assert_ne!(
        evaluate(&cards, plain_flags()).hand,
        HandType::Flush,
        "a debuffed Wild card is only its printed suit"
    );
    cards[4].borrow_mut().debuffed = false;
    assert_eq!(evaluate(&cards, plain_flags()).hand, HandType::Flush);
}

#[test]
fn test_a_stone_card_is_never_a_suit() {
    let cards = deb_cards(
        &[
            (Rank::Ace, S),
            (Rank::Jack, S),
            (Rank::Nine, S),
            (Rank::Six, S),
            (Rank::Three, S),
        ],
        false,
    );
    cards[4].borrow_mut().enhancement = Enhancement::Stone;
    assert_ne!(evaluate(&cards, plain_flags()).hand, HandType::Flush);
}

// ==========================================================================
// tests/test_straight_reads_debuffed_ranks.py -- "A debuffed card still counts
// towards a straight."
//
// get_straight (misc_functions.lua:548-590) asks each card nothing but its id,
// and Card:get_id (card.lua:957-962) never looks at the debuff. Only a Stone
// card is left out, by the random negative id it returns. The flush half
// already reads a debuffed card's printed suit, so a debuffed run of one suit
// is a Straight Flush. The simulator's straight dropped debuffed cards. Seed
// N1OA90W1, Abandoned Deck, stake 1, at decision 105: The Club debuffs Clubs,
// and 6-5-4-3-2 of Clubs was a Straight Flush in the game --
// (100 + 100 Devious + 50 foil) x (8 x 4.5 Madness x 3 Stencil) = 27000,
// enough for the 22000 boss -- and a level-two Flush here, 100 x 81 = 8100.
// ==========================================================================

const RUN_OF_CLUBS: [(Rank, Suit); 5] = [
    (Rank::Six, C),
    (Rank::Five, C),
    (Rank::Four, C),
    (Rank::Three, C),
    (Rank::Two, C),
];

#[test]
fn test_five_debuffed_clubs_in_a_row_are_a_straight_flush() {
    let result = evaluate(&deb_cards(&RUN_OF_CLUBS, true), plain_flags());
    assert_eq!(result.hand, HandType::StraightFlush);
    assert_eq!(result.scoring.len(), 5);
    assert!(result.contains.contains(HandType::Straight));
}

#[test]
fn test_one_debuffed_card_still_completes_a_straight() {
    let cards = deb_cards(
        &[
            (Rank::Nine, H),
            (Rank::Eight, C),
            (Rank::Seven, S),
            (Rank::Six, D),
            (Rank::Five, H),
        ],
        false,
    );
    cards[1].borrow_mut().debuffed = true;
    let result = evaluate(&cards, plain_flags());
    assert_eq!(result.hand, HandType::Straight);
    assert_eq!(result.scoring.len(), 5);
}

#[test]
fn test_a_stone_card_still_breaks_a_straight() {
    let cards = deb_cards(&RUN_OF_CLUBS, false);
    cards[2].borrow_mut().enhancement = Enhancement::Stone;
    assert!(!evaluate(&cards, plain_flags())
        .contains
        .contains(HandType::Straight));
}

fn joker_instance(name: &str) -> JokerInstance {
    JokerInstance::new(jokers::spec_or_panic(name))
}

#[test]
fn test_the_n1oa90w1_play_scores_27000() {
    let mut game = GameState::new("N1OA90W1", "Red Deck", 1);
    let mut madness = joker_instance("Madness");
    madness.counter = 4.5;
    let mut devious = joker_instance("Devious Joker");
    devious.edition = Edition::Foil;
    for instance in [madness, devious, joker_instance("Joker Stencil")] {
        game.gain_joker(&jokers::make_ref(instance));
    }
    game.ante_boss = String::new();
    let ante = game.ante;
    let club = boss_by_name("The Club");
    game.blind = Some(make_blind(BlindKind::Boss, ante, club, 1.0, 1, false));
    game._start_round();
    game.hand_levels.levels.insert(HandType::Flush, 2);
    game.hand_levels.levels.insert(HandType::ThreeOfAKind, 3);

    let hand = vec![
        card(Rank::Ten, D),
        card(Rank::Eight, C),
        card(Rank::Six, C),
        card(Rank::Five, C),
        card(Rank::Four, H),
        card(Rank::Four, C),
        card(Rank::Three, C),
        card(Rank::Two, C),
    ];
    game.full_deck.extend(hand.iter().cloned());
    game.hand = hand.clone();
    game._apply_debuffs();
    let play = [2usize, 3, 5, 6, 7];
    assert!(play.iter().all(|&i| game.hand[i].borrow().debuffed));

    let played: Vec<CardRef> = play.iter().map(|&i| game.hand[i].clone()).collect();
    assert_eq!(
        game.evaluate_selection(&played).hand,
        HandType::StraightFlush
    );
    assert_eq!(game.preview_score(&play, "roll"), 27000);
    game.step(&Action::with_cards(ActionType::Play, play.to_vec()));
    assert_eq!(game.chips_scored, 27000);
}

// ==========================================================================
// tests/test_flower_pot_seeing_double_suits.py -- "Flower Pot and Seeing Double
// count suits the way card.lua:3808-3866 does."
//
// Both tally the scoring hand into four suit counters, in two passes -- every
// card that is not a Wild card first, then the Wild cards -- and the passes ask
// Card:is_suit (card.lua:4064-4089) differently:
//
//   Flower Pot, non-Wild   `is_suit(s, true)`, bypass_debuff, in an elseif
//                          chain Hearts, Diamonds, Spades, Clubs that stops at
//                          the first suit still at zero: a debuffed card counts
//                          its suit, and under Smeared Joker a second Heart
//                          fills Diamonds.
//   Flower Pot, Wild       `is_suit(s)`, same chain: a Wild fills ONE empty
//                          suit, and a debuffed Wild fills none.
//   Seeing Double, non-Wild `is_suit(s)` for all four, no chain: under Smeared
//                          Joker a Spade is a Club as well.
//   Seeing Double, Wild    `is_suit(s)`, chain Clubs, Diamonds, Spades,
//                          Hearts.
//
// The simulator asked `counts_as_suit` for every suit instead, which lets a
// Wild card fill all four, refuses a debuffed card outright, and knows nothing
// of Smeared Joker. The numbers are the headless engine's, measured with
// Scenario on a fresh run.
// ==========================================================================

fn fp_score(
    hand: &str,
    play: &[usize],
    joker_names: &[&str],
    boss: Option<&str>,
    wild: &[usize],
    stone: &[usize],
) -> i64 {
    let mut game = GameState::new(0, "Red Deck", 1);
    let cards: Vec<CardRef> = hand
        .split_whitespace()
        .enumerate()
        .map(|(i, code)| {
            let (suit, rank) = code.split_once('_').expect("suit_rank");
            let c = card(
                Rank::from_code(rank).expect("rank"),
                Suit::from_code(suit).expect("suit"),
            );
            let one_based = i + 1;
            if wild.contains(&one_based) {
                c.borrow_mut().enhancement = Enhancement::Wild;
            }
            if stone.contains(&one_based) {
                c.borrow_mut().enhancement = Enhancement::Stone;
            }
            c
        })
        .collect();
    game.jokers = joker_names.iter().map(|n| jokers::make(n)).collect();
    let played: Vec<CardRef> = play.iter().map(|&i| cards[i - 1].clone()).collect();
    let held: Vec<CardRef> = cards
        .iter()
        .enumerate()
        .filter(|(i, _)| !play.contains(&(i + 1)))
        .map(|(_, c)| c.clone())
        .collect();
    game.hand = played.iter().chain(held.iter()).cloned().collect();
    game.full_deck = game.hand.clone();
    if let Some(boss) = boss {
        let ante = game.ante;
        game.blind = Some(make_blind(
            BlindKind::Boss,
            ante,
            boss_by_name(boss),
            1.0,
            1,
            false,
        ));
    }
    game._apply_debuffs();
    let result = evaluate(
        &played,
        EvalFlags {
            splash: game.splash(),
            smeared: game.has_smeared(),
            four_fingers: game.four_fingers(),
            shortcut: game.shortcut_joker(),
        },
    );
    score_hand(&mut game, &result, &played, &held).score()
}

/// label, hand, play, jokers, boss, wild, engine score.
const FP_CASES: [(&str, &str, &[usize], &[&str], Option<&str>, &[usize], i64); 8] = [
    (
        "pot_wild_fills_one_suit",
        "H_7 S_7 C_7 D_2 D_3 S_4 H_5 C_9",
        &[1, 2, 3],
        &["Flower Pot"],
        None,
        &[3],
        153,
    ),
    (
        "pot_debuffed_diamond_counts",
        "H_9 D_9 S_4 C_4 H_2 S_3 C_6 S_8",
        &[1, 2, 3, 4],
        &["Flower Pot"],
        Some("The Window"),
        &[],
        222,
    ),
    (
        "pot_smeared_heart_fills_diamonds",
        "H_9 H_9 S_4 C_4 H_2 S_3 C_6 S_8",
        &[1, 2, 3, 4],
        &["Smeared Joker", "Flower Pot"],
        None,
        &[],
        276,
    ),
    (
        "pot_four_plain_suits",
        "H_9 D_9 S_4 C_4 H_2 S_3 C_6 S_8",
        &[1, 2, 3, 4],
        &["Flower Pot"],
        None,
        &[],
        276,
    ),
    (
        "double_smeared_spades_are_clubs",
        "S_9 S_9 D_4 H_5 H_2 D_3 H_6 D_8",
        &[1, 2],
        &["Smeared Joker", "Seeing Double"],
        None,
        &[],
        112,
    ),
    (
        "double_club_and_wild",
        "C_9 H_9 D_4 H_5 H_2 D_3 H_6 D_8",
        &[1, 2],
        &["Seeing Double"],
        None,
        &[2],
        112,
    ),
    (
        "double_two_wilds",
        "C_9 H_9 D_4 H_5 H_2 D_3 H_6 D_8",
        &[1, 2],
        &["Seeing Double"],
        None,
        &[1, 2],
        112,
    ),
    (
        "double_debuffed_diamond_does_not_count",
        "C_9 D_9 S_4 H_5 H_2 S_3 H_6 S_8",
        &[1, 2],
        &["Seeing Double"],
        Some("The Window"),
        &[],
        38,
    ),
];

#[test]
fn test_matches_the_engine() {
    for (label, hand, play, jokers, boss, wild, engine) in FP_CASES {
        assert_eq!(
            fp_score(hand, play, jokers, boss, wild, &[]),
            engine,
            "{label}"
        );
    }
}

#[test]
fn test_flower_pot_ignores_a_debuffed_wild() {
    // The Wild pass asks without bypass_debuff (card.lua:3824-3827), and the
    // Wild card is skipped by the first pass, so a debuffed Wild -- The Window
    // debuffs every Wild card (blind.lua:626, card.lua:4081) -- fills nothing.
    // Hearts, Spades and Clubs are there; Diamonds never is.
    let hand = "H_7 S_7 C_7 D_7 D_3 S_4 H_5 C_9";
    let with_pot = fp_score(
        hand,
        &[1, 2, 3, 4],
        &["Flower Pot"],
        Some("The Window"),
        &[4],
        &[],
    );
    let without = fp_score(hand, &[1, 2, 3, 4], &[], Some("The Window"), &[4], &[]);
    assert_eq!(with_pot, without);
}

#[test]
fn test_flower_pot_stone_card_has_no_suit() {
    // `if self.ability.effect == 'Stone Card' then return false end`
    // (card.lua:4078), bypass or not.
    let hand = "H_7 S_7 C_7 D_7 D_3 S_4 H_5 C_9";
    let with_pot = fp_score(hand, &[1, 2, 3, 4], &["Flower Pot"], None, &[], &[4]);
    let without = fp_score(hand, &[1, 2, 3, 4], &[], None, &[], &[4]);
    assert_eq!(with_pot, without);
}

// ==========================================================================
// tests/test_shoot_the_moon_stone_queen.py -- "Shoot the Moon pays for a held
// Queen by id, and a Stone Queen has none."
//
// card.lua:3272-3273 asks `context.other_card:get_id() == 12`, and get_id gives
// a Stone card a random negative (card.lua:958-960), so a Stone Queen held in
// hand adds no mult. The simulator matched on the printed rank and paid +13
// for it; Baron already had the Stone check for Kings.
// ==========================================================================

fn stm_score(held: &[CardRef]) -> i64 {
    let mut game = GameState::new("MOONSTNE", "Red Deck", 1);
    game.gain_joker(&jokers::make("Shoot the Moon"));
    let played = vec![card(Rank::Two, S)];
    let result = game.evaluate_selection(&played);
    score_hand(&mut game, &result, &played, held).score()
}

#[test]
fn test_a_held_queen_gives_thirteen_mult() {
    assert!(stm_score(&[card(Rank::Queen, H)]) > stm_score(&[]));
}

#[test]
fn test_a_held_stone_queen_gives_nothing() {
    let stone = stone_card(Rank::Queen, H);
    assert_eq!(stm_score(&[stone]), stm_score(&[]));
}
