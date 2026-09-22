//! `GameState::deep_clone` is Python's `copy.deepcopy`, and this pins it.
//!
//! The policy forks a run for every price it computes, so a *shallow* clone
//! would alias the fork back into the run and quietly change what a later
//! decision sees. There is no compile-time sign of that, and the numbers would
//! still look plausible, so the two properties are asserted directly:
//!
//! 1. the clone digests identically to the original, per observation key; and
//! 2. no `Card`, `Joker` or `Consumable` is shared with it, and mutating the
//!    clone leaves the original byte-identical.

use std::rc::Rc;

use jimbot_sim::game::{Action, GameState};
use jimbot_sim::state::{key_digests, state_dict};

/// Play a run into a state worth cloning: past the opening deal, with jokers,
/// consumables and a shop reached.
fn played_state(seed: &str, deck: &str, stake: i32) -> GameState {
    let mut game = GameState::new(seed, deck, stake);
    let mut steps = 0;
    while !game.is_over() && steps < 80 {
        let actions = game.legal_actions();
        if actions.is_empty() {
            break;
        }
        // Deterministic: take the first legal action, so the fixture is not a
        // random walk. Enough to open packs and reach a shop.
        let action: Action = actions[0].clone();
        game.step(&action);
        steps += 1;
    }
    game
}

fn digests(game: &GameState) -> std::collections::BTreeMap<String, u64> {
    key_digests(&state_dict(game, &[], 0, 0))
}

#[test]
fn a_fork_digests_like_the_run() {
    for (seed, deck, stake) in [
        ("ABCDEFGH", "Red Deck", 1),
        ("KJH7TR2M", "Blue Deck", 5),
        ("QWERTYUZ", "Checkered Deck", 8),
    ] {
        let game = played_state(seed, deck, stake);
        let fork = game.deep_clone();
        assert_eq!(
            digests(&game),
            digests(&fork),
            "{}/{}/{}: the fork is a different state",
            seed,
            deck,
            stake
        );
    }
}

#[test]
fn a_fork_shares_nothing_with_the_run() {
    let game = played_state("ABCDEFGH", "Red Deck", 1);
    let fork = game.deep_clone();

    fn cards(g: &GameState) -> Vec<usize> {
        g.full_deck
            .iter()
            .chain(g.draw_pile.iter())
            .chain(g.hand.iter())
            .chain(g.discard_pile.iter())
            .map(|c| Rc::as_ptr(c) as *const () as usize)
            .collect()
    }

    let before = cards(&game);
    for pointer in cards(&fork) {
        assert!(
            !before.contains(&pointer),
            "the fork shares a Card with the run"
        );
    }

    let jokers_before: Vec<usize> = game
        .jokers
        .iter()
        .map(|j| Rc::as_ptr(j) as *const () as usize)
        .collect();
    for joker in &fork.jokers {
        assert!(!jokers_before.contains(&(Rc::as_ptr(joker) as *const () as usize)));
    }
    let consumables_before: Vec<usize> = game
        .consumables
        .iter()
        .map(|c| Rc::as_ptr(c) as *const () as usize)
        .collect();
    for consumable in &fork.consumables {
        assert!(!consumables_before.contains(&(Rc::as_ptr(consumable) as *const () as usize)));
    }
}

#[test]
fn mutating_the_fork_leaves_the_run_alone() {
    let mut game = played_state("ABCDEFGH", "Red Deck", 1);
    let before = digests(&game);
    let money_before = game.money;
    let mut fork = game.deep_clone();

    fork.money += 1000;
    fork.chips_scored += 500;
    if let Some(card) = fork.hand.first().cloned() {
        let current = card.borrow().debuffed;
        card.borrow_mut().debuffed = !current;
    }
    if let Some(card) = fork.draw_pile.first().cloned() {
        card.borrow_mut().extra_chips += 7;
    }
    if let Some(joker) = fork.jokers.first().cloned() {
        joker.borrow_mut().counter += 3.0;
    }

    assert_eq!(game.money, money_before, "the run's money moved");
    assert_eq!(digests(&game), before, "the run's state moved");
}
