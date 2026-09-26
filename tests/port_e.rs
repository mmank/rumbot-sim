//! Discard, refused-hand, debuff-recheck and money-order tests, ported from the
//! Python suite.
//!
//! Each `#[test]` mirrors one `def test_x()` under
//! `external/jimbot-sim/tests/`, grouped here by the file it came from. The doc
//! comments carry the reasoning the Python test recorded -- these were written
//! over months against real divergences, and the comment is often the only
//! statement of *why* the assertion has the shape it does.
//!
//! The port is "direct": the same position is rebuilt with `GameState` and the
//! same value asserted. Where the Python test parametrises, one Rust `#[test]`
//! loops the same cases so the function name still matches.

use std::collections::BTreeMap;
use std::rc::Rc;

use jimbot_sim::blinds::{all_bosses, boss_by_name, make_blind, BlindKind, BossEffect};
use jimbot_sim::cards::{
    debuffed_of, make_card, rank_of, suit_of, uid_of, CardRef, Edition, Enhancement, Rank, Seal,
    Suit,
};
use jimbot_sim::consumables::spec_or_panic;
use jimbot_sim::game::{Action, ActionType, GameState, Phase};
use jimbot_sim::hands::HandType;
use jimbot_sim::jokers::{self, JokerRef};
use jimbot_sim::rng::{pseudohash, py_mod, round13, TW223};

// ==========================================================================
// shared builders
// ==========================================================================

fn joker(name: &str) -> JokerRef {
    jokers::make(name)
}

fn find_joker(game: &GameState, name: &str) -> JokerRef {
    game.jokers
        .iter()
        .find(|j| j.borrow().name() == name)
        .unwrap_or_else(|| panic!("no {} in the row", name))
        .clone()
}

fn counter(game: &GameState, name: &str) -> f64 {
    find_joker(game, name).borrow().counter
}

fn secondary(game: &GameState, name: &str) -> f64 {
    find_joker(game, name).borrow().secondary
}

/// Python `_run`: boot a run, gain jokers, start the round.
fn run(seed: &str, deck: &str, names: &[&str]) -> GameState {
    let mut game = GameState::new(seed, deck, 1);
    for name in names {
        game.gain_joker(&joker(name));
    }
    game._start_round();
    game
}

/// The hand levels as a comparable map, the way the Python test reads
/// `dict(game.hand_levels.levels)`.
fn levels(game: &GameState) -> BTreeMap<String, i32> {
    game.hand_levels
        .levels
        .iter()
        .map(|(hand, level)| (format!("{:?}", hand), *level))
        .collect()
}

// ==========================================================================
// tests/test_discarded_debuffed_and_stone_cards.py -- "What a discarded card
// that is debuffed, or Stone, sets off -- and what not."
// ==========================================================================

/// The first `n` cards of the hand, made into what the test needs.
fn dress(game: &GameState, n: usize, rank: Option<Rank>, debuffed: bool, stone: bool) {
    for card in &game.hand[..n] {
        let mut c = card.borrow_mut();
        if let Some(rank) = rank {
            c.rank = rank;
        }
        c.debuffed = debuffed;
        if stone {
            c.enhancement = Enhancement::Stone;
        }
    }
}

fn discard_indices(game: &mut GameState, n: usize) -> i64 {
    let before = game.money;
    game._discard(&(0..n).collect::<Vec<usize>>());
    (game.money - before) as i64
}

#[test]
fn test_mail_in_rebate_pays_only_for_a_live_card_of_its_rank() {
    // card.lua:2825-2828: `not context.other_card.debuff and
    // context.other_card:get_id() == G.GAME.current_round.mail_card.id`.
    // A debuffed card pays nothing; a Stone card's get_id is a negative random
    // number (card.lua:958-960), so it matches no rank either.
    for (debuffed, stone, paid) in [(false, false, 5), (true, false, 0), (false, true, 0)] {
        let mut game = run("TESTSEED", "Red Deck", &["Mail-In Rebate"]);
        game.mail_rank = Some(Rank::King);
        dress(&game, 1, Some(Rank::King), debuffed, stone);
        assert_eq!(
            discard_indices(&mut game, 1),
            paid,
            "stone={stone} debuffed={debuffed}"
        );
    }
}

#[test]
fn test_mail_in_rebate_counts_the_live_ones_beside_a_debuffed_one() {
    // Per card: the check is inside the loop at state_events.lua:399-404.
    // A debuffed King and a Stone King pay nothing; the live King pays $5.
    let mut game = run("TESTSEED", "Red Deck", &["Mail-In Rebate"]);
    game.mail_rank = Some(Rank::King);
    dress(&game, 3, Some(Rank::King), false, false);
    game.hand[0].borrow_mut().debuffed = true;
    game.hand[1].borrow_mut().enhancement = Enhancement::Stone;
    assert_eq!(discard_indices(&mut game, 3), 5);
}

#[test]
fn test_hit_the_road_grows_only_on_a_live_jack() {
    // card.lua:2835-2838.
    for (debuffed, stone, gain) in [(false, false, 0.5), (true, false, 0.0), (false, true, 0.0)] {
        let mut game = run("TESTSEED", "Red Deck", &["Hit the Road"]);
        let before = counter(&game, "Hit the Road");
        dress(&game, 1, Some(Rank::Jack), debuffed, stone);
        game._discard(&[0]);
        let grown = counter(&game, "Hit the Road") - before;
        assert!(
            (grown - gain).abs() < 1e-9,
            "stone={stone} debuffed={debuffed}: {grown} != {gain}"
        );
    }
}

fn faceless(row: &[&str], rank: Rank, debuffed: &[usize], stone: &[usize]) -> i64 {
    let names: Vec<&str> = std::iter::once("Faceless Joker")
        .chain(row.iter().copied())
        .collect();
    let mut game = run("TESTSEED", "Red Deck", &names);
    dress(&game, 3, Some(rank), false, false);
    for &i in debuffed {
        game.hand[i].borrow_mut().debuffed = true;
    }
    for &i in stone {
        game.hand[i].borrow_mut().enhancement = Enhancement::Stone;
    }
    discard_indices(&mut game, 3)
}

#[test]
fn test_faceless_joker_pays_for_three_live_face_cards() {
    assert_eq!(faceless(&[], Rank::King, &[], &[]), 5);
}

#[test]
fn test_faceless_joker_does_not_count_a_debuffed_face_card() {
    // card.lua:2861 `v:is_face()`, and card.lua:965
    // `if self.debuff and not from_boss then return end`.
    assert_eq!(faceless(&[], Rank::King, &[0], &[]), 0);
}

#[test]
fn test_faceless_joker_does_not_count_a_stone_king() {
    // card.lua:966-967: a Stone card's get_id is negative (card.lua:958).
    assert_eq!(faceless(&[], Rank::King, &[], &[0]), 0);
}

#[test]
fn test_faceless_joker_with_pareidolia_counts_any_card() {
    // card.lua:967 `... or next(find_joker("Pareidolia"))` -- a Two is a face
    // card, and so is a Stone card, whatever get_id drew.
    assert_eq!(faceless(&["Pareidolia"], Rank::Two, &[], &[1]), 5);
}

#[test]
fn test_pareidolia_does_not_lift_the_debuff() {
    // card.lua:965 returns before card.lua:967 is reached.
    assert_eq!(faceless(&["Pareidolia"], Rank::Two, &[2], &[]), 0);
}

#[test]
fn test_a_purple_seal_makes_a_tarot_only_when_the_card_is_live() {
    // card.lua:2242-2243 `if self.debuff then return nil end` comes before the
    // Purple Seal's branch at card.lua:2253-2254.
    for (debuffed, made) in [(false, 1), (true, 0)] {
        let mut game = run("TESTSEED", "Red Deck", &[]);
        game.consumables.clear();
        dress(&game, 1, None, debuffed, false);
        game.hand[0].borrow_mut().seal = Seal::Purple;
        game._discard(&[0]);
        assert_eq!(game.consumables.len(), made, "debuffed={debuffed}");
    }
}

#[test]
fn test_these_count_a_debuffed_card_all_the_same() {
    // None of these branches tests other_card.debuff, so a hand of debuffed
    // cards moves them exactly as a live one does.
    //
    //   Ramen            card.lua:2757
    //   Yorick           card.lua:2788
    //   Trading Card     card.lua:2802
    //   Green Joker      card.lua:2846
    //   Burnt Joker      card.lua:2749 pre_discard
    #[derive(Debug, PartialEq)]
    enum Read {
        Counter(f64),
        Secondary(f64),
        Trading((i64, usize)),
        Levels(BTreeMap<String, i32>),
    }

    let cases: [(&str, usize, Option<Rank>); 5] = [
        ("Ramen", 3, None),
        ("Yorick", 3, None),
        ("Trading Card", 1, None),
        ("Green Joker", 3, None),
        ("Burnt Joker", 2, Some(Rank::Nine)),
    ];

    for (name, n, rank) in cases {
        let read = |game: &GameState| -> Read {
            match name {
                "Ramen" | "Green Joker" => Read::Counter(counter(game, name)),
                "Yorick" => Read::Secondary(secondary(game, name)),
                "Trading Card" => Read::Trading((game.money as i64, game.full_deck.len())),
                "Burnt Joker" => Read::Levels(levels(game)),
                other => panic!("unhandled {other}"),
            }
        };
        let mut outcomes = Vec::new();
        for debuffed in [false, true] {
            let mut game = run("TESTSEED", "Red Deck", &[name]);
            if name == "Green Joker" {
                find_joker(&game, name).borrow_mut().counter = 3.0;
            }
            dress(&game, n, rank, debuffed, false);
            let before = read(&game);
            game._discard(&(0..n).collect::<Vec<usize>>());
            outcomes.push((before, read(&game)));
        }
        assert_ne!(
            outcomes[0].0, outcomes[0].1,
            "{name}: the live case did not move"
        );
        assert_eq!(
            outcomes[1], outcomes[0],
            "{name}: a debuffed card moved it differently"
        );
    }
}

