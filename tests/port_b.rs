//! Joker-behaviour tests, ported from the Python suite.
//!
//! Each `#[test]` mirrors one `def test_x()` under
//! `external/jimbot-sim/tests/`, grouped here by the file it came from. The
//! doc comments carry the reasoning the Python test recorded -- these were
//! written over months against real divergences, and the comment is often the
//! only statement of *why* the assertion has the shape it does.
//!
//! The port is "direct" wherever the Python test builds a position and asserts a
//! number: the same position is rebuilt with `GameState` and the same value
//! asserted. Where Python monkeypatches (`rng.chance`, `poll_edition`,
//! `draw_joker`) or drives the out-of-scope `compare` driver, the Rust test does
//! the same thing a different way and says so in the test's own comment.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::rc::Rc;

use jimbot_sim::blinds::{boss_by_name, make_blind, BlindKind};
use jimbot_sim::cards::{make_card, CardRef, Edition, Enhancement, Rank, Seal, Suit};
use jimbot_sim::consumables;
use jimbot_sim::game::{Action, ActionType, GameState, PackChoice, Phase, Tag};
use jimbot_sim::hands::{HandType, GAME_PAIRS_ORDER, SECRET_HANDS};
use jimbot_sim::jokers::{self, JokerInstance, JokerRef, JokerSpec};
use jimbot_sim::shop::ShopSlot;
use jimbot_sim::state;

// ==========================================================================
// shared builders
// ==========================================================================

fn joker_ref(name: &str) -> JokerRef {
    jokers::make(name)
}

fn find_joker(game: &GameState, name: &str) -> JokerRef {
    game.jokers
        .iter()
        .find(|j| j.borrow().name() == name)
        .unwrap_or_else(|| panic!("no {} in the row", name))
        .clone()
}

fn counter_of(game: &GameState, name: &str) -> f64 {
    find_joker(game, name).borrow().counter
}

/// Python's `_run(*names)`: a fresh TESTSEED Red Deck run holding these jokers,
/// then a round started.
fn run(names: &[&str]) -> GameState {
    let mut game = GameState::new("TESTSEED", "Red Deck", 1);
    for name in names {
        game.gain_joker(&joker_ref(name));
    }
    game._start_round();
    game
}

fn debuff(game: &mut GameState, name: &str) -> JokerRef {
    let joker = find_joker(game, name);
    game.set_joker_debuff(&joker, true);
    joker
}

// ==========================================================================
// tests/test_registry_honesty.py -- "What "implemented" currently means,
// joker by joker."
//
// All 150 are registered, which is what the shop needs; registration is not
// implementation, so the jokers that carry no behaviour are named. A joker
// cannot quietly slip from "working" to "declared", and nothing can be added to
// the not-yet-built list without saying so.
// ==========================================================================

/// Every hook a joker can carry. Kept in step with JokerSpec deliberately: a
/// hook missing from this list makes the jokers that use it look behaviourless,
/// which is how thirteen implemented jokers were briefly still counted hollow.
const HOOK_NAMES: [&str; 27] = [
    "update",
    "scored",
    "lucky_trigger",
    "scored_growth",
    "held",
    "independent",
    "other_joker",
    "round_end",
    "discarded",
    "retrigger_scored",
    "retrigger_held",
    "copier",
    "on_blind_select",
    "on_round_start",
    "on_sell",
    "on_reroll",
    "on_pack_skip",
    "on_pack_open",
    "on_cards_destroyed",
    "on_glass_shattered",
    "on_shop_end",
    "round_money",
    "before",
    "before_hand",
    "after_hand",
    "on_first_discard",
    "on_debuffed_hand",
];

const DECLARATION_NAMES: [&str; 13] = [
    "hand_size",
    "extra_hands",
    "extra_discards",
    "free_rerolls",
    "debt_limit",
    "interest_bonus",
    "free_planets",
    "allows_duplicates",
    "prevents_death",
    "disables_boss_on_sell",
    "enhancement_gate",
    "hand_size_from_counter",
    "rerolls_a_hand",
];

/// Correct with no hook: the run or the evaluator reads these directly.
fn read_elsewhere() -> HashSet<&'static str> {
    [
        "Four Fingers",
        "Shortcut",
        "Splash",
        "Smeared Joker",
        "Pareidolia",
        "Oops! All 6s",
        "Chicot",
    ]
    .into_iter()
    .collect()
}

fn has_hook(spec: &JokerSpec) -> bool {
    spec.update.is_some()
        || spec.scored.is_some()
        || spec.lucky_trigger.is_some()
        || spec.scored_growth.is_some()
        || spec.held.is_some()
        || spec.independent.is_some()
        || spec.other_joker.is_some()
        || spec.round_end.is_some()
        || spec.discarded.is_some()
        || spec.retrigger_scored.is_some()
        || spec.retrigger_held.is_some()
        || spec.copier.is_some()
        || spec.on_blind_select.is_some()
        || spec.on_round_start.is_some()
        || spec.on_sell.is_some()
        || spec.on_reroll.is_some()
        || spec.on_pack_skip.is_some()
        || spec.on_pack_open.is_some()
        || spec.on_cards_destroyed.is_some()
        || spec.on_glass_shattered.is_some()
        || spec.on_shop_end.is_some()
        || spec.round_money.is_some()
        || spec.before.is_some()
        || spec.before_hand.is_some()
        || spec.after_hand.is_some()
        || spec.on_first_discard.is_some()
        || spec.on_debuffed_hand.is_some()
}

fn has_declaration(spec: &JokerSpec) -> bool {
    spec.hand_size != 0
        || spec.extra_hands != 0
        || spec.extra_discards != 0
        || spec.free_rerolls != 0
        || spec.debt_limit != 0
        || spec.interest_bonus != 0
        || spec.free_planets
        || spec.allows_duplicates
        || spec.prevents_death
        || spec.disables_boss_on_sell
        || !spec.enhancement_gate.is_empty()
        || spec.hand_size_from_counter
        || spec.rerolls_a_hand
}

fn behaviourless() -> HashSet<&'static str> {
    jokers::all_specs()
        .iter()
        .filter(|spec| !has_hook(spec) && !has_declaration(spec))
        .map(|spec| spec.name)
        .collect()
}

#[test]
fn test_all_150_are_registered() {
    assert_eq!(jokers::all_specs().len(), 150);
}

#[test]
fn test_the_hollow_jokers_are_the_ones_we_say_they_are() {
    // NOT_YET_BUILT is empty, and worth keeping: the two that used to be here
    // (Diet Cola's Double Tag pool, Hallucination's booster pack) were blocked on
    // machinery that now exists. The set stays so anything new has to be named.
    let not_yet_built: HashSet<&'static str> = HashSet::new();
    let expected: HashSet<&'static str> = read_elsewhere().union(&not_yet_built).copied().collect();
    assert_eq!(behaviourless(), expected);
}

#[test]
fn test_nothing_claims_to_be_both() {
    let not_yet_built: HashSet<&'static str> = HashSet::new();
    let overlap: HashSet<&'static str> = read_elsewhere()
        .intersection(&not_yet_built)
        .copied()
        .collect();
    assert!(overlap.is_empty());
}

#[test]
fn test_the_hook_list_matches_the_spec() {
    // The Python test reflects over `dataclasses.fields(JokerSpec)` to prove
    // HOOKS names every callable field -- otherwise this file measures hollowness
    // against a stale list. Rust has no reflection, so the equivalent is a
    // pattern that names *every* field: adding one to JokerSpec stops this
    // compiling, the way dropping one from HOOKS fails the Python assert.
    let spec = JokerSpec::DEFAULT;
    let JokerSpec {
        name: _,
        rarity: _,
        text: _,
        cost: _,
        init_counter: _,
        init_secondary: _,
        update,
        update_before_scoring: _,
        scored,
        lucky_trigger,
        scored_growth,
        held,
        independent,
        other_joker,
        round_end,
        discarded,
        retrigger_scored,
        retrigger_held,
        on_blind_select,
        on_round_start,
        on_sell,
        round_money,
        on_reroll,
        on_pack_skip,
        on_pack_open,
        on_cards_destroyed,
        on_glass_shattered,
        on_shop_end,
        rerolls_a_hand: _,
        before_hand,
        before,
        on_debuffed_hand,
        after_hand,
        on_first_discard,
        copier,
        enhancement_gate: _,
        hand_size: _,
        hand_size_from_counter: _,
        extra_hands: _,
        extra_discards: _,
        free_rerolls: _,
        debt_limit: _,
        interest_bonus: _,
        free_planets: _,
        allows_duplicates: _,
        prevents_death: _,
        disables_boss_on_sell: _,
    } = spec;
    let hooks = [
        update.is_some(),
        scored.is_some(),
        lucky_trigger.is_some(),
        scored_growth.is_some(),
        held.is_some(),
        independent.is_some(),
        other_joker.is_some(),
        round_end.is_some(),
        discarded.is_some(),
        retrigger_scored.is_some(),
        retrigger_held.is_some(),
        copier.is_some(),
        on_blind_select.is_some(),
        on_round_start.is_some(),
        on_sell.is_some(),
        on_reroll.is_some(),
        on_pack_skip.is_some(),
        on_pack_open.is_some(),
        on_cards_destroyed.is_some(),
        on_glass_shattered.is_some(),
        on_shop_end.is_some(),
        round_money.is_some(),
        before.is_some(),
        before_hand.is_some(),
        after_hand.is_some(),
        on_first_discard.is_some(),
        on_debuffed_hand.is_some(),
    ];
    assert_eq!(
        hooks.len(),
        HOOK_NAMES.len(),
        "JokerSpec and HOOKS disagree: {HOOK_NAMES:?}"
    );
    assert!(
        hooks.iter().all(|set| !set),
        "the default spec carries no hook"
    );
    assert_eq!(
        has_hook(&spec),
        hooks.iter().any(|set| *set),
        "has_hook disagrees with the field-by-field view"
    );
}

#[test]
fn test_every_name_is_real() {
    let unknown: Vec<&str> = read_elsewhere()
        .into_iter()
        .filter(|name| jokers::spec(name).is_none())
        .collect();
    assert!(
        unknown.is_empty(),
        "listed a joker that is not registered: {unknown:?}"
    );
}

#[test]
fn test_every_declaration_is_read_by_something() {
    // A declared flag with no reader is a joker that silently does nothing. Mr.
    // Bones went the whole project that way -- registered, offered, bought and
    // worth exactly nothing -- because prevents_death appeared in the spec and
    // nowhere else. Python greps the package and skips jokers.py; the Rust
    // equivalent reads src/*.rs and skips the registry and the spec module.
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/src");
    let mut body = String::new();
    for entry in std::fs::read_dir(dir).expect("the src directory") {
        let path = entry.expect("a src entry").path();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if name == "jokers.rs" || name == "joker_specs.rs" {
            continue;
        }
        if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            body.push_str(&std::fs::read_to_string(&path).expect("a src file"));
            body.push('\n');
        }
    }
    let unread: Vec<&str> = DECLARATION_NAMES
        .into_iter()
        .filter(|flag| !body.contains(flag))
        .collect();
    assert!(
        unread.is_empty(),
        "declared on JokerSpec and read by nothing: {unread:?}"
    );
}

// ==========================================================================
// tests/test_growth_hooks.py -- "The jokers that grow on something other than a
// hand being played."
//
// Five of these had a counter that nothing ever moved -- Canio, Yorick, Glass
// Joker, Hit the Road and Perkeo -- so each sat at its starting value for whole
// runs while its text promised otherwise. Glass Joker is the one worth care:
// reading "per Glass card destroyed" gets it wrong, and so does reading the
// game's own `context.glass_shattered`, which nothing ever fires. The live
// branch counts destroyed cards carrying `.shattered`, and that flag is written
// at different times by different destroyers: scoring and discarding set it
// inline, so it pays; the tarots queue it as an animation, after, so it pays
// nothing. The Hanged Man is the exception, patched around with a second handler.
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

/// Beat the blind in force and press Cash Out, as a round really ends.
fn round_end(game: &mut GameState, kind: BlindKind) {
    game.blind = Some(make_blind(kind, game.ante, None, 1.0, 1, false));
    game._beat_blind(true);
    game._cash_out();
}

fn face_card(game: &GameState) -> CardRef {
    game.hand
        .iter()
        .find(|c| {
            matches!(
                jimbot_sim::cards::rank_of(c),
                Rank::Jack | Rank::Queen | Rank::King
            )
        })
        .unwrap()
        .clone()
}

#[test]
fn test_canio_counts_a_face_card_destroyed_by_a_tarot() {
    let mut game = run(&["Canio"]);
    let face = face_card(&game);
    let before = counter_of(&game, "Canio");
    game.remove_card(&face, false);
    assert_eq!(counter_of(&game, "Canio"), before + 1.0);
}

