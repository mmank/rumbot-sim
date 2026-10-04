//! Joker Stencil counts the stencils, not just itself.
//!
//! Ported from the simulator's Python fix `fa9f8e0` (its
//! `tests/test_joker_stencil_counts_stencils.py` is the same four rows). The
//! row is the only thing moving -- a Pair of Kings out of eight cards -- and
//! the four scores are the engine's own numbers: X5 for one stencil, X16 for
//! two beside a plain joker, X125 for three, all confirmed against the game.
//!
//!     cargo test -p rumbot_sim --offline --test joker_stencil

use rumbot_sim::cards::{make_card, Rank, Suit};
use rumbot_sim::game::GameState;
use rumbot_sim::jokers;
use rumbot_sim::scoring;

// "S_K H_K D_2 C_5 H_7 S_9 D_3 C_4": a pair of Kings, and six held cards.
const HAND: [(Rank, Suit); 8] = [
    (Rank::King, Suit::Spades),
    (Rank::King, Suit::Hearts),
    (Rank::Two, Suit::Diamonds),
    (Rank::Five, Suit::Clubs),
    (Rank::Seven, Suit::Hearts),
    (Rank::Nine, Suit::Spades),
    (Rank::Three, Suit::Diamonds),
    (Rank::Four, Suit::Clubs),
];
const PLAY: [usize; 2] = [0, 1];

fn sim_score(row: &[&str]) -> i64 {
    let mut game = GameState::new("SEED0000", "Red Deck", 1);
    game.jokers = row.iter().map(|name| jokers::make(name)).collect();

    let cards: Vec<_> = HAND.iter().map(|(r, s)| make_card(*r, *s)).collect();
    let played: Vec<_> = PLAY.iter().map(|i| cards[*i].clone()).collect();
    let held: Vec<_> = cards
        .iter()
        .enumerate()
        .filter(|(i, _)| !PLAY.contains(i))
        .map(|(_, c)| c.clone())
        .collect();
    game.hand = cards.clone();
    game.full_deck = cards;

    let result = game.evaluate_selection(&played);
    scoring::score_hand(&mut game, &result, &played, &held).score()
}

#[test]
fn a_stencil_counts_every_stencil() {
    // No stencil: a plain Joker's +4 on the pair's 30 chips x 6 mult.
    assert_eq!(sim_score(&["Joker"]), 180, "no stencil");
    // One stencil reads the same whether it counts itself or adds one.
    assert_eq!(
        sim_score(&["Joker Stencil", "Joker"]),
        360,
        "one stencil is unchanged"
    );
    // Two count each other: X4 each, so X16.
    assert_eq!(
        sim_score(&["Joker Stencil", "Joker", "Joker Stencil"]),
        1440,
        "two stencils count each other"
    );
    // Three, X5 each: X125.
    assert_eq!(
        sim_score(&["Joker Stencil", "Joker Stencil", "Joker Stencil"]),
        7500,
        "three of them"
    );
}

#[test]
fn two_stencils_beat_the_old_one_for_itself_count() {
    // The bug this ports: `empty + 1` gave each X3 for two stencils in five
    // slots beside a joker (X9), where the game gives each X4 (X16).
    // 30 chips x 6 mult x 9 = 1620 is what the old formula scored.
    assert_ne!(
        sim_score(&["Joker Stencil", "Joker", "Joker Stencil"]),
        1620,
        "the old `+1 for itself` count is still being used"
    );
}
