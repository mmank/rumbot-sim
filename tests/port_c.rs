//! Tags, vouchers, consumable use, and the pack/shop corners, ported from the
//! Python suite.
//!
//! Each `#[test]` mirrors one `def test_x()` under
//! `external/jimbot-sim/tests/`, grouped here by the file it came from. The doc
//! comments carry the reasoning the Python test recorded -- these were written
//! over months against real divergences, and the comment is often the only
//! statement of *why* the assertion has the shape it does.
//!
//! The port is "direct": the same position is rebuilt with `GameState` and the
//! same numbers asserted. Where the Python test is `parametrize`d, the Rust test
//! loops the same parameter list under the one Python function name. The one
//! test whose assertion cannot be rebuilt from the engine (`test_vouchers.py`'s
//! declared-but-unread scan) reads the Rust sources from `CARGO_MANIFEST_DIR`,
//! the same way the Python one reads the package.

use std::collections::{BTreeSet, HashSet};

use jimbot_sim::blinds::{ante_base_chips, make_blind, BlindKind};
use jimbot_sim::cards::{make_card, CardRef, Edition, Rank, Suit};
use jimbot_sim::consumables::spec_or_panic;
use jimbot_sim::game::{
    tag_by_key, tag_key, Action, ActionType, GameState, PackChoice, Phase, Tag, IMMEDIATE_TAGS,
    TAG_POOL,
};
use jimbot_sim::hands::{HandType, HANDLIST, SECRET_HANDS};
use jimbot_sim::jokers::{self, JokerRef};
use jimbot_sim::shop::{all_vouchers, pack_from_key, voucher_by_key, PackKind, PackSpec};
use jimbot_sim::shop_pool::{build_voucher_pool, UNAVAILABLE};
use jimbot_sim::tag_data::TAG_DATA;

fn run() -> GameState {
    GameState::new("TESTSEED", "Red Deck", 1)
}

fn run_with_tags(tags: &[Tag]) -> GameState {
    let mut game = run();
    game.tags.extend_from_slice(tags);
    game
}

/// A joker as a run holds it: the instance, not the registry entry.
fn joker(name: &str) -> JokerRef {
    jokers::make(name)
}

fn editioned(name: &str, edition: Edition) -> JokerRef {
    let j = jokers::make(name);
    j.borrow_mut().edition = edition;
    j
}

/// `_skip` from test_tags.py: offer the next blind, then take the skip.
fn skip(game: &mut GameState) {
    game._next_blind();
    game.phase = Phase::BlindSelect;
    game.step(&Action::new(ActionType::SkipBlind));
}

fn apply(name: &str, game: &mut GameState) {
    (spec_or_panic(name).apply.expect("a registered consumable has an apply"))(game, &[]);
}

fn hand_targets(game: &GameState, n: usize) -> Vec<CardRef> {
    game.hand.iter().take(n).cloned().collect()
}

// ==========================================================================
// tests/test_tags.py -- "The skip tags, audited the way the jokers,
// consumables and vouchers were."
// ==========================================================================

#[test]
fn test_every_tag_in_the_pool_maps_to_an_effect() {
    // The pool draws all twenty-four; a key with no entry is a lost reward.
    // Seven tags -- Handy, Garbage, Speed, Top-up, Orbital, Voucher and D6 --
    // were once missing from the key map, and a missing tag was not inert: the
    // pool still drew it, the skip still happened, and the reward evaporated.
    let unmapped: Vec<&str> = TAG_DATA
        .iter()
        .map(|row| row.key)
        .filter(|key| tag_by_key(key).is_none())
        .collect();
    assert!(
        unmapped.is_empty(),
        "drawn by the pool and dropped on the floor: {unmapped:?}"
    );
}

#[test]
fn test_the_mapping_names_no_tag_the_game_does_not_have() {
    let keys: HashSet<&str> = TAG_DATA.iter().map(|row| row.key).collect();
    for tag in TAG_POOL {
        assert!(
            keys.contains(tag_key(tag)),
            "the key map names a tag the data table does not: {}",
            tag_key(tag)
        );
    }
}

#[test]
fn test_skipping_a_blind_is_counted() {
    // Nothing incremented this, so Throwback read zero all run.
    let mut game = run();
    assert_eq!(game.blinds_skipped, 0);
    skip(&mut game);
    assert_eq!(game.blinds_skipped, 1);
}

#[test]
fn test_throwback_scores_more_after_a_skip() {
    // X0.25 a skip, read off the run counter every time it scores. The counter
    // was never written, so this joker scored X1 for whole runs.
    let scored = |skips: i32| {
        let mut game = run();
        game.gain_joker(&joker("Throwback"));
        game.blinds_skipped = skips;
        game.hand = vec![make_card(Rank::Two, Suit::Clubs)];
        game.preview_score(&[0], "roll")
    };
    assert!(scored(2) > scored(0));
    assert!(scored(4) > scored(2));

    // and the counter the joker reads is the one a real skip moves
    let mut game = run();
    game.gain_joker(&joker("Throwback"));
    game.hand = vec![make_card(Rank::Two, Suit::Clubs)];
    let flat = game.preview_score(&[0], "roll");
    skip(&mut game);
    skip(&mut game);
    game.hand = vec![make_card(Rank::Two, Suit::Clubs)];
    assert!(game.preview_score(&[0], "roll") > flat);
}

// ------------------------------------------------------------------
// the tags that pay the instant the blind is skipped
// ------------------------------------------------------------------

#[test]
fn test_handy_pays_a_dollar_for_every_hand_played_this_run() {
    let mut game = run_with_tags(&[Tag::Handy]);
    game.hands_played = 7;
    let before = game.money;
    game._fire_immediate_tags();
    assert_eq!(game.money, before + 7);
    assert!(!game.tags.contains(&Tag::Handy));
}

#[test]
fn test_garbage_pays_for_discards_banked_over_the_whole_run() {
    // G.GAME.unused_discards, not this round's leftovers.
    let mut game = run_with_tags(&[Tag::Garbage]);
    game.unused_discards = 9;
    let before = game.money;
    game._fire_immediate_tags();
    assert_eq!(game.money, before + 9);
}

#[test]
fn test_the_discard_meter_banks_at_the_end_of_every_round() {
    let mut game = run();
    game._start_round();
    game.chips_scored = game.blind.as_ref().unwrap().target;
    let left = game.discards_left;
    game._beat_blind(true);
    assert_eq!(game.unused_discards, left);
}

#[test]
fn test_speed_pays_five_a_skip_and_counts_its_own() {
    // skip_blind increments before it hands the tag over.
    let mut game = run();
    game.blinds_skipped = 3;
    game.tags.push(Tag::Speed);
    let before = game.money;
    game._fire_immediate_tags();
    assert_eq!(game.money, before + 15);
}

#[test]
fn test_top_up_makes_two_common_jokers() {
    let mut game = run_with_tags(&[Tag::TopUp]);
    game._fire_immediate_tags();
    assert_eq!(game.jokers.len(), 2);
}

#[test]
fn test_top_up_makes_only_as_many_as_there_is_room_for() {
    // The room is re-checked before each, not once for the pair.
    let mut game = run_with_tags(&[Tag::TopUp]);
    while (game.jokers.len() as i32) < game.joker_slots() - 1 {
        game.gain_joker(&joker("Joker"));
    }
    let before = game.jokers.len();
    game._fire_immediate_tags();
    assert_eq!(game.jokers.len(), before + 1);
}