#[test]
fn test_castle_skips_a_debuffed_card_of_its_suit() {
    // card.lua:2815 (and is_suit's own debuff test, card.lua:4077).
    let mut game = run("TESTSEED", "Red Deck", &["Castle"]);
    let before = counter(&game, "Castle");
    dress(&game, 2, None, false, false);
    let castle_suit = game.castle_suit.unwrap();
    for card in &game.hand[..2] {
        card.borrow_mut().suit = castle_suit;
    }
    game.hand[0].borrow_mut().debuffed = true;
    game._discard(&[0, 1]);
    assert_eq!(counter(&game, "Castle") - before, 3.0);
}

// ==========================================================================
// tests/test_refused_hand_after_pass.py -- "A hand the boss refuses still runs
// the after-hand pass, and nothing else."
//
// G.FUNCS.evaluate_play splits on `if not G.GAME.blind:debuff_hand(...)`
// (state_events.lua:614). Everything inside the first branch is skipped for a
// refused hand: the `before` joker pass (628-638), the cards, the jokers' main
// effects, and the destroying pass with its glass roll (950-996). The refused
// branch asks every joker `debuffed_hand` (1015-1027) -- and then, outside the
// `if`, for every hand, comes the `after` pass (1068-1075).
//
// Two jokers answer `after`: Ice Cream (card.lua:3571) and Seltzer
// (card.lua:3601). Everything else that grows on a played hand is `before`.
// Sixth Sense answers `destroying_card` (card.lua:2603-2604), inside the block.
//
// The simulator ran its after-hand updates only inside score_hand, which a
// refused hand never reaches. Seed VJPW2C6Z, Anaglyph Deck, stake 8 on the
// headless engine at decision 46: a Three of a Kind The Mouth zeroed took the
// game's Ice Cream from 40 to 35 and left the simulator's on 40.
// ==========================================================================

fn mouth() -> &'static BossEffect {
    boss_by_name("The Mouth").unwrap()
}

fn psychic() -> &'static BossEffect {
    boss_by_name("The Psychic").unwrap()
}

const PAIR: [(Rank, Suit); 5] = [
    (Rank::Nine, Suit::Hearts),
    (Rank::Nine, Suit::Diamonds),
    (Rank::Five, Suit::Hearts),
    (Rank::Four, Suit::Clubs),
    (Rank::Two, Suit::Clubs),
];
const TRIPS: [(Rank, Suit); 5] = [
    (Rank::Seven, Suit::Hearts),
    (Rank::Seven, Suit::Clubs),
    (Rank::Seven, Suit::Diamonds),
    (Rank::Five, Suit::Diamonds),
    (Rank::Four, Suit::Spades),
];

fn on_boss(seed: &str, boss: &'static BossEffect, names: &[&str]) -> GameState {
    let mut game = GameState::new(seed, "Red Deck", 1);
    for name in names {
        game.gain_joker(&joker(name));
    }
    game.ante_boss = String::new();
    game.blind = Some(make_blind(
        BlindKind::Boss,
        game.ante,
        Some(boss),
        1.0,
        1,
        false,
    ));
    game._start_round();
    game.blind.as_mut().unwrap().target = 1_000_000_000_000; // never cleared
    game.hands_left = 10;
    game
}

/// Chips the play adds to the round.
fn play(game: &mut GameState, specs: &[(Rank, Suit)]) -> i64 {
    game.hand = specs.iter().map(|(r, s)| make_card(*r, *s)).collect();
    let before = game.chips_scored;
    game.step(&Action::with_cards(
        ActionType::Play,
        (0..specs.len()).collect(),
    ));
    game.chips_scored - before
}

#[test]
fn test_ice_cream_melts_on_a_refused_hand() {
    // card.lua:3571-3594 under context.after (state_events.lua:1068-1070).
    let mut game = on_boss("VJPW2C6Z", mouth(), &["Ice Cream"]);
    assert!(play(&mut game, &PAIR) > 0);
    assert_eq!(counter(&game, "Ice Cream"), 95.0);
    assert_eq!(play(&mut game, &TRIPS), 0);
    assert_eq!(counter(&game, "Ice Cream"), 90.0);
}

#[test]
fn test_seltzer_counts_a_refused_hand() {
    // card.lua:3601-3628, the same `after` pass.
    let mut game = on_boss("VJPW2C6Z", mouth(), &["Seltzer"]);
    play(&mut game, &PAIR);
    assert_eq!(counter(&game, "Seltzer"), 9.0);
    assert_eq!(play(&mut game, &TRIPS), 0);
    assert_eq!(counter(&game, "Seltzer"), 8.0);
}

#[test]
fn test_ice_cream_melts_away_on_a_refused_hand() {
    // `extra.chips - chip_mod <= 0` eats it, refused hand or not.
    let mut game = on_boss("VJPW2C6Z", mouth(), &["Ice Cream"]);
    play(&mut game, &PAIR);
    game.jokers[0].borrow_mut().counter = 5.0;
    play(&mut game, &TRIPS);
    assert!(game.jokers.is_empty());
}

#[test]
fn test_a_copier_does_not_melt_it_twice() {
    // Both branches are `not context.blueprint`.
    let mut game = on_boss("VJPW2C6Z", mouth(), &["Blueprint", "Ice Cream"]);
    play(&mut game, &PAIR);
    play(&mut game, &TRIPS);
    assert_eq!(counter(&game, "Ice Cream"), 90.0);
}

#[test]
fn test_a_debuffed_ice_cream_does_not_melt() {
    // calculate_joker returns nil for a debuffed joker (card.lua:2291-2292).
    let mut game = on_boss("VJPW2C6Z", mouth(), &["Ice Cream"]);
    play(&mut game, &PAIR);
    let ice_cream = find_joker(&game, "Ice Cream");
    game.set_joker_debuff(&ice_cream, true);
    play(&mut game, &TRIPS);
    assert_eq!(counter(&game, "Ice Cream"), 95.0);
}

#[test]
fn test_before_jokers_do_not_move_on_a_refused_hand() {
    // Green Joker (card.lua:3563) and Ride the Bus (3525) are `before`,
    // skipped with the rest of the block (state_events.lua:614, 628-638).
    let mut game = on_boss("VJPW2C6Z", mouth(), &["Green Joker", "Ride the Bus"]);
    play(&mut game, &PAIR);
    assert_eq!(counter(&game, "Green Joker"), 1.0);
    assert_eq!(counter(&game, "Ride the Bus"), 1.0);
    play(&mut game, &TRIPS);
    assert_eq!(counter(&game, "Green Joker"), 1.0);
    assert_eq!(counter(&game, "Ride the Bus"), 1.0);
}

/// Play one card of `rank`, taken from the deck so it belongs to the run.
fn one_card(game: &mut GameState, rank: Rank, enhancement: Enhancement) -> (CardRef, i64) {
    let card = game
        .draw_pile
        .iter()
        .chain(game.hand.iter())
        .find(|c| rank_of(c) == rank)
        .cloned()
        .unwrap_or_else(|| panic!("no {rank:?} in the deck"));
    if let Some(pos) = game
        .draw_pile
        .iter()
        .position(|c| uid_of(c) == uid_of(&card))
    {
        game.draw_pile.remove(pos);
    }
    game.hand.insert(0, card.clone());
    card.borrow_mut().enhancement = enhancement;
    let index = game
        .hand
        .iter()
        .position(|c| uid_of(c) == uid_of(&card))
        .unwrap();
    let before = game.chips_scored;
    game.step(&Action::with_cards(ActionType::Play, vec![index]));
    (card, game.chips_scored - before)
}

#[test]
fn test_dna_copies_nothing_the_psychic_refuses() {
    // DNA is `context.before` (card.lua:3501-3524): one card into The
    // Psychic is refused, and the deck does not grow.
    let mut game = on_boss("VJPW2C6Z", psychic(), &["DNA"]);
    let deck = game.full_deck.len();
    let (_, scored) = one_card(&mut game, Rank::King, Enhancement::None);
    assert_eq!(scored, 0);
    assert_eq!(game.full_deck.len(), deck);
}

#[test]
fn test_dna_still_copies_an_allowed_card() {
    let mut game = on_boss("VJPW2C6Z", psychic(), &["DNA"]);
    game.blind.as_mut().unwrap().disabled = true;
    let deck = game.full_deck.len();
    one_card(&mut game, Rank::King, Enhancement::None);
    assert_eq!(game.full_deck.len(), deck + 1);
}

#[test]
fn test_sixth_sense_takes_nothing_the_psychic_refuses() {
    // Sixth Sense is `context.destroying_card` (card.lua:2603-2604), asked
    // at state_events.lua:957 inside the block.
    let mut game = on_boss("VJPW2C6Z", psychic(), &["Sixth Sense"]);
    let deck = game.full_deck.len();
    let (card, scored) = one_card(&mut game, Rank::Six, Enhancement::None);
    assert_eq!(scored, 0);
    assert!(game.full_deck.iter().any(|c| uid_of(c) == uid_of(&card)));
    assert_eq!(game.full_deck.len(), deck);
    assert!(game.consumables.is_empty());
    assert!(!game.rng.pools.contains_key("sixth"));
}

