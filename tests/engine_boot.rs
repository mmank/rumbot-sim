//! The engine constructs a run and answers the questions everything else asks.
//!
//! This is the smoke test that the module graph holds together: a `GameState`
//! boots from a seed, builds the game's own 52-card deck in the game's own
//! order, and the deck's own config lands on the run.

use rumbot_sim::cards::{rank_of, suit_of, Suit};
use rumbot_sim::game::{GameState, Phase, BASE_CONSUMABLE_SLOTS, BASE_HAND_SIZE};
use rumbot_sim::jokers;

#[test]
fn a_run_boots_from_a_seed() {
    let game = GameState::new("SEED0000", "Red Deck", 1);
    assert_eq!(game.phase, Phase::BlindSelect);
    assert!(!game.is_over());
    assert_eq!(game.full_deck.len(), 52);
    assert_eq!(game.draw_pile.len(), 52);
    // The draw pile is the whole deck, just shuffled.
    let mut uids: Vec<u64> = game
        .draw_pile
        .iter()
        .map(rumbot_sim::cards::uid_of)
        .collect();
    uids.sort();
    let mut deck_uids: Vec<u64> = game
        .full_deck
        .iter()
        .map(rumbot_sim::cards::uid_of)
        .collect();
    deck_uids.sort();
    assert_eq!(uids, deck_uids);
    assert_eq!(game.hand_size(), BASE_HAND_SIZE);
    assert_eq!(game.consumable_slots(), BASE_CONSUMABLE_SLOTS);
    assert_eq!(game.joker_slots(), 5);
    // Red Deck pays an extra discard, which is its whole effect.
    assert_eq!(game.discards_left, 4);
    assert_eq!(game.hands_left, 4);
}

#[test]
fn the_deck_a_run_is_played_with_is_applied() {
    // Yellow Deck starts with ten dollars on top of the base four.
    let yellow = GameState::new("SEED0000", "Yellow Deck", 1);
    assert_eq!(yellow.money, 14);
    // Black Deck trades a hand for a joker slot.
    let black = GameState::new("SEED0000", "Black Deck", 1);
    assert_eq!(black.hands_left, 3);
    assert_eq!(black.joker_slots(), 6);
    // Painted Deck is +2 hand size for -1 joker slot.
    let painted = GameState::new("SEED0000", "Painted Deck", 1);
    assert_eq!(painted.hand_size(), 10);
    assert_eq!(painted.joker_slots(), 4);
}

#[test]
fn the_checkered_deck_converts_in_place() {
    let game = GameState::new("SEED0000", "Checkered Deck", 1);
    assert_eq!(game.full_deck.len(), 52);
    // Its whole deck is Spades and Hearts, and every card kept its place.
    for card in &game.full_deck {
        let suit = suit_of(card);
        assert!(suit == Suit::Spades || suit == Suit::Hearts, "{:?}", suit);
    }
    // In place means the ids are the same as a plain deck's.
    let plain = GameState::new("SEED0000", "Red Deck", 1);
    let ranks: Vec<_> = game.full_deck.iter().map(|c| rank_of(c).value()).collect();
    let plain_ranks: Vec<_> = plain.full_deck.iter().map(|c| rank_of(c).value()).collect();
    assert_eq!(ranks, plain_ranks);
}

#[test]
fn an_unregistered_joker_is_a_loud_failure() {
    // "Contributed nothing at all rather than contributing wrongly" is how a
    // whole class of joker bugs stayed invisible. A missing spec must panic.
    assert!(
        jokers::spec("Joker").is_some(),
        "the spec table is filled in"
    );
    assert!(std::panic::catch_unwind(|| jokers::spec_or_panic("No Such Joker")).is_err());
}