#[test]
fn test_orbital_levels_one_hand_by_three() {
    let mut game = run_with_tags(&[Tag::Orbital]);
    game._fire_immediate_tags();
    let levelled: Vec<HandType> = HANDLIST
        .into_iter()
        .filter(|h| game.hand_levels.level(*h) != 1)
        .collect();
    assert_eq!(levelled.len(), 1);
    assert_eq!(game.hand_levels.level(levelled[0]), 4);
}

#[test]
fn test_orbital_never_names_a_hand_the_run_has_not_seen() {
    // Same visible-hands pool as To Do List: nine until a secret hand lands.
    let mut seen: HashSet<HandType> = HashSet::new();
    for seed in ["A", "B", "C", "D", "E", "F", "G", "H"] {
        let mut game = GameState::new(seed, "Red Deck", 1);
        game.tags.push(Tag::Orbital);
        game._fire_immediate_tags();
        let named = HANDLIST
            .into_iter()
            .find(|h| game.hand_levels.level(*h) != 1)
            .expect("the Orbital Tag levels a hand");
        assert!(!SECRET_HANDS.contains(&named));
        seen.insert(named);
    }
    assert!(seen.len() > 1, "eight seeds and it named one hand every time");
}

#[test]
fn test_a_double_tag_levels_the_same_hand_twice() {
    // G.orbital_hand is handed to the copy, so both name one hand.
    //
    // Remembering the choice per ante and blind is what reproduces that.
    let mut game = run_with_tags(&[Tag::Orbital, Tag::Orbital]);
    game._fire_immediate_tags();
    let levelled: Vec<HandType> = HANDLIST
        .into_iter()
        .filter(|h| game.hand_levels.level(*h) != 1)
        .collect();
    assert_eq!(levelled.len(), 1);
    assert_eq!(game.hand_levels.level(levelled[0]), 7);
}

// ------------------------------------------------------------------
// the two that wait for the shop
// ------------------------------------------------------------------

#[test]
fn test_a_voucher_tag_puts_a_second_voucher_in_the_shop() {
    let mut game = run();
    game._roll_voucher();
    game.tags.push(Tag::Voucher);
    game._open_shop();
    let offered = &game.shop.as_ref().unwrap().vouchers;
    assert_eq!(offered.len(), 2);
    assert_ne!(offered[0].key, offered[1].key, "the shop must not repeat one");
}

#[test]
fn test_both_shop_vouchers_can_be_bought() {
    let mut game = run();
    game._roll_voucher();
    game.tags.push(Tag::Voucher);
    game._open_shop();
    game.money = 100;
    let offers = game.shop.as_ref().unwrap().vouchers.clone();
    let (first, second) = (offers[0], offers[1]);

    game.step(&Action::at(ActionType::BuyVoucher, 0));
    assert!(game.vouchers.iter().any(|v| v.key == first.key));
    let offered = &game.shop.as_ref().unwrap().vouchers;
    assert_eq!(offered.len(), 1);
    assert_eq!(offered[0].key, second.key);

    game.step(&Action::at(ActionType::BuyVoucher, 0));
    assert!(game.vouchers.iter().any(|v| v.key == second.key));
    assert!(game.shop.as_ref().unwrap().vouchers.is_empty());
}

#[test]
fn test_a_d6_tag_makes_the_first_reroll_free_and_the_next_cheap() {
    // temp_reroll_cost = 0: the price starts at nothing and climbs as usual.
    //
    // Not the same as Chaos the Clown, which is one spare reroll that does not
    // move the price at all.
    let mut plain = run();
    plain._open_shop();
    assert_eq!(plain.shop.as_ref().unwrap().reroll_cost(0), 5);

    let mut game = run();
    game.tags.push(Tag::DSix);
    game._open_shop();
    assert_eq!(game.shop.as_ref().unwrap().reroll_cost(0), 0);
    game.shop.as_mut().unwrap().rerolls += 1;
    assert_eq!(game.shop.as_ref().unwrap().reroll_cost(0), 1);
}

#[test]
fn test_the_shop_tags_are_spent_when_they_fire() {
    let mut game = run();
    game._roll_voucher();
    game.tags.extend_from_slice(&[Tag::Voucher, Tag::DSix]);
    game._open_shop();
    assert!(!game.tags.contains(&Tag::Voucher));
    assert!(!game.tags.contains(&Tag::DSix));
}

#[test]
fn test_the_immediate_list_holds_only_tags_that_pay_on_the_skip() {
    let actual: HashSet<Tag> = IMMEDIATE_TAGS.into_iter().collect();
    let expected: HashSet<Tag> =
        [Tag::Handy, Tag::Garbage, Tag::Speed, Tag::TopUp, Tag::Orbital]
            .into_iter()
            .collect();
    assert_eq!(actual, expected);
}

// ------------------------------------------------------------------
// stacking, which is not a corner case
// ------------------------------------------------------------------

#[test]
fn test_every_double_tag_copies_the_next_one_not_just_the_first() {
    // add_tag walks the whole list firing tag_add and never breaks.
    //
    // Marcin's route to a stack: sell a pile of Diet Colas, each of which
    // leaves a free Double Tag behind. The Anaglyph Deck gets there without
    // any jokers at all -- a Double every time a boss falls.
    let mut game = run();
    game.tags.extend_from_slice(&[Tag::Double; 4]);
    game.add_tag_by_key("tag_voucher");
    assert_eq!(game.tags.iter().filter(|t| **t == Tag::Voucher).count(), 5);
    assert!(
        !game.tags.contains(&Tag::Double),
        "all four are spent, not one"
    );
}

#[test]
fn test_a_double_tag_does_not_copy_another_double_tag() {
    let mut game = run();
    game.tags.push(Tag::Double);
    game.add_tag_by_key("tag_double");
    assert_eq!(game.tags.iter().filter(|t| **t == Tag::Double).count(), 2);
}

#[test]
fn test_the_anaglyph_deck_hands_over_a_double_tag_after_each_boss() {
    let mut game = GameState::new("TESTSEED", "Anaglyph Deck", 1);
    game.blind_index = 2;
    game._next_blind();
    game._start_round();
    game.chips_scored = game.blind.as_ref().unwrap().target;
    game._beat_blind(true);
    assert!(game.tags.contains(&Tag::Double));
}

#[test]
fn test_a_shop_can_hold_an_arbitrary_number_of_vouchers() {
    // One row, one card limit, raised by one for each Voucher Tag held.
    let mut game = run();
    game._roll_voucher();
    game.tags.extend_from_slice(&[Tag::Voucher; 5]);
    game._open_shop();

    let offered = &game.shop.as_ref().unwrap().vouchers;
    assert_eq!(offered.len(), 6, "the round's own plus one per tag");
    let keys: BTreeSet<&str> = offered.iter().map(|v| v.key).collect();
    assert_eq!(
        keys.len(),
        offered.len(),
        "each draw withholds the row so far"
    );
    assert!(!game.tags.contains(&Tag::Voucher));
}