#[test]
fn test_a_refused_glass_card_does_not_roll() {
    // The glass roll (state_events.lua:961) is inside the block too, so a
    // refused hand leaves the 'glass' stream where it was.
    let mut game = on_boss("VJPW2C6Z", psychic(), &[]);
    let deck = game.full_deck.len();
    for _ in 0..3 {
        one_card(&mut game, Rank::Ace, Enhancement::Glass);
    }
    assert!(!game.rng.pools.contains_key("glass"));
    assert_eq!(game.full_deck.len(), deck);
}

// ==========================================================================
// tests/test_debuff_recheck_on_card_change.py -- "A card changed mid-round is
// asked Blind:debuff_card again, then and there."
//
// Every write to a playing card's identity ends by re-running the boss's debuff
// test on that one card:
//
//   * Card:set_ability -- The Lovers, The Chariot, Justice, The Devil, The
//     Tower, Midas Mask, Vampire -- ends `if not initial then
//     G.GAME.blind:debuff_card(self) end` (card.lua:365);
//   * Card:set_base -- Strength, Sigil, Ouija -- ends with the same line
//     (card.lua:143);
//   * Card:change_suit -- The Star, The Moon, The Sun, The World -- calls it
//     unconditionally (card.lua:561);
//   * copy_card -- Death -- writes `new_card.debuff = other.debuff`
//     (common_events.lua:2178), so the left card takes the right card's flag.
//
// The simulator only re-evaluated debuffs at the start of the round and before
// a play, so a discard in between saw the old flags (5LPYZ3QU, Magic Deck,
// stake 1, ante 6, The Window: Castle +6 in the game, +9 here).
// ==========================================================================

/// A boss round holding exactly `hand`, debuffed as the boss would.
fn boss_round(boss: &'static BossEffect, hand: Vec<CardRef>, names: &[&str]) -> GameState {
    let mut game = GameState::new("TESTSEED", "Red Deck", 1);
    for name in names {
        game.gain_joker(&joker(name));
    }
    game.ante_boss = String::new();
    game.blind = Some(make_blind(
        BlindKind::Boss,
        game.ante,
        Some(boss),
        1.0,
        1,
        false,
    ));
    game._start_round();
    game.blind.as_mut().unwrap().target = 1_000_000_000_000; // never beaten
    game.hand = hand.clone();
    game.full_deck.extend(hand);
    game._apply_debuffs();
    game
}

fn discard_cards(game: &mut GameState, cards: &[CardRef]) {
    let indices: Vec<usize> = cards
        .iter()
        .map(|c| {
            game.hand
                .iter()
                .position(|h| uid_of(h) == uid_of(c))
                .unwrap()
        })
        .collect();
    game._discard(&indices);
}

fn window() -> &'static BossEffect {
    boss_by_name("The Window").unwrap() // debuffs Diamonds
}

fn plant() -> &'static BossEffect {
    boss_by_name("The Plant").unwrap() // debuffs face cards
}

#[test]
fn test_the_lovers_under_the_window_debuffs_the_wild_card_it_makes() {
    // card.lua:1143 set_ability -> card.lua:365 -> blind.lua:626, and
    // card.lua:4081 makes a Wild card a Diamond. Castle skips it
    // (card.lua:2815). The 5LPYZ3QU stop, reduced.
    let eight = make_card(Rank::Eight, Suit::Clubs);
    let six = make_card(Rank::Six, Suit::Clubs);
    let five = make_card(Rank::Five, Suit::Clubs);
    let mut game = boss_round(
        window(),
        vec![
            make_card(Rank::Ten, Suit::Hearts),
            eight.clone(),
            six.clone(),
            five.clone(),
            make_card(Rank::Two, Suit::Spades),
        ],
        &["Castle"],
    );
    game.castle_suit = Some(Suit::Clubs);
    assert!(!debuffed_of(&eight));
    game.use_consumable(spec_or_panic("The Lovers"), &[eight.clone()], false);
    assert_eq!(eight.borrow().enhancement, Enhancement::Wild);
    assert!(debuffed_of(&eight));
    let first = game.hand[0].clone();
    discard_cards(&mut game, &[first, eight, six, five]);
    assert_eq!(counter(&game, "Castle"), 6.0);
}

#[test]
fn test_the_star_debuffs_a_card_it_turns_into_a_diamond() {
    // card.lua:561: change_suit re-runs debuff_card.
    let club = make_card(Rank::Nine, Suit::Clubs);
    let mut game = boss_round(
        window(),
        vec![club.clone(), make_card(Rank::Two, Suit::Spades)],
        &["Castle"],
    );
    game.castle_suit = Some(Suit::Diamonds);
    game.use_consumable(spec_or_panic("The Star"), &[club.clone()], false);
    assert_eq!(suit_of(&club), Suit::Diamonds);
    assert!(debuffed_of(&club));
    discard_cards(&mut game, &[club]);
    assert_eq!(counter(&game, "Castle"), 0.0);
}

#[test]
fn test_the_sun_releases_a_diamond_it_turns_into_a_heart() {
    // card.lua:561, the other way: blind.lua:653 set_debuff(false).
    let diamond = make_card(Rank::Nine, Suit::Diamonds);
    let mut game = boss_round(
        window(),
        vec![diamond.clone(), make_card(Rank::Two, Suit::Spades)],
        &["Castle"],
    );
    game.castle_suit = Some(Suit::Hearts);
    assert!(debuffed_of(&diamond));
    game.use_consumable(spec_or_panic("The Sun"), &[diamond.clone()], false);
    assert_eq!(suit_of(&diamond), Suit::Hearts);
    assert!(!debuffed_of(&diamond));
    discard_cards(&mut game, &[diamond]);
    assert_eq!(counter(&game, "Castle"), 3.0);
}

#[test]
fn test_the_tower_releases_a_diamond_it_turns_to_stone() {
    // card.lua:365, then blind.lua:626 -- a Stone card is no suit
    // (card.lua:4078) -- so the card falls through to set_debuff(false).
    let diamond = make_card(Rank::Nine, Suit::Diamonds);
    let mut game = boss_round(
        window(),
        vec![diamond.clone(), make_card(Rank::Two, Suit::Spades)],
        &[],
    );
    assert!(debuffed_of(&diamond));
    game.use_consumable(spec_or_panic("The Tower"), &[diamond.clone()], false);
    assert!(!debuffed_of(&diamond));
}

#[test]
fn test_strength_under_the_plant_debuffs_a_ten_it_makes_a_jack() {
    // card.lua:1128 set_base -> card.lua:143 -> blind.lua:630.
    let ten = make_card(Rank::Ten, Suit::Spades);
    let king = make_card(Rank::King, Suit::Hearts);
    let mut game = boss_round(
        plant(),
        vec![ten.clone(), king.clone(), make_card(Rank::Two, Suit::Clubs)],
        &[],
    );
    assert!(!debuffed_of(&ten));
    assert!(debuffed_of(&king));
    game.use_consumable(spec_or_panic("Strength"), &[ten.clone()], false);
    assert_eq!(rank_of(&ten), Rank::Jack);
    assert!(debuffed_of(&ten));
    game.use_consumable(spec_or_panic("Strength"), &[king.clone()], false);
    assert_eq!(rank_of(&king), Rank::Ace);
    assert!(!debuffed_of(&king));
}

#[test]
fn test_sigil_rechecks_every_card_in_hand() {
    // card.lua:1242 set_base on every held card, whichever suit is drawn.
    let hand = vec![
        make_card(Rank::Nine, Suit::Diamonds),
        make_card(Rank::Four, Suit::Clubs),
        make_card(Rank::Six, Suit::Hearts),
    ];
    let mut game = boss_round(window(), hand.clone(), &[]);
    game.use_consumable(spec_or_panic("Sigil"), &[], false);
    let suit = suit_of(&hand[0]);
    assert!(hand.iter().all(|c| suit_of(c) == suit));
    let want = suit == Suit::Diamonds;
    assert!(hand.iter().all(|c| debuffed_of(c) == want));
}

#[test]
fn test_ouija_rechecks_every_card_in_hand() {
    // card.lua:1256 set_base on every held card, whichever rank is drawn.
    let hand = vec![
        make_card(Rank::King, Suit::Diamonds),
        make_card(Rank::Four, Suit::Clubs),
    ];
    let mut game = boss_round(plant(), hand.clone(), &[]);
    game.use_consumable(spec_or_panic("Ouija"), &[], false);
    let rank = rank_of(&hand[0]);
    assert!(hand.iter().all(|c| rank_of(c) == rank));
    let want = rank.is_face();
    assert!(hand.iter().all(|c| debuffed_of(c) == want));
}

#[test]
fn test_death_copies_the_right_cards_debuff() {
    // common_events.lua:2178, `new_card.debuff = other.debuff`.
    let left = make_card(Rank::Five, Suit::Clubs);
    let right = make_card(Rank::Nine, Suit::Diamonds);
    let mut game = boss_round(window(), vec![left.clone(), right.clone()], &[]);
    assert!(debuffed_of(&right));
    assert!(!debuffed_of(&left));
    game.use_consumable(
        spec_or_panic("Death"),
        &[left.clone(), right.clone()],
        false,
    );
    assert_eq!(suit_of(&left), Suit::Diamonds);
    assert!(debuffed_of(&left));

    // The other way: a Diamond spent on a Club takes the Club's live flag.
    let left = make_card(Rank::Nine, Suit::Diamonds);
    let right = make_card(Rank::Five, Suit::Clubs);
    let mut game = boss_round(window(), vec![left.clone(), right.clone()], &[]);
    game.use_consumable(
        spec_or_panic("Death"),
        &[left.clone(), right.clone()],
        false,
    );
    assert_eq!(suit_of(&left), Suit::Clubs);
    assert!(!debuffed_of(&left));
}

