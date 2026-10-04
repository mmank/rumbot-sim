//! Flow, blind, boss, round and preview tests, ported from the Python suite.
//!
//! Each `#[test]` mirrors one `def test_x()` under
//! `external/jimbot-sim/tests/`, grouped here by the file it came from. The doc
//! comments carry the reasoning the Python test recorded -- these were written
//! over months against real divergences, and the comment is often the only
//! statement of *why* the assertion has the shape it does.
//!
//! The port is "direct": the same position is rebuilt with `GameState` and the
//! same value asserted. Where Python `monkeypatch`es an engine draw, the Rust
//! test either drives the mechanism the patch stood in for directly (and says
//! so) or uses a seed on which the engine makes the same draw, found with the
//! Python reference and named in the test.

use std::collections::HashSet;
use std::rc::Rc;

use rumbot_sim::blinds::{
    boss_by_name, make_blind, BlindKind, BossEffect, BOSSES, FINISHER_BOSSES,
};
use rumbot_sim::cards::{make_card, uid_of, CardRef, Edition, Enhancement, Rank, Seal, Suit};
use rumbot_sim::consumables::spec_or_panic;
use rumbot_sim::game::{Action, ActionType, GameState, PackChoice, Phase, BOSS_REROLL_COST};
use rumbot_sim::hands::HandType;
use rumbot_sim::jokers::{self, JokerRef};
use rumbot_sim::rng::RunRng;
use rumbot_sim::scoring::held_triggers;
use rumbot_sim::shop::{voucher_by_key, PackKind, PackSpec, ShopSlot};
use rumbot_sim::shop_pool::{draw_joker, PackCard};

// ==========================================================================
// shared builders
// ==========================================================================

fn joker(name: &str) -> JokerRef {
    jokers::make(name)
}

/// An instance with a given edition, for the creators that read it.
fn editioned(name: &str, edition: Edition) -> JokerRef {
    let j = jokers::make(name);
    j.borrow_mut().edition = edition;
    j
}

fn joker_names(game: &GameState) -> Vec<String> {
    game.jokers
        .iter()
        .map(|j| j.borrow().name().to_string())
        .collect()
}

fn hand_uids(game: &GameState) -> Vec<u64> {
    game.hand.iter().map(uid_of).collect()
}

// ==========================================================================
// tests/test_running_out.py -- "The two ways a run ends without the blind ever
// being scored."
//
//   no hand size    GAME_OVER set directly, in draw_from_deck_to_hand. No target
//                   check, no Mr. Bones, no cash-out. Suspended while an Arcana
//                   or Spectral pack is open, because that pack deals you a hand
//                   and may be how you fix it.
//
//   no cards        end_round(), called the moment hand, deck and play area are
//                   all empty. Not itself a loss -- it is the ordinary end of a
//                   round, and a run that had already met the target cashes out
//                   normally.
//
// The simulator could reach neither. hand_size floored at one rather than zero,
// so the first was unreachable by construction, and the second was a hard
// GAME_OVER that ignored the score. Mr. Bones was worse than either: the spec
// carried a prevents_death flag that nothing ever read.
// ==========================================================================

/// `_run(*names)`: a run standing in a round, holding the named jokers.
///
/// They are given before the round starts, which is how a run that already
/// holds four Stuntmen meets its next blind: the hand is never dealt at all.
fn running_out_run(names: &[&str]) -> GameState {
    let mut game = GameState::new("TESTSEED", "Red Deck", 1);
    for name in names {
        game.gain_joker(&joker(name));
    }
    game._start_round();
    game
}

/// `_pack(kind)` from test_running_out.py: a normal, three-option booster.
fn pack_of(kind: PackKind) -> PackSpec {
    PackSpec {
        kind,
        size: "normal",
        options: 3,
        picks: 1,
        cost: 4,
        key: "",
    }
}

/// `_out_of_hands`: spend the last hand on a single card with the score at
/// `fraction` of the target.
fn out_of_hands(game: &mut GameState, fraction: f64) {
    let target = game.blind.as_ref().unwrap().target;
    game.chips_scored = (target as f64 * fraction) as i64;
    game.hands_left = 1;
    game.hand = vec![make_card(Rank::Two, Suit::Clubs)];
    game.step(&Action::with_cards(ActionType::Play, vec![0]));
}

#[test]
fn test_hand_size_can_reach_zero() {
    // CardArea:update floors it at zero. Flooring at one hid the whole rule.
    let game = running_out_run(&["Stuntman", "Stuntman", "Stuntman", "Stuntman"]);
    assert_eq!(game.hand_size(), 0);
}

#[test]
fn test_no_hand_size_and_no_cards_ends_the_run() {
    let mut game = running_out_run(&["Stuntman", "Stuntman", "Stuntman", "Stuntman"]);
    game.hand.clear();
    game._draw_to_hand_size();
    assert_eq!(game.phase, Phase::GameOver);
}

#[test]
fn test_no_hand_size_but_cards_still_held_is_survivable() {
    // Engine: limit 0 with eight cards in hand stays in SELECTING_HAND.
    let mut game = running_out_run(&[]);
    for _ in 0..4 {
        game.gain_joker(&joker("Stuntman"));
    }
    assert_eq!(game.hand_size(), 0);
    assert!(!game.hand.is_empty());
    game._draw_to_hand_size();
    assert_eq!(game.phase, Phase::Playing);
}

#[test]
fn test_a_round_that_starts_with_no_hand_size_ends_at_once() {
    // There is no hand to hold on to: the deal itself is what kills it.
    let game = running_out_run(&["Stuntman", "Stuntman", "Stuntman", "Stuntman"]);
    assert_eq!(game.phase, Phase::GameOver);
}

#[test]
fn test_an_arcana_pack_suspends_it() {
    let mut game = running_out_run(&["Stuntman", "Stuntman", "Stuntman", "Stuntman"]);
    game.hand.clear();
    game.phase = Phase::Pack;
    game.pack = Some(pack_of(PackKind::Arcana));
    game._draw_to_hand_size();
    assert_eq!(game.phase, Phase::Pack);
}

#[test]
fn test_a_buffoon_pack_does_not_suspend_it() {
    // Only the two that deal a hand. A Buffoon pack deals nothing.
    let mut game = running_out_run(&["Stuntman", "Stuntman", "Stuntman", "Stuntman"]);
    game.hand.clear();
    game.phase = Phase::Pack;
    game.pack = Some(pack_of(PackKind::Buffoon));
    game._draw_to_hand_size();
    assert_eq!(game.phase, Phase::GameOver);
}

#[test]
fn test_beating_the_target_does_not_save_you_from_it() {
    // Engine: target met, still GAME_OVER. It is not a round-end check.
    let mut game = running_out_run(&["Stuntman", "Stuntman", "Stuntman", "Stuntman"]);
    game.hand.clear();
    game.chips_scored = game.blind.as_ref().unwrap().target + 1;
    game._draw_to_hand_size();
    assert_eq!(game.phase, Phase::GameOver);
}

#[test]
fn test_mr_bones_does_not_save_you_from_it_either() {
    // Engine: Mr. Bones held, still GAME_OVER, and he is not consumed.
    let mut game = running_out_run(&["Stuntman", "Stuntman", "Stuntman", "Stuntman", "Mr. Bones"]);
    game.hand.clear();
    game.chips_scored = game.blind.as_ref().unwrap().target / 2;
    game._draw_to_hand_size();
    assert_eq!(game.phase, Phase::GameOver);
    assert!(joker_names(&game).iter().any(|n| n == "Mr. Bones"));
}

#[test]
fn test_an_empty_deck_and_an_empty_hand_ends_the_run() {
    let mut game = running_out_run(&[]);
    game.hand.clear();
    game.draw_pile.clear();
    game._draw_to_hand_size();
    assert_eq!(game.phase, Phase::GameOver);
}

#[test]
fn test_an_empty_deck_with_cards_still_in_hand_carries_on() {
    let mut game = running_out_run(&[]);
    game.draw_pile.clear();
    game._draw_to_hand_size();
    assert_eq!(game.phase, Phase::Playing);
}

#[test]
fn test_mr_bones_cannot_save_a_run_that_is_out_of_cards() {
    // He fires, and it buys nothing.
    //
    // The engine dissolves him, then drops the run back into SELECTING_HAND
    // with the hand and deck still empty, so end_round runs again immediately
    // and there is nothing left to spend the second time.
    let mut game = running_out_run(&["Mr. Bones"]);
    game.hand.clear();
    game.draw_pile.clear();
    game.chips_scored = game.blind.as_ref().unwrap().target; // as generous as it gets
    game._draw_to_hand_size();
    assert_eq!(game.phase, Phase::GameOver);
}

#[test]
fn test_mr_bones_saves_a_run_at_a_quarter_of_the_target() {
    let mut game = running_out_run(&["Mr. Bones"]);
    out_of_hands(&mut game, 0.30);
    assert_eq!(game.phase, Phase::RoundEval);
    assert!(!joker_names(&game).iter().any(|n| n == "Mr. Bones"));
}

#[test]
fn test_mr_bones_does_nothing_below_a_quarter() {
    let mut game = running_out_run(&["Mr. Bones"]);
    out_of_hands(&mut game, 0.10);
    assert_eq!(game.phase, Phase::GameOver);
    assert!(joker_names(&game).iter().any(|n| n == "Mr. Bones"));
}

#[test]
fn test_a_run_without_mr_bones_simply_ends() {
    let mut game = running_out_run(&[]);
    out_of_hands(&mut game, 0.30);
    assert_eq!(game.phase, Phase::GameOver);
}

#[test]
fn test_a_saved_round_pays_everything_except_the_blind_reward() {
    // Engine, from $10: saved cashes out at $12, beaten at $15.
    let mut saved = running_out_run(&["Mr. Bones"]);
    let reward = saved.blind.as_ref().unwrap().reward;
    out_of_hands(&mut saved, 0.30);

    let mut beaten = running_out_run(&["Mr. Bones"]);
    beaten.chips_scored = beaten.blind.as_ref().unwrap().target;
    beaten.hands_left = 1;
    beaten.hand = vec![make_card(Rank::Two, Suit::Clubs)];
    beaten.step(&Action::with_cards(ActionType::Play, vec![0]));

    assert!(reward > 0);
    assert_eq!(saved.pending_payout, beaten.pending_payout - reward as i64);
}

#[test]
fn test_a_saved_round_moves_on_to_the_next_blind() {
    // The engine marks the failed blind Defeated -- you do not replay it.
    let mut game = running_out_run(&["Mr. Bones"]);
    let index = game.blind_index;
    out_of_hands(&mut game, 0.30);
    assert_eq!(game.blind_index, index + 1);
}

#[test]
fn test_to_the_moon_doubles_interest_past_the_cap() {
    // Engine at $100 against the $25 cap: base $5, one moon $10, two $15.
    let paid = |moons: usize| -> i64 {
        let names: Vec<&str> = std::iter::repeat("To the Moon").take(moons).collect();
        let mut game = running_out_run(&names);
        game.money = 100;
        game.chips_scored = game.blind.as_ref().unwrap().target;
        game.hands_left = 1;
        game.hand = vec![make_card(Rank::Two, Suit::Clubs)];
        let reward = game.blind.as_ref().unwrap().reward;
        game.step(&Action::with_cards(ActionType::Play, vec![0]));
        game.pending_payout - reward as i64 // the hand was spent
    };
    assert_eq!((paid(0), paid(1), paid(2)), (5, 10, 15));
}