#[test]
fn test_canio_ignores_a_number_card() {
    let mut game = run(&["Canio"]);
    let plain = game
        .hand
        .iter()
        .find(|c| {
            !matches!(
                jimbot_sim::cards::rank_of(c),
                Rank::Jack | Rank::Queen | Rank::King
            )
        })
        .unwrap()
        .clone();
    let before = counter_of(&game, "Canio");
    game.remove_card(&plain, false);
    assert_eq!(counter_of(&game, "Canio"), before);
}

#[test]
fn test_glass_joker_is_paid_when_a_glass_card_shatters_while_scoring() {
    let mut game = run(&["Glass Joker"]);
    let card = game.hand[0].clone();
    card.borrow_mut().enhancement = Enhancement::Glass;
    let before = counter_of(&game, "Glass Joker");
    game.remove_card(&card, true);
    assert_eq!(counter_of(&game, "Glass Joker"), before + 0.75);
}

#[test]
fn test_a_tarot_eating_a_glass_card_pays_glass_joker_nothing() {
    // Familiar, Grim, Incantation, Immolate: each destroys a card (or cards)
    // for the run, and each queues the shatter *after* the jokers are told, so
    // Glass Joker gets nothing. Engine: a uniform glass hand is X1.00 before and
    // after, for each one.
    for _tarot in ["Familiar", "Grim", "Incantation", "Immolate"] {
        let mut game = run(&["Glass Joker"]);
        let card = game.hand[0].clone();
        card.borrow_mut().enhancement = Enhancement::Glass;
        let before = counter_of(&game, "Glass Joker");
        game.remove_card(&card, false);
        assert_eq!(counter_of(&game, "Glass Joker"), before);
    }
}

#[test]
fn test_the_hanged_man_pays_glass_joker_by_its_own_side_door() {
    // Engine: two glass cards selected moved it X1.00 -> X1.75.
    let mut game = run(&["Glass Joker"]);
    let glass: Vec<CardRef> = game.hand[..2].to_vec();
    for card in &glass {
        card.borrow_mut().enhancement = Enhancement::Glass;
    }
    let before = counter_of(&game, "Glass Joker");
    let spec = consumables::spec_or_panic("The Hanged Man");
    (spec.apply.unwrap())(&mut game, &glass);
    assert_eq!(counter_of(&game, "Glass Joker"), before + 1.5);
}

#[test]
fn test_the_hanged_man_pays_nothing_for_plain_cards() {
    // Engine: two kings selected, glass +0.00, canio +2.00.
    let mut game = run(&["Glass Joker", "Canio"]);
    let plain: Vec<CardRef> = game.hand[..2].to_vec();
    for card in &plain {
        card.borrow_mut().rank = Rank::King;
    }
    let before_glass = counter_of(&game, "Glass Joker");
    let before_canio = counter_of(&game, "Canio");
    let spec = consumables::spec_or_panic("The Hanged Man");
    (spec.apply.unwrap())(&mut game, &plain);
    assert_eq!(counter_of(&game, "Glass Joker"), before_glass);
    assert_eq!(counter_of(&game, "Canio"), before_canio + 2.0);
}

#[test]
fn test_canio_eats_a_tarot_victim_even_though_glass_joker_does_not() {
    let mut game = run(&["Glass Joker", "Canio"]);
    let card = face_card(&game);
    card.borrow_mut().enhancement = Enhancement::Glass;
    let glass_before = counter_of(&game, "Glass Joker");
    let canio_before = counter_of(&game, "Canio");
    game.remove_card(&card, false);
    assert_eq!(counter_of(&game, "Glass Joker"), glass_before);
    assert_eq!(counter_of(&game, "Canio"), canio_before + 1.0);
}

#[test]
fn test_hit_the_road_counts_discarded_jacks() {
    let mut game = run(&["Hit the Road"]);
    game.hand = vec![
        make_card(Rank::Jack, Suit::Spades),
        make_card(Rank::Two, Suit::Clubs),
        make_card(Rank::Jack, Suit::Hearts),
    ];
    let before = counter_of(&game, "Hit the Road");
    game.step(&Action::with_cards(ActionType::Discard, vec![0, 1, 2]));
    assert_eq!(counter_of(&game, "Hit the Road"), before + 1.0); // two jacks
}

#[test]
fn test_yorick_counts_down_to_its_next_multiplier() {
    // Twenty-three cards, counted down on the joker rather than run-wide.
    let mut game = run(&["Yorick"]);
    let joker = game.jokers[0].clone();
    assert_eq!(joker.borrow().secondary, 23.0);
    game.hand = (0..5).map(|_| make_card(Rank::Two, Suit::Clubs)).collect();
    game.step(&Action::with_cards(
        ActionType::Discard,
        vec![0, 1, 2, 3, 4],
    ));
    assert_eq!(joker.borrow().secondary, 18.0);
    assert_eq!(
        joker.borrow().counter,
        1.0,
        "it should not have paid out yet"
    );

    joker.borrow_mut().secondary = 1.0;
    game.hand = vec![make_card(Rank::Two, Suit::Clubs)];
    game.discards_left = 1;
    game.step(&Action::with_cards(ActionType::Discard, vec![0]));
    assert_eq!(joker.borrow().counter, 2.0);
    assert_eq!(
        joker.borrow().secondary,
        23.0,
        "and it starts counting again"
    );
}

#[test]
fn test_perkeo_copies_a_consumable_when_the_shop_closes() {
    let mut game = run(&["Perkeo"]);
    let fool = game.hold_consumable(consumables::spec_or_panic("The Fool"), Edition::None);
    game.consumables.push(fool);
    game.shop = None;
    game._leave_shop();
    let names: Vec<&str> = game
        .consumables
        .iter()
        .map(|c| c.borrow().spec.name)
        .collect();
    assert_eq!(names, vec!["The Fool", "The Fool"]);
}

#[test]
fn test_perkeo_copies_nothing_from_an_empty_row() {
    let mut game = run(&["Perkeo"]);
    game.shop = None;
    game._leave_shop();
    assert!(game.consumables.is_empty());
}

#[test]
fn test_rocket_grows_on_the_boss_it_is_paid_for() {
    // The boss that raises the payout is already paying the raised one: the
    // end_of_round branch raises `extra.dollars` and the cash-out rows are built
    // from calculate_dollar_bonus afterwards, so the first boss pays three
    // dollars, not one, and the boss after it five. Nothing moved this counter
    // at all before -- a Rocket paid a dollar a round for the whole run.
    let mut game = run(&["Rocket"]);
    for kind in [
        BlindKind::Small,
        BlindKind::Boss,
        BlindKind::Small,
        BlindKind::Boss,
    ] {
        round_end(&mut game, kind);
    }
    assert_eq!(paid(&game, "Rocket"), vec![1, 3, 3, 5]);
}

#[test]
fn test_castle_gains_chips_for_the_rounds_suit_only() {
    // card.lua:2814: +3 per discarded card of `castle_card.suit`, per card.
    // Nothing moved this counter, so a Castle was +0 chips however much the run
    // discarded.
    let mut game = run(&["Castle"]);
    game.castle_suit = Some(Suit::Hearts);
    game.hand = vec![
        make_card(Rank::Two, Suit::Hearts),
        make_card(Rank::Nine, Suit::Hearts),
        make_card(Rank::King, Suit::Clubs),
    ];
    game.step(&Action::with_cards(ActionType::Discard, vec![0, 1, 2]));
    assert_eq!(counter_of(&game, "Castle"), 6.0); // the two hearts, not the club
}

#[test]
fn test_castle_ignores_a_debuffed_card() {
    // `not context.other_card.debuff`, in the same line.
    let mut game = run(&["Castle"]);
    game.castle_suit = Some(Suit::Hearts);
    let hearts = vec![
        make_card(Rank::Two, Suit::Hearts),
        make_card(Rank::Nine, Suit::Hearts),
    ];
    hearts[0].borrow_mut().debuffed = true;
    game.hand = hearts;
    game.step(&Action::with_cards(ActionType::Discard, vec![0, 1]));
    assert_eq!(counter_of(&game, "Castle"), 3.0);
}

#[test]
fn test_pareidolia_stops_ride_the_bus_from_ever_growing() {
    // Every card is a face card (card.lua:967), so no hand is face-free. The
    // simulator asked `rank.is_face` and counted to eight while the engine sat
    // at zero.
    let hand = || {
        vec![
            make_card(Rank::Two, Suit::Clubs),
            make_card(Rank::Two, Suit::Hearts),
            make_card(Rank::Nine, Suit::Spades),
        ]
    };
    let mut game = run(&["Ride the Bus"]);
    game.hand = hand();
    game.step(&Action::with_cards(ActionType::Play, vec![0, 1, 2]));
    assert_eq!(
        counter_of(&game, "Ride the Bus"),
        1.0,
        "no face cards, so it grows"
    );

    let mut game = run(&["Ride the Bus", "Pareidolia"]);
    game.hand = hand();
    game.step(&Action::with_cards(ActionType::Play, vec![0, 1, 2]));
    assert_eq!(
        counter_of(&game, "Ride the Bus"),
        0.0,
        "every card is a face card"
    );
}

#[test]
fn test_trading_card_shatters_the_glass_card_it_eats() {
    // Discard destruction sets the flag inline, so this one does pay.
    let mut game = run(&["Glass Joker", "Trading Card"]);
    let card = game.hand[0].clone();
    card.borrow_mut().enhancement = Enhancement::Glass;
    let before = counter_of(&game, "Glass Joker");
    game.step(&Action::with_cards(ActionType::Discard, vec![0]));
    assert_eq!(counter_of(&game, "Glass Joker"), before + 0.75);
}

// ==========================================================================
// tests/test_matador_trigger.py -- "Matador pays only when the boss's ability
// actually went off."
//
// card.lua:3719-3729 (joker_main) and card.lua:2735-2745 (debuffed_hand) both
// read `G.GAME.blind.triggered`, and nothing else. The simulator paid $8 for
// every hand played into any boss, so a Flush of Spades into The Head -- which
// debuffs Hearts and has nothing to object to in that hand -- paid eight
// dollars a hand.
//
//   * play_cards_from_highlighted clears the flag before anything else.
//   * Blind:debuff_hand begins `if self.debuff then self.triggered = false`, so
//     The Hook's, The Tooth's and Crimson Heart's own set is wiped again.
//   * debuff_hand sets it when the hand is refused (The Psychic's five cards,
//     The Eye's repeat, The Mouth's other hand) and for The Arm when the hand
//     is above level 1 and The Ox when it is the most played hand.
//   * modify_hand sets it for The Flint, which runs only for a hand that was
//     not refused.
//   * the scoring loop sets it for every debuffed card in the scoring hand.
//
// A refused hand still pays, through the debuffed_hand context -- the simulator
// skipped every joker on a refused hand.
// ==========================================================================

fn flush(suit: Suit) -> Vec<CardRef> {
    // Five fresh cards, no face card among them: nothing a Plant or a Pillar
    // could hold against them, and no state left over from a test that played
    // them before.
    vec![
        make_card(Rank::Two, suit),
        make_card(Rank::Five, suit),
        make_card(Rank::Seven, suit),
        make_card(Rank::Nine, suit),
        make_card(Rank::Ten, suit),
    ]
}

fn boss_game(name: &str, jokers: &[&str]) -> GameState {
    let mut game = GameState::new("TESTSEED", "Red Deck", 1);
    for joker in jokers {
        game.gain_joker(&joker_ref(joker));
    }
    game.ante_boss = String::new();
    game.blind = Some(make_blind(
        BlindKind::Boss,
        game.ante,
        boss_by_name(name),
        1.0,
        1,
        false,
    ));
    game._start_round();
    game.blind.as_mut().unwrap().target = 1_000_000_000_000; // never beaten
    game
}

/// Play exactly these cards; what the hand paid or cost in dollars.
fn matador_play(game: &mut GameState, cards: &[CardRef]) -> i32 {
    let mut hand: Vec<CardRef> = cards.to_vec();
    hand.push(make_card(Rank::Three, Suit::Diamonds));
    hand.push(make_card(Rank::Four, Suit::Diamonds));
    hand.push(make_card(Rank::Six, Suit::Diamonds));
    game.hand = hand;
    // The boss debuffs what is in the deck (_apply_debuffs walks full_deck),
    // so cards made up for the test have to be put there.
    let extras: Vec<CardRef> = game
        .hand
        .iter()
        .filter(|c| !game.full_deck.iter().any(|d| Rc::ptr_eq(d, c)))
        .cloned()
        .collect();
    game.full_deck.extend(extras);
    let before = game.money;
    game.step(&Action::with_cards(
        ActionType::Play,
        (0..cards.len()).collect(),
    ));
    game.money - before
}
#[test]
fn test_nothing_triggered_pays_nothing() {
    // A Flush of plain Spades, no face card: nothing for the boss to do.
    for boss in [
        "The Head",
        "The Club",
        "The Window",
        "The Plant",
        "The Wall",
        "The Needle",
        "The Manacle",
        "The Water",
    ] {
        let mut game = boss_game(boss, &["Matador"]);
        assert_eq!(matador_play(&mut game, &flush(Suit::Spades)), 0, "{boss}");
    }
}