#[test]
fn test_no_boss_no_debuff() {
    // Out of a boss round the blind has no debuff table (blind.lua:85), and
    // debuff_card ends set_debuff(false) (blind.lua:653).
    let mut game = GameState::new("TESTSEED", "Red Deck", 1);
    game._start_round();
    let card = game.hand[0].clone();
    card.borrow_mut().suit = Suit::Diamonds;
    game.use_consumable(spec_or_panic("The Lovers"), &[card.clone()], false);
    assert_eq!(card.borrow().enhancement, Enhancement::Wild);
    assert!(!debuffed_of(&card));
}

// ==========================================================================
// tests/test_native_rng.py -- "The native RNG must agree with the Python one
// bit for bit."
//
// The Python test compares the hand-written C `native` module against the pure
// Python `rng` module. Rust has no `native` module -- `rng.rs` *is* the port of
// `rng.py` -- so the behaviour being pinned is `rng`'s own contract, replayed
// from `tools/gen_native_rng_fixture.py` (values produced by the Python `rng`
// module, `%.17g` so bits compare exactly). The two layers compound --
// pseudohash feeds pseudoseed feeds math.randomseed feeds a Tausworthe
// generator -- so a last-bit difference becomes a different joker in the shop.
// A port that is merely close passes no record.
//
// The Python test is skipped when the C library has not been built; there is no
// such skip here because the Rust RNG is always present.
// ==========================================================================

const NATIVE_RNG: &str = include_str!("fixtures/native_rng.txt");

fn rng_bits_eq(got: f64, want: f64, what: &str) {
    assert_eq!(
        got.to_bits(),
        want.to_bits(),
        "{}: got {:?} ({:016x}) want {:?} ({:016x})",
        what,
        got,
        got.to_bits(),
        want,
        want.to_bits()
    );
}

#[test]
fn test_pseudohash_matches_on_the_pools_the_game_names() {
    // The literal keys first: these are the ones a divergence would ruin. The
    // fixture carries them as the first 15 `hash` records.
    let mut checked = 0;
    for line in NATIVE_RNG.lines().filter(|l| l.starts_with("hash\t")) {
        let f: Vec<&str> = line.split('\t').collect();
        rng_bits_eq(pseudohash(f[1]), f[2].parse().unwrap(), line);
        checked += 1;
        if checked == 15 {
            break;
        }
    }
    assert_eq!(checked, 15);
}

#[test]
fn test_pseudohash_matches_on_pool_names_joined_to_seeds() {
    // What pseudoseed actually hashes: the key with the run's seed appended.
    let mut checked = 0;
    for line in NATIVE_RNG.lines().filter(|l| l.starts_with("hash\t")) {
        let f: Vec<&str> = line.split('\t').collect();
        rng_bits_eq(pseudohash(f[1]), f[2].parse().unwrap(), line);
        checked += 1;
    }
    assert_eq!(checked, 3015, "15 literal keys plus 3,000 random strings");
}

#[test]
fn test_round13_matches() {
    let mut checked = 0;
    for line in NATIVE_RNG.lines().filter(|l| l.starts_with("round13\t")) {
        let f: Vec<&str> = line.split('\t').collect();
        let value: f64 = f[1].parse().unwrap();
        rng_bits_eq(round13(value), f[2].parse().unwrap(), line);
        checked += 1;
    }
    assert_eq!(checked, 5006);
}

#[test]
fn test_the_pool_step_matches() {
    // pseudoseed's advance, which every named draw goes through.
    let mut checked = 0;
    for line in NATIVE_RNG.lines().filter(|l| l.starts_with("poolstep\t")) {
        let f: Vec<&str> = line.split('\t').collect();
        let state: f64 = f[1].parse().unwrap();
        let got = round13(py_mod(2.134453429141 + state * 1.72431234, 1.0)).abs();
        rng_bits_eq(got, f[2].parse().unwrap(), line);
        checked += 1;
    }
    assert_eq!(checked, 3000);
}

#[test]
fn test_the_generator_matches_draw_for_draw() {
    let mut checked = 0;
    for line in NATIVE_RNG.lines().filter(|l| l.starts_with("tw223\t")) {
        let f: Vec<&str> = line.split('\t').collect();
        let seed: f64 = f[1].parse().unwrap();
        let want: Vec<f64> = f[2].split(',').map(|s| s.parse().unwrap()).collect();
        let mut generator = TW223::new(seed);
        let got: Vec<f64> = (0..want.len()).map(|_| generator.step()).collect();
        assert_eq!(got.len(), want.len(), "{line}");
        for (draw, (g, w)) in got.iter().zip(want.iter()).enumerate() {
            rng_bits_eq(*g, *w, &format!("{line} [draw {draw}]"));
        }
        checked += 1;
    }
    assert_eq!(checked, 120);
}

#[test]
fn test_the_draw_forms_match() {
    // random(), random(n) and random(a, b) -- the three shapes Lua offers.
    let mut checked = 0;
    for line in NATIVE_RNG.lines().filter(|l| l.starts_with("drawforms\t")) {
        let f: Vec<&str> = line.split('\t').collect();
        let seed: f64 = f[1].parse().unwrap();
        let want: Vec<f64> = f[2].split(',').map(|s| s.parse().unwrap()).collect();
        let mut generator = TW223::new(seed);
        let got = [
            generator.random(None, None),
            generator.random(Some(52.0), None),
            generator.random(Some(1.0), Some(6.0)),
        ];
        for (i, (g, w)) in got.iter().zip(want.iter()).enumerate() {
            rng_bits_eq(*g, *w, &format!("{line} [form {i}]"));
        }
        checked += 1;
    }
    assert_eq!(checked, 60);
}

// ==========================================================================
// tests/test_dollar_bonus_rules.py -- "The cash-out screen's joker rows:
// Satellite's count, and debuffed jokers."
//
// Satellite (card.lua:1667-1674) pays $1 for every entry of
// G.GAME.consumeable_usage whose set is Planet -- one per *distinct* Planet used
// this run, however often each was used. set_consumeable_usage
// (functions/misc_functions.lua:1184-1196) writes that table from
// Card:use_consumeable (card.lua:1093). The simulator declared `unique_planets`
// and never wrote it (6F5UPNKL and BUYS36P1 cashed out a dollar short).
//
// A debuffed joker pays no row: calculate_dollar_bonus opens `if self.debuff
// then return end` (card.lua:1656). A perishable is debuffed by
// calculate_perishable in end_round (state_events.lua:109) before the rows are
// built -- so a Golden Joker in its last round pays nothing (TLCIZGT5, $4 over).
// ==========================================================================

/// What one joker has been paid at each cash-out, from the run's log.
fn paid(game: &GameState, name: &str) -> Vec<i64> {
    let prefix = format!("{name}: +$");
    game.logs
        .iter()
        .filter(|line| line.starts_with(&prefix))
        .map(|line| line.split("+$").nth(1).unwrap().parse::<i64>().unwrap())
        .collect()
}

fn round_end(game: &mut GameState, kind: BlindKind) {
    game.blind = Some(make_blind(kind, game.ante, None, 1.0, 1, false));
    game._beat_blind(true);
    game._cash_out();
}

#[test]
fn test_satellite_pays_nothing_before_a_planet() {
    let mut game = run("TESTSEED", "Red Deck", &["Satellite"]);
    round_end(&mut game, BlindKind::Small);
    assert!(paid(&game, "Satellite").is_empty());
}

#[test]
fn test_satellite_pays_a_dollar_per_distinct_planet() {
    let mut game = run("TESTSEED", "Red Deck", &["Satellite"]);
    for name in ["Mercury", "Mercury", "Venus"] {
        game.use_consumable(spec_or_panic(name), &[], false);
    }
    round_end(&mut game, BlindKind::Small);
    assert_eq!(paid(&game, "Satellite"), vec![2]);
}

#[test]
fn test_satellite_does_not_count_tarots_or_spectrals() {
    let mut game = run("TESTSEED", "Red Deck", &["Satellite"]);
    game.use_consumable(spec_or_panic("Pluto"), &[], false);
    game.use_consumable(spec_or_panic("Black Hole"), &[], false);
    round_end(&mut game, BlindKind::Small);
    assert_eq!(paid(&game, "Satellite"), vec![1]);
}

#[test]
fn test_a_golden_joker_pays_four() {
    let mut game = run("TESTSEED", "Red Deck", &["Golden Joker"]);
    round_end(&mut game, BlindKind::Small);
    assert_eq!(paid(&game, "Golden Joker"), vec![4]);
}

#[test]
fn test_a_golden_joker_that_perishes_this_round_pays_nothing() {
    let mut game = run("TESTSEED", "Red Deck", &["Golden Joker"]);
    {
        let mut joker = game.jokers[0].borrow_mut();
        joker.perishable = true;
        joker.perish_tally = 1;
    }
    round_end(&mut game, BlindKind::Small);
    assert!(game.jokers[0].borrow().debuffed);
    assert!(paid(&game, "Golden Joker").is_empty());
}

#[test]
fn test_a_debuffed_rocket_pays_nothing() {
    let mut game = run("TESTSEED", "Red Deck", &["Rocket"]);
    game.jokers[0].borrow_mut().debuffed = true;
    round_end(&mut game, BlindKind::Small);
    assert!(paid(&game, "Rocket").is_empty());
}