#[test]
fn test_a_debuffed_to_the_moon_pays_nothing_extra() {
    // Debuffing a joker runs remove_from_deck, which takes the counter with it.
    //
    // Measured: interest_amount reads 1 with no To the Moon, 2 while one is
    // held, and 1 again the moment it is debuffed.
    let mut game = running_out_run(&["To the Moon"]);
    game.jokers[0].borrow_mut().debuffed = true;
    game.money = 100;
    game.chips_scored = game.blind.as_ref().unwrap().target;
    game.hands_left = 1;
    game.hand = vec![make_card(Rank::Two, Suit::Clubs)];
    let reward = game.blind.as_ref().unwrap().reward;
    game.step(&Action::with_cards(ActionType::Play, vec![0]));
    assert_eq!(game.pending_payout - reward as i64, 5);
}

// ==========================================================================
// tests/test_hand_size_growth_deals.py -- "A hand size that grows during a
// round is dealt into at once."
//
// CardArea:change_size does not just move the limit: it raises card_limit and
// then, if delta > 0 and the limit is above one and this is G.hand and there is
// a card in it and the state is DRAW_TO_HAND or SELECTING_HAND, draws |delta|
// cards off the top of the deck and re-sorts. It is |delta|, not "up to the
// limit": a hand already over its limit still gets them. A decrease only lowers
// the limit; nothing is discarded.
//
// Seen on U2EBFAQ2, Painted Deck, stake 5, decision 186: the policy sold
// Stuntman while selecting a hand, and the game's hand went from 8 cards to 10
// while the simulator's stayed at 8.
//
// A consumable is different: G.STATES.PLAY_TAROT is set for the whole effect,
// so the change_size event a Judgement or Hex causes deals nothing.
// ==========================================================================

/// `_round(*names, deck="Red Deck")`: a run standing in a freshly dealt round.
fn dealt_round(names: &[&str], deck: &str) -> GameState {
    let mut game = GameState::new("TESTSEED", deck, 1);
    for name in names {
        game.gain_joker(&joker(name));
    }
    game._start_round();
    assert_eq!(game.phase, Phase::Playing);
    game
}

/// `_assert_sorted`: the hand does not move when it is asked to sort again.
fn assert_sorted(game: &mut GameState) {
    let before = hand_uids(game);
    game._sort_hand();
    assert_eq!(
        hand_uids(game),
        before,
        "the hand is re-sorted after the deal"
    );
}

fn uid_set(cards: &[CardRef]) -> HashSet<u64> {
    cards.iter().map(uid_of).collect()
}

#[test]
fn test_selling_stuntman_mid_round_deals_two_from_the_top() {
    // U2EBFAQ2 decision 186: Painted Deck, 8 cards held, Stuntman sold.
    let mut game = dealt_round(&["Stuntman"], "Painted Deck");
    assert_eq!(game.hand_size(), 8);
    assert_eq!(game.hand.len(), 8);
    let top: Vec<CardRef> = game.draw_pile[game.draw_pile.len() - 2..].to_vec();
    let pile = game.draw_pile.len();

    game.step(&Action::at(ActionType::SellJoker, 0));

    assert_eq!(game.hand_size(), 10);
    assert_eq!(game.hand.len(), 10);
    assert_eq!(game.draw_pile.len(), pile - 2);
    let held = uid_set(&game.hand);
    for uid in uid_set(&top) {
        assert!(held.contains(&uid));
    }
    assert_sorted(&mut game);
}

#[test]
fn test_selling_merry_andy_mid_round_deals_one() {
    let mut game = dealt_round(&["Merry Andy"], "Red Deck");
    assert_eq!(game.hand.len(), 7);
    let top = game.draw_pile[game.draw_pile.len() - 1].clone();

    game.step(&Action::at(ActionType::SellJoker, 0));

    assert_eq!(game.hand.len(), 8);
    assert!(game.hand.iter().any(|c| Rc::ptr_eq(c, &top)));
    assert_sorted(&mut game);
}

#[test]
fn test_a_juggler_joining_mid_round_deals_one() {
    let mut game = dealt_round(&[], "Red Deck");
    let top = game.draw_pile[game.draw_pile.len() - 1].clone();

    game.gain_joker(&joker("Juggler"));

    assert_eq!(game.hand_size(), 9);
    assert_eq!(game.hand.len(), 9);
    assert!(game.hand.iter().any(|c| Rc::ptr_eq(c, &top)));
}

#[test]
fn test_a_decrease_discards_nothing() {
    // cardarea.lua: only lowers the limit.
    let mut game = dealt_round(&[], "Red Deck");
    game.gain_joker(&joker("Stuntman"));
    assert_eq!(game.hand_size(), 6);
    assert_eq!(game.hand.len(), 8);
}

#[test]
fn test_the_deal_is_the_delta_not_a_top_up() {
    // Over the limit already, the sale still deals both cards.
    let mut game = dealt_round(&[], "Red Deck");
    game.gain_joker(&joker("Stuntman")); // limit 6, 8 held
    game.step(&Action::at(ActionType::SellJoker, 0)); // limit 8
    assert_eq!(game.hand.len(), 10);
}

#[test]
fn test_a_limit_still_at_one_or_less_deals_nothing() {
    // `real_card_limit > 1` is checked after the change.
    let mut game = dealt_round(&[], "Red Deck");
    for _ in 0..4 {
        game.gain_joker(&joker("Stuntman"));
    }
    assert_eq!(game.hand_size(), 0);
    assert_eq!(game.hand.len(), 8);
    game.gain_joker(&joker("Juggler"));
    assert_eq!(game.hand_size(), 1);
    assert_eq!(game.hand.len(), 8);
}

#[test]
fn test_a_debuffed_joker_leaves_without_dealing() {
    // remove_from_deck is guarded by added_to_deck, which a debuff cleared.
    let mut game = GameState::new("TESTSEED", "Red Deck", 1);
    let stuntman = joker("Stuntman");
    game.gain_joker(&stuntman);
    stuntman.borrow_mut().debuffed = true;
    game._start_round();
    assert_eq!(game.hand.len(), 8);

    game.step(&Action::at(ActionType::SellJoker, 0));

    assert_eq!(game.hand.len(), 8);
}

#[test]
fn test_nothing_is_dealt_outside_a_round() {
    let mut game = GameState::new("TESTSEED", "Red Deck", 1);
    game.gain_joker(&joker("Stuntman"));
    assert_eq!(game.phase, Phase::BlindSelect);
    assert!(game.hand.is_empty());
    game.step(&Action::at(ActionType::SellJoker, 0));
    assert!(game.hand.is_empty());
}

#[test]
fn test_hex_destroying_stuntman_deals_nothing() {
    // Engine, same position: limit 6 -> 8, the hand stays at 8 cards.
    let mut game = dealt_round(&[], "Red Deck");
    game.gain_joker(&joker("Joker"));
    game.gain_joker(&editioned("Stuntman", Edition::Foil));
    assert_eq!(game.hand_size(), 6);
    assert_eq!(game.hand.len(), 8);

    game.use_consumable(spec_or_panic("Hex"), &[], false);

    assert_eq!(joker_names(&game), vec!["Joker"]);
    assert_eq!(game.hand_size(), 8);
    assert_eq!(game.hand.len(), 8);
}

#[test]
fn test_judgement_making_a_juggler_deals_nothing() {
    // Engine, same position: limit 8 -> 9, the hand stays at 8 cards.
    //
    // Python monkeypatches shop_pool.draw_joker to make the Judgement draw
    // j_juggler. Rust cannot patch the draw, so the test uses seed JUD00040 --
    // found with the Python reference -- on which Judgement makes a Juggler
    // naturally.
    let mut game = GameState::new("JUD00040", "Red Deck", 1);
    game._start_round();

    game.use_consumable(spec_or_panic("Judgement"), &[], false);

    assert_eq!(joker_names(&game), vec!["Juggler"]);
    assert_eq!(game.hand_size(), 9);
    assert_eq!(game.hand.len(), 8);
}

#[test]
fn test_luchador_against_the_manacle_deals_two_even_over_the_limit() {
    // Engine: 8 held under a limit of 7, Luchador sold, 10 held.
    //
    // The top-up this used to do for change_size's card gave 9.
    let manacle = boss_by_name("The Manacle").unwrap();
    let mut game = dealt_round(&[], "Red Deck");
    game.gain_joker(&joker("Luchador"));
    let ante = game.ante;
    game.blind = Some(make_blind(
        BlindKind::Boss,
        ante,
        Some(manacle),
        1.0,
        1,
        false,
    ));
    assert_eq!(game.hand_size(), 7);
    assert_eq!(game.hand.len(), 8);

    game.step(&Action::at(ActionType::SellJoker, 0));

    assert_eq!(game.hand_size(), 8);
    assert_eq!(game.hand.len(), 10);
}

#[test]
fn test_luchador_against_the_manacle_still_deals_two() {
    // Blind:disable: change_size(1) deals one, draw_from_deck_to_hand(1)
    // another, so the hand ends one over its limit.
    let manacle = boss_by_name("The Manacle").unwrap();
    let mut game = GameState::new("TESTSEED", "Red Deck", 1);
    game.gain_joker(&joker("Luchador"));
    game.ante_boss = String::new();
    let ante = game.ante;
    game.blind = Some(make_blind(
        BlindKind::Boss,
        ante,
        Some(manacle),
        1.0,
        1,
        false,
    ));
    game._start_round();
    assert_eq!(game.hand_size(), 7);
    assert_eq!(game.hand.len(), 7);

    game.step(&Action::at(ActionType::SellJoker, 0));

    assert_eq!(game.hand_size(), 8);
    assert_eq!(game.hand.len(), 9);
}

// ==========================================================================
// tests/test_recording_eight_fixes.py -- "Two bugs recording 8 found, pinned."
//
// They were invisible to every other test: they need a legendary joker and a
// mid-shop Merry Andy, and nothing else in the suite arranges either. Recording
// 8 is a real run that does both, which is the argument for replaying real
// games rather than only generated ones. Several more divergences were pinned
// here as the recording was worked through.
// ==========================================================================

/// `_in_a_round(deck="Red Deck")`: a run that has offered, then taken, its blind.
fn in_a_round(deck: &str) -> GameState {
    let mut game = GameState::new("TESTSEED", deck, 1);
    game._next_blind();
    game._start_round();
    game
}