#[test]
fn test_a_debuffed_scoring_card_triggers_the_boss() {
    // state_events.lua:655-656.
    for (boss, suit) in [
        ("The Club", Suit::Clubs),
        ("The Goad", Suit::Spades),
        ("The Head", Suit::Hearts),
        ("The Window", Suit::Diamonds),
    ] {
        let mut game = boss_game(boss, &["Matador"]);
        assert_eq!(matador_play(&mut game, &flush(suit)), 8, "{boss}");
    }
}

#[test]
fn test_a_debuffed_high_card_triggers_the_boss() {
    // The engine's own case: K and Q of Clubs into The Club pays $8.
    let mut game = boss_game("The Club", &["Matador"]);
    let hand = vec![
        make_card(Rank::King, Suit::Clubs),
        make_card(Rank::Queen, Suit::Clubs),
    ];
    assert_eq!(matador_play(&mut game, &hand), 8);
}

#[test]
fn test_a_face_card_triggers_the_plant() {
    let mut game = boss_game("The Plant", &["Matador"]);
    let hand = vec![make_card(Rank::King, Suit::Spades)];
    assert_eq!(matador_play(&mut game, &hand), 8);
}

#[test]
fn test_a_debuffed_card_that_does_not_score_does_not_trigger() {
    // High Card scores only its best card; the debuffed Club rides along.
    let mut game = boss_game("The Club", &["Matador"]);
    let hand = vec![
        make_card(Rank::Ace, Suit::Spades),
        make_card(Rank::Nine, Suit::Clubs),
        make_card(Rank::Seven, Suit::Hearts),
        make_card(Rank::Five, Suit::Spades),
        make_card(Rank::Two, Suit::Hearts),
    ];
    assert_eq!(matador_play(&mut game, &hand), 0);
}

#[test]
fn test_the_flint_pays() {
    let mut game = boss_game("The Flint", &["Matador"]);
    assert_eq!(matador_play(&mut game, &flush(Suit::Spades)), 8);
}

#[test]
fn test_the_hook_does_not_pay_for_its_discard() {
    // press_play's flag is wiped by debuff_hand before any joker reads it.
    let mut game = boss_game("The Hook", &["Matador"]);
    assert_eq!(matador_play(&mut game, &flush(Suit::Spades)), 0);
}

#[test]
fn test_the_tooth_takes_its_dollars_and_matador_pays_nothing() {
    let mut game = boss_game("The Tooth", &["Matador"]);
    assert_eq!(matador_play(&mut game, &flush(Suit::Spades)), -5);
}

#[test]
fn test_a_refused_hand_pays_through_the_debuffed_hand_context() {
    // The Psychic refuses four cards, and that is the trigger.
    let mut game = boss_game("The Psychic", &["Matador"]);
    let four = flush(Suit::Spades)[..4].to_vec();
    assert_eq!(matador_play(&mut game, &four), 8);
    assert_eq!(game.chips_scored, 0);
}

#[test]
fn test_the_psychic_does_not_pay_for_five_cards() {
    let mut game = boss_game("The Psychic", &["Matador"]);
    assert_eq!(matador_play(&mut game, &flush(Suit::Spades)), 0);
}

#[test]
fn test_the_mouth_pays_only_for_the_hand_it_refuses() {
    let mut game = boss_game("The Mouth", &["Matador"]);
    assert_eq!(matador_play(&mut game, &flush(Suit::Spades)), 0); // sets the hand
    let pair = vec![
        make_card(Rank::Ace, Suit::Hearts),
        make_card(Rank::Ace, Suit::Clubs),
    ];
    assert_eq!(matador_play(&mut game, &pair), 8); // refused
}

#[test]
fn test_the_arm_pays_only_above_level_one() {
    let mut game = boss_game("The Arm", &["Matador"]);
    assert_eq!(matador_play(&mut game, &flush(Suit::Spades)), 0);
    game.hand_levels.levels.insert(HandType::Flush, 3);
    assert_eq!(matador_play(&mut game, &flush(Suit::Spades)), 8);
}

#[test]
fn test_the_flag_does_not_outlive_its_hand() {
    // Cleared at the start of every play, disabled blind or not.
    let mut game = boss_game("The Flint", &["Matador"]);
    assert_eq!(matador_play(&mut game, &flush(Suit::Spades)), 8);
    game.blind.as_mut().unwrap().disabled = true;
    assert_eq!(matador_play(&mut game, &flush(Suit::Spades)), 0);
}

#[test]
fn test_a_debuffed_matador_pays_nothing() {
    let mut game = boss_game("The Flint", &["Matador"]);
    game.jokers[0].borrow_mut().debuffed = true;
    assert_eq!(matador_play(&mut game, &flush(Suit::Spades)), 0);
}

#[test]
fn test_a_blueprint_copy_pays_on_a_refused_hand() {
    // Blueprint hands the debuffed_hand context straight to the copy
    // (card.lua:2305-2317).
    let mut game = boss_game("The Psychic", &["Blueprint", "Matador"]);
    let four = flush(Suit::Spades)[..4].to_vec();
    assert_eq!(matador_play(&mut game, &four), 16);
}

// `GameState::preview_trigger`: a preview sets the boss's trigger for the
// play it scores. Off (the default, and what every fixture was recorded
// under) it keeps the flag the last real play left.

/// What a preview of exactly these cards says they pay, the hand dealt as
/// `matador_play` deals it.
fn matador_preview(game: &mut GameState, cards: &[CardRef], trigger: bool) -> i64 {
    let mut hand: Vec<CardRef> = cards.to_vec();
    hand.push(make_card(Rank::Three, Suit::Diamonds));
    hand.push(make_card(Rank::Four, Suit::Diamonds));
    hand.push(make_card(Rank::Six, Suit::Diamonds));
    game.hand = hand;
    let extras: Vec<CardRef> = game
        .hand
        .iter()
        .filter(|c| !game.full_deck.iter().any(|d| Rc::ptr_eq(d, c)))
        .cloned()
        .collect();
    game.full_deck.extend(extras);
    game._apply_debuffs();
    game.preview_trigger = trigger;
    let indices: Vec<usize> = (0..cards.len()).collect();
    let before = game.blind.as_ref().unwrap().triggered;
    let dollars = game.preview_outcome(&indices).1;
    assert_eq!(game.blind.as_ref().unwrap().triggered, before, "put back");
    dollars
}

#[test]
fn test_a_preview_keeps_the_last_plays_trigger_unless_asked() {
    // A Club Flush into The Club sets the boss off; a Spade Flush after it
    // does not, which the stale flag previewed as $8.
    let mut game = boss_game("The Club", &["Matador"]);
    assert_eq!(matador_play(&mut game, &flush(Suit::Clubs)), 8);
    assert_eq!(matador_preview(&mut game, &flush(Suit::Spades), false), 8);
    assert_eq!(matador_preview(&mut game, &flush(Suit::Spades), true), 0);
    assert_eq!(matador_preview(&mut game, &flush(Suit::Clubs), true), 8);
    assert_eq!(matador_play(&mut game, &flush(Suit::Spades)), 0);
}

#[test]
fn test_a_preview_sets_the_arms_and_the_oxs_trigger() {
    let mut game = boss_game("The Arm", &["Matador"]);
    game.hand_levels.levels.insert(HandType::Flush, 3);
    assert_eq!(matador_preview(&mut game, &flush(Suit::Spades), false), 0);
    assert_eq!(matador_preview(&mut game, &flush(Suit::Spades), true), 8);
    assert_eq!(game.hand_levels.level(HandType::Flush), 3);
    let mut game = boss_game("The Ox", &["Matador"]);
    game.most_played_hand = HandType::Flush;
    assert_eq!(matador_preview(&mut game, &flush(Suit::Spades), false), 0);
    // The trigger only: The Ox's $0 is not previewed.
    assert_eq!(matador_preview(&mut game, &flush(Suit::Spades), true), 8);
    let pair = vec![
        make_card(Rank::Ace, Suit::Hearts),
        make_card(Rank::Ace, Suit::Clubs),
    ];
    assert_eq!(matador_preview(&mut game, &pair, true), 0);
}

#[test]
fn test_a_preview_pays_matador_for_a_refused_hand() {
    let mut game = boss_game("The Psychic", &["Blueprint", "Matador"]);
    let four = flush(Suit::Spades)[..4].to_vec();
    let money = game.money;
    assert_eq!(matador_preview(&mut game, &four, false), 0);
    assert_eq!(matador_preview(&mut game, &four, true), 16);
    assert_eq!(game.money, money);
    let logs = game.logs.len();
    assert_eq!(matador_preview(&mut game, &four, true), 16);
    assert_eq!(game.logs.len(), logs);
}
// ==========================================================================
// tests/test_joker_room.py -- "A joker needs a free slot. A Negative joker does
// not."
//
// G.FUNCS.can_select_card (button_callbacks.lua:2112) and the shop's can_buy at
// 2396 both add one to the limit when the card is Negative. `joker_slots`
// already counts the Negatives in the row, because add_to_deck raises the limit
// as one arrives -- so the property answers "how many fit" for what is held,
// and says nothing about what is being offered. The card on the shelf has not
// arrived. Asking only the property refused every Negative into a full row, in
// the shop and out of a Buffoon pack, both of which the game allows.
// ==========================================================================

fn full_row(mut game: GameState) -> GameState {
    // Five jokers, none of them Negative: the row is exactly full.
    while (game.jokers.len() as i32) < game.joker_slots() {
        game.gain_joker(&joker_ref("Joker"));
    }
    assert_eq!(game.jokers.len() as i32, game.joker_slots());
    assert_eq!(game.joker_slots(), 5);
    game
}

fn room_game() -> GameState {
    full_row(GameState::new("ROOMTEST", "Red Deck", 1))
}

fn blue_joker(negative: bool) -> JokerRef {
    let mut instance = JokerInstance::new(jokers::spec_or_panic("Blueprint"));
    instance.edition = if negative {
        Edition::Negative
    } else {
        Edition::None
    };
    jokers::make_ref(instance)
}

fn open_buffoon(mut game: GameState, negative: bool) -> GameState {
    game.phase = Phase::Pack;
    game.pack_options = vec![PackChoice::Joker(blue_joker(negative))];
    game.pack_picks_left = 1;
    game
}

fn shop_with(mut game: GameState, joker: JokerRef) -> GameState {
    game.phase = Phase::Shop;
    game._open_shop();
    let mut slot = ShopSlot::new("joker", 1);
    slot.joker = Some(joker);
    game.shop.as_mut().unwrap().slots = vec![slot];
    // `money`, not `dollars`: a Negative joker's edition is five of `extra_cost`.
    game.money = 50;
    game
}

fn open_arcana_with(mut game: GameState, name: &str) -> GameState {
    // An Arcana pack holding one card, as the shop would open it.
    game.phase = Phase::Pack;
    game.pack_options = vec![PackChoice::Consumable(consumables::spec_or_panic(name))];
    game.pack_picks_left = 1;
    game
}