// ==========================================================================
// tests/test_todo_list_pays_before_the_jokers.py -- "To Do List's $4 is in hand
// before any joker's main effect reads the money."
//
// The list pays under `context.before` (card.lua:3491-3499):
//
//     ease_dollars(self.ability.extra.dollars)
//     G.GAME.dollar_buffer = ... + self.ability.extra.dollars
//
// and evaluate_play runs that pass over the whole row before the cards score
// (state_events.lua:628-638). Bootstraps (card.lua:4046) and Bull (3936) read
// `G.GAME.dollars + G.GAME.dollar_buffer` in the joker_main pass, so both count
// the $4 wherever the list sits. The branch has no `not context.blueprint`, so a
// Blueprint copying the list pays another $4 in the same pass.
//
// The simulator paid it from the main pass in row order, which left a Bootstraps
// to the list's left reading the money from before the hand (2MIUP34I, Zodiac
// Deck, stake 8: the game scored 46 x 31 = 1426, the shadow 46 x 29 = 1334).
// ==========================================================================

const ACE: [(Rank, Suit); 1] = [(Rank::Ace, Suit::Spades)]; // High Card: 5 + 11 chips, 1 mult

fn todo_game(seed: &str, names: &[&str], money: i32, named: HandType) -> GameState {
    let mut game = GameState::new(seed, "Red Deck", 1);
    for name in names {
        game.gain_joker(&joker(name));
    }
    game._start_round();
    game.blind.as_mut().unwrap().target = 1_000_000_000_000;
    game.hands_left = 10;
    if game
        .jokers
        .iter()
        .any(|j| j.borrow().name() == "To Do List")
    {
        find_joker(&game, "To Do List").borrow_mut().named_hand = Some(named);
    }
    game.money = money;
    game
}

fn play_ace(game: &mut GameState) -> i64 {
    game.hand = ACE.iter().map(|(r, s)| make_card(*r, *s)).collect();
    let before = game.chips_scored;
    game.step(&Action::with_cards(ActionType::Play, vec![0]));
    game.chips_scored - before
}

#[test]
fn test_bootstraps_left_of_the_list_counts_its_four_dollars() {
    let mut game = todo_game(
        "2MIUP34I",
        &["Bootstraps", "To Do List"],
        3,
        HandType::HighCard,
    );
    assert_eq!(play_ace(&mut game), 16 * (1 + 2)); // $3 + $4 = $7: one lot of +2
    assert_eq!(game.money, 7);
}

#[test]
fn test_the_order_of_the_row_does_not_matter() {
    let mut game = todo_game(
        "2MIUP34I",
        &["To Do List", "Bootstraps"],
        3,
        HandType::HighCard,
    );
    assert_eq!(play_ace(&mut game), 16 * 3);
}

#[test]
fn test_a_blueprint_on_the_list_pays_in_the_same_pass() {
    let mut game = todo_game(
        "2MIUP34I",
        &["Bootstraps", "Blueprint", "To Do List"],
        2,
        HandType::HighCard,
    );
    assert_eq!(play_ace(&mut game), 16 * (1 + 4)); // $2 + $4 + $4 = $10
    assert_eq!(game.money, 10);
}

#[test]
fn test_bull_counts_the_four_dollars_too() {
    let mut game = todo_game("2MIUP34I", &["Bull", "To Do List"], 1, HandType::HighCard);
    assert_eq!(play_ace(&mut game), (16 + 2 * 5) * 1);
}

#[test]
fn test_a_hand_the_list_does_not_name_pays_nothing() {
    let mut game = todo_game("2MIUP34I", &["Bootstraps", "To Do List"], 3, HandType::Pair);
    assert_eq!(play_ace(&mut game), 16);
    assert_eq!(game.money, 3);
}

// ==========================================================================
// tests/test_rent_on_a_joker_leaving_at_round_end.py -- "A rental that leaves at
// the end of a round still pays that round's rent."
//
// end_round walks G.jokers.cards once (state_events.lua:99-110) and, for each
// joker in turn, asks calculate_joker({end_of_round}) and then calls
// calculate_rental and calculate_perishable on that same card
// (state_events.lua:101, 108-109). A joker that destroys itself on that context
// has not left the row by then. So calculate_rental (card.lua:2271-2276) still
// charges ease_dollars(-G.GAME.rental_rate) on the card that is leaving, and the
// interest row in evaluate_round reads a balance that $3 has already come out
// of.
//
// The simulator ran every round_end hook first and charged rent afterwards, over
// whatever jokers were left (DS2IGPRB and N97LC9AB both stopped on a rental Gros
// Michel going extinct as a boss fell).
// ==========================================================================

fn rental_round(seed: &str, name: &str, money: i32, counter: Option<f64>) -> (GameState, JokerRef) {
    let mut game = GameState::new(seed, "Red Deck", 1);
    let instance = joker(name);
    {
        let mut j = instance.borrow_mut();
        j.rental = true;
        if let Some(counter) = counter {
            j.counter = counter;
        }
    }
    game.gain_joker(&instance);
    game._start_round();
    game.money = money;
    game.hands_left = 0;
    game.discards_left = 0;
    game.blind = Some(make_blind(BlindKind::Small, game.ante, None, 1.0, 1, false));
    (game, instance)
}

fn still_held(game: &GameState, joker: &JokerRef) -> bool {
    game.jokers.iter().any(|j| Rc::ptr_eq(j, joker))
}

#[test]
fn test_a_rental_gros_michel_that_goes_extinct_pays_its_rent() {
    // The Python test forces the extinction roll with a monkeypatch on
    // rng.chance. Rust cannot patch the engine, so this uses the seed SEED0000,
    // found with the Python reference, on which Gros Michel goes extinct at the
    // very first small-blind round end -- the same draw the patch stood in for.
    // Interest reads what the rent left: $16 less $3 is two blocks of five, not
    // three.
    let (mut game, joker) = rental_round("SEED0000", "Gros Michel", 16, None);
    game._beat_blind(true);
    assert!(!still_held(&game, &joker));
    assert!(game.pool_flags.contains("gros_michel_extinct"));
    assert_eq!(game.money, 16 - 3);
    assert_eq!(game.pending_payout, 3 + 13 / 5);
}

#[test]
fn test_a_rental_popcorn_eaten_at_round_end_pays_its_rent() {
    // card.lua:2946-2962: Popcorn on its last four mult queues its removal and
    // is still in the end_round loop when calculate_rental runs.
    let (mut game, joker) = rental_round("TESTSEED", "Popcorn", 10, Some(4.0));
    game._beat_blind(true);
    assert!(!still_held(&game, &joker));
    assert_eq!(game.money, 10 - 3);
}

#[test]
fn test_a_rental_mr_bones_that_saves_the_run_pays_its_rent() {
    // card.lua:3047-3062 returns `saved` and only queues start_dissolve;
    // state_events.lua:103-108 then charges rent on it in the same pass.
    let (mut game, joker) = rental_round("TESTSEED", "Mr. Bones", 10, None);
    game.chips_scored = game.blind.as_ref().unwrap().target / 2;
    game._lose_round();
    assert!(!still_held(&game, &joker));
    assert_eq!(game.money, 10 - 3);
}

// ==========================================================================
// tests/test_recording_twelve_fixes.py -- "Two simulator bugs from recording 12,
// and neither is about scoring."
//
// **Crimson Heart picks by age, not by position.** It debuffs one random joker a
// hand, drawn with `pseudorandom_element(jokers, pseudoseed('crimson_heart'))`
// (blind.lua:594) -- and that helper sorts the table by `sort_id` before it
// indexes. This sorted by where the joker sat in the row instead, which is not
// an order the game would ever draw from, because a player drags jokers about.
//
// **The five finisher bosses pay $8.** P_BLINDS in game.lua gives every one of
// the twenty-three ordinary bosses `dollars = 5` and each of bl_final_acorn,
// bl_final_bell, bl_final_heart, bl_final_leaf and bl_final_vessel `dollars = 8`.
// ==========================================================================

#[test]
fn test_crimson_heart_ignores_the_row_order() {
    // Reordering the row must not change who gets debuffed.
    let heart = boss_by_name("Crimson Heart").unwrap();
    let victim = |reversed_row: bool| -> String {
        let mut game = GameState::new("TESTSEED", "Red Deck", 1);
        game.endless = true;
        game._start_round();
        game.blind = Some(make_blind(BlindKind::Boss, 8, Some(heart), 1.0, 1, false));
        for name in ["Joker", "Baron", "Mime", "Blueprint"] {
            game.gain_joker(&joker(name));
        }
        if reversed_row {
            game.jokers.reverse();
        }
        game._play(vec![0, 1]);
        game.jokers
            .iter()
            .find(|j| j.borrow().debuffed)
            .map(|j| j.borrow().name().to_string())
            .unwrap_or_default()
    };

    let forward = victim(false);
    let reversed = victim(true);
    assert_eq!(forward, reversed);
    assert!(!forward.is_empty(), "someone must have been debuffed");
}

#[test]
fn test_a_finisher_boss_pays_eight() {
    for boss in all_bosses() {
        let blind = make_blind(BlindKind::Boss, 8, Some(boss), 1.0, 1, false);
        let want = if boss.is_finisher { 8 } else { 5 };
        assert_eq!(blind.reward, want, "{} pays {}", boss.name, blind.reward);
    }
}

#[test]
fn test_the_ordinary_blinds_are_unchanged() {
    assert_eq!(
        make_blind(BlindKind::Small, 1, None, 1.0, 1, false).reward,
        3
    );
    assert_eq!(make_blind(BlindKind::Big, 1, None, 1.0, 1, false).reward, 4);
    assert_eq!(
        make_blind(BlindKind::Small, 1, None, 1.0, 1, true).reward,
        0
    );
}