#[test]
fn test_a_legendary_draw_ignores_the_append_and_the_ante() {
    // get_current_pool drops both for a legendary:
    //
    //     _pool_key = 'Joker'..rarity..((not _legendary and _append) or '')
    //     return _pool, _pool_key..(not _legendary and ante or '')
    //
    // so The Soul draws from "Joker4" wherever and whenever it is opened. The
    // simulator was asking for "Joker4sou8", which is a perfectly good stream
    // with a perfectly plausible legendary in it -- just not the right one.
    // Recording 8 stopped on it at step 190 of 443, Chicot against Triboulet.
    let mut picks: HashSet<String> = HashSet::new();
    for (ante, append) in [(1, "sou"), (8, "sou"), (4, "jud"), (2, ""), (8, "")] {
        let mut rng = RunRng::new("ABCD1234");
        let key = draw_joker(
            &mut rng,
            ante,
            &[] as &[&str],
            &[] as &[&str],
            false,
            Some(4),
            &[] as &[&str],
            append,
        );
        picks.insert(key);
    }
    assert_eq!(
        picks.len(),
        1,
        "a legendary draw varied with ante or source: {picks:?}"
    );
}

#[test]
fn test_an_ordinary_draw_still_varies_with_the_ante_and_the_source() {
    // The opposite has to stay true, or the fix has broken every other pool.
    let mut picks: HashSet<String> = HashSet::new();
    for ante in 1..=6 {
        let mut rng = RunRng::new("ABCD1234");
        picks.insert(draw_joker(
            &mut rng,
            ante,
            &[] as &[&str],
            &[] as &[&str],
            false,
            Some(3),
            &[] as &[&str],
            "",
        ));
    }
    assert!(picks.len() > 1, "rare draws stopped varying with the ante");

    let mut by_source: HashSet<String> = HashSet::new();
    for append in ["", "sou", "jud", "wra"] {
        let mut rng = RunRng::new("ABCD1234");
        by_source.insert(draw_joker(
            &mut rng,
            3,
            &[] as &[&str],
            &[] as &[&str],
            false,
            Some(3),
            &[] as &[&str],
            append,
        ));
    }
    assert!(
        by_source.len() > 1,
        "rare draws stopped varying with the source"
    );
}

#[test]
fn test_merry_andy_hands_over_its_discards_on_arrival() {
    // Card:add_to_deck does it immediately:
    //
    //     if self.ability.d_size > 0 then
    //         G.GAME.round_resets.discards = ... + self.ability.d_size
    //         ease_discard(self.ability.d_size)
    //     end
    //
    // The round allowance already counted the joker, so the discards were not
    // lost -- they arrived a round late, which looks right everywhere except the
    // shop the joker was bought in. Recording 8 stopped on it at step 206: five
    // discards recorded against two simulated.
    let mut game = in_a_round("Red Deck");
    let before = game.discards_left;
    game.gain_joker(&joker("Merry Andy"));
    assert_eq!(
        game.discards_left,
        before + 3,
        "expected {} discards, got {}",
        before + 3,
        game.discards_left
    );
}

#[test]
fn test_selling_it_takes_them_back() {
    let mut game = in_a_round("Red Deck");
    let before = game.discards_left;
    game.gain_joker(&joker("Merry Andy"));
    let last = game.jokers.len() - 1;
    game.step(&Action::at(ActionType::SellJoker, last as i32));
    assert_eq!(game.discards_left, before);
}

#[test]
fn test_the_giving_back_is_clamped_at_zero() {
    // ease_discard is `mod = math.max(-discards_left, mod)`, so losing the
    // joker after the discards are spent cannot push the count negative.
    let mut game = in_a_round("Red Deck");
    game.gain_joker(&joker("Merry Andy"));
    game.discards_left = 1;
    let last = game.jokers.len() - 1;
    game.step(&Action::at(ActionType::SellJoker, last as i32));
    assert_eq!(game.discards_left, 0);
}

#[test]
fn test_a_joker_with_no_discards_moves_nothing() {
    let mut game = in_a_round("Red Deck");
    let before = game.discards_left;
    game.gain_joker(&joker("Joker"));
    assert_eq!(game.discards_left, before);
}

/// Python's `consumables._editionless`: the jokers with no edition, oldest
/// first. Order decides the answer -- the caller draws from this by index and
/// the game sorts by sort_id first.
fn editionless(game: &GameState) -> Vec<JokerRef> {
    let mut plain: Vec<JokerRef> = game
        .jokers
        .iter()
        .filter(|j| j.borrow().edition == Edition::None)
        .cloned()
        .collect();
    plain.sort_by_key(|j| j.borrow().uid);
    plain
}

#[test]
fn test_a_random_joker_draw_goes_by_age_not_by_row_position() {
    // The Wheel of Fortune, Ectoplasm and Hex all pick a joker out of an ordered
    // pool, and the game orders it with pseudorandom_element:
    //
    //     if keys[1].v.sort_id then
    //         table.sort(keys, function (a, b) return a.v.sort_id < b.v.sort_id end)
    //
    // sort_id is creation order, so the draw depends on how old a joker is and
    // never on where it sits. Drawing from row order returns the wrong joker the
    // moment anything has been dragged -- reproducibly, out of the right stream.
    let mut game = GameState::new("TESTSEED", "Red Deck", 1);
    let first = joker("Joker");
    let second = joker("Misprint");
    game.gain_joker(&first);
    game.gain_joker(&second);
    assert!(
        first.borrow().uid < second.borrow().uid,
        "the row did not stamp ages in order"
    );

    // Dragging the row must not change what a draw sees.
    game.jokers = vec![second, first];
    let names: Vec<String> = editionless(&game)
        .iter()
        .map(|j| j.borrow().name().to_string())
        .collect();
    assert_eq!(names, vec!["Joker", "Misprint"]);
}

#[test]
fn test_a_joker_is_aged_where_it_is_built_not_where_it_joins_the_row() {
    // Card:init stamps sort_id, and buying moves the shelf's own card into the
    // row, so the age is the shelf's. This test used to claim the opposite --
    // that the row stamps it -- and the recordings' joker_ids refute that.
    let mut game = GameState::new("TESTSEED", "Red Deck", 1);
    let older = joker("Joker");
    let younger = joker("Misprint");
    assert!(older.borrow().uid < younger.borrow().uid);
    game.gain_joker(&younger);
    game.gain_joker(&older);
    assert!(
        older.borrow().uid < younger.borrow().uid,
        "joining the row restamped the age"
    );
}

#[test]
fn test_a_copy_is_younger_than_its_original() {
    // Duplication in the game runs Card:init, so the copy takes the next sort_id
    // and sorts after the original. Two jokers at the same age would leave the
    // draw depending on how table.sort breaks ties.
    let mut game = GameState::new("TESTSEED", "Red Deck", 1);
    let original = joker("Joker");
    game.gain_joker(&original);
    game.add_joker_copy(&original, "");
    let copy = game.jokers.last().unwrap().clone();
    assert!(!Rc::ptr_eq(&copy, &original));
    assert!(copy.borrow().uid > original.borrow().uid);
}

#[test]
fn test_a_brainstorm_copying_a_mime_retriggers_held_cards() {
    // held_triggers read each joker's own spec, so a copier offered no
    // retrigger and the count came out one short. It failed quietly -- nothing
    // raised, the number was merely smaller -- and it is the same count the
    // end-of-round pass uses.
    //
    // Recording 8 stopped on it: two gold Kings with red seals, held under a
    // Mime with a Brainstorm copying it, paid $18 against the game's $24.
    let mut game = GameState::new("TESTSEED", "Red Deck", 1);
    let card = make_card(Rank::King, Suit::Diamonds);
    card.borrow_mut().enhancement = Enhancement::Gold;
    card.borrow_mut().seal = Seal::Red;

    game.gain_joker(&joker("Mime"));
    // base 1 + red seal 1 + Mime 1
    assert_eq!(held_triggers(&mut game, &card), 3);

    game.gain_joker(&joker("Brainstorm"));
    // ...and the Brainstorm copies the leftmost joker, which is the Mime
    assert_eq!(
        held_triggers(&mut game, &card),
        4,
        "the copier did not retrigger; a gold card here pays $12, not $9"
    );
}

#[test]
fn test_a_copier_with_nothing_to_copy_adds_no_retrigger() {
    let mut game = GameState::new("TESTSEED", "Red Deck", 1);
    game.gain_joker(&joker("Brainstorm"));
    let card = make_card(Rank::King, Suit::Diamonds);
    assert_eq!(held_triggers(&mut game, &card), 1);
}

#[test]
fn test_the_tooth_charges_before_the_hand_scores() {
    // Blind:press_play runs before evaluate_play, so a Tooth has already taken
    // its dollar a card by the time a joker reads the money -- and Bootstraps
    // reads it, at two mult for every five dollars held.
    //
    // Recording 8 stopped on it: three cards into a Tooth leaves $9002, so the
    // game scores 2*floor(9002/5) = 3600 mult where charging afterwards scores
    // 3602 off the $9005 it still thinks it has.
    let mut game = GameState::new("TESTSEED", "Red Deck", 1);
    game.money = 100;
    game.gain_joker(&joker("Bootstraps"));
    game.ante_boss = "bl_tooth".to_string();
    game.blind_index = 2;
    game._next_blind();
    game._start_round();
    assert!(game.boss().is_some());
    assert_eq!(game.boss().unwrap().money_per_card_played, -1);

    let before = game.money;
    let played = 3;
    game.step(&Action::with_cards(ActionType::Play, vec![0, 1, 2]));
    assert_eq!(
        game.money,
        before - played,
        "The Tooth charged {}, expected {}",
        before - game.money,
        played
    );
}

#[test]
fn test_bootstraps_reads_the_money_the_tooth_has_already_taken() {
    // The point of the ordering, stated as a number rather than an order.
    let mut game = GameState::new("TESTSEED", "Red Deck", 1);
    game.money = 100;
    game.gain_joker(&joker("Bootstraps"));
    game.ante_boss = "bl_tooth".to_string();
    game.blind_index = 2;
    game._next_blind();
    game._start_round();

    game.step(&Action::with_cards(ActionType::Play, vec![0, 1, 2]));
    // $100 becomes $97 before scoring: 2*floor(97/5) = 38, not 2*floor(100/5).
    assert_eq!(game.money, 97);
}

#[test]
fn test_dnas_copy_counts_as_a_held_card() {
    // DNA runs before the hand scores and puts its copy in hand, so the game
    // scores that copy as a held card like any other. Reading the hand before
    // the before-hand hooks left it out of the held pass entirely.
    let mut game = GameState::new("TESTSEED", "Red Deck", 1);
    game.gain_joker(&joker("DNA"));
    game._next_blind();
    game._start_round();

    let steel = make_card(Rank::King, Suit::Spades);
    steel.borrow_mut().enhancement = Enhancement::Steel;
    game.hand[0] = steel;

    game.step(&Action::with_cards(ActionType::Play, vec![0]));
    // The copy is in the deck and was drawn: the hand did not simply shrink
    // by the card that was played.
    assert_eq!(
        game.full_deck.len(),
        53,
        "DNA did not add a permanent copy: deck is {}",
        game.full_deck.len()
    );
}

#[test]
fn test_the_held_pass_sees_a_card_added_before_scoring() {
    // The narrow version: whatever before_hand puts in hand is held.
    let mut game = GameState::new("TESTSEED", "Red Deck", 1);
    game._next_blind();
    game._start_round();
    let played = game.hand[0].clone();
    let extra = make_card(Rank::Two, Suit::Hearts);
    extra.borrow_mut().enhancement = Enhancement::Steel;
    game.add_card_to_hand(&extra);
    let held: Vec<&CardRef> = game
        .hand
        .iter()
        .filter(|c| !Rc::ptr_eq(c, &played))
        .collect();
    assert!(
        held.iter().any(|c| Rc::ptr_eq(c, &extra)),
        "a card added to hand before scoring must be read as held"
    );
}