#[test]
fn test_buying_from_a_long_voucher_row_takes_the_right_one() {
    let mut game = run();
    game._roll_voucher();
    game.tags.extend_from_slice(&[Tag::Voucher; 3]);
    game._open_shop();
    game.money = 100;

    let wanted = game.shop.as_ref().unwrap().vouchers[2];
    game.step(&Action::at(ActionType::BuyVoucher, 2));
    assert!(game.vouchers.iter().any(|v| v.key == wanted.key));
    assert!(!game
        .shop
        .as_ref()
        .unwrap()
        .vouchers
        .iter()
        .any(|v| v.key == wanted.key));
    assert_eq!(game.shop.as_ref().unwrap().vouchers.len(), 3);
}

#[test]
fn test_juggle_tags_stack_and_last_one_round() {
    let mut game = run();
    let plain = game.hand_size();
    game.tags.extend_from_slice(&[Tag::Juggle; 3]);
    game._next_blind();
    game._start_round();
    assert_eq!(game.hand_size(), plain + 9, "three tags, nine cards");

    game.chips_scored = game.blind.as_ref().unwrap().target;
    game._beat_blind(true);
    assert_eq!(
        game.hand_size(),
        plain,
        "and they are handed back at round end"
    );
}

#[test]
fn test_every_investment_tag_pays() {
    let mut game = run();
    game.blind_index = 2;
    game._next_blind();
    game._start_round();
    game.tags.extend_from_slice(&[Tag::Investment; 3]);
    game.chips_scored = game.blind.as_ref().unwrap().target;
    let before = game.money;
    game._beat_blind(true);
    // A cash-out row, not money the moment the boss falls.
    assert_eq!(game.money, before);
    assert!(game.pending_payout >= 75);
}

#[test]
fn test_uncommon_tags_stack_across_the_shop_slots() {
    let mut game = run();
    game._roll_voucher();
    game.tags.extend_from_slice(&[Tag::Uncommon; 2]);
    game._open_shop();
    // Free by coupon rather than by price: set_cost zeroes a couponed
    // card after working the price out, and the tag's joker is couponed.
    let free = game
        .shop
        .as_ref()
        .unwrap()
        .slots
        .iter()
        .filter(|s| s.kind == "joker" && game.slot_price(s) == 0)
        .count();
    assert_eq!(free, 2);
    assert!(!game.tags.contains(&Tag::Uncommon));
}

// ------------------------------------------------------------------
// the Anaglyph + Negative Tag engine
// ------------------------------------------------------------------

#[test]
fn test_a_negative_tag_makes_a_shop_joker_negative_and_free() {
    let mut game = run();
    game.tags.push(Tag::Negative);
    game._roll_voucher();
    game._open_shop();

    let marked: Vec<usize> = game
        .shop
        .as_ref()
        .unwrap()
        .slots
        .iter()
        .enumerate()
        .filter(|(_, s)| {
            s.joker
                .as_ref()
                .is_some_and(|j| j.borrow().edition == Edition::Negative)
        })
        .map(|(i, _)| i)
        .collect();
    assert_eq!(marked.len(), 1);
    let slot = game.shop.as_ref().unwrap().slots[marked[0]].clone();
    assert_eq!(game.slot_price(&slot), 0, "the tag coupons it as well");
}

#[test]
fn test_a_negative_joker_does_not_take_a_slot() {
    let mut game = run();
    let base = game.joker_slots();
    game.gain_joker(&editioned("Joker", Edition::Negative));
    assert_eq!(game.joker_slots(), base + 1);
    assert_eq!(game.jokers.len(), 1);
}

#[test]
fn test_a_debuffed_negative_joker_keeps_its_slot() {
    // remove_from_deck(from_debuff) sets queue_negative_removal instead.
    //
    // The negative block is the one thing a debuff deliberately does not undo,
    // which is why joker_slots counts every joker rather than the active ones.
    let mut game = run();
    let base = game.joker_slots();
    let joker = editioned("Joker", Edition::Negative);
    game.gain_joker(&joker);
    joker.borrow_mut().debuffed = true;
    assert_eq!(game.joker_slots(), base + 1);
}

#[test]
fn test_the_anaglyph_negative_loop_compounds() {
    // Marcin: people end up with an absurd number of jokers this way.
    //
    // A boss falls, Anaglyph leaves a Double Tag, the next Negative Tag is
    // doubled, each copy marks a shop joker negative and free, and each
    // negative joker bought raises the joker limit -- so the row grows without
    // ever spending a slot. Tags that find no joker in the shop wait for the
    // next one rather than being lost.
    let mut game = GameState::new("TESTSEED", "Anaglyph Deck", 1);
    game.money = 500;
    let base = game.joker_slots();

    for _ in 0..5 {
        game.blind_index = 2;
        game._next_blind();
        game._start_round();
        game.chips_scored = game.blind.as_ref().unwrap().target;
        game._beat_blind(true);
        game.add_tag_by_key("tag_negative");
        game._roll_voucher();
        game._open_shop();
        let count = game.shop.as_ref().unwrap().slots.len();
        for i in (0..count).rev() {
            let (is_neg, room) = {
                let slot = &game.shop.as_ref().unwrap().slots[i];
                (
                    slot.joker
                        .as_ref()
                        .is_some_and(|j| j.borrow().edition == Edition::Negative),
                    (game.jokers.len() as i32) < game.joker_slots(),
                )
            };
            if is_neg && room {
                game.step(&Action::at(ActionType::Buy, i as i32));
            }
        }
        game.shop = None;
    }

    assert!(game.joker_slots() > base, "the limit never moved");
    assert_eq!(
        game.jokers.len() as i32,
        game.joker_slots() - base,
        "every joker bought was negative, so none of them cost a slot"
    );
    assert!(
        game.tags.iter().filter(|t| **t == Tag::Negative).count() > 0,
        "tags with no joker to mark should wait, not evaporate"
    );
}

#[test]
fn test_perkeos_copy_does_not_take_a_consumable_slot() {
    // The same mechanism as the negative joker.
    //
    // add_to_deck raises G.consumeables' card limit for a negative consumable
    // exactly as it raises the joker limit for a negative joker, and Perkeo's
    // whole point is that its copy is free. The row used to hold shared
    // registry entries, so there was nowhere to record the edition and the copy
    // took a slot like any other card.
    let mut game = run();
    game.gain_joker(&joker("Perkeo"));
    let fool = game.hold_consumable(spec_or_panic("The Fool"), Edition::None);
    game.consumables.push(fool);
    let base = game.consumable_slots();

    game._leave_shop();
    assert_eq!(game.consumables.len(), 2);
    assert_eq!(game.consumable_slots(), base + 1);
    let editions: Vec<Edition> = game
        .consumables
        .iter()
        .map(|c| c.borrow().edition)
        .collect();
    assert_eq!(editions, vec![Edition::None, Edition::Negative]);
}

#[test]
fn test_perkeo_copies_into_a_row_that_is_already_full() {
    // A full row is exactly when the copy being free matters.
    let mut game = run();
    game.gain_joker(&joker("Perkeo"));
    while (game.consumables.len() as i32) < game.consumable_slots() {
        let fool = game.hold_consumable(spec_or_panic("The Fool"), Edition::None);
        game.consumables.push(fool);
    }
    let held = game.consumables.len();

    game._leave_shop();
    assert_eq!(game.consumables.len(), held + 1);
}