// ==========================================================================
// tests/test_lucky_rolls.py -- "A preview counts the Lucky rolls a play makes."
//
// Lucky Cat gains X0.25 each time a Lucky card triggers and one of its rolls
// hits (card.lua:3076-3081). The hit is random; the trigger is not. Hanging
// Chad retriggers the first card scored twice, so a Lucky card at the front of
// a play rolls three times and anywhere else once -- and that count is what a
// policy farming a Lucky Cat can compare plays by, where a preview's score only
// says how one draw of the rolls came out.
// ==========================================================================

fn lucky_pair(names: &[&str]) -> GameState {
    let mut game = GameState::new("AWEFRTUZ", "Blue Deck", 1);
    for name in names {
        game.gain_joker(&joker(name));
    }
    game._start_round();
    let lucky = make_card(Rank::Queen, Suit::Hearts);
    lucky.borrow_mut().enhancement = Enhancement::Lucky;
    game.hand = vec![
        make_card(Rank::Queen, Suit::Spades),
        lucky,
        make_card(Rank::Two, Suit::Clubs),
        make_card(Rank::Three, Suit::Clubs),
    ];
    game
}

#[test]
fn test_a_lucky_card_rolls_once_a_trigger() {
    let mut game = lucky_pair(&[]);
    assert_eq!(game.preview_outcome(&[0, 1]).2, 1);
    assert_eq!(game.preview_outcome(&[2, 3]).2, 0);
}

#[test]
fn test_hanging_chad_on_a_lucky_card_is_three_rolls() {
    let mut game = lucky_pair(&["Hanging Chad"]);
    assert_eq!(game.preview_outcome(&[1, 0]).2, 3);
    assert_eq!(game.preview_outcome(&[0, 1]).2, 1);
}

#[test]
fn test_the_outcome_agrees_with_the_other_previews() {
    let mut game = lucky_pair(&["Hanging Chad", "Lucky Cat"]);
    let (score, dollars, _) = game.preview_outcome(&[1, 0]);
    assert_eq!((score, dollars), game.preview_value(&[1, 0], "roll"));
    assert_eq!(score, game.preview_score(&[1, 0], "roll"));
}

// ==========================================================================
// tests/test_debuffed_glass_does_not_shatter.py -- "A debuffed Glass card never
// shatters, and never draws for it."
//
// state_events.lua:961 --
//
//     if scoring_hand[i].ability.name == 'Glass Card'
//         and not scoring_hand[i].debuff
//         and pseudorandom('glass') < ... then
//
// -- stops at the debuff, before `pseudorandom`, so the 'glass' stream is left
// exactly where it was. Unreachable until a debuffed card could score, which it
// can in a flush (QWERTYUI, Blue Deck, stake 1: a Glass Ace debuffed by The Club
// under Smeared Joker broke here and did not break in the game).
// ==========================================================================

fn glass_card(debuffed: bool) -> CardRef {
    let card = make_card(Rank::Ace, Suit::Spades);
    card.borrow_mut().enhancement = Enhancement::Glass;
    card.borrow_mut().debuffed = debuffed;
    card
}

#[test]
fn test_a_debuffed_glass_card_does_not_shatter() {
    let mut game = GameState::new("QWERTYUI", "Blue Deck", 1);
    for _ in 0..40 {
        // any draw at all would break one
        assert!(jimbot_sim::scoring::shattered_glass(&mut game, &[glass_card(true)]).is_empty());
    }
}

#[test]
fn test_it_does_not_move_the_glass_stream() {
    let mut game = GameState::new("QWERTYUI", "Blue Deck", 1);
    let before = game.rng.state();
    jimbot_sim::scoring::shattered_glass(&mut game, &[glass_card(true)]);
    assert_eq!(game.rng.state(), before);
}

#[test]
fn test_a_live_glass_card_still_rolls() {
    let mut game = GameState::new("QWERTYUI", "Blue Deck", 1);
    let before = game.rng.state();
    let mut broke = 0;
    for _ in 0..200 {
        broke += jimbot_sim::scoring::shattered_glass(&mut game, &[glass_card(false)]).len();
    }
    assert_ne!(game.rng.state(), before);
    assert!(
        broke > 0 && broke < 200,
        "one in four, not never and not always"
    );
}

// ==========================================================================
// tests/test_hook_takes_before_scoring.py -- "The Hook's two cards are gone
// before the hand scores."
//
// Blind:press_play is where The Hook lives, and the game runs it between moving
// the played cards out of the hand and scoring anything, so the two cards it
// takes are already in the discard by the time the held pass runs, and a Steel
// card among them pays nothing. Scoring first gave the round one extra held
// trigger per hand (recording 11: 75 x 193.5 against the game's 75 x 186).
//
// And a Hook discard is a real discard: the seals fire and the jokers fire, and
// only the discard count, the cost and the redraw are skipped. A purple-sealed
// card taken by The Hook makes its Tarot.
// ==========================================================================

fn hook() -> &'static BossEffect {
    boss_by_name("The Hook").unwrap()
}

fn on_the_hook(cards: usize, seed: &str) -> GameState {
    let mut game = GameState::new(seed, "Red Deck", 1);
    game._start_round();
    game.blind = Some(make_blind(BlindKind::Boss, 1, Some(hook()), 1.0, 1, false));
    game.hand = game.hand[..cards].to_vec();
    game
}

fn score_play(game: &mut GameState) -> i64 {
    let before = game.chips_scored;
    game._play(vec![0, 1]);
    game.chips_scored - before
}

#[test]
fn test_a_steel_card_the_hook_takes_does_not_score() {
    // Two cards to play and the rest of the hand Steel, so whatever The Hook
    // takes it takes Steel. Both remaining cards are taken, so the held pass
    // must find none and the score must match a plain hand's.
    let mut game = on_the_hook(4, "TESTSEED");
    for card in &game.hand[2..] {
        card.borrow_mut().enhancement = Enhancement::Steel;
    }
    let hook_taken = score_play(&mut game);

    let mut plain = on_the_hook(4, "TESTSEED");
    plain.blind = Some(make_blind(BlindKind::Boss, 1, None, 1.0, 1, false));
    for card in &plain.hand[2..] {
        card.borrow_mut().enhancement = Enhancement::Steel;
    }
    let with_steel = score_play(&mut plain);

    let mut bare = on_the_hook(4, "TESTSEED");
    bare.blind = Some(make_blind(BlindKind::Boss, 1, None, 1.0, 1, false));
    let without_steel = score_play(&mut bare);

    assert!(
        with_steel > without_steel,
        "the Steel cards were never scoring"
    );
    assert_eq!(
        hook_taken, without_steel,
        "The Hook took both Steel cards and they still paid: {hook_taken}, against {with_steel} held and {without_steel} not"
    );
}

#[test]
fn test_a_purple_seal_the_hook_takes_makes_its_tarot() {
    let mut game = on_the_hook(4, "TESTSEED");
    for card in &game.hand[2..] {
        card.borrow_mut().seal = Seal::Purple;
    }
    game.consumables.clear();

    game._play(vec![0, 1]);

    assert!(
        !game.consumables.is_empty(),
        "The Hook discarded two purple-sealed cards and neither made a Tarot"
    );
}

#[test]
fn test_a_hook_discard_costs_no_discard() {
    let mut game = on_the_hook(4, "TESTSEED");
    let discards = game.discards_left;
    let used = game.discards_used;

    game._play(vec![0, 1]);

    assert_eq!((game.discards_left, game.discards_used), (discards, used));
}

// ==========================================================================
// tests/test_riff_raff_creation.py -- "Riff-Raff's jokers come from Riff-Raff's
// own streams, edition included."
//
// The game (card.lua:2529-2543) makes them with
// `create_card('Joker', G.jokers, nil, 0, nil, nil, nil, 'rif')`, and
// create_card (functions/common_events.lua:2082):
//
// * names the pool with key_append 'rif' -- get_current_pool builds
//   'Joker'..rarity..'rif'..ante, so an ante-one Riff-Raff draws from
//   "Joker1rif1" (the simulator drew from "Joker11");
// * polls an edition, poll_edition('edi'..key_append..ante)
//   (common_events.lua:2149), under the run's edition rate -- so a Polychrome
//   Red Card came out plain before;
// * fixes how many it makes at blind selection: jokers_to_create =
//   min(2, card_limit - (#jokers + joker_buffer)) (card.lua:2530), so a Negative
//   first joker does not buy a second slot's worth.
//
// The expected rows are the game's own, read off the headless smoke test.
// ==========================================================================

fn riff_row(seed: &str, deck: &str, stake: i32, keys: &[&str]) -> GameState {
    let mut game = GameState::new(seed, deck, stake);
    for key in keys {
        let name = jimbot_sim::shop_pool::name_by_joker_key(key).unwrap();
        game.gain_joker(&joker(name));
    }
    game
}

/// Only Riff-Raff's hook: the rows below hold nothing else that reacts.
fn select_blind_riff(game: &mut GameState) {
    let row: Vec<JokerRef> = game.jokers.clone();
    for joker in row {
        if joker.borrow().name() == "Riff-Raff" {
            let hook = jokers::spec_or_panic("Riff-Raff").on_blind_select.unwrap();
            hook(&joker, game);
        }
    }
}

fn riff_keys(game: &GameState) -> Vec<&'static str> {
    game.jokers
        .iter()
        .map(|j| jimbot_sim::shop_pool::key_by_joker_name(j.borrow().name()).unwrap())
        .collect()
}