#[test]
fn test_a_brainstorm_copying_a_dna_makes_a_second_copy() {
    // The game guards its other DNA branch with `not context.blueprint` and
    // this one with nothing, so a copied DNA does fire.
    //
    // The copies are permanent, so a simulator making one where the game makes
    // two drifts further from it with every such hand.
    let mut game = GameState::new("TESTSEED", "Red Deck", 1);
    game.gain_joker(&joker("DNA"));
    game.gain_joker(&joker("Brainstorm"));
    game._next_blind();
    game._start_round();

    let before = game.full_deck.len();
    game.step(&Action::with_cards(ActionType::Play, vec![0]));
    assert_eq!(
        game.full_deck.len(),
        before + 2,
        "expected two copies -- the DNA and the Brainstorm copying it -- got {}",
        game.full_deck.len() - before
    );
}

#[test]
fn test_one_dna_alone_makes_one_copy() {
    let mut game = GameState::new("TESTSEED", "Red Deck", 1);
    game.gain_joker(&joker("DNA"));
    game._next_blind();
    game._start_round();
    let before = game.full_deck.len();
    game.step(&Action::with_cards(ActionType::Play, vec![0]));
    assert_eq!(game.full_deck.len(), before + 1);
}

// ==========================================================================
// tests/test_finisher_bosses.py -- "The bosses that do something to the run
// rather than to a card."
//
// Nine bosses carried no mechanical modifier at all. For four of them that is
// right: The House, The Wheel, The Fish and The Mark draw cards face down, and
// an engine with full information has nothing to hide. The other five were
// simply not built -- one switches a joker off every hand, one debuffs the whole
// deck, one shuffles the joker row, which decides the order effects resolve in.
// ==========================================================================

/// `_under(boss_name, jokers=())`: a run standing in a round against one boss.
fn under(boss: &str, jokers: &[&str]) -> GameState {
    let mut game = GameState::new("TESTSEED", "Red Deck", 1);
    for name in jokers {
        game.gain_joker(&joker(name));
    }
    game.ante_boss = String::new();
    let ante = game.ante;
    let effect = boss_by_name(boss).unwrap_or_else(|| panic!("no boss {boss}"));
    game.blind = Some(make_blind(
        BlindKind::Boss,
        ante,
        Some(effect),
        1.0,
        1,
        false,
    ));
    game._start_round();
    game
}

fn debuffed_names(game: &GameState) -> Vec<String> {
    game.jokers
        .iter()
        .filter(|j| j.borrow().debuffed)
        .map(|j| j.borrow().name().to_string())
        .collect()
}

fn index_of(cards: &[CardRef], target: &CardRef) -> usize {
    cards
        .iter()
        .position(|c| Rc::ptr_eq(c, target))
        .expect("the card is in the hand")
}

/// Every modifier field other than the name, the text, the chip multiplier and
/// the finisher flag is at its Python dataclass default.
fn is_blank(b: &BossEffect) -> bool {
    b.debuff_suit.is_none()
        && !b.debuff_face
        && b.hand_size_delta == 0
        && b.hands_delta == 0
        && b.discards_delta == 0
        && b.min_cards_played == 0
        && b.money_per_card_played == 0
        && !b.zero_money_on_most_played
        && b.discard_random_on_play == 0
        && !b.level_down_played_hand
        && !b.no_repeat_hand
        && !b.lock_first_hand_type
        && !b.debuff_previously_played
        && !b.halve_base
        && !b.always_draw_three
        && !b.shuffles_jokers
        && !b.debuff_until_sale
        && !b.debuff_a_joker
        && !b.forces_a_card
}

#[test]
fn test_the_face_down_bosses_are_the_only_blank_ones() {
    // A blank boss has to be blank for a reason, and the reason is stated.
    let cosmetic: HashSet<&str> = ["The House", "The Wheel", "The Fish", "The Mark"]
        .into_iter()
        .collect();
    let blank: HashSet<&str> = BOSSES
        .iter()
        .chain(FINISHER_BOSSES.iter())
        .filter(|b| b.chip_mult == 2.0 && is_blank(b))
        .map(|b| b.name)
        .collect();
    assert_eq!(blank, cosmetic);
}

#[test]
fn test_the_serpent_deals_three_however_much_room_there_is() {
    let mut game = under("The Serpent", &[]);
    let dealt = game.hand.len();
    game.step(&Action::with_cards(ActionType::Discard, vec![0, 1]));
    // Two gone and three back: the hand ends up larger than it started, which
    // is the whole shape of the blind.
    assert_eq!(game.hand.len(), dealt - 2 + 3);
}

#[test]
fn test_amber_acorn_shuffles_the_joker_row() {
    // Order decides the order effects resolve in, so this is not cosmetic.
    let names = [
        "Joker",
        "Greedy Joker",
        "Lusty Joker",
        "Wrathful Joker",
        "Gluttonous Joker",
    ];
    let game = under("Amber Acorn", &names);
    assert_ne!(joker_names(&game), names.to_vec());
}

#[test]
fn test_crimson_heart_switches_one_joker_off_each_hand() {
    // One from the moment the hand is dealt, and a different one after each hand
    // played. See test_crimson_heart_picks_on_the_draw.
    let mut game = under("Crimson Heart", &["Joker", "Greedy Joker"]);
    let first = debuffed_names(&game);
    assert_eq!(first.len(), 1);
    game.step(&Action::with_cards(ActionType::Play, vec![0]));
    let second = debuffed_names(&game);
    assert_eq!(second.len(), 1);
    assert_ne!(second, first);
}

#[test]
fn test_verdant_leaf_debuffs_the_deck_until_a_joker_is_sold() {
    let mut game = under("Verdant Leaf", &["Joker"]);
    assert!(
        game.hand.iter().all(|c| c.borrow().debuffed),
        "the whole deck should be off"
    );
    game.step(&Action::at(ActionType::SellJoker, 0));
    game._apply_debuffs();
    assert!(
        !game.hand.iter().any(|c| c.borrow().debuffed),
        "selling should lift it"
    );
}

#[test]
fn test_cerulean_bell_forces_a_card_into_every_hand() {
    let game = under("Cerulean Bell", &[]);
    let forced = game.forced_card.clone().expect("a card is forced");
    assert!(game.hand.iter().any(|c| Rc::ptr_eq(c, &forced)));

    let index = index_of(&game.hand, &forced);
    let others: Vec<usize> = (0..game.hand.len())
        .filter(|i| *i != index)
        .take(2)
        .collect();
    assert!(!game.is_legal(&Action::with_cards(ActionType::Play, others.clone())));
    let with_forced = vec![index, others[0]];
    assert!(game.is_legal(&Action::with_cards(ActionType::Play, with_forced)));
}

#[test]
fn test_dragging_does_not_shake_the_forced_card_off() {
    // It follows the card, not the position.
    //
    // The game sets ability.forced_selection on the card itself and keeps it
    // highlighted -- you cannot deselect it, and moving it around the hand does
    // not change that. So the constraint has to be about identity: a simulator
    // that remembered an index would let a player drag their way out of the
    // blind, and the replay harness reorders hands to follow recorded drags.
    let mut game = under("Cerulean Bell", &[]);
    let forced = game.forced_card.clone().expect("a card is forced");
    game.hand.reverse(); // the player drags it about

    assert!(game
        .forced_card
        .as_ref()
        .is_some_and(|f| Rc::ptr_eq(f, &forced)));
    let index = index_of(&game.hand, &forced);
    let others: Vec<usize> = (0..game.hand.len())
        .filter(|i| *i != index)
        .take(2)
        .collect();
    assert!(!game.is_legal(&Action::with_cards(ActionType::Play, others.clone())));
    assert!(game.is_legal(&Action::with_cards(
        ActionType::Play,
        vec![index, others[0]]
    )));
}

#[test]
fn test_a_forced_card_is_replaced_once_it_is_gone() {
    let mut game = under("Cerulean Bell", &[]);
    let first = game.forced_card.clone().expect("a card is forced");
    let index = index_of(&game.hand, &first);
    game.step(&Action::with_cards(ActionType::Discard, vec![index]));
    let now = game.forced_card.clone().expect("a card is still forced");
    assert!(!Rc::ptr_eq(&now, &first));
    assert!(game.hand.iter().any(|c| Rc::ptr_eq(c, &now)));
}