#[test]
fn test_using_the_negative_copy_gives_the_slot_back() {
    // remove_from_deck lowers the limit again, so the credit is not permanent.
    let mut game = run();
    game.gain_joker(&joker("Perkeo"));
    let fool = game.hold_consumable(spec_or_panic("The Fool"), Edition::None);
    game.consumables.push(fool);
    let base = game.consumable_slots();
    game._leave_shop();
    assert_eq!(game.consumable_slots(), base + 1);

    let negative = game
        .consumables
        .iter()
        .position(|c| c.borrow().edition == Edition::Negative)
        .expect("Perkeo's copy is negative");
    game.consumables.remove(negative);
    assert_eq!(game.consumable_slots(), base);
}

// ==========================================================================
// tests/test_vouchers.py -- "The vouchers, audited the way the joker flags
// and the consumables were."
// ==========================================================================

/// `_run(*keys)` from test_vouchers.py: redeem these and hand back the run.
fn voucher_run(keys: &[&str]) -> GameState {
    let mut game = run();
    for key in keys {
        let voucher = voucher_by_key(key).unwrap_or_else(|| panic!("no voucher {key}"));
        game.vouchers.push(voucher);
        game._redeem_voucher(voucher);
    }
    game
}

/// `GameState.discount_percent`: Clearance Sale and Liquidation, the biggest won.
fn discount_percent(game: &GameState) -> i32 {
    game.vouchers
        .iter()
        .map(|v| v.discount_percent)
        .max()
        .unwrap_or(0)
}

fn shop_rates(game: &GameState) -> std::collections::HashMap<String, f64> {
    game._shop_rates()
}

// ------------------------------------------------------------------
// set, not add
// ------------------------------------------------------------------

#[test]
fn test_an_upgrade_replaces_its_base_rate_rather_than_stacking() {
    // G.GAME.tarot_rate = 4*extra, an assignment. Summing gives 41.6.
    assert_eq!(shop_rates(&voucher_run(&["v_tarot_merchant"]))["Tarot"], 9.6);
    let both = voucher_run(&["v_tarot_merchant", "v_tarot_tycoon"]);
    assert_eq!(shop_rates(&both)["Tarot"], 32.0);
}

#[test]
fn test_the_planet_pair_behaves_the_same_way() {
    let both = voucher_run(&["v_planet_merchant", "v_planet_tycoon"]);
    assert_eq!(shop_rates(&both)["Planet"], 32.0);
}

#[test]
fn test_liquidation_replaces_clearance_sale() {
    assert_eq!(discount_percent(&voucher_run(&["v_clearance_sale"])), 25);
    let both = voucher_run(&["v_clearance_sale", "v_liquidation"]);
    assert_eq!(discount_percent(&both), 50);
}

#[test]
fn test_glow_up_replaces_hone() {
    assert_eq!(voucher_run(&["v_hone"]).edition_rate(), 2.0);
    assert_eq!(voucher_run(&["v_hone", "v_glow_up"]).edition_rate(), 4.0);
}

#[test]
fn test_money_tree_replaces_seed_money() {
    // The cap is in five-dollar blocks: $50 is 10, $100 is 20.
    assert_eq!(voucher_run(&["v_seed_money"]).interest_cap(), 10);
    assert_eq!(voucher_run(&["v_seed_money", "v_money_tree"]).interest_cap(), 20);
}

// ------------------------------------------------------------------
// add, not set
// ------------------------------------------------------------------

#[test]
fn test_the_hand_and_discard_pairs_stack() {
    let plain = run().round_allowance(true);
    let both = voucher_run(&["v_grabber", "v_nacho_tong"]).round_allowance(true);
    assert_eq!(both.0, plain.0 + 2);

    let discards = voucher_run(&["v_wasteful", "v_recyclomancy"]).round_allowance(true);
    assert_eq!(discards.1, plain.1 + 2);
}

#[test]
fn test_paint_brush_and_palette_stack() {
    let plain = run().hand_size();
    assert_eq!(voucher_run(&["v_paint_brush"]).hand_size(), plain + 1);
    assert_eq!(
        voucher_run(&["v_paint_brush", "v_palette"]).hand_size(),
        plain + 2
    );
}

#[test]
fn test_overstock_and_overstock_plus_stack() {
    let plain = run()._shop_slot_count();
    assert_eq!(voucher_run(&["v_overstock_norm"])._shop_slot_count(), plain + 1);
    let both = voucher_run(&["v_overstock_norm", "v_overstock_plus"]);
    assert_eq!(both._shop_slot_count(), plain + 2);
}

#[test]
fn test_a_voucher_hands_over_its_extra_for_the_round_in_progress() {
    // ease_hands_played fires on redemption, not only from the next round.
    let mut game = run();
    game._start_round();
    let before = game.hands_left;
    let voucher = voucher_by_key("v_grabber").unwrap();
    game.vouchers.push(voucher);
    game._redeem_voucher(voucher);
    assert_eq!(game.hands_left, before + 1);
}

// ------------------------------------------------------------------
// Hieroglyph and Petroglyph
// ------------------------------------------------------------------

#[test]
fn test_hieroglyph_costs_an_ante_and_a_hand() {
    let plain = run();
    let game = voucher_run(&["v_hieroglyph"]);
    assert_eq!(game.ante, plain.ante - 1);
    assert_eq!(
        game.round_allowance(true).0,
        plain.round_allowance(true).0 - 1
    );
}

#[test]
fn test_petroglyph_costs_an_ante_and_a_discard() {
    let plain = run();
    let game = voucher_run(&["v_hieroglyph", "v_petroglyph"]);
    assert_eq!(game.ante, plain.ante - 2);
    assert_eq!(
        game.round_allowance(true).1,
        plain.round_allowance(true).1 - 1
    );
}

#[test]
fn test_the_ante_really_does_go_below_one() {
    // ease_ante is a bare addition with no floor.
    //
    // Measured: two Hieroglyphs from ante one leave the engine at minus one,
    // and get_blind_amount returns 100 for anything under one. A clamp at one
    // is not a safety net -- it asks 300 where the engine asks 100, and it
    // names every ante-keyed pool wrongly, which moves the whole shop stream.
    let game = voucher_run(&["v_hieroglyph"]);
    assert_eq!(game.ante, 0);
    let game2 = voucher_run(&["v_hieroglyph", "v_petroglyph"]);
    assert_eq!(game2.ante, -1);

    assert_eq!(ante_base_chips(0, 1), 100);
    assert_eq!(ante_base_chips(-1, 1), 100);
    assert_eq!(ante_base_chips(1, 1), 300);
}

// ------------------------------------------------------------------
// the pool
// ------------------------------------------------------------------