fn has_action(game: &GameState, kind: ActionType) -> bool {
    game.legal_actions().iter().any(|a| a.r#type == kind)
}

#[test]
fn test_a_full_row_has_no_room_for_an_ordinary_joker() {
    assert!(!room_game().room_for_joker(&blue_joker(false)));
}

#[test]
fn test_a_full_row_has_room_for_a_negative_joker() {
    // It takes no slot -- add_to_deck raises the limit as it arrives.
    assert!(room_game().room_for_joker(&blue_joker(true)));
}

#[test]
fn test_a_row_with_a_gap_has_room_for_either() {
    let mut game = room_game();
    game.jokers.pop();
    assert!(game.room_for_joker(&blue_joker(false)));
    assert!(game.room_for_joker(&blue_joker(true)));
}

#[test]
fn test_a_buffoon_joker_is_not_offered_into_a_full_row() {
    let game = open_buffoon(room_game(), false);
    assert!(!has_action(&game, ActionType::PickPack));
    assert!(!game.is_legal(&Action::at(ActionType::PickPack, 0)));
}

#[test]
fn test_a_negative_buffoon_joker_is_offered_into_a_full_row() {
    // The one the simulator refused and the game allows.
    let game = open_buffoon(room_game(), true);
    assert!(has_action(&game, ActionType::PickPack));
    assert!(game.is_legal(&Action::at(ActionType::PickPack, 0)));
}

#[test]
fn test_a_shop_joker_is_not_offered_into_a_full_row() {
    let game = shop_with(room_game(), blue_joker(false));
    assert!(!has_action(&game, ActionType::Buy));
    assert!(!game.is_legal(&Action::at(ActionType::Buy, 0)));
}

#[test]
fn test_a_negative_shop_joker_is_offered_into_a_full_row() {
    let game = shop_with(room_game(), blue_joker(true));
    assert!(has_action(&game, ActionType::Buy));
    assert!(game.is_legal(&Action::at(ActionType::Buy, 0)));
}

#[test]
fn test_creation_is_still_stopped_by_a_full_row() {
    // A joker made rather than taken is checked before it exists: Riff-raff and
    // a Judgement ask for room and then create (card.lua:2529, 3967), so there is
    // no edition to consult and a full row stops the creation whatever it would
    // have rolled. Routing creation through room_for_joker would quietly give
    // the simulator free jokers.
    let mut game = room_game();
    let before = game.jokers.len();
    game.add_random_joker("test", None, false, "", false);
    assert_eq!(game.jokers.len(), before);
}

#[test]
fn test_a_judgement_is_not_offered_out_of_a_pack_into_a_full_row() {
    // A pack consumable's button is can_use_consumeable, not the looser
    // can_select_card. Judgement needs a free slot there exactly as it does from
    // the consumable row.
    let game = open_arcana_with(room_game(), "Judgement");
    assert!(!has_action(&game, ActionType::PickPack));
    assert!(!game.is_legal(&Action::at(ActionType::PickPack, 0)));
}

#[test]
fn test_a_judgement_is_offered_out_of_a_pack_into_a_gap() {
    let mut game = room_game();
    game.jokers.pop();
    let game = open_arcana_with(game, "Judgement");
    assert!(game.is_legal(&Action::at(ActionType::PickPack, 0)));
}

#[test]
fn test_a_planet_is_still_offered_out_of_a_pack_with_a_full_row() {
    // The row is full of jokers; a Planet needs none of them.
    let game = open_arcana_with(room_game(), "Mercury");
    assert!(game.is_legal(&Action::at(ActionType::PickPack, 0)));
}
// ==========================================================================
// tests/test_invisible_joker_sold.py -- "Invisible Joker, sold after two rounds,
// copies a random other joker."
//
// The simulator counted its rounds and did nothing when it was sold. The game
// answers selling_self (card.lua:2371-2390): with invis_rounds >= extra (2) and
// not context.blueprint it copies a pseudorandom_element of the row's other
// jokers, room-checked with the sold card still held. Card:sell_card fires that
// before the card dissolves (card.lua:1599), so the room check counts the
// Invisible Joker as still in the row. A debuffed joker answers no context at
// all (card.lua:2292).
// ==========================================================================

fn invisible_row(names: &[&str], invisible_rounds: f64) -> GameState {
    let mut game = GameState::new("TESTSEED", "Red Deck", 1);
    for name in names {
        let joker = joker_ref(name);
        if *name == "Invisible Joker" {
            joker.borrow_mut().counter = invisible_rounds;
        }
        game.gain_joker(&joker);
    }
    game
}

fn sell_invisible(game: &mut GameState) {
    let index = game
        .jokers
        .iter()
        .position(|j| j.borrow().name() == "Invisible Joker")
        .unwrap();
    game.step(&Action::at(ActionType::SellJoker, index as i32));
}

fn row_names(game: &GameState) -> Vec<String> {
    game.jokers
        .iter()
        .map(|j| j.borrow().name().to_string())
        .collect()
}

/// A copy of the run's RNG state, so a probe draw does not move the run.
fn copy_rng(rng: &jimbot_sim::rng::RunRng) -> jimbot_sim::rng::RunRng {
    let mut out = jimbot_sim::rng::RunRng::new(rng.seed.clone());
    out.pools = rng.pools.clone();
    out.live = rng.live.clone();
    out.pessimistic = rng.pessimistic;
    out
}

#[test]
fn test_a_charged_invisible_joker_leaves_a_copy_behind() {
    // pseudorandom_element sorts by sort_id before it draws
    // (misc_functions.lua:253-268), so the row is dragged out of age order here:
    // a draw over the row as it stands picks the other joker.
    let mut game = invisible_row(&["Joker", "Greedy Joker", "Invisible Joker"], 2.0);
    let joker = game.jokers[0].clone();
    let greedy = game.jokers[1].clone();
    let invisible = game.jokers[2].clone();
    game.jokers = vec![greedy.clone(), invisible, joker.clone()];
    let mut probe = copy_rng(&game.rng);
    let expected_name = probe
        .choice("invisible", &[joker.clone(), greedy.clone()])
        .borrow()
        .name()
        .to_string();

    sell_invisible(&mut game);

    assert_eq!(row_names(&game)[..2], ["Greedy Joker", "Joker"]);
    assert_eq!(
        game.jokers.len(),
        3,
        "the sale should leave a copy in the row"
    );
    let clone = game.jokers[2].clone();
    assert_eq!(clone.borrow().name(), expected_name);
    assert!(!Rc::ptr_eq(&clone, &joker) && !Rc::ptr_eq(&clone, &greedy));
    assert!(clone.borrow().uid > joker.borrow().uid.max(greedy.borrow().uid));
}

#[test]
fn test_one_round_is_not_enough() {
    let mut game = invisible_row(&["Joker", "Invisible Joker"], 1.0);
    sell_invisible(&mut game);
    assert_eq!(row_names(&game), ["Joker"]);
}

#[test]
fn test_a_debuffed_invisible_joker_copies_nothing() {
    // calculate_joker opens with `if self.debuff then return nil end`.
    let mut game = invisible_row(&["Joker", "Invisible Joker"], 2.0);
    game.jokers[1].borrow_mut().debuffed = true;
    sell_invisible(&mut game);
    assert_eq!(row_names(&game), ["Joker"]);
}

#[test]
fn test_alone_it_copies_nothing() {
    let mut game = invisible_row(&["Invisible Joker"], 2.0);
    sell_invisible(&mut game);
    assert!(game.jokers.is_empty());
}

#[test]
fn test_the_copy_of_a_negative_is_not_negative() {
    // copy_card's strip_edition skips set_edition entirely
    // (common_events.lua:2169-2171), and the caller passes it for a Negative.
    let mut game = invisible_row(&["Joker", "Invisible Joker"], 2.0);
    game.jokers[0].borrow_mut().edition = Edition::Negative;
    sell_invisible(&mut game);
    let editions: Vec<Edition> = game.jokers.iter().map(|j| j.borrow().edition).collect();
    assert_eq!(editions, vec![Edition::Negative, Edition::None]);
}

#[test]
fn test_the_copy_keeps_everything_else_the_original_has() {
    // The loop over other.ability (common_events.lua:2161-2167) carries the
    // counter, the stickers and hands_played_at_create -- set_ability stamps the
    // new card's own (card.lua:337) and the loop writes over it.
    let mut game = invisible_row(&["Loyalty Card", "Invisible Joker"], 2.0);
    let loyalty = game.jokers[0].clone();
    {
        let mut l = loyalty.borrow_mut();
        l.edition = Edition::Foil;
        l.perishable = true;
        l.perish_tally = 3;
    }
    game.hands_played = 7;
    sell_invisible(&mut game);
    let clone = game.jokers[1].clone();
    assert_eq!(clone.borrow().name(), "Loyalty Card");
    assert_eq!(clone.borrow().edition, Edition::Foil);
    assert!(clone.borrow().perishable);
    assert_eq!(clone.borrow().perish_tally, 3);
    assert_eq!(
        clone.borrow().hands_at_create,
        loyalty.borrow().hands_at_create
    );
    assert_eq!(clone.borrow().hands_at_create, 0);
}

#[test]
fn test_a_copied_invisible_joker_starts_counting_again() {
    let mut game = invisible_row(&["Invisible Joker", "Invisible Joker"], 3.0);
    game.step(&Action::at(ActionType::SellJoker, 0));
    assert_eq!(row_names(&game), ["Invisible Joker", "Invisible Joker"]);
    let counters: Vec<f64> = game.jokers.iter().map(|j| j.borrow().counter).collect();
    assert_eq!(counters, vec![3.0, 0.0]);
}

#[test]
fn test_a_full_row_still_gets_its_copy() {
    // #G.jokers.cards <= card_limit is read with the sold card still held.
    let mut game = invisible_row(
        &[
            "Joker",
            "Greedy Joker",
            "Lusty Joker",
            "Wrathful Joker",
            "Invisible Joker",
        ],
        2.0,
    );
    assert_eq!(game.jokers.len() as i32, game.joker_slots());
    sell_invisible(&mut game);
    assert_eq!(game.jokers.len(), 5);
}

#[test]
fn test_a_negative_invisible_joker_still_counts_its_own_slot() {
    // Six cards against a limit of six while it is held, so the copy is made --
    // and the row ends one over once remove_from_deck takes the slot back.
    let mut game = invisible_row(
        &[
            "Joker",
            "Greedy Joker",
            "Lusty Joker",
            "Wrathful Joker",
            "Gluttonous Joker",
            "Invisible Joker",
        ],
        2.0,
    );
    game.jokers[5].borrow_mut().edition = Edition::Negative;
    assert_eq!(game.jokers.len() as i32, game.joker_slots());
    assert_eq!(game.joker_slots(), 6);
    sell_invisible(&mut game);
    assert_eq!(game.jokers.len(), 6);
    assert_eq!(game.joker_slots(), 5);
}
// ==========================================================================
// tests/test_hook_burnt_emperor_8ball.py -- "Three simulator-only effects a
// headless smoke test found."
//
// 1. Burnt Joker does not level on a Hook discard: card.lua:2749 gates on
//    `G.GAME.current_round.discards_used <= 0 and not context.hook`. The Hook
//    passes the flag into the pre_discard context. The simulator levelled the
//    two cards The Hook took.
// 2. The Emperor cannot make another Emperor: G.FUNCS.use_card takes the card
//    out of its area but does not remove it -- and used_jokers is not cleared --
//    until it dissolves, after its effect. Its two Tarots are created inside
//    that window, and get_current_pool blanks every centre in used_jokers, so
//    The Emperor is not in the pool it draws from.
// 3. 8 Ball spends no roll when the consumable row is full: card.lua:3106-3107
//    checks room *first*, then rolls. The simulator rolled for every scored 8.
// ==========================================================================

fn levels(game: &GameState) -> BTreeMap<String, i32> {
    game.hand_levels
        .levels
        .iter()
        .filter(|(_, n)| **n != 1)
        .map(|(hand, n)| (hand.label().to_string(), *n))
        .collect()
}

fn burnt_on_the_hook(seed: &str) -> GameState {
    let mut game = GameState::new(seed, "Red Deck", 1);
    game.gain_joker(&joker_ref("Burnt Joker"));
    game._start_round();
    game.blind = Some(make_blind(
        BlindKind::Boss,
        1,
        boss_by_name("The Hook"),
        1.0,
        1,
        false,
    ));
    game.hand = game.hand[..4].to_vec();
    game
}

#[test]
fn test_burnt_joker_ignores_a_hook_discard() {
    let mut game = burnt_on_the_hook("TESTSEED");
    let discard_before = game.discard_pile.len();

    game._play(vec![0, 1]);

    assert!(
        game.discard_pile.len() - discard_before >= 2,
        "The Hook took nothing, so this test proves nothing"
    );
    assert!(
        levels(&game).is_empty(),
        "Burnt Joker levelled the cards The Hook discarded: {:?}",
        levels(&game)
    );
}

#[test]
fn test_burnt_joker_still_levels_the_first_real_discard_after_a_hook() {
    // A Hook discard does not count as the round's discard either.
    let mut game = burnt_on_the_hook("TESTSEED");
    game._play(vec![0, 1]);
    assert_eq!(game.discards_used, 0);

    game._discard(&[0]);

    let expected: BTreeMap<String, i32> = [("High Card".to_string(), 2)].into_iter().collect();
    assert_eq!(
        levels(&game),
        expected,
        "the first discard of the round did not level High Card"
    );
}

fn made_by_emperor(seed: &str, held: bool) -> Vec<String> {
    let mut game = GameState::new(seed, "Red Deck", 1);
    game._start_round();
    game.phase = Phase::Playing;
    game.consumables.clear();
    if held {
        let emperor =
            game.hold_consumable(consumables::spec_or_panic("The Emperor"), Edition::None);
        game.consumables.push(emperor);
        game.step(&Action::at(ActionType::UseConsumable, 0));
    } else {
        // buy-and-use and a pack pick: the card is in no area while it is used
        let spec = consumables::spec_or_panic("The Emperor");
        game.use_consumable(spec, &[], false);
    }
    game.consumables
        .iter()
        .map(|c| c.borrow().spec.name.to_string())
        .collect()
}

fn emperor_seeds() -> Vec<String> {
    (0..160).map(|i| format!("EMP{i:04}")).collect()
}

#[test]
fn test_the_emperor_used_from_a_slot_never_makes_an_emperor() {
    let mut bad: Vec<(String, Vec<String>)> = Vec::new();
    for seed in emperor_seeds() {
        let names = made_by_emperor(&seed, true);
        assert_eq!(names.len(), 2, "{seed}: {names:?}");
        if names.iter().any(|n| n == "The Emperor") {
            bad.push((seed, names));
        }
    }
    assert!(bad.is_empty(), "The Emperor made itself: {bad:?}");
}

#[test]
fn test_the_emperor_used_from_nowhere_never_makes_an_emperor() {
    let mut bad: Vec<(String, Vec<String>)> = Vec::new();
    for seed in emperor_seeds() {
        let names = made_by_emperor(&seed, false);
        if names.iter().any(|n| n == "The Emperor") {
            bad.push((seed, names));
        }
    }
    assert!(bad.is_empty(), "The Emperor made itself: {bad:?}");
}

#[test]
fn test_the_emperor_is_back_in_the_pool_once_it_is_gone() {
    // used_jokers is cleared when the card is removed, so the next draw may
    // offer an Emperor again -- the blank lasts exactly as long as the use.
    let mut game = GameState::new("TESTSEED", "Red Deck", 1);
    game._start_round();
    game.phase = Phase::Playing;
    let spec = consumables::spec_or_panic("The Emperor");
    game.use_consumable(spec, &[], false);
    assert!(!game.seen_centers().contains("c_emperor"));
}

fn eights(seed: &str, full_row: bool) -> GameState {
    let mut game = GameState::new(seed, "Red Deck", 1);
    game.gain_joker(&joker_ref("8 Ball"));
    game._start_round();
    game.blind = Some(make_blind(BlindKind::Boss, 1, None, 1.0, 1, false));
    game.consumables.clear();
    if full_row {
        while (game.consumables.len() as i32) < game.consumable_slots() {
            let hermit =
                game.hold_consumable(consumables::spec_or_panic("The Hermit"), Edition::None);
            game.consumables.push(hermit);
        }
    }
    for card in game.hand[..4].to_vec() {
        let mut c = card.borrow_mut();
        c.rank = Rank::Eight;
        c.enhancement = Enhancement::None;
        c.debuffed = false;
    }
    game
}

#[test]
fn test_8_ball_spends_no_roll_with_a_full_row() {
    let mut game = eights("TESTSEED", true);
    game._play(vec![0, 1, 2, 3]);

    let mut fresh = GameState::new("TESTSEED", "Red Deck", 1);
    assert_eq!(
        game.rng.pseudorandom("8ball", None, None),
        fresh.rng.pseudorandom("8ball", None, None),
        "8 Ball rolled for its 8s with no room for the Tarot"
    );
}

#[test]
fn test_8_ball_stops_rolling_once_its_tarot_fills_the_row() {
    // Room is re-read for every 8 -- consumeable_buffer is the game's way of
    // counting a Tarot it has promised but not yet built -- so with one free
    // slot the rolls stop at the first success.
    //
    // Python monkeypatches `rng.chance` to record each roll. Rust cannot wrap a
    // method, so the same fact is read off the run's own bit-exact stream: a run
    // whose row is already full rolls *zero* times, so calling
    // `pseudorandom("8ball")` on it walks the untouched stream v1, v2, ...; the
    // one-slot run's next draw must be v_{k+1} where k is the first draw below
    // 1/4. Rolling for every 8 leaves it at v5 instead whenever k < 4 -- exactly
    // the case the Python assertion catches.
    let mut fired_somewhere = false;
    for i in 0..40 {
        let seed = format!("EIGHT{i:03}");

        // A: the row is full, so the 8ball stream is never advanced.
        let mut full = eights(&seed, true);
        full._play(vec![0, 1, 2, 3]);
        let stream: Vec<f64> = (0..5)
            .map(|_| full.rng.pseudorandom("8ball", None, None))
            .collect();
        // The first success, 1-based; none means all four missed.
        let first = (1..=4).find(|k| stream[k - 1] < 1.0 / 4.0);

        // B: exactly one free slot.
        let mut game = eights(&seed, true);
        game.consumables.pop();
        let before = game.consumables.len();
        game._play(vec![0, 1, 2, 3]);
        let made = game.consumables.len() > before;
        let next = game.rng.pseudorandom("8ball", None, None);

        assert_eq!(made, first.is_some(), "{seed}: whether a Tarot was made");
        if first.is_some() {
            fired_somewhere = true;
            let expected = stream[first.unwrap()];
            assert_eq!(
                next, expected,
                "{seed}: 8 Ball kept rolling after its Tarot filled the row"
            );
        } else {
            assert_eq!(next, stream[4], "{seed}: a miss must not stop the rolls");
        }
    }
    assert!(
        fired_somewhere,
        "no seed made a Tarot, so this proves nothing"
    );
}
// ==========================================================================
// tests/test_pool_flags_and_gates.py -- "Cavendish only after Gros Michel dies,
// and a gated joker only with its card."
//
// Two gates in get_current_pool (common_events.lua:2012-2028) decide whether a
// joker can be rolled at all, and neither is on the joker's text:
//
//   * yes_pool_flag / no_pool_flag. Gros Michel carries 'gros_michel_extinct'
//     and Cavendish yes_pool_flag for the same name; card.lua:3037 sets the flag
//     in the branch where Gros Michel goes extinct. Destroying it without the
//     flag left Cavendish unobtainable -- Marcin's QWEFRTUZ run stopped on it.
//   * enhancement_gate. Lucky Cat needs a Lucky card in G.playing_cards, Glass
//     Joker a Glass card. The shop, the packs and Judgement checked it; the
//     Uncommon and Rare Tags did not.
// ==========================================================================

/// A round ended with Gros Michel held: the request path, not the helper.
fn michel_round(seed: &str) -> (GameState, JokerRef) {
    let mut game = GameState::new(seed, "Blue Deck", 1);
    let michel = joker_ref("Gros Michel");
    game.gain_joker(&michel);
    game.blind = Some(make_blind(BlindKind::Small, game.ante, None, 1.0, 1, false));
    game._beat_blind(true);
    (game, michel)
}

#[test]
fn test_extinction_sets_the_flag() {
    // Python monkeypatches `rng.chance` to force the 1-in-6. Rust cannot wrap a
    // method, so the extinction branch is found by seed: the flag must be set on
    // exactly the rounds where Gros Michel is gone.
    let mut seen_extinction = false;
    for i in 0..40 {
        let seed = format!("MICHEL{i:02}");
        let (game, michel) = michel_round(&seed);
        if game.pool_flags.contains("gros_michel_extinct") {
            seen_extinction = true;
            assert!(
                !game.jokers.iter().any(|j| Rc::ptr_eq(j, &michel)),
                "{seed}: the flag is set but Gros Michel is still held"
            );
        }
    }
    assert!(
        seen_extinction,
        "no seed went extinct, so this proves nothing"
    );
}

#[test]
fn test_surviving_does_not() {
    let mut seen_survival = false;
    for i in 0..40 {
        let seed = format!("MICHEL{i:02}");
        let (game, michel) = michel_round(&seed);
        if game.jokers.iter().any(|j| Rc::ptr_eq(j, &michel)) {
            seen_survival = true;
            assert!(
                !game.pool_flags.contains("gros_michel_extinct"),
                "{seed}: a surviving Gros Michel set the flag"
            );
        }
    }
    assert!(seen_survival, "no seed survived, so this proves nothing");
}

#[test]
fn test_the_flag_swaps_gros_michel_for_cavendish_in_the_pool() {
    let gros = jimbot_sim::shop_pool::key_by_joker_name("Gros Michel").unwrap();
    let owned: [&str; 0] = [];
    let seen: [&str; 0] = [];
    let no_flags: [&str; 0] = [];
    let before = jimbot_sim::shop_pool::build_pool(1u8, &owned, &seen, false, &no_flags);
    let after =
        jimbot_sim::shop_pool::build_pool(1u8, &owned, &seen, false, &["gros_michel_extinct"]);
    assert!(before.contains(&gros.to_string()) && !before.contains(&"j_cavendish".to_string()));
    assert!(after.contains(&"j_cavendish".to_string()) && !after.contains(&gros.to_string()));
    assert_eq!(
        before.len(),
        after.len(),
        "entries are blanked, never removed"
    );
}

#[test]
fn test_the_shop_stocks_with_the_run_flags() {
    let mut game = GameState::new("QWEFRTUZ", "Blue Deck", 1);
    game.pool_flags.insert("gros_michel_extinct".to_string());
    let mut seen: HashSet<String> = HashSet::new();
    for _ in 0..400 {
        game._open_shop();
        for slot in &game.shop.as_ref().unwrap().slots {
            if let Some(joker) = &slot.joker {
                seen.insert(joker.borrow().name().to_string());
            }
        }
        if seen.contains("Cavendish") {
            break;
        }
    }
    assert!(seen.contains("Cavendish") && !seen.contains("Gros Michel"));
}

#[test]
fn test_a_rare_tag_joker_respects_the_enhancement_gate() {
    // The Python test spies on `shop_pool.draw_joker` and checks the kwargs
    // include `owned_enhancements` and `pool_flags`. Rust cannot intercept the
    // call, and no rarity-3 joker carries a gate or a pool flag -- so the two
    // arguments cannot change a *rare* draw and no fixture could tell them apart.
    // What is checkable is the two halves of the claim: the Rare Tag really does
    // fill a slot with a joker, and the call in `_forced_shop_slot` really does
    // hand the draw the run's enhancements and flags.
    let mut game = GameState::new("QWEFRTUZ", "Blue Deck", 1);
    game.tags.push(Tag::Rare);
    game._open_shop();
    let entry = game
        .shop
        .as_ref()
        .unwrap()
        .slots
        .iter()
        .find(|slot| slot.couponed && slot.joker.is_some());
    assert!(entry.is_some(), "the Rare Tag drew no joker");

    let source = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/game.rs"))
        .expect("game.rs");
    let start = source
        .find("pub fn _forced_shop_slot")
        .expect("_forced_shop_slot exists");
    let tail = &source[start..];
    let end = tail[1..]
        .find("\n    pub fn ")
        .map(|offset| offset + 1)
        .unwrap_or(tail.len());
    let body = &tail[..end];
    assert!(
        body.contains("draw_joker") && body.contains("&owned") && body.contains("&flags"),
        "the Rare Tag's draw does not pass owned_enhancements / pool_flags"
    );
}
// ==========================================================================
// tests/test_deck_compare.py -- "Two decks holding different cards are a
// difference."
//
// FATBOY02 stopped 299 decisions into a live run with a 6D in the game's hand
// and a red-sealed 6H in the shadow's -- a deck that had disagreed for some
// time, and showed it only when a draw happened to deal the difference. Both
// sides report the deck's make-up for the observation encoder; nothing compared
// it.
//
// `jimbot_sim.compare` is the policy's live driver, which the port leaves out of
// scope (see rust/PORTING.md). The two pure functions the assertions use --
// `deck_cards` and `differences` -- are reimplemented here as test-local code,
// the same way `tests/port_a.rs` reimplements the replay driver's `ranked`.
// `differences` compares every top-level state key, so "no difference" means the
// whole observation agrees.
// ==========================================================================

fn state_name_of(value: &state::StateValue) -> String {
    if let state::StateValue::Map(m) = value {
        if let Some(state::StateValue::Str(name)) = m.get("state_name") {
            return name.clone();
        }
    }
    String::new()
}

fn count_list(value: &state::StateValue, field: &str) -> Vec<i64> {
    if let state::StateValue::Map(m) = value {
        if let Some(state::StateValue::List(items)) = m.get(field) {
            return items
                .iter()
                .map(|v| match v {
                    state::StateValue::Int(n) => *n,
                    _ => 0,
                })
                .collect();
        }
    }
    Vec::new()
}

/// `tally`: label each slot, skip an unlabelled or empty one, strip the "-".
fn tally(
    out: &mut BTreeMap<String, i64>,
    counts: &state::StateValue,
    field: &str,
    labels: &[&str],
    base: i64,
    prefix: &str,
) {
    for (slot, count) in count_list(counts, field).iter().enumerate() {
        let label = match labels.get((slot as i64 + base) as usize) {
            Some(label) => *label,
            None => continue,
        };
        if *count == 0 {
            continue;
        }
        let stripped = label.trim_start_matches('-');
        let name = if stripped.is_empty() {
            "none"
        } else {
            stripped
        };
        out.insert(format!("{prefix}{name}"), *count);
    }
}

const RANK_LABELS: [&str; 14] = [
    "", "2", "3", "4", "5", "6", "7", "8", "9", "10", "J", "Q", "K", "A",
];
const SUIT_LABELS: [&str; 5] = ["", "S", "H", "C", "D"];
const ENHANCEMENT_LABELS: [&str; 9] = [
    "plain", "bonus", "mult", "wild", "glass", "steel", "stone", "gold", "lucky",
];
const SEAL_LABELS: [&str; 5] = ["", "-gold", "-red", "-blue", "-purple"];
const EDITION_LABELS: [&str; 5] = ["", "-foil", "-holo", "-poly", "-neg"];

fn deck_cards(game: &GameState) -> BTreeMap<String, i64> {
    // What the whole deck holds, by name: {"6": 4, "D": 13, "steel": 1}. Only
    // the counts, so this catches a card that is not the same card.
    let counts = state::deck_counts(&game.full_deck);
    let mut out = BTreeMap::new();
    tally(&mut out, &counts, "ranks", &RANK_LABELS, 1, "");
    tally(&mut out, &counts, "suits", &SUIT_LABELS, 1, "");
    tally(
        &mut out,
        &counts,
        "enhancements",
        &ENHANCEMENT_LABELS,
        0,
        "",
    );
    tally(&mut out, &counts, "seals", &SEAL_LABELS, 0, "seal ");
    tally(
        &mut out,
        &counts,
        "editions",
        &EDITION_LABELS,
        0,
        "edition ",
    );
    out
}

fn py_map(m: &BTreeMap<String, i64>) -> String {
    let inner = m
        .iter()
        .map(|(k, v)| format!("'{k}': {v}"))
        .collect::<Vec<_>>()
        .join(", ");
    format!("{{{inner}}}")
}
fn differences(game: &GameState, shadow: &GameState) -> Vec<String> {
    let a = state::state_dict(game, &[], 0, 0);
    let b = state::state_dict(shadow, &[], 0, 0);
    let mut out = Vec::new();

    // The deck is compared only between rounds: mid-hand it is in flight, and
    // the live driver re-reads after every action until the difference clears.
    let name_a = state_name_of(&a);
    let name_b = state_name_of(&b);
    if name_a == name_b && (name_a == "SHOP" || name_a == "BLIND_SELECT") {
        let held = deck_cards(game);
        let mirrored = deck_cards(shadow);
        let mut keys: BTreeSet<String> = held.keys().cloned().collect();
        keys.extend(mirrored.keys().cloned());
        let keys: Vec<String> = keys
            .into_iter()
            .filter(|k| held.get(k).unwrap_or(&0) != mirrored.get(k).unwrap_or(&0))
            .collect();
        if !keys.is_empty() {
            let want: BTreeMap<String, i64> = keys
                .iter()
                .map(|k| (k.clone(), *held.get(k).unwrap_or(&0)))
                .collect();
            let got: BTreeMap<String, i64> = keys
                .iter()
                .map(|k| (k.clone(), *mirrored.get(k).unwrap_or(&0)))
                .collect();
            out.push(format!(
                "deck_cards    game {}\ndeck_cards    shadow {}",
                py_map(&want),
                py_map(&got)
            ));
        }
    }

    if let (state::StateValue::Map(ma), state::StateValue::Map(mb)) = (&a, &b) {
        let mut keys: BTreeSet<&String> = ma.keys().collect();
        keys.extend(mb.keys());
        for key in keys {
            if key == "deck_cards" {
                continue;
            }
            if ma.get(key) != mb.get(key) {
                out.push(format!("{key}: differs"));
            }
        }
    }
    out
}

fn deck_game() -> GameState {
    GameState::new("AWEFRTUZ", "Red Deck", 1)
}

fn deck_lines(lines: &[String]) -> Vec<String> {
    lines
        .iter()
        .filter(|line| line.starts_with("deck_cards"))
        .cloned()
        .collect()
}

#[test]
fn test_a_deck_of_the_same_cards_is_no_difference() {
    assert!(differences(&deck_game(), &deck_game()).is_empty());
}

#[test]
fn test_the_card_that_is_not_the_same_card_is_named() {
    let mut game = deck_game();
    let mut shadow = deck_game();
    game.add_card_to_hand(&make_card(Rank::Six, Suit::Diamonds));
    let card = make_card(Rank::Six, Suit::Hearts);
    card.borrow_mut().seal = Seal::Red;
    shadow.add_card_to_hand(&card);
    let found = deck_lines(&differences(&game, &shadow));
    assert_eq!(found.len(), 1, "{found:?}");
    // Only what differs: the suits that moved and the seal, not all 52.
    assert!(found[0].contains("'D': 14"), "{}", found[0]);
    assert!(found[0].contains("'H': 14"), "{}", found[0]);
    assert!(found[0].contains("'seal red': 1"), "{}", found[0]);
    assert!(!found[0].contains("'S'"), "{}", found[0]);
}

#[test]
fn test_an_enhancement_on_one_side_only() {
    let game = deck_game();
    let mut shadow = deck_game();
    let card = shadow.full_deck[0].clone();
    shadow.set_enhancement(&card, Enhancement::Steel);
    let found = deck_lines(&differences(&game, &shadow));
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].contains("steel"), "{}", found[0]);
}