#[test]
fn test_a_targeting_tarot_under_the_bell_takes_the_forced_card_too() {
    // The forced card is still highlighted when a tarot is used on the hand.
    //
    // unhighlight_all will not take it off, so the Magician's gate counts it --
    // `mod_num >= #G.hand.highlighted` -- and the effect lands on it. The
    // simulator offered The Magician on two *other* cards.
    let mut game = under("Cerulean Bell", &[]);
    let magician = spec_or_panic("The Magician");
    let held = game.hold_consumable(magician, Edition::None);
    game.consumables.push(held);
    let forced = game.forced_card.clone().expect("a card is forced");
    let fi = index_of(&game.hand, &forced);
    let others: Vec<usize> = (0..game.hand.len()).filter(|i| *i != fi).collect();
    let use_action = |cards: Vec<usize>| Action {
        r#type: ActionType::UseConsumable,
        index: 0,
        cards,
    };

    let offered: Vec<Vec<usize>> = game
        .legal_actions()
        .iter()
        .filter(|a| a.r#type == ActionType::UseConsumable)
        .map(|a| a.cards.clone())
        .collect();
    assert!(!offered.is_empty());
    assert!(offered.iter().all(|cards| cards.contains(&fi)));
    assert!(!game.is_legal(&use_action(vec![others[0], others[1]])));
    assert!(!game.is_legal(&use_action(vec![others[0]])));
    assert!(game.is_legal(&use_action(vec![fi, others[0]])));
    assert!(game.is_legal(&use_action(vec![fi])));

    // Nothing is forced once the card has left the hand, or the boss is off.
    let card = game.forced_card.clone().unwrap();
    game.hand.retain(|c| !Rc::ptr_eq(c, &card));
    assert!(game.is_legal(&use_action(vec![0, 1])));
    game.hand.insert(fi, card);
    game.blind.as_mut().unwrap().disabled = true;
    assert!(game.is_legal(&use_action(vec![others[0], others[1]])));
}

#[test]
fn test_a_suit_boss_reads_the_cards_the_game_reads() {
    // blind.lua asks `card:is_suit(suit, true)`, not the printed suit.
    //
    // A Wild Card is every suit, so any of the four suit bosses debuffs it; a
    // Stone Card has no suit and none of them touch it; a Smeared Joker pairs
    // hearts with diamonds and spades with clubs. Reading `card.suit` let a wild
    // Five score under The Goad -- five chips and a Greedy Joker's three mult.
    fn debuffs(boss: &str, cards: &[CardRef], jokers: &[&str]) -> Vec<bool> {
        let mut game = under(boss, jokers);
        game.full_deck = cards.to_vec();
        game._apply_debuffs();
        cards.iter().map(|c| c.borrow().debuffed).collect()
    }

    let plain = make_card(Rank::Five, Suit::Spades);
    let wild = make_card(Rank::Five, Suit::Diamonds);
    wild.borrow_mut().enhancement = Enhancement::Wild;
    let stone = make_card(Rank::Five, Suit::Spades);
    stone.borrow_mut().enhancement = Enhancement::Stone;
    let heart = make_card(Rank::Five, Suit::Hearts);

    assert_eq!(
        debuffs("The Goad", &[plain, wild, stone, heart], &[]),
        vec![true, true, false, false]
    );
    // Smeared makes spades and clubs one suit, so The Goad takes clubs too.
    let club = make_card(Rank::Five, Suit::Clubs);
    assert_eq!(
        debuffs("The Goad", &[club.clone()], &["Smeared Joker"]),
        vec![true]
    );
    assert_eq!(debuffs("The Goad", &[club], &[]), vec![false]);
}

#[test]
fn test_cerulean_bell_forces_its_card_into_discards_as_well_as_plays() {
    // `is_legal` and `legal_actions` have to be the same answer.
    //
    // The Bell keeps its card highlighted, so neither a play nor a discard can
    // go without it. `is_legal` refused such a discard and `legal_actions`
    // offered one, and a policy that proposes only what the list offers had its
    // move refused 247 decisions into a run.
    let mut game = under("Cerulean Bell", &[]);
    game.discards_left = 3;
    let forced = game.forced_card.clone().expect("a card is forced");
    let fi = index_of(&game.hand, &forced);

    for action in game.legal_actions() {
        if matches!(action.r#type, ActionType::Play | ActionType::Discard) {
            assert!(action.cards.contains(&fi), "{action}");
            assert!(game.is_legal(&action), "{action}");
        }
    }

    let without: Vec<usize> = (0..game.hand.len()).filter(|i| *i != fi).take(3).collect();
    assert!(!game.is_legal(&Action::with_cards(ActionType::Discard, without)));
}

// ==========================================================================
// tests/test_boss_reroll.py -- "Who may re-roll the boss blind, and how often."
//
// Three different answers depending on what the run has redeemed, and the
// simulator had none of them: the button was not modelled at all, so a
// recording where the player pressed it diverged from that step on.
//
//   no voucher        no reroll at all
//   Director's Cut    one an ante; reset_blinds gives it back when a boss falls
//   Retcon            any number
//
// Affordability is measured against the debt floor rather than against zero, so
// a Credit Card lets a broke run keep re-rolling.
// ==========================================================================

fn reroll_run(voucher_keys: &[&str], money: i32) -> GameState {
    let mut game = GameState::new("TESTSEED", "Red Deck", 1);
    game.money = money;
    for key in voucher_keys {
        game.vouchers.push(voucher_by_key(key).unwrap());
    }
    game
}

#[test]
fn test_without_a_voucher_there_is_no_reroll() {
    assert!(!reroll_run(&[], 100).can_reroll_boss());
}

#[test]
fn test_directors_cut_allows_one_an_ante() {
    let mut game = reroll_run(&["v_directors_cut"], 100);
    assert!(game.can_reroll_boss());
    game.step(&Action::new(ActionType::RerollBoss));
    assert!(
        !game.can_reroll_boss(),
        "Director's Cut re-rolled twice in an ante"
    );
}

#[test]
fn test_retcon_allows_any_number() {
    let mut game = reroll_run(&["v_retcon"], 100);
    for _ in 0..4 {
        assert!(game.can_reroll_boss());
        game.step(&Action::new(ActionType::RerollBoss));
    }
    assert!(game.can_reroll_boss());
}

#[test]
fn test_the_reroll_costs_ten_and_changes_the_boss() {
    let mut game = reroll_run(&["v_retcon"], 100);
    let money = game.money;
    let mut seen: HashSet<String> = HashSet::new();
    seen.insert(game.ante_boss.clone());
    for _ in 0..6 {
        game.step(&Action::new(ActionType::RerollBoss));
        seen.insert(game.ante_boss.clone());
    }
    assert_eq!(game.money, money - 6 * BOSS_REROLL_COST);
    assert!(seen.len() > 1, "six re-rolls and the boss never changed");
}

#[test]
fn test_a_run_that_cannot_pay_cannot_reroll() {
    assert!(!reroll_run(&["v_retcon"], BOSS_REROLL_COST - 1).can_reroll_boss());
    assert!(reroll_run(&["v_retcon"], BOSS_REROLL_COST).can_reroll_boss());
}

#[test]
fn test_beating_a_boss_gives_the_reroll_back() {
    let mut game = reroll_run(&["v_directors_cut"], 100);
    game.step(&Action::new(ActionType::RerollBoss));
    assert!(!game.can_reroll_boss());
    game.boss_rerolled = false; // what reset_blinds does at cash-out
    assert!(game.can_reroll_boss());
}

#[test]
fn test_the_reroll_is_listed_where_it_is_legal() {
    // is_legal is the exact membership test for legal_actions(); the button was
    // allowed by the one and missing from the other.
    for game in [
        reroll_run(&[], 100),
        reroll_run(&["v_directors_cut"], 100),
        reroll_run(&["v_retcon"], 5),
    ] {
        let listed = game
            .legal_actions()
            .iter()
            .any(|a| a.r#type == ActionType::RerollBoss);
        assert_eq!(listed, game.is_legal(&Action::new(ActionType::RerollBoss)));
    }
    assert!(reroll_run(&["v_directors_cut"], 100)
        .legal_actions()
        .iter()
        .any(|a| a.r#type == ActionType::RerollBoss));
}

// ==========================================================================
// tests/test_end_of_round_resets.py -- "The end_of_round joker branches the
// simulator was missing."
//
// end_round asks every joker `{end_of_round = true}` the moment a round is over;
// each branch is `not context.blueprint` and a debuffed joker answers nothing.
//
// Hit the Road resets its x_mult to 1 when the round ends, and the simulator
// never did, so every Jack discarded stayed in the X for the rest of the run.
// Cavendish rolls its 1 in 1000 there as well, on its own stream, and, unlike
// Gros Michel, sets no pool flag when it goes.
// ==========================================================================

fn resets_run(names: &[&str]) -> GameState {
    let mut game = GameState::new("VIBC905W", "Red Deck", 1);
    for name in names {
        game.gain_joker(&joker(name));
    }
    game._start_round();
    game
}

/// `_end(game, kind)`: put a blind in force and close the round.
fn end_round(game: &mut GameState, kind: BlindKind) {
    let ante = game.ante;
    let scaling = game.blind_scaling();
    game.blind = Some(make_blind(kind, ante, None, 1.0, scaling, false));
    game._beat_blind(true);
}

fn on_seed(seed: &str, names: &[&str]) -> GameState {
    let mut game = GameState::new(seed, "Red Deck", 1);
    for name in names {
        game.gain_joker(&joker(name));
    }
    game._start_round();
    game
}

#[test]
fn test_hit_the_road_resets_when_the_round_ends() {
    let mut game = resets_run(&["Hit the Road"]);
    game.jokers[0].borrow_mut().counter = 2.5;
    end_round(&mut game, BlindKind::Small);
    assert_eq!(game.jokers[0].borrow().counter, 1.0);
}

#[test]
fn test_hit_the_road_resets_on_any_blind() {
    // No `blind.boss` condition, unlike Campfire.
    for kind in [BlindKind::Small, BlindKind::Big, BlindKind::Boss] {
        let mut game = resets_run(&["Hit the Road"]);
        game.jokers[0].borrow_mut().counter = 1.5;
        end_round(&mut game, kind);
        assert_eq!(game.jokers[0].borrow().counter, 1.0);
    }
}

#[test]
fn test_a_debuffed_hit_the_road_keeps_its_x() {
    let mut game = resets_run(&["Hit the Road"]);
    let j = game.jokers[0].clone();
    j.borrow_mut().counter = 2.0;
    game.set_joker_debuff(&j, true);
    end_round(&mut game, BlindKind::Small);
    assert_eq!(game.jokers[0].borrow().counter, 2.0);
}

#[test]
fn test_cavendish_rolls_at_the_end_of_the_round() {
    let mut game = resets_run(&["Cavendish"]);
    assert!(!game.rng.pools.contains_key("cavendish"));
    end_round(&mut game, BlindKind::Small);
    assert!(game.rng.pools.contains_key("cavendish"));
    assert_eq!(joker_names(&game), vec!["Cavendish"]);
}

#[test]
fn test_a_debuffed_cavendish_does_not_roll() {
    let mut game = resets_run(&["Cavendish"]);
    let j = game.jokers[0].clone();
    game.set_joker_debuff(&j, true);
    end_round(&mut game, BlindKind::Small);
    assert!(!game.rng.pools.contains_key("cavendish"));
}

#[test]
fn test_cavendish_goes_extinct_without_a_pool_flag() {
    // Python monkeypatches `game.rng.chance` to force the 1-in-1000 and to
    // record its odds. Rust cannot patch the method, so the test finds a seed
    // whose fresh run draws the same "cavendish" pool at its 1-in-1000 -- the
    // draw is a pure function of the seed before the round end touches it --
    // and asserts the same extinction, with no flag.
    let mut seed = None;
    for i in 0..200_000 {
        let candidate = format!("CAV{i:06}");
        let mut probe = RunRng::new(&candidate);
        if probe.chance("cavendish", 1.0, 1000.0) {
            seed = Some(candidate);
            break;
        }
    }
    let seed = seed.expect("a seed whose Cavendish roll fires");

    let mut game = on_seed(&seed, &["Cavendish"]);
    end_round(&mut game, BlindKind::Small);
    assert!(joker_names(&game).is_empty());
    assert!(!game.pool_flags.contains("gros_michel_extinct"));
}

// ==========================================================================
// tests/test_mouth_locks_the_first_hand.py -- "The Mouth zeroes every hand but
// the round's first type, however often played."
//
// `only_hand` is the first hand's name and nothing else: a debuffed hand
// returns before the assignment, so a zeroed type never becomes allowed. It is
// cleared by set_blind and is not written by the `check` call made while cards
// are only highlighted.
//
// The simulator asked "was this type played this round" instead, and a zeroed
// hand still counts as played.
// ==========================================================================

type Spec = (Rank, Suit);

const PAIR: [Spec; 5] = [
    (Rank::Nine, Suit::Hearts),
    (Rank::Nine, Suit::Diamonds),
    (Rank::Five, Suit::Hearts),
    (Rank::Four, Suit::Clubs),
    (Rank::Two, Suit::Clubs),
];
const STRAIGHT: [Spec; 5] = [
    (Rank::Ace, Suit::Spades),
    (Rank::King, Suit::Clubs),
    (Rank::Queen, Suit::Hearts),
    (Rank::Jack, Suit::Clubs),
    (Rank::Ten, Suit::Diamonds),
];
const TRIPS: [Spec; 5] = [
    (Rank::Seven, Suit::Hearts),
    (Rank::Seven, Suit::Clubs),
    (Rank::Seven, Suit::Diamonds),
    (Rank::Five, Suit::Diamonds),
    (Rank::Four, Suit::Spades),
];

fn mouth_round() -> GameState {
    let mut game = GameState::new("Q4BAHUP3", "Red Deck", 1);
    game.ante_boss = String::new();
    let ante = game.ante;
    let mouth = boss_by_name("The Mouth").unwrap();
    game.blind = Some(make_blind(
        BlindKind::Boss,
        ante,
        Some(mouth),
        1.0,
        1,
        false,
    ));
    game._start_round();
    game.blind.as_mut().unwrap().target = 10i64.pow(12); // never cleared
    game.hands_left = 10;
    game
}

/// `_deal`: put these cards in hand and return every index.
fn deal(game: &mut GameState, specs: &[Spec]) -> Vec<usize> {
    game.hand = specs.iter().map(|(r, s)| make_card(*r, *s)).collect();
    (0..specs.len()).collect()
}

/// `_play`: chips the play adds to the round.
fn mouth_play(game: &mut GameState, specs: &[Spec]) -> i64 {
    let before = game.chips_scored;
    let picks = deal(game, specs);
    game.step(&Action::with_cards(ActionType::Play, picks));
    game.chips_scored - before
}

#[test]
fn test_the_q4bahup3_round() {
    // Pair, Straight, Three of a Kind, Three of a Kind: only the Pair scores.
    let mut game = mouth_round();
    assert!(mouth_play(&mut game, &PAIR) > 0);
    assert_eq!(mouth_play(&mut game, &STRAIGHT), 0);
    assert_eq!(mouth_play(&mut game, &TRIPS), 0);
    assert_eq!(
        mouth_play(&mut game, &TRIPS),
        0,
        "a zeroed Three of a Kind does not become the round's hand"
    );
    assert!(mouth_play(&mut game, &PAIR) > 0);
}

#[test]
fn test_preview_sees_the_zero() {
    // The policy values plays with preview_score, so the zero must show there.
    let mut game = mouth_round();
    mouth_play(&mut game, &PAIR);
    mouth_play(&mut game, &TRIPS);
    let trips = deal(&mut game, &TRIPS);
    assert_eq!(game.preview_score(&trips, "roll"), 0);
    let pair = deal(&mut game, &PAIR);
    assert!(game.preview_score(&pair, "roll") > 0);
}

#[test]
fn test_previewing_does_not_pick_the_hand() {
    // `check` mode: highlighting a hand never sets only_hand.
    let mut game = mouth_round();
    let trips = deal(&mut game, &TRIPS);
    assert!(game.preview_score(&trips, "roll") > 0);
    assert!(mouth_play(&mut game, &PAIR) > 0);
    assert_eq!(mouth_play(&mut game, &TRIPS), 0);
}

#[test]
fn test_a_new_blind_clears_the_lock() {
    // set_blind: `if self.name == 'The Mouth' and not reset then only_hand = false`.
    let mut game = mouth_round();
    mouth_play(&mut game, &PAIR);
    let ante = game.ante;
    let mouth = boss_by_name("The Mouth").unwrap();
    game.blind = Some(make_blind(
        BlindKind::Boss,
        ante,
        Some(mouth),
        1.0,
        1,
        false,
    ));
    game._start_round();
    game.blind.as_mut().unwrap().target = 10i64.pow(12);
    game.hands_left = 10;
    assert!(mouth_play(&mut game, &TRIPS) > 0);
    assert_eq!(mouth_play(&mut game, &PAIR), 0);
}

#[test]
fn test_a_disabled_mouth_zeroes_nothing() {
    // `if self.disabled then return end` comes before the lock.
    let mut game = mouth_round();
    game.blind.as_mut().unwrap().disabled = true;
    assert!(mouth_play(&mut game, &PAIR) > 0);
    assert!(mouth_play(&mut game, &TRIPS) > 0);
    assert!(mouth_play(&mut game, &PAIR) > 0);
}

// ==========================================================================
// tests/test_hands_left_blind_select.py -- "The hands and discards on the blind
// select screen are the ones cash-out set."
//
// The game sets G.GAME.current_round.hands_left in exactly three places --
// start_run, cash_out and new_round -- and otherwise only moves it by
// ease_hands_played. Leaving the shop and skipping a blind touch neither.
// Troubadour's -1 hand is written to round_resets.hands alone, so one bought in
// the shop leaves the counter where cash-out put it until the next blind is
// taken.
//
// The simulator recomputed the allowance in _next_blind, so it read 3 on the
// blind select screen where the game reads 4.
// ==========================================================================

/// `_shop(game, joker=None)`: a shop open with one joker on its shelf.
fn open_shop(mut game: GameState, shelved: Option<JokerRef>) -> GameState {
    game.phase = Phase::Shop;
    game._open_shop();
    let slots = match shelved {
        Some(j) => {
            let mut slot = ShopSlot::new("joker", 1);
            slot.joker = Some(j);
            vec![slot]
        }
        None => vec![],
    };
    game.shop.as_mut().unwrap().slots = slots;
    game.money = 50;
    game
}

fn troubadour() -> JokerRef {
    joker("Troubadour")
}

#[test]
fn test_the_run_starts_with_the_allowance_on_the_counter() {
    // game.lua: the first blind select already shows the hands.
    let game = GameState::new("HANDSLFT", "Red Deck", 1);
    assert_eq!(game.phase, Phase::BlindSelect);
    assert_eq!(game.hands_left, 4);
}

#[test]
fn test_a_troubadour_bought_in_the_shop_waits_for_the_blind() {
    // card.lua moves round_resets.hands only; new_round applies it.
    let mut game = open_shop(
        GameState::new("HANDSLFT", "Red Deck", 1),
        Some(troubadour()),
    );
    assert_eq!(game.hands_left, 4);
    game.step(&Action::at(ActionType::Buy, 0));
    assert_eq!(game.hands_left, 4);
    game.step(&Action::new(ActionType::LeaveShop));
    assert_eq!(game.phase, Phase::BlindSelect);
    assert_eq!(game.hands_left, 4);
    game.step(&Action::new(ActionType::SelectBlind));
    assert_eq!(game.hands_left, 3);
}

#[test]
fn test_skipping_a_blind_does_not_reset_the_counter_either() {
    let mut game = open_shop(
        GameState::new("HANDSLFT", "Red Deck", 1),
        Some(troubadour()),
    );
    game.step(&Action::at(ActionType::Buy, 0));
    game.step(&Action::new(ActionType::LeaveShop));
    game.step(&Action::new(ActionType::SkipBlind));
    assert_eq!(game.phase, Phase::BlindSelect);
    assert_eq!(game.hands_left, 4);
    game.step(&Action::new(ActionType::SelectBlind));
    assert_eq!(game.hands_left, 3);
}

#[test]
fn test_a_troubadour_sold_in_the_shop_gives_its_hand_back_next_round() {
    // card.lua: the mirror image -- 3 on the screen, 4 once selected.
    let mut game = GameState::new("HANDSLFT", "Red Deck", 1);
    game.gain_joker(&troubadour());
    // What cash-out would have left with the Troubadour held.
    let (hands, discards) = game.round_allowance(true);
    game.hands_left = hands;
    game.discards_left = discards;
    assert_eq!(game.hands_left, 3);
    game = open_shop(game, None);
    game.step(&Action::at(ActionType::SellJoker, 0));
    game.step(&Action::new(ActionType::LeaveShop));
    assert_eq!(game.hands_left, 3);
    game.step(&Action::new(ActionType::SelectBlind));
    assert_eq!(game.hands_left, 4);
}

#[test]
fn test_a_merry_andy_bought_in_the_shop_still_pays_its_discards_at_once() {
    // card.lua: d_size is eased immediately, so it must survive leaving the shop
    // now that nothing recomputes the counter there.
    let mut game = open_shop(
        GameState::new("HANDSLFT", "Red Deck", 1),
        Some(joker("Merry Andy")),
    );
    let before = game.discards_left;
    game.step(&Action::at(ActionType::Buy, 0));
    game.step(&Action::new(ActionType::LeaveShop));
    assert_eq!(game.discards_left, before + 3);
}

// ==========================================================================
// tests/test_todo_list_creation.py -- "To Do List names its hand the moment the
// card is made, not a round later."
//
// Card:set_ability runs from Card:init for every card built, and for a To Do
// List it draws from every visible hand. So a To Do List in a shop, a Buffoon
// pack or out of Judgement already names a hand, and pays for it in the round
// it is bought. The simulator only rolled the hand at the end of a round, which
// left a newly made one on None, and it never took the creation draw, so every
// later end-of-round roll landed on a different hand from the game's.
//
// copy_card goes through set_ability too, so an Ankh copy spends a draw -- and
// then copies the ability table over it, keeping the original's hand.
// ==========================================================================

fn pack_joker_entry(key: &str) -> PackCard {
    PackCard {
        set: "Joker",
        key: Some(key.to_string()),
        edition: Some("none"),
        ..Default::default()
    }
}

fn pack_joker_ref(game: &mut GameState, key: &str) -> JokerRef {
    match game._pack_card(&pack_joker_entry(key)) {
        PackChoice::Joker(j) => j,
        _ => panic!("the pack card was not a joker"),
    }
}

fn todo(game: &GameState) -> Vec<JokerRef> {
    game.jokers
        .iter()
        .filter(|j| j.borrow().name() == "To Do List")
        .cloned()
        .collect()
}

/// `_pair()`: two identical runs, one to act on and one to read the stream from.
fn todo_pair() -> (GameState, GameState) {
    (
        GameState::new("TODOSEED", "Red Deck", 1),
        GameState::new("TODOSEED", "Red Deck", 1),
    )
}

#[test]
fn test_a_pack_to_do_list_names_a_hand_straight_away() {
    let (mut game, mut twin) = todo_pair();
    let j = pack_joker_ref(&mut game, "j_todo_list");
    let visible = twin.visible_hands();
    let expected = twin.rng.choice("to_do", &visible);
    assert!(j.borrow().named_hand.is_some());
    assert_eq!(j.borrow().named_hand, Some(expected));
}

#[test]
fn test_the_round_end_roll_follows_the_creation_draw() {
    // One stream: creation, then the reroll that excludes the current hand.
    let (mut game, mut twin) = todo_pair();
    let j = pack_joker_ref(&mut game, "j_todo_list");
    game.gain_joker(&j);
    game._reroll_todo_hands();

    let visible = twin.visible_hands();
    let first = twin.rng.choice("to_do", &visible);
    let pool: Vec<HandType> = visible.iter().cloned().filter(|h| *h != first).collect();
    let second = twin.rng.choice("to_do", &pool);
    assert_eq!(todo(&game)[0].borrow().named_hand, Some(second));
}

#[test]
fn test_a_shop_to_do_list_names_a_hand() {
    // Python monkeypatches shop_pool.draw_shop_card to force j_todo_list. The
    // Rust test uses seed SLOT00059, found with the Python reference, on which
    // the first _roll_slot is a To Do List.
    let mut game = GameState::new("SLOT00059", "Red Deck", 1);
    let mut twin = GameState::new("SLOT00059", "Red Deck", 1);
    let slot = game._roll_slot();
    let j = slot.joker.expect("the slot holds a joker");
    assert_eq!(j.borrow().name(), "To Do List");
    let visible = twin.visible_hands();
    let expected = twin.rng.choice("to_do", &visible);
    assert_eq!(j.borrow().named_hand, Some(expected));
}

#[test]
fn test_a_judgement_to_do_list_names_a_hand() {
    // Python monkeypatches shop_pool.draw_joker to force j_todo_list. The Rust
    // test uses seed JT00264, found with the Python reference, on which
    // Judgement makes a To Do List.
    let mut game = GameState::new("JT00264", "Red Deck", 1);
    let mut twin = GameState::new("JT00264", "Red Deck", 1);
    game.add_random_joker("Judgement", None, false, "jud", false);
    let visible = twin.visible_hands();
    let expected = twin.rng.choice("to_do", &visible);
    assert_eq!(todo(&game)[0].borrow().named_hand, Some(expected));
}

#[test]
fn test_an_ankh_copy_spends_a_draw_and_keeps_the_original_hand() {
    let (mut game, mut twin) = todo_pair();
    let original = joker("To Do List");
    let named = game.visible_hands()[3];
    original.borrow_mut().named_hand = Some(named);
    game.gain_joker(&original);

    game.add_joker_copy(&original, "Ankh");
    let copy = todo(&game)[1].clone();
    assert_eq!(copy.borrow().named_hand, original.borrow().named_hand);

    // The copy's set_ability draw is spent, so the next roll is the second
    // draw of the stream, not the first.
    let visible = twin.visible_hands();
    let _spent = twin.rng.choice("to_do", &visible);
    let pool: Vec<HandType> = visible
        .iter()
        .cloned()
        .filter(|h| Some(*h) != original.borrow().named_hand)
        .collect();
    let expected = twin.rng.choice("to_do", &pool);
    game._reroll_todo_hands();
    assert_eq!(todo(&game)[0].borrow().named_hand, Some(expected));
}

#[test]
fn test_other_jokers_take_no_to_do_draw() {
    let (mut game, mut twin) = todo_pair();
    pack_joker_ref(&mut game, "j_joker");
    let j = pack_joker_ref(&mut game, "j_todo_list");
    game.gain_joker(&j);
    let visible = twin.visible_hands();
    let expected = twin.rng.choice("to_do", &visible);
    assert_eq!(todo(&game)[0].borrow().named_hand, Some(expected));
}

// ==========================================================================
// tests/test_preview_refused_hand.py -- "`preview_play` of a hand the boss
// refuses hands back the row `after` left."
//
// evaluate_play skips the scoring block for a refused hand but asks every joker
// `context.after` for every hand played, outside that `if`. Ice Cream and
// Seltzer answer it, so a hand The Mouth zeroes still melts one and counts the
// other down -- which GameState._play does. preview_play said 0 for such a hand
// and returned the copies untouched, so a policy pricing the row a play leaves
// saw a refused hand as free.
//
// The row that comes back must be the row a real step leaves, for a refused
// hand and a scored one alike, and the run's own row must not move.
// ==========================================================================

const ICE_ROW: [&str; 4] = ["Ice Cream", "Seltzer", "Green Joker", "Ride the Bus"];

fn preview_mouth(names: &[&str]) -> GameState {
    let mut game = GameState::new("VJPW2C6Z", "Red Deck", 1);
    for name in names {
        game.gain_joker(&joker(name));
    }
    game.ante_boss = String::new();
    let ante = game.ante;
    let mouth = boss_by_name("The Mouth").unwrap();
    game.blind = Some(make_blind(
        BlindKind::Boss,
        ante,
        Some(mouth),
        1.0,
        1,
        false,
    ));
    game._start_round();
    game.blind.as_mut().unwrap().target = 10i64.pow(12); // never cleared
    game.hands_left = 10;
    game
}

fn row_of(jokers: &[JokerRef]) -> Vec<(String, f64)> {
    jokers
        .iter()
        .map(|j| (j.borrow().name().to_string(), j.borrow().counter))
        .collect()
}

/// `_preview_then_play`: (preview score, preview row, real gain, real row),
/// checking on the way that the preview left the run's own row alone.
fn preview_then_play(
    game: &mut GameState,
    specs: &[Spec],
) -> (i64, Vec<(String, f64)>, i64, Vec<(String, f64)>) {
    let picks = deal(game, specs);
    let real = game.jokers.clone();
    let before = row_of(&game.jokers);
    let (score, row) = game.preview_play(&picks, "roll");
    assert_eq!(row_of(&game.jokers), before);
    assert!(game
        .jokers
        .iter()
        .zip(real.iter())
        .all(|(a, b)| Rc::ptr_eq(a, b)));
    assert_eq!(game.jokers.len(), real.len());
    assert!(!row.iter().any(|r| real.iter().any(|j| Rc::ptr_eq(r, j))));
    let gained = game.chips_scored;
    game.step(&Action::with_cards(ActionType::Play, picks));
    (
        score,
        row_of(&row),
        game.chips_scored - gained,
        row_of(&game.jokers),
    )
}

#[test]
fn test_a_refused_hand_previews_the_after_pass() {
    // state_events.lua under The Mouth: Ice Cream 95 -> 90, Seltzer 9 -> 8;
    // Green Joker and Ride the Bus are `before` and stay.
    let mut game = preview_mouth(&ICE_ROW);
    let picks = deal(&mut game, &PAIR);
    game.step(&Action::with_cards(ActionType::Play, picks));
    let (score, row, gained, real) = preview_then_play(&mut game, &TRIPS);
    assert_eq!(score, 0);
    assert_eq!(gained, 0);
    assert_eq!(row, real);
    assert_eq!(
        real,
        vec![
            ("Ice Cream".to_string(), 90.0),
            ("Seltzer".to_string(), 8.0),
            ("Green Joker".to_string(), 1.0),
            ("Ride the Bus".to_string(), 1.0),
        ]
    );
}

#[test]
fn test_a_refused_hand_previews_the_jokers_it_eats() {
    // Ice Cream on 5 and Seltzer on 1 are both gone after the refused hand, in
    // the preview's row and the real one alike.
    let mut game = preview_mouth(&["Ice Cream", "Seltzer", "Joker"]);
    let picks = deal(&mut game, &PAIR);
    game.step(&Action::with_cards(ActionType::Play, picks));
    game.jokers[0].borrow_mut().counter = 5.0;
    game.jokers[1].borrow_mut().counter = 1.0;
    let (score, row, gained, real) = preview_then_play(&mut game, &TRIPS);
    assert_eq!(score, 0);
    assert_eq!(gained, 0);
    assert_eq!(row, real);
    assert_eq!(real, vec![("Joker".to_string(), 0.0)]);
}

#[test]
fn test_a_scored_hand_previews_the_same_row_as_the_step() {
    let mut game = preview_mouth(&ICE_ROW);
    let (score, row, gained, real) = preview_then_play(&mut game, &PAIR);
    assert!(score > 0 && score == gained);
    assert_eq!(row, real);
    assert_eq!(
        real,
        vec![
            ("Ice Cream".to_string(), 95.0),
            ("Seltzer".to_string(), 9.0),
            ("Green Joker".to_string(), 1.0),
            ("Ride the Bus".to_string(), 1.0),
        ]
    );
}

#[test]
fn test_a_blueprint_does_not_melt_the_preview_twice() {
    // Both `after` branches are `not context.blueprint`.
    let mut game = preview_mouth(&["Blueprint", "Ice Cream"]);
    let picks = deal(&mut game, &PAIR);
    game.step(&Action::with_cards(ActionType::Play, picks));
    let (_, row, _, real) = preview_then_play(&mut game, &TRIPS);
    assert_eq!(row, real);
    assert_eq!(real[1], ("Ice Cream".to_string(), 90.0));
}

// ==========================================================================
// tests/test_preview_is_read_only.py -- "preview_score is a question, not a
// move."
//
// It runs the whole scoring pipeline, and the pipeline writes: Space Joker
// levels the played hand one time in four, 8 Ball makes a Tarot for a scored
// eight, Hiker adds five chips to every scored card for good, Vampire and Midas
// Touch change enhancements. The preview copied the jokers and put back the
// played cards' enhancements and left the rest where scoring had moved it.
//
// A bot that previews every subset of its hand asks two hundred times a
// decision. The first one to do so took High Card to level 140 through Space
// Joker alone and won a run at ante eleven on hands the game never dealt --
// which is how this was found, and why the whole of what scoring can touch is
// snapshotted now.
// ==========================================================================

const PREVIEW_SEED: &str = "PREVIEW1";

/// `_run(*joker_names, seed)`: a run standing in its first round.
fn preview_run(names: &[&str], seed: &str) -> GameState {
    let mut game = GameState::new(seed, "Red Deck", 1);
    for name in names {
        game.gain_joker(&joker(name));
    }
    game.step(&Action::new(ActionType::SelectBlind));
    assert_eq!(game.phase, Phase::Playing);
    game
}

fn consumable_names(game: &GameState) -> Vec<String> {
    game.consumables
        .iter()
        .map(|c| c.borrow().spec.name.to_string())
        .collect()
}

/// Every combination of hand indices, sizes one to five.
fn combos(n: usize, k: usize) -> Vec<Vec<usize>> {
    fn rec(start: usize, n: usize, k: usize, cur: &mut Vec<usize>, out: &mut Vec<Vec<usize>>) {
        if cur.len() == k {
            out.push(cur.clone());
            return;
        }
        for i in start..n {
            cur.push(i);
            rec(i + 1, n, k, cur, out);
            cur.pop();
        }
    }
    let mut out = Vec::new();
    rec(0, n, k, &mut Vec::new(), &mut out);
    out
}

/// `_every_subset`: preview every subset of one to five cards.
fn every_subset(game: &mut GameState) {
    let n = game.hand.len();
    for size in 1..=5 {
        for combo in combos(n, size) {
            game.preview_score(&combo, "roll");
        }
    }
}

/// `_fingerprint`: everything scoring can move, in a stable string.
fn fingerprint(game: &GameState) -> String {
    let mut out = String::new();
    for c in &game.full_deck {
        let b = c.borrow();
        out.push_str(&format!(
            "c|{}|{:?}|{:?}|{:?}|{:?}|{:?}|{}\n",
            uid_of(c),
            b.rank,
            b.suit,
            b.enhancement,
            b.edition,
            b.seal,
            b.extra_chips
        ));
    }
    out.push_str(&format!("hand|{:?}\n", hand_uids(game)));
    let mut levels: Vec<(String, i32)> = game
        .hand_levels
        .levels
        .iter()
        .map(|(h, v)| (h.label().to_string(), *v))
        .collect();
    levels.sort();
    out.push_str(&format!("levels|{levels:?}\n"));
    let mut plays: Vec<(String, i32)> = game
        .hand_levels
        .plays
        .iter()
        .map(|(h, v)| (h.label().to_string(), *v))
        .collect();
    plays.sort();
    out.push_str(&format!("plays|{plays:?}\n"));
    out.push_str(&format!("consumables|{:?}\n", consumable_names(game)));
    let jokers: Vec<(String, u64, u64)> = game
        .jokers
        .iter()
        .map(|j| {
            let b = j.borrow();
            (
                b.name().to_string(),
                b.counter.to_bits(),
                b.secondary.to_bits(),
            )
        })
        .collect();
    out.push_str(&format!("jokers|{jokers:?}\n"));
    out.push_str(&format!("money|{}\n", game.money));
    let mut pools: Vec<(String, u64)> = game
        .rng
        .pools
        .iter()
        .map(|(k, v)| (k.clone(), v.to_bits()))
        .collect();
    pools.sort();
    out.push_str(&format!("pools|{pools:?}\n"));
    out
}

#[test]
fn test_space_joker_does_not_level_through_a_preview() {
    // The preview's throwaway stream is named off the seed, so whether the
    // one-in-four fires is a property of the seed. On SPACEHU8 it does, in the
    // first hand's subsets, and the unfixed preview levelled a hand for good.
    let mut game = preview_run(&["Space Joker"], "SPACEHU8");
    let before = fingerprint(&game);
    for _ in 0..3 {
        every_subset(&mut game);
    }
    assert_eq!(fingerprint(&game), before);
}

#[test]
fn test_eight_ball_makes_no_tarot_through_a_preview() {
    let mut game = preview_run(&["8 Ball"], PREVIEW_SEED);
    for card in game.hand[..3].to_vec() {
        card.borrow_mut().rank = Rank::Eight;
    }
    let before = fingerprint(&game);
    every_subset(&mut game);
    assert_eq!(fingerprint(&game), before);
    assert!(game.consumables.is_empty());
}

#[test]
fn test_hiker_adds_no_chips_through_a_preview() {
    let mut game = preview_run(&["Hiker"], PREVIEW_SEED);
    let before = fingerprint(&game);
    every_subset(&mut game);
    assert_eq!(fingerprint(&game), before);
    assert!(game.hand.iter().all(|c| c.borrow().extra_chips == 0));
}

#[test]
fn test_a_preview_is_what_the_play_then_scores() {
    // Without randomness in the way, the answer is the move, to the chip.
    let mut game = preview_run(&["Joker", "Hiker"], PREVIEW_SEED);
    let n = game.hand.len();
    let mut best: Vec<usize> = Vec::new();
    let mut best_score = i64::MIN;
    for size in 1..=5 {
        for combo in combos(n, size) {
            let score = game.preview_score(&combo, "roll");
            if score > best_score {
                best_score = score;
                best = combo;
            }
        }
    }
    let predicted = game.preview_score(&best, "roll");
    let before = game.chips_scored;
    game.step(&Action::with_cards(ActionType::Play, best));
    assert_eq!(game.chips_scored - before, predicted);
}

// ==========================================================================
// tests/test_preview_play.py -- "`preview_play` is `preview_score` handing back
// the jokers it scored with."
//
// A scaling joker grows while a hand scores, and `preview_score` restores the
// row afterwards so a policy can preview two hundred plays a decision without
// moving the run. That is right for the score and hides what the play would
// make of the jokers, which is exactly what farming a Square Joker is about.
// `preview_play` returns the copied row as scoring left it; the run's own row
// is still restored.
// ==========================================================================

fn square_game() -> GameState {
    let mut game = GameState::new("QWERTYUI", "Blue Deck", 1);
    game.gain_joker(&joker("Square Joker"));
    game._start_round();
    game
}

#[test]
fn test_a_four_card_play_grows_the_returned_row() {
    let mut game = square_game();
    let (_, row) = game.preview_play(&[0, 1, 2, 3], "roll");
    assert_eq!(row[0].borrow().counter, 4.0);
    assert_eq!(
        game.jokers[0].borrow().counter,
        0.0,
        "the run's row must be untouched"
    );
}

#[test]
fn test_a_five_card_play_does_not() {
    let mut game = square_game();
    let (_, row) = game.preview_play(&[0, 1, 2, 3, 4], "roll");
    assert_eq!(row[0].borrow().counter, 0.0);
}

#[test]
fn test_the_score_is_preview_score() {
    let mut game = square_game();
    assert_eq!(
        game.preview_play(&[0, 1, 2, 3], "roll").0,
        game.preview_score(&[0, 1, 2, 3], "roll")
    );
}

// ==========================================================================
// tests/test_preview_money.py -- "The money a play earns in expectation, exact
// rather than rolled."
// ==========================================================================

fn money_hand(names: &[&str], card: CardRef) -> GameState {
    let mut game = GameState::new("MONEY001", "Red Deck", 1);
    for name in names {
        game.gain_joker(&joker(name));
    }
    game._start_round();
    game.hand = vec![card, make_card(Rank::Two, Suit::Clubs)];
    game
}

#[test]
fn test_a_coin_counts_at_its_odds() {
    let king = make_card(Rank::King, Suit::Hearts);
    let mut business = money_hand(&["Business Card"], king.clone());
    assert_eq!(business.preview_money(&[0]).1, 1.0);
    // Blueprint copying the Card flips a second coin.
    let mut both = money_hand(&["Blueprint", "Business Card"], king.clone());
    assert_eq!(both.preview_money(&[0]).1, 2.0);
    // And Hanging Chad scores the King three times.
    let mut chad = money_hand(&["Hanging Chad", "Blueprint", "Business Card"], king);
    assert_eq!(chad.preview_money(&[0]).1, 6.0);
}

#[test]
fn test_certain_money_counts_in_full_and_a_lucky_card_at_one_in_fifteen() {
    let seal = make_card(Rank::Six, Suit::Clubs);
    seal.borrow_mut().seal = Seal::Gold;
    let mut sealed = money_hand(&[], seal);
    assert_eq!(sealed.preview_money(&[0]).1, 3.0);

    let lucky = make_card(Rank::Six, Suit::Clubs);
    lucky.borrow_mut().enhancement = Enhancement::Lucky;
    let mut lucky_game = money_hand(&[], lucky);
    let expected = lucky_game.preview_money(&[0]).1;
    assert!((expected - 20.0 / 15.0).abs() < 1e-9);
}

#[test]
fn test_the_rolled_preview_is_untouched() {
    let king = make_card(Rank::King, Suit::Hearts);
    let mut game = money_hand(&["Business Card"], king);
    let rolled = game.preview_value(&[0], "roll").1;
    assert!(rolled == 0 || rolled == 2);
    game.preview_money(&[0]);
    let fresh = GameState::new("MONEY001", "Red Deck", 1);
    assert_eq!(game.money, fresh.money);
}

// ==========================================================================
// tests/test_preview_counts_the_play.py -- "A preview counts the hand as played
// before it scores, as the play does."
//
// evaluate_play increments `G.GAME.hands[text].played` before the jokers'
// context.before pass, and Obelisk decides there whether the hand just played is
// now the most played. `_play` has always counted first; `preview_play` did not,
// so it saw the count one play late. With two hands tied, playing one of them
// makes it the most played and Obelisk resets -- and the preview said it grew.
// ==========================================================================

const COUNT_HAND: [Spec; 8] = [
    (Rank::King, Suit::Spades),
    (Rank::King, Suit::Hearts),
    (Rank::Nine, Suit::Clubs),
    (Rank::Seven, Suit::Diamonds),
    (Rank::Five, Suit::Spades),
    (Rank::Three, Suit::Hearts),
    (Rank::Two, Suit::Clubs),
    (Rank::Four, Suit::Diamonds),
];

/// `_tied_with_obelisk`: Obelisk at X2 with Pair and one other hand both played
/// twice, so playing the Pair makes it the most played and Obelisk resets.
fn tied_with_obelisk() -> (GameState, HandType) {
    let mut game = GameState::new("PLOQ83ZX", "Blue Deck", 1);
    let obelisk = joker("Obelisk");
    obelisk.borrow_mut().counter = 2.0;
    game.gain_joker(&obelisk);
    game._start_round();
    game.hand = COUNT_HAND.iter().map(|(r, s)| make_card(*r, *s)).collect();
    let pair = game
        .evaluate_selection(&[game.hand[0].clone(), game.hand[1].clone()])
        .hand;
    let other = HandType::ALL.iter().find(|h| **h != pair).copied().unwrap();
    game.hand_levels.plays.insert(pair, 2);
    game.hand_levels.plays.insert(other, 2);
    (game, pair)
}

#[test]
fn test_the_preview_scores_what_the_play_scores_when_obelisk_resets() {
    let (mut game, _) = tied_with_obelisk();
    // Python deepcopies the run to replay the play; two identically built runs
    // are the same thing.
    let (mut fork, _) = tied_with_obelisk();
    let before = fork.chips_scored;
    fork.step(&Action::with_cards(ActionType::Play, vec![0, 1]));
    let scored = fork.chips_scored - before;
    assert_eq!(
        fork.jokers[0].borrow().counter,
        1.0,
        "the play should reset Obelisk"
    );
    assert_eq!(game.preview_score(&[0, 1], "roll"), scored);
}

#[test]
fn test_the_preview_puts_the_count_back() {
    let (mut game, pair) = tied_with_obelisk();
    game.preview_score(&[0, 1], "roll");
    assert_eq!(*game.hand_levels.plays.get(&pair).unwrap(), 2);
    assert_eq!(game.jokers[0].borrow().counter, 2.0);
}

// ==========================================================================
// tests/test_amber_acorn_shuffle.py -- "Amber Acorn shuffles the joker row three
// times, each from id order."
//
// blind.lua queues three events, each `G.jokers:shuffle('aajk')`, and
// CardArea:shuffle is pseudoshuffle, which sorts the list by sort_id before it
// shuffles. So the order the player left the row in does not matter at all, and
// the row that comes out is the third 'aajk' shuffle of the jokers in id order.
// The simulator shuffled twice and never sorted.
// ==========================================================================

const ACORN_NAMES: [&str; 5] = [
    "Joker",
    "Jolly Joker",
    "Zany Joker",
    "Mad Joker",
    "Crazy Joker",
];

fn acorn_round(reverse_row: bool) -> GameState {
    let mut game = GameState::new("QWERTYUI", "Blue Deck", 1);
    game.endless = true;
    for name in ACORN_NAMES {
        game.gain_joker(&joker(name));
    }
    if reverse_row {
        game.jokers.reverse();
    }
    let acorn = boss_by_name("Amber Acorn").unwrap();
    game.blind = Some(make_blind(BlindKind::Boss, 8, Some(acorn), 1.0, 1, false));
    game._start_round();
    game
}

#[test]
fn test_the_row_the_player_left_does_not_matter() {
    let as_bought = joker_names(&acorn_round(false));
    let dragged = joker_names(&acorn_round(true));
    assert_eq!(as_bought, dragged);
}

#[test]
fn test_it_is_three_shuffles_of_the_id_order() {
    let game = acorn_round(true);
    let mut expected = game.jokers.clone();
    let mut rng = RunRng::new("QWERTYUI");
    for _ in 0..3 {
        expected.sort_by_key(|j| j.borrow().uid);
        rng.shuffle(&mut expected, "aajk");
    }
    assert_eq!(
        joker_names(&game),
        expected
            .iter()
            .map(|j| j.borrow().name().to_string())
            .collect::<Vec<_>>()
    );
}