#[test]
fn test_an_upgrade_is_not_offered_before_its_base_is_redeemed() {
    let pool = build_voucher_pool(&[] as &[&str], &[] as &[&str]);
    let keys: HashSet<&str> = all_vouchers().iter().map(|v| v.key).collect();
    let gated: HashSet<&str> = all_vouchers()
        .iter()
        .filter(|v| !v.requires.is_empty())
        .map(|v| v.key)
        .collect();
    let live: HashSet<String> = pool
        .iter()
        .filter(|k| *k != UNAVAILABLE)
        .cloned()
        .collect();
    let expected: HashSet<String> = keys
        .difference(&gated)
        .map(|k| k.to_string())
        .collect();
    assert_eq!(live, expected);

    let with_base = build_voucher_pool(&["v_hieroglyph"] as &[&str], &[] as &[&str]);
    assert!(with_base.iter().any(|k| k == "v_petroglyph"));
    assert!(
        !with_base.iter().any(|k| k == "v_hieroglyph"),
        "redeemed cannot come again"
    );
}

#[test]
fn test_a_voucher_already_on_offer_is_withheld() {
    // Which is what a Voucher Tag's second slot needs.
    let plain = build_voucher_pool(&[] as &[&str], &[] as &[&str]);
    assert!(plain.iter().any(|k| k == "v_blank"));
    let offered = build_voucher_pool(&[] as &[&str], &["v_blank"] as &[&str]);
    assert!(!offered.iter().any(|k| k == "v_blank"));
}

#[test]
fn test_every_voucher_either_carries_a_field_or_is_read_by_key() {
    // The declared-but-unread check, turned on the vouchers.
    //
    // Blank is the only one that legitimately does nothing. The other four
    // without a field are read straight out of used_vouchers where they act --
    // Telescope and Omen Globe in pack contents, Observatory in scoring,
    // Director's Cut and Retcon at the boss reroll.
    let read_by_key = [
        "v_telescope",
        "v_observatory",
        "v_omen_globe",
        "v_directors_cut",
        "v_retcon",
    ];

    // The Python test scans its package; the Rust one scans its crate.
    let src_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files: Vec<std::path::PathBuf> = std::fs::read_dir(&src_dir)
        .expect("src/ is readable")
        .map(|entry| entry.expect("a dir entry").path())
        .filter(|p| p.extension().is_some_and(|e| e == "rs"))
        .collect();
    files.sort();
    let body: String = files
        .iter()
        .map(|p| std::fs::read_to_string(p).expect("a source file is readable"))
        .collect::<Vec<_>>()
        .join("\n");

    // Blank is allowed to be here. It does nothing to a run by design, and
    // its real jobs are elsewhere: gating Antimatter through the requires
    // chain, and counting toward Antimatter's profile unlock, which this
    // simulator does not model at all.
    let mut inert: Vec<&str> = Vec::new();
    for voucher in all_vouchers() {
        let carries_a_field = voucher.shop_slots != 0
            || voucher.consumable_slots != 0
            || voucher.extra_hands != 0
            || voucher.extra_discards != 0
            || voucher.reroll_discount != 0
            || voucher.discount_percent != 0
            || voucher.interest_cap != 0
            || voucher.joker_slots != 0
            || voucher.hand_size != 0
            || voucher.ante_shift != 0
            || voucher.tarot_rate != 0.0
            || voucher.planet_rate != 0.0
            || voucher.edition_rate != 0.0
            || voucher.playing_card_rate != 0.0;
        if carries_a_field {
            continue;
        }
        if read_by_key.contains(&voucher.key) {
            assert!(
                body.contains(voucher.key),
                "{} is read by key nowhere",
                voucher.key
            );
            continue;
        }
        inert.push(voucher.name);
    }

    assert_eq!(
        inert,
        vec!["Blank"],
        "a voucher that does nothing at all"
    );
}

// ------------------------------------------------------------------
// what a discount actually does to a price
// ------------------------------------------------------------------

#[test]
fn test_a_discount_floors_the_price_rather_than_rounding_it() {
    // Card:set_cost, card.lua:375:
    //
    //     cost = max(1, floor((base_cost + extra_cost + 0.5) * (100 - d)/100))
    //
    // A ten dollar voucher under Clearance Sale is seven, not the seven and a
    // half that rounds to eight. Vouchers and packs went through a `price` that
    // rounded, so a live run against the real game stopped on the dollar.
    let game = voucher_run(&["v_clearance_sale"]);
    assert_eq!(game.price(10), 7);
    assert_eq!(game.price(4), 3);
    // A dollar is the floor, however deep the discount.
    let deep = voucher_run(&["v_clearance_sale", "v_liquidation"]);
    assert_eq!(deep.price(1), 1);
}

#[test]
fn test_a_discount_redeemed_in_a_shop_reprices_what_is_still_on_the_shelf() {
    // set_cost runs over every card on screen, not once at stocking time.
    //
    // Clearance Sale is bought *from* a shop, and the cards beside it are
    // cheaper the moment it lands. Pricing at stocking time meant the run paid
    // the old price for everything it bought afterwards.
    let mut game = run();
    game._open_shop();
    let before: Vec<i32> = game
        .shop
        .as_ref()
        .unwrap()
        .slots
        .iter()
        .map(|s| game.slot_price(s))
        .collect();
    let voucher = voucher_by_key("v_clearance_sale").unwrap();
    game.vouchers.push(voucher);
    game._redeem_voucher(voucher);
    let after: Vec<i32> = game
        .shop
        .as_ref()
        .unwrap()
        .slots
        .iter()
        .map(|s| game.slot_price(s))
        .collect();
    assert_ne!(after, before);
    assert!(after.iter().zip(&before).all(|(a, b)| a <= b));
    for (index, slot) in game.shop.as_ref().unwrap().slots.iter().enumerate() {
        let now = after[index];
        let rental = slot.joker.as_ref().is_some_and(|j| j.borrow().rental);
        if slot.couponed || rental {
            continue;
        }
        let extra = if let Some(joker) = &slot.joker {
            jokers::edition_value(joker.borrow().edition)
        } else if let Some(card) = &slot.card {
            jokers::edition_value(card.borrow().edition)
        } else {
            0
        };
        let scaled = (slot.base_cost + extra) as f64 + 0.5;
        let expected = ((scaled * 0.75) as i32).max(1);
        assert_eq!(now, expected);
    }
}

#[test]
fn test_interest_is_paid_on_what_is_left_after_the_rent() {
    // state_events.lua:99-109 charges rent inside the end-of-round pass, and
    // update_round_eval builds the payout rows afterwards -- so the interest
    // row reads a balance the rent has already come out of.
    //
    // Two rentals on a five dollar balance is the case that showed it: the game
    // paid no interest and the simulator paid a dollar of it, which a live run
    // against the engine carried for the rest of the run.
    let payout = |rentals: i32| {
        let mut game = run();
        for _ in 0..rentals {
            let j = joker("Joker");
            j.borrow_mut().rental = true;
            game.gain_joker(&j);
        }
        game._start_round();
        game.money = 5;
        game.hands_left = 0;
        game.discards_left = 0;
        game.blind = Some(make_blind(BlindKind::Small, 1, None, 1.0, 1, false));
        game._beat_blind(true);
        game.pending_payout
    };

    // $5 with no rent: one block of five, so a dollar of interest on top of
    // the small blind's three.
    assert_eq!(payout(0), 3 + 1);
    // Two rentals take six off the five before the row is built.
    assert_eq!(payout(2), 3);
}

// ==========================================================================
// tests/test_consumable_use.py -- "When the game lets a consumable be used,
// and what it costs when it is."
// ==========================================================================