#[test]
fn test_the_deck_is_read_by_name() {
    let mut game = deck_game();
    let card = make_card(Rank::Ace, Suit::Spades);
    {
        let mut c = card.borrow_mut();
        c.seal = Seal::Gold;
        c.enhancement = Enhancement::Gold;
    }
    game.add_card_to_hand(&card);
    let held = deck_cards(&game);
    assert_eq!(held["A"], 5);
    assert_eq!(held["S"], 14);
    assert_eq!(held["seal gold"], 1);
    assert_eq!(held["gold"], 1);
    assert_eq!(held["plain"], 52);
}

#[test]
fn test_a_hand_in_flight_is_not_compared() {
    // Mid-round the deck is in flight, and the live driver re-reads until every
    // difference clears -- a tenth of a second at a time, after every action.
    // A deck that has really diverged is still caught at the shop.
    let mut game = deck_game();
    let mut shadow = deck_game();
    for one in [&mut game, &mut shadow] {
        one.step(&Action::new(ActionType::SelectBlind));
        assert_eq!(one.phase, Phase::Playing);
    }
    let card = make_card(Rank::Six, Suit::Hearts);
    card.borrow_mut().seal = Seal::Red;
    shadow.add_card_to_hand(&card);
    assert!(deck_lines(&differences(&game, &shadow)).is_empty());
}
// ==========================================================================
// tests/test_pack_joker_edition_rate.py -- "A joker from a Buffoon pack or an
// Uncommon/Rare Tag polls at the run's rate."
//
// Every joker create_card makes ends with
// `poll_edition('edi'..(key_append or '')..G.GAME.round_resets.ante)`
// (common_events.lua:2149), and poll_edition's ordinary branch widens the
// polychrome, holographic and foil bands by the global G.GAME.edition_rate
// (2071-2076) whatever the caller passes. Hone and Glow Up set that rate to
// their `extra` (card.lua:1900-1903). A Buffoon pack creates its jokers through
// create_card with key_append 'buf' (card.lua:1774), the Uncommon and Rare Tags
// with 'uta' and 'rta' (tag.lua:370, 356) -- so all three take the rate as a shop
// joker does. The simulator passed it to the shop, to add_random_joker and to a
// Standard pack, and not to these three, which moved the polychrome and
// holographic boundaries back to their unhoned places.
// ==========================================================================