#[test]
fn test_riff_raff_makes_the_jokers_the_game_makes() {
    // (seed, deck, stake, the row when the blind was selected, what the game
    // made). The expected rows are the game's own.
    let rows: [(&str, &str, i32, &[&str], &[(&str, Edition)]); 6] = [
        (
            "S85SICBL",
            "Magic Deck",
            5,
            &["j_riff_raff"],
            &[
                ("j_droll", Edition::None),
                ("j_red_card", Edition::Polychrome),
            ],
        ),
        (
            "IGS6H949",
            "Checkered Deck",
            1,
            &["j_banner", "j_riff_raff"],
            &[("j_juggler", Edition::None), ("j_chaos", Edition::None)],
        ),
        (
            "WJ09CGHG",
            "Yellow Deck",
            8,
            &["j_ice_cream", "j_riff_raff"],
            &[("j_sly", Edition::None), ("j_scary_face", Edition::None)],
        ),
        (
            "U2EBFAQ2",
            "Painted Deck",
            5,
            &["j_riff_raff"],
            &[
                ("j_business", Edition::None),
                ("j_reserved_parking", Edition::None),
            ],
        ),
        (
            "08F809NE",
            "Red Deck",
            7,
            &["j_riff_raff"],
            &[
                ("j_gluttenous_joker", Edition::None),
                ("j_popcorn", Edition::None),
            ],
        ),
        (
            "LC4JWH61",
            "Nebula Deck",
            7,
            &["j_gluttenous_joker", "j_riff_raff"],
            &[("j_mad", Edition::None), ("j_faceless", Edition::None)],
        ),
    ];

    for (seed, deck, stake, row, made) in rows {
        let mut game = riff_row(seed, deck, stake, row);
        select_blind_riff(&mut game);
        let mut want: Vec<&str> = row.to_vec();
        want.extend(made.iter().map(|(key, _)| *key));
        assert_eq!(riff_keys(&game), want, "{seed}");
        let editions: Vec<Edition> = game.jokers[row.len()..]
            .iter()
            .map(|j| j.borrow().edition)
            .collect();
        assert_eq!(
            editions,
            made.iter().map(|(_, e)| *e).collect::<Vec<_>>(),
            "{seed}"
        );
    }
}

#[test]
fn test_riff_raff_count_is_fixed_before_a_negative_arrives() {
    // One free slot, a Negative first joker: still one joker (card.lua:2530).
    //
    // The Python test monkeypatches shop_pool.poll_edition to always return
    // "negative". Rust cannot patch the engine, so this uses the seed CJJRYRE6,
    // found with the Python reference, whose Riff-Raff joker naturally polls a
    // Negative edition -- the same draw the patch stood in for.
    let mut game = riff_row(
        "CJJRYRE6",
        "Red Deck",
        1,
        &["j_riff_raff", "j_joker", "j_banner", "j_juggler"],
    );
    assert_eq!(game.joker_slots() - game.jokers.len() as i32, 1);
    select_blind_riff(&mut game);
    assert_eq!(game.jokers.len(), 5);
    assert_eq!(
        game.jokers.last().unwrap().borrow().edition,
        Edition::Negative
    );
}

#[test]
fn test_a_created_joker_polls_its_edition_under_its_append() {
    // poll_edition('edi'..key_append..ante), common_events.lua:2149.
    //
    // The Python test monkeypatches shop_pool.poll_edition to record the key and
    // edition_rate it was called with. Rust cannot patch the engine, but
    // poll_edition advances the RNG pool named by its key, so after the call the
    // pool `edi<append><ante>` is present exactly when the append was used. That
    // is the same behaviour: the append names the edition stream, and getting it
    // wrong draws the right joker from the wrong place. (The edition_rate
    // argument is not separately observable; the pool is the load-bearing part.)
    for append in ["rif", "jud", "sou", "wra", "top"] {
        let mut game = GameState::new("EDIPOLL1", "Red Deck", 1);
        game.ante = 3;
        game.add_random_joker("test", None, append == "sou", append, false);
        let key = format!("edi{append}3");
        assert!(
            game.rng.pools.contains_key(&key),
            "the edition was not polled under {key}: {:?}",
            game.rng.pools.keys().collect::<Vec<_>>()
        );
    }
}

// ==========================================================================
// Erosion counts below the deck the run started with, not below 52.
//
// game.lua:2375 sets G.GAME.starting_deck_size = #G.playing_cards as the run
// deals its deck, and card.lua:3894 reads that. On an Abandoned Deck it is 40,
// so a fresh Erosion pays nothing. DM46XNV1 / Abandoned / stake 8: the shadow
// paid it +48 Mult, scored 21436 where the game scored 7616, called the next
// hand a safe farm and waited on a cash-out the game never offered.
// ==========================================================================

#[test]
fn test_erosion_pays_nothing_on_a_whole_abandoned_deck() {
    let mut bare = run("TESTSEED", "Abandoned Deck", &[]);
    let mut eroded = run("TESTSEED", "Abandoned Deck", &["Erosion"]);
    assert_eq!(eroded.starting_deck_size, 40);
    assert_eq!(
        eroded.preview_score(&[0], "pessimistic"),
        bare.preview_score(&[0], "pessimistic")
    );
    // One card gone from the full deck, and it pays +4.
    let gone = eroded.full_deck.pop().unwrap();
    eroded.draw_pile.retain(|c| !std::rc::Rc::ptr_eq(c, &gone));
    assert!(eroded.preview_score(&[0], "pessimistic") > bare.preview_score(&[0], "pessimistic"));
}

// ==========================================================================
// tests/test_vagabond_reads_money_before_the_hand.py -- "Vagabond reads the
// money the hand was played with, not what it pays."
//
// card.lua:3743-3744 asks `G.GAME.dollars <= extra` in joker_main, and every
// payout the hand makes -- a Gold Seal's $3, Matador's $8 -- is an
// `ease_dollars` event still queued at that point. Read after the hand, a
// Matador's $8 stopped the tarot the game made (FATMAN06: the game made The
// Devil and the simulator nothing).
// ==========================================================================

fn vagabond_game(money: i32) -> GameState {
    let mut game = GameState::new("TESTSEED", "Red Deck", 1);
    game.gain_joker(&joker("Vagabond"));
    game._start_round();
    game.money = money;
    game.consumables = Vec::new();
    game
}

#[test]
fn test_a_gold_seal_paid_in_the_hand_does_not_stop_the_tarot() {
    let mut game = vagabond_game(2);
    game.hand[0].borrow_mut().seal = Seal::Gold;
    game._play(vec![0]);
    assert_eq!(game.money, 5);
    assert_eq!(game.consumables.len(), 1);
}

#[test]
fn test_five_dollars_held_makes_none() {
    let mut game = vagabond_game(5);
    game._play(vec![0]);
    assert_eq!(game.consumables.len(), 0);
}

// ==========================================================================
// tests/test_madness_eats_by_age.py -- "Madness eats a joker picked by age, and
// a joker's age is when it was built."
//
// card.lua:2503-2509, on setting_blind and not on a boss:
//
//     local joker_to_destroy = #destructable_jokers > 0
//         and pseudorandom_element(destructable_jokers, pseudoseed('madness'))
//
// pseudorandom_element (misc_functions.lua:260-261) sorts that table by sort_id
// before it indexes, so the row order the list was built in does not matter. And
// sort_id is stamped in Card:init (card.lua:24-25) -- at construction. A shop
// builds its jokers in slot order (game.lua:3111-3113), and buying one moves
// that same Card into the row, so a joker bought second out of slot one is
// *older* than one bought first out of slot two (N1OA90W1 stopped on this).
// ==========================================================================

fn survivors(game: &GameState) -> Vec<String> {
    game.jokers
        .iter()
        .map(|j| j.borrow().name().to_string())
        .collect()
}

#[test]
fn test_madness_draws_by_age_not_by_row_position() {
    // Same jokers, same ages, same seed -- only the row order differs.
    let mut rows = Vec::new();
    for order in [[0usize, 1, 2], [0, 2, 1]] {
        let mut game = GameState::new("MADNESS1", "Red Deck", 1);
        let built = [joker("Madness"), joker("Popcorn"), joker("Mystic Summit")];
        for &i in &order {
            game.gain_joker(&built[i]);
        }
        game.step(&Action::new(ActionType::SelectBlind));
        assert_eq!(game.jokers[0].borrow().name(), "Madness");
        assert_eq!(game.jokers.len(), 2, "Madness ate nothing");
        let mut names = survivors(&game);
        names.sort();
        rows.push(names);
    }
    assert_eq!(
        rows[0], rows[1],
        "Madness picked its victim by where the jokers sit in the row"
    );
}