/// `_run` from test_consumable_use.py: a round, then the phase under test.
fn use_run(joker_names: &[&str], phase: Phase) -> GameState {
    let mut game = GameState::new("TESTSEED", "Red Deck", 1);
    for name in joker_names {
        game.gain_joker(&joker(name));
    }
    game._start_round();
    game.phase = phase;
    game
}

fn can_use(game: &GameState, name: &str, targets: &[CardRef]) -> bool {
    game.can_use_consumable(spec_or_panic(name), targets)
}

// Measured "shop" column. Everything else in the roster came back no.
const USABLE_IN_A_SHOP: [&str; 14] = [
    "The Fool",
    "The High Priestess",
    "The Emperor",
    "The Hermit",
    "The Wheel of Fortune",
    "Temperance",
    "Judgement",
    "Wraith",
    "Ectoplasm",
    "Ankh",
    "Hex",
    "The Soul",
    "Black Hole",
    "Pluto",
];

const REFUSED_IN_A_SHOP: [&str; 27] = [
    "The Magician",
    "The Empress",
    "The Hierophant",
    "Strength",
    "The Hanged Man",
    "Death",
    "The Star",
    "The Moon",
    "The Sun",
    "The World",
    "Familiar",
    "Grim",
    "Incantation",
    "Sigil",
    "Ouija",
    "Immolate",
    "The Lovers",
    "The Chariot",
    "Justice",
    "The Devil",
    "The Tower",
    "Talisman",
    "Aura",
    "Deja Vu",
    "Trance",
    "Medium",
    "Cryptid",
];

#[test]
fn test_the_consumables_a_shop_allows() {
    for name in USABLE_IN_A_SHOP {
        let mut game = use_run(&["Joker"], Phase::Shop);
        game.last_tarot_planet = "c_death".to_string();
        assert!(can_use(&game, name, &[]), "{name} should be usable in a shop");
    }
}

#[test]
fn test_the_consumables_a_shop_refuses() {
    // They select cards, and the shop has no hand to select from.
    //
    // An Arcana or Spectral pack is the exception the game builds for exactly
    // this: it deals a hand when it opens so its cards have somewhere to land.
    for name in REFUSED_IN_A_SHOP {
        let game = use_run(&["Joker"], Phase::Shop);
        let spec = spec_or_panic(name);
        let n = spec.targets.max(1) as usize;
        let targets = hand_targets(&game, n);
        assert!(!can_use(&game, name, &targets), "{name} should be refused");
    }
}

#[test]
fn test_a_pack_gives_a_targeting_tarot_somewhere_to_land() {
    let game = use_run(&["Joker"], Phase::Pack);
    let targets = hand_targets(&game, 1);
    assert!(can_use(&game, "The Magician", &targets));
}

#[test]
fn test_the_joker_makers_need_a_free_slot() {
    for name in ["Judgement", "The Soul", "Wraith"] {
        let mut game = use_run(&[], Phase::Playing);
        assert!(can_use(&game, name, &[]));
        while (game.jokers.len() as i32) < game.joker_slots() {
            game.gain_joker(&joker("Joker"));
        }
        assert!(!can_use(&game, name, &[]));
    }
}

#[test]
fn test_the_consumable_makers_may_use_their_own_slot() {
    // `or self.area == G.consumeables` -- using it frees the slot it sits in.
    for name in ["The Emperor", "The High Priestess"] {
        let mut game = use_run(&[], Phase::Playing);
        let held = game.hold_consumable(spec_or_panic(name), Edition::None);
        game.consumables.push(held);
        while (game.consumables.len() as i32) < game.consumable_slots() {
            let fool = game.hold_consumable(spec_or_panic("The Fool"), Edition::None);
            game.consumables.push(fool);
        }
        assert!(can_use(&game, name, &[]));
    }
}

#[test]
fn test_they_need_a_joker_with_no_edition() {
    // Engine: no jokers = no, all foil = no, one plain among them = yes.
    for name in ["Ectoplasm", "Hex", "The Wheel of Fortune"] {
        let mut game = use_run(&[], Phase::Playing);
        assert!(!can_use(&game, name, &[]), "an empty row has nothing to edition");

        for _ in 0..3 {
            game.gain_joker(&editioned("Joker", Edition::Foil));
        }
        assert!(!can_use(&game, name, &[]), "every joker already has an edition");

        game.gain_joker(&joker("Joker"));
        assert!(can_use(&game, name, &[]));
    }
}

#[test]
fn test_aura_refuses_a_card_that_already_has_an_edition() {
    // `(not G.hand.highlighted[1].edition)` -- its own branch, its own rule.
    let game = use_run(&[], Phase::Playing);
    let card = game.hand[0].clone();
    assert!(can_use(&game, "Aura", &[card.clone()]));
    card.borrow_mut().edition = Edition::Polychrome;
    assert!(!can_use(&game, "Aura", &[card]));
}

#[test]
fn test_the_random_destroyers_want_a_card_to_spare() {
    // `#G.hand.cards > 1`. They eat a card chosen at random.
    for name in ["Familiar", "Grim", "Incantation", "Immolate", "Sigil", "Ouija"] {
        let mut game = use_run(&[], Phase::Playing);
        assert!(can_use(&game, name, &[]));
        game.hand.truncate(1);
        assert!(!can_use(&game, name, &[]));
    }
}

#[test]
fn test_the_fool_needs_something_to_copy() {
    let mut game = use_run(&[], Phase::Playing);
    game.last_tarot_planet = String::new();
    assert!(!can_use(&game, "The Fool", &[]));
    game.last_tarot_planet = "c_death".to_string();
    assert!(can_use(&game, "The Fool", &[]));
}

#[test]
fn test_the_fool_will_not_copy_itself() {
    let mut game = use_run(&[], Phase::Playing);
    game.last_tarot_planet = "c_fool".to_string();
    assert!(!can_use(&game, "The Fool", &[]));
}

#[test]
fn test_ankh_asks_only_for_a_joker_which_is_the_bug() {
    // can_use_consumeable enables it; check_use then refuses a full row.
    //
    // The two disagree, and the disagreement is reachable: buy-and-use an Ankh
    // with a full joker row and the card is charged for, removed, and filed
    // nowhere. Keeping the gate loose here is what makes refuses_use reachable.
    let mut game = use_run(&[], Phase::Playing);
    assert!(!can_use(&game, "Ankh", &[]), "no jokers at all");
    game.gain_joker(&joker("Joker"));
    while (game.jokers.len() as i32) < game.joker_slots() {
        game.gain_joker(&joker("Joker"));
    }
    assert!(can_use(&game, "Ankh", &[]), "the button is live even with a full row");
    assert!(
        game.refuses_use(spec_or_panic("Ankh")),
        "and then it says No Room"
    );
}

// ------------------------------------------------------------------
// what a use costs
// ------------------------------------------------------------------

#[test]
fn test_ectoplasm_costs_more_hand_size_every_time() {
    // Engine: eight becomes seven, and ecto_minus is left reading two.
    let mut game = use_run(&[], Phase::Playing);
    for _ in 0..3 {
        game.gain_joker(&joker("Joker"));
    }
    let start = game.hand_size();

    apply("Ectoplasm", &mut game);
    assert_eq!(game.hand_size(), start - 1);
    apply("Ectoplasm", &mut game);
    assert_eq!(game.hand_size(), start - 3); // a further two
    apply("Ectoplasm", &mut game);
    assert_eq!(game.hand_size(), start - 6); // and a further three
}