fn honed(seed: &str) -> GameState {
    let mut game = GameState::new(seed, "Red Deck", 1);
    let voucher = jimbot_sim::shop::voucher_by_key("v_hone").unwrap();
    game.vouchers.push(voucher);
    game._redeem_voucher(voucher);
    assert_eq!(game.edition_rate(), 2.0);
    game
}

#[test]
fn test_a_buffoon_pack_joker_is_polled_at_the_runs_edition_rate() {
    // H7NS6Y2Y, Checkered Deck, stake 1, holding Hone: its first Buffoon pack of
    // ante 6 offered Mr. Bones and a Polychrome Mail-In Rebate in the real game,
    // where the simulator had a Holographic one. The second 'edibuf6' poll is
    // 0.99058: above 1 - 0.006*2 (polychrome at Hone's rate) but below
    // 1 - 0.006 (polychrome without it), and above 1 - 0.02 either way.
    let pack = |rate: f64| -> Vec<(String, String)> {
        let mut rng = jimbot_sim::rng::RunRng::new("H7NS6Y2Y");
        let cards = jimbot_sim::shop_pool::pack_contents(
            &mut rng,
            "Buffoon",
            2,
            6,
            &[] as &[&str],
            &[] as &[&str],
            false,
            &[] as &[&str],
            &[] as &[&str],
            &[] as &[&str],
            false,
            false,
            false,
            false,
            None,
            None,
            rate,
        );
        cards
            .iter()
            .map(|c| (c.key.clone().unwrap(), c.edition.unwrap().to_string()))
            .collect()
    };

    assert_eq!(
        pack(2.0),
        vec![
            ("j_mr_bones".to_string(), "none".to_string()),
            ("j_mail".to_string(), "polychrome".to_string())
        ]
    );
    // Without Hone the same roll is only holographic, so the rate is what the
    // comparison above is actually checking.
    assert_eq!(
        pack(1.0),
        vec![
            ("j_mr_bones".to_string(), "none".to_string()),
            ("j_mail".to_string(), "holo".to_string())
        ]
    );
}

fn edition_tier(edition: Edition) -> i32 {
    match edition {
        Edition::None => 0,
        Edition::Foil => 1,
        Edition::Holographic => 2,
        Edition::Polychrome => 3,
        Edition::Negative => 4,
    }
}

fn tag_slot(seed: &str, tag: Tag, honed_game: bool) -> (String, Edition) {
    let mut game = if honed_game {
        honed(seed)
    } else {
        GameState::new(seed, "Red Deck", 1)
    };
    game.ante = 3;
    game.tags.push(tag);
    let slot = game._forced_shop_slot().expect("the tag fills a slot");
    let joker = slot.joker.expect("the tag put a joker on the shelf");
    let name = joker.borrow().name().to_string();
    let edition = joker.borrow().edition;
    (name, edition)
}

#[test]
fn test_a_tag_joker_is_polled_at_the_runs_edition_rate() {
    // create_card(..., 'uta') / 'rta' (tag.lua:370, 356) -> common_events.lua:2149.
    //
    // Python monkeypatches `poll_edition` to record the key and the rate it was
    // handed. Rust cannot intercept it, so the rate is read off its effect
    // instead: the same seed is run with Hone (rate 2) and without (rate 1), and
    // the widened band can only move the edition up the ladder -- and must move
    // it for at least one seed, or the tag ignored the rate.
    for (tag, append) in [(Tag::Uncommon, "uta"), (Tag::Rare, "rta")] {
        let mut differed = false;
        for i in 0..60 {
            let seed = format!("EDIPOLL{i:02}");
            let (plain_name, plain_edition) = tag_slot(&seed, tag, false);
            let (honed_name, honed_edition) = tag_slot(&seed, tag, true);
            assert_eq!(
                honed_name, plain_name,
                "{seed}: the rate must not change which joker is drawn"
            );
            assert!(
                edition_tier(honed_edition) >= edition_tier(plain_edition),
                "{seed}: Hone narrowed the {append} band"
            );
            if honed_edition != plain_edition {
                differed = true;
            }
        }
        assert!(
            differed,
            "the {append} tag polled at the plain rate, so Hone did nothing to it"
        );
    }
}
// ==========================================================================
// tests/test_declared_flags.py -- "The flags JokerSpec declares, checked against
// what the game does with them."
//
// test_registry_honesty proves each of these has a reader. That is a weaker
// claim than it sounds, and this file is the audit that followed: the name
// appearing somewhere says nothing about the reader being right. Four of the
// thirteen were wrong in a way that changed play. The recurring one is *when* an
// effect lands: a joker's contribution to hand size, hands, discards, free
// rerolls, interest and the debt floor is written by Card:add_to_deck when it is
// bought and unwritten by remove_from_deck when it goes, and set_debuff calls
// remove_from_deck, so a debuffed joker's counter comes off with it. Burglar is
// the exception: it is ease_hands_played(+3) on setting_blind, which runs after
// the blind has decided the allowance, so it stacks on top of a boss.
// ==========================================================================

fn declared_run(names: &[&str], boss: Option<&str>) -> GameState {
    let mut game = GameState::new("TESTSEED", "Red Deck", 1);
    for name in names {
        game.gain_joker(&joker_ref(name));
    }
    if let Some(boss) = boss {
        game.ante_boss = String::new();
        game.blind = Some(make_blind(
            BlindKind::Boss,
            game.ante,
            boss_by_name(boss),
            1.0,
            1,
            false,
        ));
    }
    game._start_round();
    game
}

#[test]
fn test_burglar_gives_three_hands_and_takes_every_discard() {
    // Engine: 4 hands and 4 discards becomes 7 and 0.
    let game = declared_run(&["Burglar"], None);
    assert_eq!((game.hands_left, game.discards_left), (7, 0));
}

#[test]
fn test_two_burglars_stack() {
    // Engine: 10.
    assert_eq!(declared_run(&["Burglar", "Burglar"], None).hands_left, 10);
}

#[test]
fn test_burglar_beats_a_boss_that_dictates_the_allowance() {
    // The whole point of the timing. The Needle allows one hand; Burglar adds
    // its three afterwards, on setting_blind, so the round has four. Folding the
    // +3 into the base allowance let The Needle overwrite the lot and the run
    // played one.
    assert_eq!(declared_run(&[], Some("The Needle")).hands_left, 1);
    assert_eq!(declared_run(&["Burglar"], Some("The Needle")).hands_left, 4);
}