#[test]
fn test_a_joker_bought_later_from_an_earlier_slot_is_older() {
    // The N1OA90W1 shop: slot two bought first, then slot one. The ages are
    // stamped when the shop builds the shelf, not when each is bought.
    use jimbot_sim::shop::ShopSlot;

    let shop_then_blind = |buys: [i32; 2]| -> (GameState, JokerRef, JokerRef) {
        let mut game = GameState::new("N1OA90W1", "Red Deck", 1);
        game.gain_joker(&joker("Madness"));
        game.phase = Phase::Shop;
        game._open_shop();
        let popcorn = joker("Popcorn");
        let summit = joker("Mystic Summit");
        {
            let shop = game.shop.as_mut().unwrap();
            shop.slots = vec![
                ShopSlot {
                    kind: "joker",
                    base_cost: 1,
                    couponed: false,
                    joker: Some(popcorn.clone()),
                    consumable: None,
                    card: None,
                    sort_id: jimbot_sim::cards::next_sort_id(),
                },
                ShopSlot {
                    kind: "joker",
                    base_cost: 1,
                    couponed: false,
                    joker: Some(summit.clone()),
                    consumable: None,
                    card: None,
                    sort_id: jimbot_sim::cards::next_sort_id(),
                },
            ];
        }
        game.money = 50;
        for index in buys {
            game.step(&Action::at(ActionType::Buy, index));
        }
        game.step(&Action::new(ActionType::LeaveShop));
        game.step(&Action::new(ActionType::SelectBlind));
        (game, popcorn, summit)
    };

    let (game, popcorn, summit) = shop_then_blind([1, 0]);
    assert!(
        popcorn.borrow().uid < summit.borrow().uid,
        "the age was stamped on purchase, not where the shop built the joker"
    );
    // The control buys the same shelf in slot order, so row order and age
    // agree; the draw off 'madness' is the same in both runs.
    let (control, _, _) = shop_then_blind([0, 0]);
    assert_eq!(game.jokers.len(), 2);
    assert_eq!(control.jokers.len(), 2);
    assert_eq!(survivors(&game), survivors(&control));
}

// ==========================================================================
// tests/test_card_ids_are_ages.py -- "A card's id is where it was *built*, and
// the shuffle reads it."
//
// Card:init is the only place the game touches its card counter:
//
//     G.sort_id = (G.sort_id or 0) + 1
//     self.sort_id = G.sort_id
//
// A booster builds all of its cards in one loop the moment it opens
// (card.lua:1740-1780), so slot one is older than slot four whichever the player
// takes first. And `pseudoshuffle` sorts the list by id before it shuffles, so
// the shuffle never sees the order a deck happens to be lying in -- it sees id
// order. That makes a wrong id a wrong *deal* (recording 10 stopped on a mega
// Standard pack taken four-then-one).
// ==========================================================================

#[test]
fn test_a_pack_slot_keeps_its_build_order_when_taken_out_of_order() {
    use jimbot_sim::game::PackChoice;
    use jimbot_sim::shop_pool::PackCard;

    let mut game = GameState::new("TESTSEED", "Red Deck", 1);
    // Build the two the way _open_pack does: in slot order, before either is
    // picked. That is the whole claim -- slot one is older.
    let pack_card = |edition: &'static str, seal: Option<String>| PackCard {
        set: "Playing",
        key: None,
        rank: Some("Q".to_string()),
        suit: Some("D".to_string()),
        enhancement: None,
        edition: Some(edition),
        seal,
        eternal: false,
        perishable: false,
        rental: false,
    };
    let slot_one = match game._pack_card(&pack_card("none", Some("Blue".to_string()))) {
        PackChoice::Card(card) => card,
        _ => panic!("a Playing pack card is a Card"),
    };
    let slot_four = match game._pack_card(&pack_card("polychrome", None)) {
        PackChoice::Card(card) => card,
        _ => panic!("a Playing pack card is a Card"),
    };
    assert!(uid_of(&slot_one) < uid_of(&slot_four));

    // Now take them in the other order, as recording 10 does.
    game.add_card(&slot_four);
    game.add_card(&slot_one);
    assert!(
        uid_of(&slot_one) < uid_of(&slot_four),
        "add_card restamped the id and made it an acquisition order"
    );
}

#[test]
fn test_the_round_shuffle_sorts_by_id_first() {
    // A deck lying in a different order deals the same, as pseudoshuffle does.
    let mut one = GameState::new("TESTSEED", "Red Deck", 1);
    let mut two = GameState::new("TESTSEED", "Red Deck", 1);
    // Same cards, one deck's list rotated. pseudoshuffle sorts before it
    // shuffles, so the deal must not notice.
    let rotated: Vec<CardRef> = two.full_deck[7..]
        .iter()
        .chain(two.full_deck[..7].iter())
        .cloned()
        .collect();
    two.full_deck = rotated;

    one._start_round();
    two._start_round();

    // Compared by face, not by id: two runs draw their ids from one global
    // counter, so the same card holds a different number in each.
    let faces = |game: &GameState| -> Vec<(Rank, Suit)> {
        game.hand.iter().map(|c| (rank_of(c), suit_of(c))).collect()
    };
    assert_eq!(faces(&one), faces(&two));
}

// ==========================================================================
// tests/test_recordings.py -- "Replay real human games through the simulator,
// action for action."
//
// In Python this replays every checked-in recording through `jimbot_sim.replay`
// + `jimbot_sim.run.SimRun` and asserts each reaches the end. Rust has no
// equivalent replayer module: `replay.py`/`run.py` are drivers that PORTING.md
// puts out of scope, and the port's own replayer is `tests/replay_fixture.rs` +
// `tools/gen_replay_fixture.py`, which replays all 15 recordings through the
// engine field-by-field (every one of the 13 fields, all 42 `state_dict` key
// digests and the RNG pool signature at every step) -- a strictly stronger check
// than "did it reach the end", stopping at the first divergence. So
// `test_the_simulator_follows_a_real_game` is not re-ported here; it is covered
// by that fixture. What is portable is the guard that the replay data is still
// present, which is ported below against the fixture layout instead.
// ==========================================================================

#[test]
fn test_the_recordings_are_still_there() {
    // A silent skip of every one of these would be easy to miss.
    //
    // The Python test checks the recordings directory. The Rust analogue is the
    // replay fixture set (`tests/fixtures/replay_<n>.txt`) that
    // `replay_fixture.rs` drives; a recording with no fixture proves nothing, so
    // the same floor applies. `13` does not exist in either set.
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let present: Vec<i32> = (1..=16)
        .filter(|n| dir.join(format!("replay_{n}.txt")).exists())
        .collect();
    assert!(
        present.len() >= 7,
        "only {} replay fixtures found",
        present.len()
    );

    // --------------------------------------------------------------------------
    // Not ported, and why (recorded here so the gap is not silent):
    //
    // tests/test_replay.py (11) -- every one drives `jimbot_sim.replay`, the
    //   recorded-action replayer: `merge_buy_and_use`, `to_move`, `Move`,
    //   `differences`, `Recording` and `replay`. That module is a driver, not the
    //   simulator, and PORTING.md explicitly leaves `replay.py`/`run.py`/`compare.py`
    //   out of the port. There is no production Rust counterpart to test, and
    //   re-implementing the driver inside the test would test the test's own
    //   translation rather than the engine. The recorded-action translation is
    //   exercised indirectly by `tools/gen_replay_fixture.py`, whose output
    //   `tests/replay_fixture.rs` replays through the engine.
    //
    // tests/test_recordings.py::test_the_simulator_follows_a_real_game -- drives the
    //   same out-of-scope replayer. Covered more strongly by
    //   `tests/replay_fixture.rs` (see the header above); not re-ported.
    //
    // tests/test_recorder_survives.py (2) -- both drive `scripts/record_replay.py`'s
    //   `do_record` against a fake bridge, monkeypatching `_connect` and
    //   `time.sleep` to simulate the game's socket dying. That is the headless
    //   bridge/recorder layer, which PORTING.md puts out of scope (it drives the
    //   game's Lua through `lupa`), with no Rust analogue.
    // --------------------------------------------------------------------------

    for want in [1, 2, 3, 4, 5] {
        assert!(present.contains(&want), "replay fixture {want} is missing");
    }
}

// ==========================================================================
// Python tests in this batch that are NOT applicable to the Rust port
// ==========================================================================
//
// 14 `def test_*` functions in this batch have no Rust counterpart, and they are
// listed here rather than dropped silently. They fall into two groups, and
// neither is about the simulator -- they test the *recording and replay tooling*
// and the *recorder process*:
//
// From `test_replay.py` (11) -- the `replay.py` translation layer:
//   test_a_buy_and_its_use_are_one_press
//   test_a_newer_recordings_buy_and_use_reads_the_same
//   test_a_sort_is_a_button_not_an_action
//   test_a_voucher_is_found_by_what_was_redeemed
//   test_a_replay_can_stop_part_way_without_comparing
//   test_a_field_the_backend_cannot_report_is_not_a_difference
//   test_chips_are_not_compared_mid_animation
//   test_ids_are_compared_by_their_order
//   test_positions_become_indices
//   test_the_card_bought_is_checked_before_it_is_bought
//   test_the_state_is_read_only_for_a_purchase
//
//   These test `to_move` (turning a *recording's* engine-shaped params -- area
//   names, one-based indices, centre keys -- into a simulator `Action`) and
//   `merge_buy_and_use` (folding a recorded buy and its immediate use into one
//   entry). The Rust port has no such layer, deliberately: the fixtures emit
//   actions that are *already* `Action`s, produced on the Python side where the
//   recordings are read, so there are no engine params here to translate. The
//   Rust simulator's contract starts at `Action`, and that contract is compared
//   step by step in `replay_fixture.rs` (4,140 actions) and `fuzz_fixture.rs`
//   (1,547 actions) -- which is what `to_move` exists to produce.
//
// From `test_recorder_survives.py` (3) -- the recorder process:
//   test_ctrl_c_still_saves
//   test_the_recording_is_saved_when_the_game_dies
//   test_the_simulator_follows_a_real_game
//
//   These drive `scripts/record_replay.py` against a fake bridge with
//   `monkeypatch`, a `KeyboardInterrupt` and an injected `_connect`. There is no
//   Rust recorder, and the port does not need one: the recordings are read by the
//   Python tooling and turned into fixtures.
//
// Nothing here is a gap in the *simulator*; if a Rust recorder is ever written,
// these become its tests.