#[test]
fn test_ouija_costs_a_flat_one_unlike_ectoplasm() {
    let mut game = use_run(&[], Phase::Playing);
    let start = game.hand_size();
    apply("Ouija", &mut game);
    apply("Ouija", &mut game);
    assert_eq!(game.hand_size(), start - 2);
}

#[test]
fn test_the_legal_action_list_agrees_with_the_gate() {
    // Whatever else changes, these two must not drift apart.
    let mut game = use_run(&["Joker"], Phase::Playing);
    game.hand.clear();
    game._open_shop();
    game.phase = Phase::Shop;
    game.consumables = vec![
        game.hold_consumable(spec_or_panic("The Magician"), Edition::None),
        game.hold_consumable(spec_or_panic("Judgement"), Edition::None),
        game.hold_consumable(spec_or_panic("Black Hole"), Edition::None),
    ];
    for action in game.legal_actions() {
        assert!(game.is_legal(&action), "{action}");
    }
    let offered: HashSet<i32> = game
        .legal_actions()
        .iter()
        .filter(|a| a.r#type == ActionType::UseConsumable)
        .map(|a| a.index)
        .collect();
    assert!(
        !offered.contains(&0),
        "The Magician selects cards; there is no hand"
    );
    assert!(offered.contains(&1) && offered.contains(&2));
}

// ==========================================================================
// tests/test_astronomer_celestial_packs.py -- "Astronomer makes a Celestial
// pack free, and the offer has to know it."
// ==========================================================================

fn astro_shop(money: i32, astronomer: bool) -> GameState {
    let mut game = GameState::new("HELLO123", "Blue Deck", 1);
    game._open_shop();
    game.money = money;
    if astronomer {
        game.gain_joker(&joker("Astronomer"));
    }
    game.shop.as_mut().unwrap().packs = vec![
        PackSpec {
            kind: PackKind::Celestial,
            size: "normal",
            options: 3,
            picks: 1,
            cost: 4,
            key: "p_celestial_normal_1",
        },
        PackSpec {
            kind: PackKind::Arcana,
            size: "normal",
            options: 3,
            picks: 1,
            cost: 4,
            key: "p_arcana_normal_1",
        },
    ];
    game
}

fn pack_buys(game: &GameState) -> Vec<i32> {
    let mut out: Vec<i32> = game
        .legal_actions()
        .iter()
        .filter(|a| a.r#type == ActionType::BuyPack)
        .map(|a| a.index)
        .collect();
    out.sort_unstable();
    out
}

fn legal_packs(game: &GameState) -> Vec<i32> {
    (0..game.shop.as_ref().unwrap().packs.len() as i32)
        .filter(|i| game.is_legal(&Action::at(ActionType::BuyPack, *i)))
        .collect()
}

#[test]
fn test_the_free_pack_is_offered_below_its_list_price() {
    let game = astro_shop(3, true);
    let packs = game.shop.as_ref().unwrap().packs.clone();
    assert_eq!(game.pack_price(&packs[0]), 0);
    assert_eq!(game.pack_price(&packs[1]), 4);
    assert_eq!(pack_buys(&game), vec![0], "only the Celestial pack is free at $3");
}

#[test]
fn test_buying_it_costs_nothing() {
    let mut game = astro_shop(3, true);
    let action = game
        .legal_actions()
        .into_iter()
        .find(|a| a.r#type == ActionType::BuyPack && a.index == 0)
        .expect("the free pack is offered");
    game.step(&action);
    assert_eq!(game.money, 3);
}

#[test]
fn test_without_astronomer_it_costs_the_list_price() {
    let game = astro_shop(3, false);
    let pack = game.shop.as_ref().unwrap().packs[0];
    assert_eq!(game.pack_price(&pack), 4);
    assert_eq!(pack_buys(&game), Vec::<i32>::new());
}

#[test]
fn test_is_legal_takes_the_free_pack_below_its_list_price() {
    // is_legal prices the pack as the offer does, not at its list price.
    //
    // The game's button is G.FUNCS.can_open (functions/button_callbacks.lua:111-119):
    //
    //     if (e.config.ref_table.cost) > 0 and
    //        (e.config.ref_table.cost > G.GAME.dollars - G.GAME.bankrupt_at)
    //
    // and set_cost has zeroed a Celestial pack under Astronomer (card.lua:380).
    // is_legal asked about the list price, so legal_actions offered the free
    // pack and is_legal refused it: seeds 72JTCTMW (Magic Deck, stake 8, $0 by
    // a $6 Jumbo), RCDKMIKP and 891F8AYE (Blue Deck, stake 3, $3 and $0 by a
    // $4 pack) all bought Astronomer and crashed proposing the pack beside it.
    let game = astro_shop(3, true);
    assert_eq!(legal_packs(&game), vec![0]);
    assert_eq!(pack_buys(&game), vec![0]);
}

#[test]
fn test_is_legal_takes_the_free_pack_in_debt() {
    // can_open's `cost > 0` escape (button_callbacks.lua:112): a pack set_cost
    // zeroed opens with any balance, a run already past its floor included.
    let game = astro_shop(-5, true);
    assert!(game.spendable() < 0);
    assert_eq!(legal_packs(&game), vec![0]);
    assert_eq!(pack_buys(&game), vec![0]);
}

#[test]
fn test_is_legal_still_refuses_the_list_price_without_astronomer() {
    // Without Astronomer set_cost leaves the list price (card.lua:369-380),
    // and can_open (button_callbacks.lua:112) refuses $4 out of $3.
    let game = astro_shop(3, false);
    assert_eq!(legal_packs(&game), Vec::<i32>::new());
    assert_eq!(pack_buys(&game), Vec::<i32>::new());
}

#[test]
fn test_is_legal_takes_a_couponed_pack_at_no_money() {
    // The Coupon Tag zeroes the boosters too, not only the shelf.
    //
    // tag.lua:448-459 sets G.GAME.shop_free and marks every card in
    // G.shop_booster couponed, and set_cost zeroes a couponed card in
    // G.shop_booster (card.lua:383). pack_price already knew it; is_legal
    // did not.
    let mut game = astro_shop(0, false);
    game.shop_free = true;
    let pack = game.shop.as_ref().unwrap().packs[1];
    assert_eq!(game.pack_price(&pack), 0);
    assert_eq!(legal_packs(&game), vec![0, 1]);
    assert_eq!(pack_buys(&game), vec![0, 1]);
}

// ==========================================================================
// tests/test_sell_inside_a_pack.py -- "A joker can be sold while a pack is
// open, and so can a consumable."
// ==========================================================================

/// `_full_row_and_a_buffoon`: a full row, a Fool held, and a pack whose only
/// card is a Mr. Bones.
fn full_row_and_a_buffoon() -> GameState {
    let mut game = GameState::new("KJH7TR2M", "Red Deck", 1);
    while (game.jokers.len() as i32) < game.joker_slots() {
        game.gain_joker(&joker("Joker"));
    }
    let fool = game.hold_consumable(spec_or_panic("The Fool"), Edition::None);
    game.consumables.push(fool);
    game.phase = Phase::Pack;
    game.pack = Some(PackSpec {
        kind: PackKind::Buffoon,
        size: "normal",
        options: 2,
        picks: 1,
        cost: 4,
        key: "",
    });
    game.pack_options = vec![PackChoice::Joker(joker("Mr. Bones"))];
    game.pack_picks_left = 1;
    game
}

fn of(game: &GameState, kind: ActionType) -> Vec<i32> {
    game.legal_actions()
        .iter()
        .filter(|a| a.r#type == kind)
        .map(|a| a.index)
        .collect()
}

#[test]
fn test_every_joker_in_a_full_row_can_be_sold_inside_a_pack() {
    let game = full_row_and_a_buffoon();
    let expected: Vec<i32> = (0..game.joker_slots()).collect();
    assert_eq!(of(&game, ActionType::SellJoker), expected);
    for i in 0..game.joker_slots() {
        assert!(game.is_legal(&Action::at(ActionType::SellJoker, i)));
    }
}

#[test]
fn test_an_eternal_joker_still_cannot_be_sold() {
    let game = full_row_and_a_buffoon();
    game.jokers[0].borrow_mut().eternal = true;
    assert!(!of(&game, ActionType::SellJoker).contains(&0));
    assert!(!game.is_legal(&Action::at(ActionType::SellJoker, 0)));
}

#[test]
fn test_a_consumable_can_be_sold_and_the_row_moved_inside_a_pack() {
    let game = full_row_and_a_buffoon();
    assert_eq!(of(&game, ActionType::SellConsumable), vec![0]);
    assert!(game.is_legal(&Action::at(ActionType::SellConsumable, 0)));
    let expected: Vec<i32> = (1..game.joker_slots()).collect();
    assert_eq!(of(&game, ActionType::SwapJokerLeft), expected);
}

#[test]
fn test_selling_makes_room_for_the_pack_joker_and_the_pack_stays_open() {
    let mut game = full_row_and_a_buffoon();
    assert!(!game
        .legal_actions()
        .iter()
        .any(|a| a.r#type == ActionType::PickPack));
    let money = game.money;
    game.step(&Action::at(ActionType::SellJoker, 0));
    assert_eq!(game.phase, Phase::Pack);
    assert!(game.money > money);
    let pick = Action::at(ActionType::PickPack, 0);
    assert!(game.is_legal(&pick));
    game.step(&pick);
    assert!(game
        .jokers
        .iter()
        .any(|j| j.borrow().name() == "Mr. Bones"));
}

#[test]
fn test_the_list_and_the_membership_test_agree_inside_a_pack() {
    let game = full_row_and_a_buffoon();
    for action in game.legal_actions() {
        assert!(game.is_legal(&action), "{action}");
    }
}

// ==========================================================================
// tests/test_ankh_full_row.py -- "An Ankh with a full joker row is not a move:
// the game presses it and nothing happens."
// ==========================================================================

/// `_run(free_slots)` from test_ankh_full_row.py.
fn ankh_run(free_slots: i32) -> GameState {
    let mut game = run();
    while (game.jokers.len() as i32) < game.joker_slots() - free_slots {
        game.gain_joker(&joker("Joker"));
    }
    game._start_round();
    game
}

fn in_spectral_pack(mut game: GameState) -> GameState {
    game.phase = Phase::Pack;
    game.pack_options = vec![PackChoice::Consumable(spec_or_panic("Ankh"))];
    game.pack_picks_left = 1;
    game
}

#[test]
fn test_ankh_is_not_taken_from_a_pack_into_a_full_row() {
    // G.FUNCS.use_card asks Card:check_use, which with `#G.jokers.cards >=
    // card_limit` shows No Room -- so from a pack the card stays where it was.
    let game = in_spectral_pack(ankh_run(0));
    assert!(!game
        .legal_actions()
        .iter()
        .any(|a| a.r#type == ActionType::PickPack));
    assert!(!game.is_legal(&Action::at(ActionType::PickPack, 0)));
}

#[test]
fn test_ankh_is_taken_from_a_pack_with_a_slot_free() {
    let game = in_spectral_pack(ankh_run(1));
    let pick = Action::at(ActionType::PickPack, 0);
    assert!(game.legal_actions().contains(&pick));
    assert!(game.is_legal(&pick));
}

#[test]
fn test_ankh_is_not_used_from_a_slot_with_a_full_row() {
    // From a slot the card stays exactly where it was too.
    let mut game = ankh_run(0);
    let ankh = game.hold_consumable(spec_or_panic("Ankh"), Edition::None);
    game.consumables.push(ankh);
    assert_eq!(game.phase, Phase::Playing);
    assert!(!game
        .legal_actions()
        .iter()
        .any(|a| a.r#type == ActionType::UseConsumable));
    assert!(!game.is_legal(&Action::at(ActionType::UseConsumable, 0)));
}

#[test]
fn test_ankh_is_used_from_a_slot_with_a_slot_free() {
    let mut game = ankh_run(1);
    let ankh = game.hold_consumable(spec_or_panic("Ankh"), Edition::None);
    game.consumables.push(ankh);
    let action = Action::at(ActionType::UseConsumable, 0);
    assert!(game.legal_actions().contains(&action));
    assert!(game.is_legal(&action));
}

// ==========================================================================
// tests/test_hallucination_after_pack_fill.py -- "Hallucination's Tarot is
// built after the pack's own cards, not before."
// ==========================================================================

fn open_arcana_with_hallucination() -> GameState {
    let mut game = GameState::new("LC4JWH61", "Nebula Deck", 1);
    game.ante = 3;
    game._open_shop();
    game.shop.as_mut().unwrap().slots.clear();
    game.gain_joker(&joker("Hallucination"));
    game._open_pack(pack_from_key("p_arcana_normal_1"), false);
    game
}

fn choice_name(choice: &PackChoice) -> String {
    match choice {
        PackChoice::Joker(j) => j.borrow().name().to_string(),
        PackChoice::Consumable(spec) => spec.name.to_string(),
        PackChoice::Card(card) => card.borrow().to_string(),
    }
}

#[test]
fn test_the_pack_is_filled_before_hallucination_draws() {
    // A card marks its centre used as it is built (card.lua:352), so by the
    // time Hallucination draws, the pack's Tarots are blanked from its pool
    // and the resample moves it on. The simulator made the Tarot first, which
    // blanked it from the *pack* instead.
    let game = open_arcana_with_hallucination();
    let pack: Vec<String> = game.pack_options.iter().map(choice_name).collect();
    assert_eq!(pack, vec!["Strength", "The Tower", "The Chariot"]);
    let held: Vec<String> = game
        .consumables
        .iter()
        .map(|c| c.borrow().spec.name.to_string())
        .collect();
    assert_eq!(held, vec!["The Empress"]);
}

#[test]
fn test_hallucination_never_duplicates_a_card_in_the_pack() {
    let game = open_arcana_with_hallucination();
    let offered: BTreeSet<String> = game.pack_options.iter().map(choice_name).collect();
    let held: BTreeSet<String> = game
        .consumables
        .iter()
        .map(|c| c.borrow().spec.name.to_string())
        .collect();
    assert!(offered.is_disjoint(&held));
}