#[test]
fn test_burglar_still_empties_the_discards_a_drunkard_added() {
    // Engine: Burglar with a Drunkard is 7 hands and 0 discards.
    let game = declared_run(&["Burglar", "Drunkard"], None);
    assert_eq!((game.hands_left, game.discards_left), (7, 0));
}

fn declared_attr(game: &GameState, attr: &str) -> i32 {
    match attr {
        "hand_size" => game.hand_size(),
        "spendable" => game.spendable(),
        other => panic!("unknown attribute {other}"),
    }
}

#[test]
fn test_a_debuffed_joker_contributes_nothing() {
    // Ten of these were measured; each read the same debuffed as absent.
    for (name, attr, delta) in [
        ("Juggler", "hand_size", 1),
        ("Merry Andy", "hand_size", -1),
        ("Stuntman", "hand_size", -2),
        ("Turtle Bean", "hand_size", 5),
        ("Credit Card", "spendable", 20),
    ] {
        let plain = declared_attr(&GameState::new("TESTSEED", "Red Deck", 1), attr);
        let held = declared_run(&[name], None);
        assert_eq!(declared_attr(&held, attr), plain + delta, "{name}");
        held.jokers[0].borrow_mut().debuffed = true;
        assert_eq!(declared_attr(&held, attr), plain, "{name}");
    }
}

#[test]
fn test_a_debuffed_drunkard_gives_no_discard() {
    assert_eq!(declared_run(&["Drunkard"], None).discards_left, 5);
    let mut game = declared_run(&["Drunkard"], None);
    game.jokers[0].borrow_mut().debuffed = true;
    let (hands, discards) = game.round_allowance(true);
    game.hands_left = hands;
    game.discards_left = discards;
    assert_eq!(game.discards_left, 4);
}

#[test]
fn test_credit_card_buys_what_the_run_cannot_pay_for() {
    // Engine at $0: one card allows a $20 buy and refuses $21.
    let mut game = declared_run(&["Credit Card"], None);
    game.money = 0;
    assert!(game.affords(20));
    assert!(!game.affords(21));
}

#[test]
fn test_credit_cards_stack() {
    // Engine at $0: two allow $40 and refuse $41.
    let mut game = declared_run(&["Credit Card", "Credit Card"], None);
    game.money = 0;
    assert!(game.affords(40));
    assert!(!game.affords(41));
}

#[test]
fn test_a_run_with_no_credit_may_not_go_into_debt() {
    let mut game = declared_run(&[], None);
    game.money = 4;
    assert!(game.affords(4));
    assert!(!game.affords(5));
}

#[test]
fn test_something_free_is_always_takeable() {
    // `(cost > dollars - bankrupt_at) and (cost > 0)` -- the second half.
    let mut game = declared_run(&[], None);
    game.money = 0;
    assert!(game.affords(0));
}

#[test]
fn test_two_to_do_lists_name_two_hands() {
    // ability.to_do_poker_hand is on the joker, not on the run. Holding it on the
    // run made the second copy overwrite the first, so a pair of them paid out
    // together or not at all.
    let mut game = declared_run(&["To Do List", "To Do List"], None);
    let mut seen: HashSet<Vec<Option<HandType>>> = HashSet::new();
    for _ in 0..20 {
        game._reroll_todo_hands();
        seen.insert(game.jokers.iter().map(|j| j.borrow().named_hand).collect());
    }
    assert!(seen.len() > 1, "they never disagreed once");
}

#[test]
fn test_a_to_do_list_never_names_the_same_hand_twice_running() {
    let mut game = declared_run(&["To Do List"], None);
    let joker = game.jokers[0].clone();
    let mut previous = joker.borrow().named_hand;
    for _ in 0..30 {
        game._reroll_todo_hands();
        assert_ne!(joker.borrow().named_hand, previous);
        previous = joker.borrow().named_hand;
    }
}

#[test]
fn test_the_secret_hands_are_not_in_the_pool_until_they_are_played() {
    let mut game = declared_run(&["To Do List"], None);
    let visible: HashSet<HandType> = game.visible_hands().into_iter().collect();
    let all: HashSet<HandType> = HandType::ALL.into_iter().collect();
    let secret: HashSet<HandType> = SECRET_HANDS.into_iter().collect();
    assert_eq!(
        visible,
        all.difference(&secret).copied().collect::<HashSet<_>>()
    );
    assert_eq!(visible.len(), 9);

    *game
        .hand_levels
        .plays
        .entry(HandType::FlushHouse)
        .or_insert(0) += 1;
    assert!(game.visible_hands().contains(&HandType::FlushHouse));
    assert_eq!(game.visible_hands().len(), 10);
}

#[test]
fn test_the_pool_is_in_the_order_the_game_walks_its_hands() {
    // pairs(G.GAME.hands) under the game's LuaJIT 2.0.5, not G.handlist: the
    // draws that read this pool index into that walk. See GAME_PAIRS_ORDER.
    let game = declared_run(&[], None);
    let expected: Vec<HandType> = GAME_PAIRS_ORDER
        .into_iter()
        .filter(|hand| !SECRET_HANDS.contains(hand))
        .collect();
    assert_eq!(game.visible_hands(), expected);
}

#[test]
fn test_telescope_breaks_a_tie_towards_the_stronger_hand() {
    // ipairs(G.handlist) with a strict >, and handlist runs strongest first.
    // Python's max over a dict keyed in enum order gave the weakest instead, so a
    // run that had played one Pair and one Two Pair had its Celestial pack forced
    // to the wrong planet.
    let mut game = declared_run(&[], None);
    game.hand_levels.plays.insert(HandType::Pair, 1);
    game.hand_levels.plays.insert(HandType::TwoPair, 1);
    assert_eq!(game._most_played_planet().as_deref(), Some("c_uranus")); // Two Pair
}

#[test]
fn test_telescope_still_prefers_the_hand_actually_played_most() {
    let mut game = declared_run(&[], None);
    game.hand_levels.plays.insert(HandType::Pair, 5);
    game.hand_levels.plays.insert(HandType::TwoPair, 1);
    assert_eq!(game._most_played_planet().as_deref(), Some("c_mercury")); // Pair
}

#[test]
fn test_a_joker_only_takes_the_stickers_its_centre_allows() {
    // card.lua:506 and 513: set_eternal and set_perishable refuse. `eternal_compat`
    // is false for the jokers that destroy themselves -- Gros Michel, Popcorn,
    // Ice Cream -- and `perishable_compat` for the ones whose value is a counter
    // they would lose. Neither was modelled, so a Ride the Bus came out of the
    // shop perishable and was debuffed five rounds later in a run where the game
    // had left it alone.
    assert_eq!(
        jimbot_sim::shop_pool::takes_sticker("Ride the Bus"),
        (true, false)
    );
    assert!(!jimbot_sim::shop_pool::takes_sticker("Gros Michel").0);

    let mut game = GameState::new("TESTSEED", "Red Deck", 8);
    for name in ["Ride the Bus", "Gros Michel", "Joker"] {
        let (eternal_ok, perishable_ok) = jimbot_sim::shop_pool::takes_sticker(name);
        for _ in 0..40 {
            let joker = joker_ref(name);
            game._apply_stickers(&joker, false);
            assert!(!(joker.borrow().eternal && !eternal_ok), "{name}");
            assert!(!(joker.borrow().perishable && !perishable_ok), "{name}");
            // The game's own mutual exclusion, both ways round.
            assert!(
                !(joker.borrow().eternal && joker.borrow().perishable),
                "{name}"
            );
        }
    }
}
// ==========================================================================
// tests/test_debuffed_jokers_answer_nothing.py -- "A debuffed joker answers no
// calculate_joker context at all."
//
// Card:calculate_joker opens (card.lua:2291-2292)
//
//     function Card:calculate_joker(context)
//         if self.debuff then return nil end
//
// and says it again for jokers at card.lua:2303. Every joker context the game
// fires goes through that door: pre_discard and discard, end_of_round,
// first_hand_drawn, selling_self, selling_card, using_consumeable,
// playing_card_added, remove_playing_cards, reroll_shop, skipping_booster,
// ending_shop, open_booster, and before / joker_main / debuffed_hand while a
// hand scores. A Blueprint or a Brainstorm copies by calling
// `other_joker:calculate_joker(context)`, so a copy of a debuffed joker is
// nothing as well. Two more doors shut the same way: find_joker skips a debuffed
// joker unless asked not to, which is how Four Fingers, Shortcut, Splash,
// Pareidolia and Smeared Joker are read; and Oops! All 6s and Chicot live in
// add_to_deck / remove_from_deck, which set_debuff runs.
//
// What is *not* behind the guard: calculate_rental and calculate_perishable, Gift
// Card's walk down the row, and a Negative's slot, which remove_from_deck(true)
// only queues for removal. calculate_dollar_bonus has a guard of its own, read
// while the round's rows are built -- before the defeated blind's
// set_blind(nil) gives a Crimson Heart joker back.
//
// RRT5KY7W stopped on it at decision 188: a discard in an ante-8 Crimson Heart
// round with Ramen debuffed, X1.85 in the game and X1.8 in the shadow.
// ==========================================================================

type DiscardState = (f64, f64, i32, usize, HashMap<HandType, i32>);

fn discard_state(game: &GameState, name: &str) -> DiscardState {
    let joker = find_joker(game, name);
    let j = joker.borrow();
    (
        j.counter,
        j.secondary,
        game.money,
        game.full_deck.len(),
        game.hand_levels.levels.clone(),
    )
}

/// Make the first five cards jacks of the round's suit, and name their indices.
fn five_jacks(game: &mut GameState, name: &str) -> Vec<usize> {
    for card in game.hand[..5].to_vec() {
        let rank = if name == "Mail-In Rebate" {
            game.mail_rank.unwrap_or(Rank::Jack)
        } else {
            Rank::Jack
        };
        let suit = game.castle_suit.unwrap_or(Suit::Spades);
        let mut c = card.borrow_mut();
        c.rank = rank;
        c.suit = suit;
    }
    vec![0, 1, 2, 3, 4]
}

const DISCARDERS: [&str; 8] = [
    "Ramen",          // card.lua:2757  discard
    "Yorick",         // card.lua:2788  discard
    "Castle",         // card.lua:2814  discard
    "Mail-In Rebate", // card.lua:2825  discard
    "Hit the Road",   // card.lua:2835  discard
    "Faceless Joker", // card.lua:2858  discard
    "Burnt Joker",    // card.lua:2749  pre_discard
    "Trading Card",   // card.lua:2802  discard
];

fn discarder_pick(game: &mut GameState, name: &str) -> Vec<usize> {
    if name == "Trading Card" {
        vec![0]
    } else {
        five_jacks(game, name)
    }
}

type RoundState = (
    Vec<(String, f64, f64, f64, Option<HandType>)>,
    Option<f64>,
    Option<f64>,
);

fn round_state(game: &GameState) -> RoundState {
    let rows = game
        .jokers
        .iter()
        .map(|joker| {
            let j = joker.borrow();
            (
                j.name().to_string(),
                j.counter,
                j.secondary,
                j.extra_sell_value,
                j.named_hand,
            )
        })
        .collect();
    let state = game.rng.state();
    (
        rows,
        state.get("gros_michel").copied(),
        state.get("to_do").copied(),
    )
}

/// name, row, blind kind, a counter to stamp first.
const ROUND_ENDERS: [(&str, &[&str], BlindKind, Option<f64>); 8] = [
    ("Popcorn", &["Popcorn"], BlindKind::Small, None), // 2945
    ("Gros Michel", &["Gros Michel"], BlindKind::Small, None), // 3019
    (
        "Invisible Joker",
        &["Invisible Joker"],
        BlindKind::Small,
        None,
    ), // 2934
    ("Egg", &["Egg"], BlindKind::Small, None),         // 2985
    ("Gift Card", &["Gift Card", "Joker"], BlindKind::Small, None), // 2993
    ("To Do List", &["To Do List"], BlindKind::Small, None), // 2975
    ("Rocket", &["Rocket"], BlindKind::Boss, None),    // 2896
    ("Campfire", &["Campfire"], BlindKind::Boss, Some(2.0)), // 2889
];

fn beat_round(
    name: &str,
    row: &[&str],
    kind: BlindKind,
    setup: Option<f64>,
    debuffed: bool,
) -> (RoundState, RoundState) {
    let mut game = run(row);
    let joker = find_joker(&game, name);
    if let Some(value) = setup {
        joker.borrow_mut().counter = value;
    }
    if debuffed {
        game.set_joker_debuff(&joker, true);
    }
    game.blind = Some(make_blind(kind, game.ante, None, 1.0, 1, false));
    let before = round_state(&game);
    game._beat_blind(true);
    (before, round_state(&game))
}
fn shop_opened(mut game: GameState) -> GameState {
    game.money = 100;
    game._open_shop();
    game
}

fn holding_the_fool(mut game: GameState) -> GameState {
    let fool = game.hold_consumable(consumables::spec_or_panic("The Fool"), Edition::None);
    game.consumables.push(fool);
    game
}

