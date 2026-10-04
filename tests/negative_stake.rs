//! A negative stake is its positive twin with every sticker on.

use rumbot_sim::game::GameState;

#[test]
fn minus_n_is_n_with_every_sticker() {
    let plain = GameState::new("NEGSTAKE", "Red Deck", 3);
    let rules = plain.sticker_rules();
    assert_eq!(plain.stake, 3);
    assert!(!plain.all_stickers);
    assert!(!rules.eternals && !rules.perishables && !rules.rentals);

    let stickered = GameState::new("NEGSTAKE", "Red Deck", -3);
    let rules = stickered.sticker_rules();
    assert_eq!(stickered.stake, 3);
    assert!(stickered.all_stickers);
    assert!(rules.eternals && rules.perishables && rules.rentals);
}

#[test]
fn everything_but_the_stickers_reads_the_positive_stake() {
    // Discards (Blue and up lose one), the blind's chips (Green and Purple
    // scale faster) and the Small Blind's reward (none from Red) all come off
    // the stored, positive stake.
    let a = GameState::new("NEGSTAKE", "Red Deck", 5);
    let b = GameState::new("NEGSTAKE", "Red Deck", -5);
    assert_eq!(a.blind_scaling(), b.blind_scaling());
    assert_eq!(a.round_allowance(false), b.round_allowance(false));
    assert_eq!(
        a.blind.as_ref().map(|x| x.target),
        b.blind.as_ref().map(|x| x.target)
    );
}

#[test]
#[should_panic(expected = "-8 is stake 8")]
fn minus_eight_is_refused() {
    // Gold already rolls every sticker, so -8 was stake 8 under a second name.
    GameState::new("NEGSTAKE", "Red Deck", -8);
}