fn hallucination() -> GameState {
    let mut game = GameState::new("LC4JWH61", "Nebula Deck", 1);
    game.ante = 3;
    game._open_shop();
    game.shop.as_mut().unwrap().slots = Vec::new();
    game.gain_joker(&joker_ref("Hallucination"));
    game
}

/// Every context outside the discard and the round's end, each named after the
/// `card.lua` branch it tests.
const OTHER_CONTEXTS: [&str; 12] = [
    "Hologram",      // playing_card_added 2457
    "Constellation", // using_consumeable 2727
    "Campfire",      // selling_card 2396
    "Canio",         // remove_playing_cards 2623
    "Glass Joker",   // using_consumeable 2709
    "Flash Card",    // reroll_shop 2404
    "Red Card",      // skipping_booster 2442
    "Perkeo",        // ending_shop 2413
    "Diet Cola",     // selling_self 2361
    "DNA",           // before 3501
    "Vagabond",      // joker_main 3743
    "Hallucination", // open_booster 2336
];

fn context_build(name: &str) -> GameState {
    match name {
        "Hologram" => run(&["Hologram"]),
        "Constellation" => run(&["Constellation"]),
        "Campfire" => run(&["Campfire"]),
        "Canio" => run(&["Canio"]),
        "Glass Joker" => run(&["Glass Joker"]),
        "Flash Card" => shop_opened(run(&["Flash Card"])),
        "Red Card" => shop_opened(run(&["Red Card"])),
        "Perkeo" => holding_the_fool(run(&["Perkeo"])),
        "Diet Cola" => run(&["Diet Cola"]),
        "DNA" => run(&["DNA"]),
        "Vagabond" => run(&["Vagabond"]),
        "Hallucination" => hallucination(),
        other => panic!("unknown context {other}"),
    }
}

fn context_read(game: &GameState, name: &str) -> f64 {
    match name {
        "Perkeo" | "Vagabond" | "Hallucination" => game.consumables.len() as f64,
        "Diet Cola" => game.tags.iter().filter(|tag| **tag == Tag::Double).count() as f64,
        "DNA" => game.full_deck.len() as f64,
        _ => counter_of(game, name),
    }
}

fn context_act(game: &mut GameState, name: &str) {
    match name {
        "Hologram" => {
            let copy = jimbot_sim::cards::copy_card(&game.hand[0]);
            game.add_card_to_hand(&copy);
        }
        "Constellation" => game.use_consumable(consumables::spec_or_panic("Mercury"), &[], false),
        "Campfire" => game.note_card_sold(),
        "Canio" => {
            let card = game.hand[0].clone();
            card.borrow_mut().rank = Rank::King;
            game.remove_card(&card, false);
        }
        "Glass Joker" => {
            let card = game.hand[0].clone();
            card.borrow_mut().enhancement = Enhancement::Glass;
            game.use_consumable(consumables::spec_or_panic("The Hanged Man"), &[card], false);
        }
        "Flash Card" => game.step(&Action::new(ActionType::Reroll)),
        "Red Card" => {
            let spec = jimbot_sim::shop::pack_from_key("p_buffoon_normal_1");
            game._open_pack(spec, false);
            game.step(&Action::new(ActionType::SkipPack));
        }
        "Perkeo" => {
            game.shop = None;
            game._leave_shop();
        }
        "Diet Cola" => game.step(&Action::at(ActionType::SellJoker, 0)),
        "DNA" => game.step(&Action::with_cards(ActionType::Play, vec![0])),
        "Vagabond" => {
            game.money = 0;
            game.step(&Action::with_cards(ActionType::Play, vec![0]));
        }
        "Hallucination" => {
            let spec = jimbot_sim::shop::pack_from_key("p_arcana_normal_1");
            game._open_pack(spec, false);
        }
        other => panic!("unknown context {other}"),
    }
}

fn context_run(name: &str, debuffed: bool) -> (f64, f64) {
    let mut game = context_build(name);
    if debuffed {
        let joker = find_joker(&game, name);
        game.set_joker_debuff(&joker, true);
    }
    let before = context_read(&game, name);
    context_act(&mut game, name);
    (before, context_read(&game, name))
}

fn score_row(row: &[&str], debuff_at: Option<usize>) -> i64 {
    let mut game = run(row);
    if let Some(index) = debuff_at {
        let joker = game.jokers[index].clone();
        game.set_joker_debuff(&joker, true);
    }
    game.preview_score(&[0, 1, 2, 3, 4], "roll")
}
#[test]
fn test_a_debuffed_ramen_keeps_its_x_mult_through_a_discard() {
    // card.lua:2292 before card.lua:2757 (`context.discard`, Ramen).
    let mut game = run(&["Ramen"]);
    let ramen = debuff(&mut game, "Ramen");
    ramen.borrow_mut().counter = 1.85;
    game._discard(&[0, 1, 2, 3, 4]);
    assert_eq!(ramen.borrow().counter, 1.85);
}

#[test]
fn test_a_debuffed_joker_takes_nothing_from_a_discard() {
    // card.lua:2292: the discard and pre_discard contexts reach a debuffed joker
    // and get nil back.
    for name in DISCARDERS {
        let mut game = run(&[name]);
        debuff(&mut game, name);
        let indices = discarder_pick(&mut game, name);
        let before = discard_state(&game, name);
        game._discard(&indices);
        assert_eq!(discard_state(&game, name), before, "{name}");
    }
}

#[test]
fn test_the_same_discard_moves_a_live_one() {
    // The control: without the debuff each of these does move.
    for name in DISCARDERS {
        let mut game = run(&[name]);
        let indices = discarder_pick(&mut game, name);
        let before = discard_state(&game, name);
        game._discard(&indices);
        assert_ne!(discard_state(&game, name), before, "{name}");
    }
}

#[test]
fn test_a_debuffed_joker_does_nothing_when_the_round_ends() {
    // card.lua:2292 before the end_of_round branches (state_events.lua:101). No
    // decay, no growth, no roll -- Gros Michel's and To Do List's streams are
    // left where they were.
    for (name, row, kind, setup) in ROUND_ENDERS {
        let (before, after) = beat_round(name, row, kind, setup, true);
        assert_eq!(after, before, "{name}");
    }
}

#[test]
fn test_a_live_one_does() {
    for (name, row, kind, setup) in ROUND_ENDERS {
        let (before, after) = beat_round(name, row, kind, setup, false);
        assert_ne!(after, before, "{name}");
    }
}

#[test]
fn test_a_crimson_heart_joker_pays_no_row_on_the_cash_out_that_frees_it() {
    // calculate_dollar_bonus returns early for a debuffed joker (card.lua:1656)
    // and the rows are built at state_events.lua:1176, while the blind's release
    // of its joker is still a queued event (1150-1153, blind.lua:333-337).
    let mut game = run(&["Golden Joker"]);
    let joker = debuff(&mut game, "Golden Joker");
    game.blind = Some(make_blind(BlindKind::Small, game.ante, None, 1.0, 1, false));
    game._beat_blind(true);
    assert!(!joker.borrow().debuffed); // freed on the cash-out screen ...
    game._cash_out();
    assert!(
        !game
            .logs
            .iter()
            .any(|line| line.starts_with("Golden Joker: +$")),
        "... and unpaid"
    );
}
#[test]
fn test_a_debuffed_joker_answers_no_other_context() {
    // card.lua:2292, for every context outside the discard and the round's end.
    for name in OTHER_CONTEXTS {
        let (before, after) = context_run(name, true);
        assert_eq!(after, before, "{name}");
    }
}

#[test]
fn test_a_live_joker_answers_it() {
    for name in OTHER_CONTEXTS {
        let (before, after) = context_run(name, false);
        assert_ne!(after, before, "{name}");
    }
}

#[test]
fn test_a_perished_certificate_makes_no_card() {
    // first_hand_drawn (game.lua:3229) reaches Certificate (card.lua:2463)
    // through the guard at card.lua:2292.
    let mut game = GameState::new("TESTSEED", "Red Deck", 1);
    let mut instance = JokerInstance::new(jokers::spec_or_panic("Certificate"));
    instance.perishable = true;
    instance.perish_tally = 0;
    instance.debuffed = true;
    game.gain_joker(&jokers::make_ref(instance));
    game._start_round();
    assert_eq!(game.hand.len() as i32, game.hand_size());
}

#[test]
fn test_blueprint_beside_a_debuffed_joker_copies_nothing() {
    // Blueprint calls G.jokers.cards[i+1]:calculate_joker (card.lua:2305-2314),
    // which is nil for a debuffed one -- it does not reach past it to the next.
    assert_eq!(
        score_row(&["Blueprint", "Joker", "Joker"], Some(1)),
        score_row(&["Joker"], None)
    );
}

#[test]
fn test_brainstorm_on_a_debuffed_first_joker_copies_nothing() {
    // Brainstorm calls G.jokers.cards[1]:calculate_joker (card.lua:2318-2327).
    assert_eq!(
        score_row(&["Joker", "Joker", "Brainstorm"], Some(0)),
        score_row(&["Joker"], None)
    );
}

#[test]
fn test_a_debuffed_blueprint_copies_nothing() {
    assert_eq!(
        score_row(&["Blueprint", "Joker"], Some(0)),
        score_row(&["Joker"], None)
    );
}

#[test]
fn test_a_debuffed_rule_joker_is_not_found() {
    // find_joker(name) skips `v.debuff` (misc_functions.lua:907); Oops! All 6s
    // halves the probabilities back in remove_from_deck (card.lua:665-669).
    let probes: [(&str, fn(&GameState) -> bool); 6] = [
        ("Four Fingers", |g| g.four_fingers()), // misc_functions.lua:524
        ("Shortcut", |g| g.shortcut_joker()),   // misc_functions.lua:567
        ("Splash", |g| g.splash()),             // state_events.lua:583
        ("Pareidolia", |g| g.has_pareidolia()), // card.lua:967
        ("Smeared Joker", |g| g.has_smeared()), // card.lua:4072
        ("Oops! All 6s", |g| g.probability_scale() > 1.0), // card.lua:665
    ];
    for (name, probe) in probes {
        let mut game = run(&[name]);
        assert!(probe(&game), "{name}");
        debuff(&mut game, name);
        assert!(!probe(&game), "{name}");
    }
}

#[test]
fn test_a_debuffed_chicot_leaves_the_boss_alone() {
    // Chicot disables the boss from setting_blind (card.lua:2492) and
    // add_to_deck (card.lua:596); both are shut to a debuffed one.
    let mut game = run(&["Chicot"]);
    game.blind = Some(make_blind(
        BlindKind::Boss,
        game.ante,
        boss_by_name("Crimson Heart"),
        1.0,
        1,
        false,
    ));
    assert!(game.boss().is_none());
    debuff(&mut game, "Chicot");
    assert!(game.boss().is_some());
}

#[test]
fn test_a_debuffed_rental_still_pays_its_rent() {
    // calculate_rental has no debuff check (card.lua:2271-2276) and end_round
    // calls it for every joker (state_events.lua:108).
    let mut game = run(&["Joker"]);
    let joker = find_joker(&game, "Joker");
    joker.borrow_mut().rental = true;
    game.set_joker_debuff(&joker, true);
    game.blind = Some(make_blind(BlindKind::Small, game.ante, None, 1.0, 1, false));
    let money = game.money;
    game._beat_blind(true);
    assert_eq!(game.money, money - 3);
}

#[test]
fn test_a_debuffed_perishable_still_counts_down() {
    // calculate_perishable has no debuff check (card.lua:2278-2289).
    let mut game = run(&["Joker"]);
    let joker = find_joker(&game, "Joker");
    {
        let mut j = joker.borrow_mut();
        j.perishable = true;
        j.perish_tally = 3;
    }
    game.set_joker_debuff(&joker, true);
    game.blind = Some(make_blind(BlindKind::Small, game.ante, None, 1.0, 1, false));
    game._beat_blind(true);
    assert_eq!(joker.borrow().perish_tally, 2);
}

#[test]
fn test_gift_card_still_raises_a_debuffed_jokers_value() {
    // Gift Card walks every G.jokers.cards (card.lua:2993-2994) and sets
    // extra_value whatever its debuff.
    let mut game = run(&["Gift Card", "Joker"]);
    let joker = debuff(&mut game, "Joker");
    game.blind = Some(make_blind(BlindKind::Small, game.ante, None, 1.0, 1, false));
    game._beat_blind(true);
    assert_eq!(joker.borrow().extra_sell_value, 1.0);
}

#[test]
fn test_a_debuffed_negative_keeps_its_slot() {
    // remove_from_deck(true) only queues the Negative's removal (card.lua:687-689).
    let mut game = run(&["Joker"]);
    let slots = game.joker_slots();
    let joker = find_joker(&game, "Joker");
    joker.borrow_mut().edition = Edition::Negative;
    assert_eq!(game.joker_slots(), slots + 1);
    game.set_joker_debuff(&joker, true);
    assert_eq!(game.joker_slots(), slots + 1);
}
