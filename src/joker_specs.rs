//! The registry's spec table: one entry per `register(...)` call in the Python
//! `jokers.py`.
//!
//! Python's table is a dict filled at import time by `register(name, rarity,
//! text, **kwargs)`; this is that table as a `const` array, in the same
//! registration order, with each keyword argument becoming a field of
//! [`JokerSpec`]. `jokers::all_specs` returns it and `jokers::spec` looks names
//! up through a lazily built index.
//!
//! ## Lambdas and factories
//!
//! A hook that takes no arguments in Python is a plain `fn` at module scope
//! here -- the exact spelling of `independent=lambda j, ctx: ctx.add_mult(4,
//! j.name)`. A lambda that *closes over* a value (a suit, a number of chips) has
//! no Rust equivalent as a closure that outlives its scope as a `fn` pointer, so
//! it becomes a factory: `suit_scorer(suit, amount)` matches on the handful of
//! `(suit, amount)` pairs the registry actually uses and returns the matching
//! `fn`. That is why `JokerSpec` is `Copy` and this table is `const`.
//!
//! ## The variables the old context borrowed
//!
//! Python's hooks read the run through `ctx.game`; the port hands the run to the
//! hook as an explicit `game: &mut GameState`, so `ctx.game.something` is
//! `game.something` throughout and `ctx.money` (the dollars the hand started
//! with, plus what it has paid so far) stays a method on [`ScoreContext`].
//!
//! The Python doc comments are copied across unchanged, because they record the
//! divergences the differential replay found: every one of them is a bug that
//! looked right on most hands and cost a run on one.

use crate::blinds::BlindKind;
use crate::cards::{CardRef, Edition, Enhancement, Rank, Suit};
use crate::consumables::ConsumableKind;
use crate::effects::ScoreContext;
use crate::game::{BlindSelectEvent, GameState};
use crate::hands::HandType;
use crate::jokers::{
    counts_for_flush, is_face, is_suit_for, suit_matches, suit_matches_for, Copier, IndepHook,
    JokerRef, JokerSpec, Rarity, ScoredHook,
};

// --------------------------------------------------------------------------
// the shared counters
// --------------------------------------------------------------------------

/// `_bump`: grow a joker's counter, optionally not below a floor.
fn bump(j: &JokerRef, amount: f64, floor: Option<f64>) {
    let mut b = j.borrow_mut();
    let value = b.counter + amount;
    b.counter = match floor {
        Some(floor) => value.max(floor),
        None => value,
    };
}

/// `_decay`: spend a joker down, and destroy it when it runs out.
///
/// Popcorn, Ice Cream, Turtle Bean and Ramen do not sit at zero doing nothing --
/// the game eats them. It checks *before* subtracting, so a Popcorn on four mult
/// with four to lose is gone rather than reduced, and a run with one of these has
/// a joker slot free again on a schedule. Flooring the counter instead, which is
/// what this did, left a dead joker taking up a slot for the rest of the run.
fn decay(j: &JokerRef, amount: f64, game: &mut GameState, floor: f64) {
    if j.borrow().counter + amount <= floor {
        game.destroy_joker(j, "eaten");
        return;
    }
    j.borrow_mut().counter += amount;
}

/// A listed probability, scaled by any Oops! All 6s in play (`_chance`).
fn chance(game: &mut GameState, key: &str, numerator: i32, denominator: i32) -> bool {
    let scale = game.probability_scale();
    game.rng
        .chance(key, numerator as f64 * scale, denominator as f64)
}

// --------------------------------------------------------------------------
// the factory hooks
// --------------------------------------------------------------------------

// -- _suit_scorer -----------------------------------------------------------

/// `_suit_scorer(suit, amount)`: +Mult on each scored card of a suit.
///
/// A factory because the value is captured: one `fn` per `(suit, amount)` pair
/// the registry uses.
const fn suit_scorer(suit: Suit, amount: i32) -> ScoredHook {
    match (suit, amount) {
        (Suit::Diamonds, 3) => gready_joker,
        (Suit::Hearts, 3) => lusty_joker,
        (Suit::Spades, 3) => wrathful_joker,
        (Suit::Clubs, 3) => gluttonous_joker,
        (Suit::Clubs, 7) => onyx_agate,
        _ => panic!("suit_scorer: unknown suit/amount"),
    }
}

fn gready_joker(j: &JokerRef, card: &CardRef, ctx: &mut ScoreContext, game: &mut GameState) {
    if suit_matches(card, Suit::Diamonds, game) {
        ctx.add_mult(3.0, j.borrow().spec.name);
    }
}

fn lusty_joker(j: &JokerRef, card: &CardRef, ctx: &mut ScoreContext, game: &mut GameState) {
    if suit_matches(card, Suit::Hearts, game) {
        ctx.add_mult(3.0, j.borrow().spec.name);
    }
}

fn wrathful_joker(j: &JokerRef, card: &CardRef, ctx: &mut ScoreContext, game: &mut GameState) {
    if suit_matches(card, Suit::Spades, game) {
        ctx.add_mult(3.0, j.borrow().spec.name);
    }
}

fn gluttonous_joker(j: &JokerRef, card: &CardRef, ctx: &mut ScoreContext, game: &mut GameState) {
    if suit_matches(card, Suit::Clubs, game) {
        ctx.add_mult(3.0, j.borrow().spec.name);
    }
}

fn onyx_agate(j: &JokerRef, card: &CardRef, ctx: &mut ScoreContext, game: &mut GameState) {
    if suit_matches(card, Suit::Clubs, game) {
        ctx.add_mult(7.0, j.borrow().spec.name);
    }
}

// -- _suit_chips / _suit_money ----------------------------------------------

/// `_suit_chips(suit, amount)`: +Chips on each scored card of a suit.
const fn suit_chips(suit: Suit, amount: i32) -> ScoredHook {
    match (suit, amount) {
        (Suit::Spades, 50) => arrowhead,
        _ => panic!("suit_chips: unknown suit/amount"),
    }
}

fn arrowhead(j: &JokerRef, card: &CardRef, ctx: &mut ScoreContext, game: &mut GameState) {
    if suit_matches(card, Suit::Spades, game) {
        ctx.add_chips(50.0, j.borrow().spec.name);
    }
}

/// `_suit_money(suit, amount)`: dollars on each scored card of a suit.
const fn suit_money(suit: Suit, amount: i32) -> ScoredHook {
    match (suit, amount) {
        (Suit::Diamonds, 1) => rough_gem,
        _ => panic!("suit_money: unknown suit/amount"),
    }
}

fn rough_gem(j: &JokerRef, card: &CardRef, ctx: &mut ScoreContext, game: &mut GameState) {
    if suit_matches(card, Suit::Diamonds, game) {
        ctx.money_gained += 1;
    }
    let _ = j;
}


// -- _hand_mult / _hand_chips / _hand_xmult ---------------------------------

/// `_hand_mult(hands, amount)`: +Mult when the played hand contains a hand.
///
/// Each of these is now the sub-hand itself: the played cards carry a table of
/// everything they contain, so "contains a Pair" is a membership test rather
/// than a list of the top hands that imply one. That list could not express a
/// Flush that happens to hold a pair, which the game counts.
const fn hand_mult(hand: HandType, amount: i32) -> IndepHook {
    match (hand, amount) {
        (HandType::Pair, 8) => jolly_joker,
        (HandType::ThreeOfAKind, 12) => zany_joker,
        (HandType::TwoPair, 10) => mad_joker,
        (HandType::Straight, 12) => crazy_joker,
        (HandType::Flush, 10) => droll_joker,
        _ => panic!("hand_mult: unknown hand/amount"),
    }
}

fn jolly_joker(j: &JokerRef, ctx: &mut ScoreContext, _game: &mut GameState) {
    if ctx.contains.contains(HandType::Pair) {
        ctx.add_mult(8.0, j.borrow().spec.name);
    }
}

fn zany_joker(j: &JokerRef, ctx: &mut ScoreContext, _game: &mut GameState) {
    if ctx.contains.contains(HandType::ThreeOfAKind) {
        ctx.add_mult(12.0, j.borrow().spec.name);
    }
}

fn mad_joker(j: &JokerRef, ctx: &mut ScoreContext, _game: &mut GameState) {
    if ctx.contains.contains(HandType::TwoPair) {
        ctx.add_mult(10.0, j.borrow().spec.name);
    }
}

fn crazy_joker(j: &JokerRef, ctx: &mut ScoreContext, _game: &mut GameState) {
    if ctx.contains.contains(HandType::Straight) {
        ctx.add_mult(12.0, j.borrow().spec.name);
    }
}

fn droll_joker(j: &JokerRef, ctx: &mut ScoreContext, _game: &mut GameState) {
    if ctx.contains.contains(HandType::Flush) {
        ctx.add_mult(10.0, j.borrow().spec.name);
    }
}

/// `_hand_chips(hands, amount)`: +Chips when the played hand contains a hand.
const fn hand_chips(hand: HandType, amount: i32) -> IndepHook {
    match (hand, amount) {
        (HandType::Pair, 50) => sly_joker,
        (HandType::ThreeOfAKind, 100) => wily_joker,
        (HandType::TwoPair, 80) => clever_joker,
        (HandType::Straight, 100) => devious_joker,
        (HandType::Flush, 80) => crafty_joker,
        _ => panic!("hand_chips: unknown hand/amount"),
    }
}

fn sly_joker(j: &JokerRef, ctx: &mut ScoreContext, _game: &mut GameState) {
    if ctx.contains.contains(HandType::Pair) {
        ctx.add_chips(50.0, j.borrow().spec.name);
    }
}

fn wily_joker(j: &JokerRef, ctx: &mut ScoreContext, _game: &mut GameState) {
    if ctx.contains.contains(HandType::ThreeOfAKind) {
        ctx.add_chips(100.0, j.borrow().spec.name);
    }
}

fn clever_joker(j: &JokerRef, ctx: &mut ScoreContext, _game: &mut GameState) {
    if ctx.contains.contains(HandType::TwoPair) {
        ctx.add_chips(80.0, j.borrow().spec.name);
    }
}

fn devious_joker(j: &JokerRef, ctx: &mut ScoreContext, _game: &mut GameState) {
    if ctx.contains.contains(HandType::Straight) {
        ctx.add_chips(100.0, j.borrow().spec.name);
    }
}

fn crafty_joker(j: &JokerRef, ctx: &mut ScoreContext, _game: &mut GameState) {
    if ctx.contains.contains(HandType::Flush) {
        ctx.add_chips(80.0, j.borrow().spec.name);
    }
}

/// `_hand_xmult(hands, factor)`: XMult when the played hand contains a hand.
const fn hand_xmult(hand: HandType, factor: f64) -> IndepHook {
    match (hand, factor as i32) {
        (HandType::Pair, 2) => the_duo,
        (HandType::ThreeOfAKind, 3) => the_trio,
        (HandType::FourOfAKind, 4) => the_family,
        (HandType::Straight, 3) => the_order,
        (HandType::Flush, 2) => the_tribe,
        _ => panic!("hand_xmult: unknown hand/factor"),
    }
}

fn the_duo(j: &JokerRef, ctx: &mut ScoreContext, _game: &mut GameState) {
    if ctx.contains.contains(HandType::Pair) {
        ctx.times_mult(2.0, j.borrow().spec.name);
    }
}

fn the_trio(j: &JokerRef, ctx: &mut ScoreContext, _game: &mut GameState) {
    if ctx.contains.contains(HandType::ThreeOfAKind) {
        ctx.times_mult(3.0, j.borrow().spec.name);
    }
}

fn the_family(j: &JokerRef, ctx: &mut ScoreContext, _game: &mut GameState) {
    if ctx.contains.contains(HandType::FourOfAKind) {
        ctx.times_mult(4.0, j.borrow().spec.name);
    }
}

fn the_order(j: &JokerRef, ctx: &mut ScoreContext, _game: &mut GameState) {
    if ctx.contains.contains(HandType::Straight) {
        ctx.times_mult(3.0, j.borrow().spec.name);
    }
}

fn the_tribe(j: &JokerRef, ctx: &mut ScoreContext, _game: &mut GameState) {
    if ctx.contains.contains(HandType::Flush) {
        ctx.times_mult(2.0, j.borrow().spec.name);
    }
}


// -- _rank_scorer -----------------------------------------------------------

const EVEN: [Rank; 5] = [Rank::Ten, Rank::Eight, Rank::Six, Rank::Four, Rank::Two];
const ODD: [Rank; 5] = [Rank::Ace, Rank::Nine, Rank::Seven, Rank::Five, Rank::Three];
const FIB: [Rank; 5] = [Rank::Ace, Rank::Two, Rank::Three, Rank::Five, Rank::Eight];

/// `_rank_scorer(ranks, chips=0, mult=0)`: chips and/or mult on scored cards of
/// a rank. A factory over the `(chips, mult)` pairs the registry uses.
const fn rank_scorer(_ranks: &'static [Rank], chips: i32, mult: i32) -> ScoredHook {
    match (chips, mult) {
        (0, 4) => rank_even_steven,
        (31, 0) => rank_odd_todd,
        (0, 8) => rank_fibonacci,
        (20, 4) => rank_scholar,
        (10, 4) => rank_walkie_talkie,
        _ => panic!("rank_scorer: unknown chips/mult"),
    }
}

fn rank_hook(
    j: &JokerRef,
    card: &CardRef,
    ctx: &mut ScoreContext,
    ranks: &[Rank],
    chips: i32,
    mult: i32,
) {
    let (is_stone, rank) = {
        let c = card.borrow();
        (c.is_stone(), c.rank)
    };
    if is_stone || !ranks.contains(&rank) {
        return;
    }
    ctx.add_chips(chips as f64, j.borrow().spec.name);
    ctx.add_mult(mult as f64, j.borrow().spec.name);
}

fn rank_even_steven(j: &JokerRef, card: &CardRef, ctx: &mut ScoreContext, _game: &mut GameState) {
    rank_hook(j, card, ctx, &EVEN, 0, 4);
}

fn rank_odd_todd(j: &JokerRef, card: &CardRef, ctx: &mut ScoreContext, _game: &mut GameState) {
    rank_hook(j, card, ctx, &ODD, 31, 0);
}

fn rank_fibonacci(j: &JokerRef, card: &CardRef, ctx: &mut ScoreContext, _game: &mut GameState) {
    rank_hook(j, card, ctx, &FIB, 0, 8);
}

fn rank_scholar(j: &JokerRef, card: &CardRef, ctx: &mut ScoreContext, _game: &mut GameState) {
    rank_hook(j, card, ctx, &[Rank::Ace], 20, 4);
}

fn rank_walkie_talkie(j: &JokerRef, card: &CardRef, ctx: &mut ScoreContext, _game: &mut GameState) {
    rank_hook(j, card, ctx, &[Rank::Ten, Rank::Four], 10, 4);
}

// -- _face_scorer -----------------------------------------------------------

/// `_face_scorer(chips=0, mult=0)`: chips and/or mult on scored face cards.
///
/// A factory over the `(chips, mult)` pairs the registry uses.
const fn face_scorer(chips: i32, mult: i32) -> ScoredHook {
    match (chips, mult) {
        (30, 0) => scary_face,
        (0, 5) => smiley_face,
        _ => panic!("face_scorer: unknown chips/mult"),
    }
}

fn face_hook(
    j: &JokerRef,
    card: &CardRef,
    ctx: &mut ScoreContext,
    game: &GameState,
    chips: i32,
    mult: i32,
) {
    if !is_face(card, game) {
        return;
    }
    ctx.add_chips(chips as f64, j.borrow().spec.name);
    ctx.add_mult(mult as f64, j.borrow().spec.name);
}

fn scary_face(j: &JokerRef, card: &CardRef, ctx: &mut ScoreContext, game: &mut GameState) {
    face_hook(j, card, ctx, game, 30, 0);
}

fn smiley_face(j: &JokerRef, card: &CardRef, ctx: &mut ScoreContext, game: &mut GameState) {
    face_hook(j, card, ctx, game, 0, 5);
}

// -- _pays_on_face ----------------------------------------------------------

/// `_pays_on_face(dollars, key)`: a face card's coin for `dollars`, counted at
/// its odds as well as paid as rolled (`ScoreContext::expect`). The roll is the
/// one it always was.
const fn pays_on_face(dollars: i32, key: &str) -> ScoredHook {
    let _ = key;
    match dollars {
        2 => business_card,
        1 => reserved_parking,
        _ => panic!("pays_on_face: unknown dollars"),
    }
}

fn pay_face_hook(
    _j: &JokerRef,
    card: &CardRef,
    ctx: &mut ScoreContext,
    game: &mut GameState,
    dollars: i32,
    key: &str,
) {
    if !is_face(card, game) {
        return;
    }
    ctx.expect(dollars as f64, 1, 2);
    if chance(game, key, 1, 2) {
        ctx.money_gained += dollars;
        ctx.chance_paid += dollars;
    }
}

fn business_card(j: &JokerRef, card: &CardRef, ctx: &mut ScoreContext, game: &mut GameState) {
    pay_face_hook(j, card, ctx, game, 2, "business");
}

fn reserved_parking(j: &JokerRef, card: &CardRef, ctx: &mut ScoreContext, game: &mut GameState) {
    pay_face_hook(j, card, ctx, game, 1, "parking");
}

// --------------------------------------------------------------------------
// the bespoke hooks, in the Python file's registration order
// --------------------------------------------------------------------------

fn joker_indep(j: &JokerRef, ctx: &mut ScoreContext, _game: &mut GameState) {
    ctx.add_mult(4.0, j.borrow().spec.name);
}

fn half_joker(j: &JokerRef, ctx: &mut ScoreContext, _game: &mut GameState) {
    if ctx.played.len() <= 3 {
        ctx.add_mult(20.0, j.borrow().spec.name);
    }
}

fn banner(j: &JokerRef, ctx: &mut ScoreContext, game: &mut GameState) {
    ctx.add_chips(30.0 * game.discards_left as f64, j.borrow().spec.name);
}

fn mystic_summit(j: &JokerRef, ctx: &mut ScoreContext, game: &mut GameState) {
    if game.discards_left == 0 {
        ctx.add_mult(15.0, j.borrow().spec.name);
    }
}

fn misprint(j: &JokerRef, ctx: &mut ScoreContext, game: &mut GameState) {
    let roll = game.rng.randint("misprint", 0.0, 23.0);
    ctx.add_mult(roll as f64, j.borrow().spec.name);
}

fn abstract_joker(j: &JokerRef, ctx: &mut ScoreContext, game: &mut GameState) {
    ctx.add_mult(3.0 * game.jokers.len() as f64, j.borrow().spec.name);
}

fn blue_joker(j: &JokerRef, ctx: &mut ScoreContext, game: &mut GameState) {
    ctx.add_chips(2.0 * game.draw_pile.len() as f64, j.borrow().spec.name);
}

fn bull(j: &JokerRef, ctx: &mut ScoreContext, _game: &mut GameState) {
    ctx.add_chips(2.0 * ctx.money() as f64, j.borrow().spec.name);
}

fn bootstraps(j: &JokerRef, ctx: &mut ScoreContext, _game: &mut GameState) {
    ctx.add_mult(2.0 * (ctx.money() / 5) as f64, j.borrow().spec.name);
}

/// `_ride_update`: a hand with no scored face card grows the streak.
fn ride_update(j: &JokerRef, ctx: &mut ScoreContext, game: &mut GameState) {
    // A debuffed face card does not score, so it does not break the streak --
    // against The Club a debuffed King leaves the counter climbing. Missing
    // this only shows up on a boss blind, which is why it survived until the
    // scenario matrix reached one.
    //
    // And `is_face`, not `rank.is_face`: Pareidolia makes every card a face
    // card (card.lua:967), so a Ride the Bus held beside one can never grow
    // at all. The game knows; this counted to eight while the engine sat at
    // zero, which the policy found by playing the engine with a shadow.
    if ctx.scoring.iter().any(|c| is_face(c, game)) {
        j.borrow_mut().counter = 0.0;
    } else {
        j.borrow_mut().counter += 1.0;
    }
}

fn ride_the_bus(j: &JokerRef, ctx: &mut ScoreContext, _game: &mut GameState) {
    ctx.add_mult(j.borrow().counter, j.borrow().spec.name);
}

fn green_update(j: &JokerRef, _ctx: &mut ScoreContext, _game: &mut GameState) {
    bump(j, 1.0, None);
}

fn green_discard(j: &JokerRef, _cards: &[CardRef], _game: &mut GameState) {
    bump(j, -1.0, Some(0.0));
}

fn green_joker(j: &JokerRef, ctx: &mut ScoreContext, _game: &mut GameState) {
    ctx.add_mult(j.borrow().counter, j.borrow().spec.name);
}

fn runner_update(j: &JokerRef, ctx: &mut ScoreContext, _game: &mut GameState) {
    if ctx.contains.contains(HandType::Straight) {
        bump(j, 15.0, None);
    }
}

fn runner(j: &JokerRef, ctx: &mut ScoreContext, _game: &mut GameState) {
    ctx.add_chips(j.borrow().counter, j.borrow().spec.name);
}

fn ice_cream_update(j: &JokerRef, _ctx: &mut ScoreContext, game: &mut GameState) {
    decay(j, -5.0, game, 0.0);
}

fn ice_cream(j: &JokerRef, ctx: &mut ScoreContext, _game: &mut GameState) {
    ctx.add_chips(j.borrow().counter, j.borrow().spec.name);
}

fn square_update(j: &JokerRef, ctx: &mut ScoreContext, _game: &mut GameState) {
    if ctx.played.len() == 4 {
        bump(j, 4.0, None);
    }
}

fn square_joker(j: &JokerRef, ctx: &mut ScoreContext, _game: &mut GameState) {
    ctx.add_chips(j.borrow().counter, j.borrow().spec.name);
}

fn supernova(j: &JokerRef, ctx: &mut ScoreContext, game: &mut GameState) {
    let plays = game.hand_levels.plays.get(&ctx.hand).copied().unwrap_or(0);
    ctx.add_mult(plays as f64, j.borrow().spec.name);
}

fn popcorn(j: &JokerRef, ctx: &mut ScoreContext, _game: &mut GameState) {
    ctx.add_mult(j.borrow().counter, j.borrow().spec.name);
}

fn popcorn_end(j: &JokerRef, game: &mut GameState) {
    decay(j, -4.0, game, 0.0);
}

fn swashbuckler(j: &JokerRef, ctx: &mut ScoreContext, game: &mut GameState) {
    let total: i32 = game
        .jokers
        .iter()
        .filter(|o| !std::rc::Rc::ptr_eq(o, j))
        .map(|o| game.sell_value(o))
        .sum();
    ctx.add_mult(total as f64, j.borrow().spec.name);
}


fn golden_joker(_j: &JokerRef, game: &mut GameState) {
    game.add_money(4, "Golden Joker");
}

/// `_faceless`: $5 when three of the discarded cards are face cards.
fn faceless(_j: &JokerRef, cards: &[CardRef], game: &mut GameState) {
    // card.lua:2858-2861 counts `v:is_face()` over the whole discard, and
    // Card:is_face (card.lua:964-969) is not the printed rank:
    //
    //     if self.debuff and not from_boss then return end
    //     local id = self:get_id()
    //     if id == 11 or id == 12 or id == 13 or next(find_joker("Pareidolia"))
    //
    // so a debuffed card is never a face card, a Stone King is not one either
    // (get_id is a random negative for Stone, card.lua:958-960), and with
    // Pareidolia held every live card is -- Stone included, since that test
    // does not look at the id.
    let faces = cards.iter().filter(|&c| is_face(c, game)).count();
    if faces >= 3 {
        game.add_money(5, "Faceless Joker");
    }
}

/// `_gros_michel_end`: 1 in 6 at the end of a round, and extinction is recorded
/// for good.
///
/// card.lua:3037 sets `G.GAME.pool_flags.gros_michel_extinct` in the same
/// branch that destroys the joker, and that flag is the whole of what gates
/// Cavendish -- `yes_pool_flag = 'gros_michel_extinct'` -- and takes Gros
/// Michel out of every later pool. Destroying it without the flag left
/// Cavendish unobtainable for the rest of any run. Marcin's live run of
/// QWEFRTUZ, Blue Deck, stake 5 stopped on it at decision 73: a reroll
/// stocked Cavendish in the game and Delayed Gratification here.
fn gros_michel_end(j: &JokerRef, game: &mut GameState) {
    if game
        .rng
        .chance("gros_michel", 1.0 * game.probability_scale(), 6.0)
    {
        game.destroy_joker(j, "Gros Michel went extinct");
        game.pool_flags.insert("gros_michel_extinct".to_string());
    }
}

/// `_cavendish_end`: 1 in 1000 at the end of a round, on its own stream, and no
/// flag.
///
/// The same end_of_round branch as Gros Michel (card.lua:3019-3020), with
/// `'cavendish'` for the key and odds of 1000 (game.lua:432); only Gros
/// Michel records its extinction (card.lua:3037). Cavendish had no hook, so
/// it could never go and never moved the 'cavendish' stream.
fn cavendish_end(j: &JokerRef, game: &mut GameState) {
    if game
        .rng
        .chance("cavendish", 1.0 * game.probability_scale(), 1000.0)
    {
        game.destroy_joker(j, "Cavendish went extinct");
    }
}

fn baron(j: &JokerRef, card: &CardRef, ctx: &mut ScoreContext, _game: &mut GameState) {
    let (rank, is_stone) = {
        let c = card.borrow();
        (c.rank, c.is_stone())
    };
    if rank == Rank::King && !is_stone {
        ctx.times_mult(1.5, j.borrow().spec.name);
    }
}

fn blackboard(j: &JokerRef, ctx: &mut ScoreContext, game: &mut GameState) {
    if ctx
        .held
        .iter()
        .all(|c| counts_for_flush(c, Suit::Spades, game) || counts_for_flush(c, Suit::Clubs, game))
    {
        ctx.times_mult(3.0, j.borrow().spec.name);
    }
}

fn card_sharp(j: &JokerRef, ctx: &mut ScoreContext, game: &mut GameState) {
    if game.hands_played_this_round.contains(&ctx.hand) {
        ctx.times_mult(3.0, j.borrow().spec.name);
    }
}

/// `_flower_pot`: X3 when the scoring hand fills all four suits
/// (card.lua:3808-3834).
///
/// Two passes over scoring_hand, each card filling at most ONE suit: the
/// first suit, Hearts-Diamonds-Spades-Clubs, that it is and that is still
/// empty. Cards that are not Wild go first and are asked with bypass_debuff
/// (`is_suit(s, true)`), so a debuffed Diamond still fills Diamonds; Wild
/// cards go second and are asked without it, so a debuffed Wild fills
/// nothing and a live one fills one empty suit, not all four. Smeared Joker
/// lets a second Heart fill Diamonds.
///
/// Asking `any card counts as s` for each suit let one Wild Seven fill the
/// two suits a Three of a Kind of 7H 7S 7C(wild) lacked: 459 against the
/// engine's 153.
///
/// 0RVVD29X, Ghost Deck, stake 1, decision 99: Smeared Joker and a Flush of
/// a Wild Queen of Diamonds, two Clubs and two Spades. The Clubs fill Spades
/// and Clubs, the Spades find nothing left, the Wild fills Hearts: no X3,
/// 150 x 394 = 59100 in the game against 177300 here.
fn flower_pot(j: &JokerRef, ctx: &mut ScoreContext, game: &mut GameState) {
    let mut found: Vec<Suit> = Vec::new();
    let order = [Suit::Hearts, Suit::Diamonds, Suit::Spades, Suit::Clubs];
    for bypass in [true, false] {
        for card in &ctx.scoring {
            let is_wild = card.borrow().enhancement == Enhancement::Wild;
            if is_wild == bypass {
                // The bypass pass wants non-Wild; the second pass wants Wild.
                continue;
            }
            for suit in order {
                if !found.contains(&suit) && is_suit_for(card, suit, game, bypass) {
                    found.push(suit);
                    break;
                }
            }
        }
    }
    if found.len() == 4 {
        ctx.times_mult(3.0, j.borrow().spec.name);
    }
}

fn stuntman(j: &JokerRef, ctx: &mut ScoreContext, _game: &mut GameState) {
    ctx.add_chips(250.0, j.borrow().spec.name);
}

fn steel_joker(j: &JokerRef, ctx: &mut ScoreContext, game: &mut GameState) {
    let count = game
        .full_deck
        .iter()
        .filter(|c| c.borrow().enhancement == Enhancement::Steel)
        .count();
    ctx.times_mult(1.0 + 0.2 * count as f64, j.borrow().spec.name);
}


/// `_vampire`: strip every enhanced card in the scoring hand, before it scores.
///
/// context.before, alongside Midas Mask: the game walks the scoring hand
/// once, up front, and sets each enhanced card back to c_base. So the
/// enhancement it takes never pays out at all -- a mult card eaten by a
/// Vampire gives its owner X0.1 and the hand nothing. Running this after the
/// cards had scored, which is what an ordinary update does, let the
/// enhancement pay first and then be removed, which is worth the whole
/// enhancement every hand.
///
/// Stone counts as enhanced here. The game's test is `center ~= c_base`, and
/// a stone card is not c_base, so a Vampire eats one and hands the card its
/// rank and suit back.
///
/// A debuffed card is left alone and earns nothing: the test is
/// `center ~= c_base and not v.debuff and not v.vampired` (card.lua:3468),
/// and a debuffed card can score in a Flush or a Pair. NQ86453Q stopped on
/// it at decision 114 -- a debuffed enhanced Queen in a Flush took the
/// Vampire to X1.3 here and left it at X1.2 in the game.
fn vampire_update(j: &JokerRef, ctx: &mut ScoreContext, game: &mut GameState) {
    let mut gained = 0i32;
    for c in &ctx.scoring {
        let (enh, debuffed) = {
            let b = c.borrow();
            (b.enhancement, b.debuffed)
        };
        if enh != Enhancement::None && !debuffed {
            game.set_enhancement(c, Enhancement::None);
            gained += 1;
        }
    }
    j.borrow_mut().counter += 0.1 * gained as f64;
}

/// A joker whose whole effect is "XMult equal to its counter": Constellation,
/// Hologram, Glass Joker, Campfire, Canio, Yorick, Madness.
fn times_counter(j: &JokerRef, ctx: &mut ScoreContext, _game: &mut GameState) {
    ctx.times_mult(j.borrow().counter, j.borrow().spec.name);
}

/// A joker whose whole effect is "+Mult equal to its counter": Red Card,
/// Flash Card, Spare Trousers, Ceremonial Dagger, Green Joker, Ride the Bus.
fn add_counter(j: &JokerRef, ctx: &mut ScoreContext, _game: &mut GameState) {
    ctx.add_mult(j.borrow().counter, j.borrow().spec.name);
}

/// A joker whose whole effect is "+Chips equal to its counter": Castle, Wee
/// Joker, Runner, Square Joker.
fn chips_counter(j: &JokerRef, ctx: &mut ScoreContext, _game: &mut GameState) {
    ctx.add_chips(j.borrow().counter, j.borrow().spec.name);
}

fn ramen_discard(j: &JokerRef, cards: &[CardRef], game: &mut GameState) {
    decay(j, -0.01 * cards.len() as f64, game, 1.0);
}

/// `_loyalty_remaining`: the game's own formula, not a modulo-6 counter.
///
/// loyalty_remaining = (every-1 - hands since created) % (every+1), with the
/// X4 firing when that equals `every`. A plain counter starting at zero fires
/// on the *first* hand instead of the sixth, which is what the simulator did.
fn loyalty_remaining(j: &JokerRef, game: &GameState) -> i32 {
    let every = 5;
    let diff = game.hands_played - j.borrow().hands_at_create;
    crate::rng::py_mod((every - 1 - diff) as f64, (every + 1) as f64) as i32
}

fn loyalty_card(j: &JokerRef, ctx: &mut ScoreContext, game: &mut GameState) {
    if loyalty_remaining(j, game) == 5 {
        ctx.times_mult(4.0, j.borrow().spec.name);
    }
}

/// Asked about each joker in turn, not once for the row: the X1.5 for an
/// Uncommon joker lands right after that joker's own effect (card.lua:3396-3408,
/// state_events.lua:918-930). The other joker's debuff is not checked there, so
/// a debuffed Uncommon joker still counts.
fn baseball_card(j: &JokerRef, other: &JokerRef, ctx: &mut ScoreContext, _game: &mut GameState) {
    if other.borrow().spec.rarity == Rarity::Uncommon && !std::rc::Rc::ptr_eq(other, j) {
        let text = format!(
            "{} on {}",
            j.borrow().spec.name,
            other.borrow().spec.name
        );
        ctx.times_mult(1.5, &text);
    }
}

// -- retrigger and copy jokers ----------------------------------------------

fn hack(_j: &JokerRef, card: &CardRef, _ctx: &mut ScoreContext, _game: &mut GameState) -> i32 {
    let (is_stone, rank) = {
        let c = card.borrow();
        (c.is_stone(), c.rank)
    };
    if !is_stone
        && matches!(rank, Rank::Two | Rank::Three | Rank::Four | Rank::Five)
    {
        1
    } else {
        0
    }
}

fn sock_and_buskin(
    _j: &JokerRef,
    card: &CardRef,
    _ctx: &mut ScoreContext,
    game: &mut GameState,
) -> i32 {
    if is_face(card, game) {
        1
    } else {
        0
    }
}

fn hanging_chad(
    _j: &JokerRef,
    card: &CardRef,
    ctx: &mut ScoreContext,
    _game: &mut GameState,
) -> i32 {
    match ctx.scoring.first() {
        Some(first) if std::rc::Rc::ptr_eq(first, card) => 2,
        _ => 0,
    }
}

fn dusk(_j: &JokerRef, _card: &CardRef, _ctx: &mut ScoreContext, game: &mut GameState) -> i32 {
    if game.hands_left == 0 {
        1
    } else {
        0
    }
}

fn mime(_j: &JokerRef, _card: &CardRef, _ctx: &mut ScoreContext, _game: &mut GameState) -> i32 {
    1
}

fn triboulet(j: &JokerRef, card: &CardRef, ctx: &mut ScoreContext, _game: &mut GameState) {
    let (rank, is_stone) = {
        let c = card.borrow();
        (c.rank, c.is_stone())
    };
    if matches!(rank, Rank::King | Rank::Queen) && !is_stone {
        ctx.times_mult(2.0, j.borrow().spec.name);
    }
}


/// `_canio`: `if val:is_face() then face_cards = face_cards + 1 end` over the
/// cards removed (card.lua:2673-2679): not a debuffed King, not a Stone one, and
/// beside Pareidolia any card at all.
fn canio(j: &JokerRef, cards: &[CardRef], game: &mut GameState) {
    let gained = cards
        .iter()
        .filter(|&c| crate::jokers::is_face_for(c, game, false))
        .count();
    j.borrow_mut().counter += gained as f64;
}

/// `_yorick`: X1 for every twenty-three cards discarded, counted down not up.
///
/// The game keeps yorick_discards ticking towards one and resets it when it
/// gets there, which is why the counter is stored on the joker rather than
/// derived from a running total: two Yoricks are on their own schedules.
fn yorick(j: &JokerRef, cards: &[CardRef], _game: &mut GameState) {
    for _ in cards {
        if j.borrow().secondary <= 1.0 {
            j.borrow_mut().secondary = 23.0;
            j.borrow_mut().counter += 1.0;
        } else {
            j.borrow_mut().secondary -= 1.0;
        }
    }
}

/// `_perkeo`: a Negative copy of one consumable held, as the shop closes.
///
/// Negative, so it does not need a slot -- which is the whole point of the
/// joker and the reason it is worth a legendary. The copy is drawn from what
/// is actually in the slots, so an empty row gets nothing.
///
/// The edition used to go nowhere, because the row held shared registry
/// entries with no room for one: the copy took a slot like any other card,
/// and a full row got nothing at all.
fn perkeo(_j: &JokerRef, game: &mut GameState) {
    if game.consumables.is_empty() {
        return;
    }
    // pseudorandom_element sorts by sort_id (misc_functions.lua:260), so the
    // draw is over the row oldest first, not in the order it is laid out.
    let mut sorted = game.consumables.clone();
    sorted.sort_by_key(|c| c.borrow().uid);
    let chosen = game.rng.choice("perkeo", &sorted);
    let spec = chosen.borrow().spec;
    let copy = game.hold_consumable(spec, Edition::Negative);
    // copy_card copies the whole ability table, extra_value included
    // (common_events.lua:2161-2167), so the copy keeps any Gift Card money.
    copy.borrow_mut().extra_sell_value = chosen.borrow().extra_sell_value;
    game.consumables.push(copy);
    game.log(format!("Perkeo: a negative {}", chosen.borrow().spec.name));
}

// --------------------------------------------------------------------------
// suit and rank scorers, second batch
// --------------------------------------------------------------------------

/// `_first_face`: the first scoring face card, which Photograph multiplies.
fn first_face(ctx: &ScoreContext, game: &GameState) -> Option<CardRef> {
    ctx.scoring.iter().find(|c| is_face(c, game)).cloned()
}

fn photograph(j: &JokerRef, card: &CardRef, ctx: &mut ScoreContext, game: &mut GameState) {
    if let Some(first) = first_face(ctx, game) {
        if std::rc::Rc::ptr_eq(&first, card) {
            ctx.times_mult(2.0, j.borrow().spec.name);
        }
    }
}

/// `context.other_card:get_id() == 12` (card.lua:3272-3273), and get_id
/// answers a Stone card with a random negative (card.lua:958-960): a Stone
/// Queen held pays nothing, as a Stone King does nothing for Baron.
fn shoot_the_moon(
    j: &JokerRef,
    card: &CardRef,
    ctx: &mut ScoreContext,
    _game: &mut GameState,
) {
    let (rank, debuffed, is_stone) = {
        let c = card.borrow();
        (c.rank, c.debuffed, c.is_stone())
    };
    if rank == Rank::Queen && !debuffed && !is_stone {
        ctx.add_mult(13.0, j.borrow().spec.name);
    }
}

/// `_lowest_held`: the card Raised Fist points at.
///
/// The game walks the hand front to back keeping any card whose id is <= the
/// best so far, so on a tie the *rightmost* of the equal-lowest cards wins.
/// Taking the first minimum instead is invisible until the two differ --
/// until one of them is debuffed, or Mime is retriggering whichever was
/// chosen. Stone cards have no rank and are skipped; a Steel card is not,
/// and can perfectly well be the lowest.
fn lowest_held(ctx: &ScoreContext) -> Option<CardRef> {
    let mut chosen: Option<CardRef> = None;
    let mut best: u8 = 15;
    for card in &ctx.held {
        let (is_stone, rank) = {
            let c = card.borrow();
            (c.is_stone(), c.rank)
        };
        if is_stone {
            continue;
        }
        if rank.value() <= best {
            chosen = Some(card.clone());
            best = rank.value();
        }
    }
    chosen
}

/// `_raised_fist`: add double that card's nominal value, once it is the one
/// being held.
///
/// This is a held-card trigger rather than an independent one, which is what
/// makes Mime retrigger it: Mime repeats abilities of cards held in hand, and
/// Raised Fist's mult is attached to the card it points at.
fn raised_fist(j: &JokerRef, card: &CardRef, ctx: &mut ScoreContext, _game: &mut GameState) {
    match lowest_held(ctx) {
        Some(low) if std::rc::Rc::ptr_eq(&low, card) => {}
        _ => return,
    }
    if card.borrow().debuffed {
        // A debuffed choice pays nothing; it does not fall through to the
        // next lowest card.
        return;
    }
    ctx.add_mult(2.0 * card.borrow().rank.chips() as f64, j.borrow().spec.name);
}


fn acrobat(j: &JokerRef, ctx: &mut ScoreContext, game: &mut GameState) {
    if game.hands_left == 0 {
        ctx.times_mult(3.0, j.borrow().spec.name);
    }
}

const SEEING_DOUBLE_WILD_ORDER: [Suit; 4] =
    [Suit::Clubs, Suit::Diamonds, Suit::Spades, Suit::Hearts];

/// `_seeing_double`: X2 when the scoring hand has a Club and another suit
/// (card.lua:3845-3866).
///
/// Cards that are not Wild count toward every suit `is_suit(s)` says they
/// are -- so under Smeared Joker a Spade is a Club too, and a Pair of Spades
/// qualifies on its own (112 on the engine, 56 here before). Wild cards then
/// fill one empty suit each, Clubs first, so a single Wild cannot be both
/// halves. None of it bypasses a debuff.
fn seeing_double(j: &JokerRef, ctx: &mut ScoreContext, game: &mut GameState) {
    let mut counts: [i32; 4] = [0; 4];
    let index = |suit: Suit| match suit {
        Suit::Clubs => 0,
        Suit::Diamonds => 1,
        Suit::Spades => 2,
        Suit::Hearts => 3,
    };
    for card in &ctx.scoring {
        if card.borrow().enhancement != Enhancement::Wild {
            for suit in Suit::ALL {
                if is_suit_for(card, suit, game, false) {
                    counts[index(suit)] += 1;
                }
            }
        }
    }
    for card in &ctx.scoring {
        if card.borrow().enhancement == Enhancement::Wild {
            for suit in SEEING_DOUBLE_WILD_ORDER {
                if counts[index(suit)] == 0 && is_suit_for(card, suit, game, false) {
                    counts[index(suit)] += 1;
                    break;
                }
            }
        }
    }
    if counts[index(Suit::Clubs)] > 0
        && (counts[index(Suit::Hearts)] > 0
            || counts[index(Suit::Diamonds)] > 0
            || counts[index(Suit::Spades)] > 0)
    {
        ctx.times_mult(2.0, j.borrow().spec.name);
    }
}

// -- jokers that scale on the deck ------------------------------------------

fn erosion(j: &JokerRef, ctx: &mut ScoreContext, game: &mut GameState) {
    let missing = 52 - game.full_deck.len() as i32;
    ctx.add_mult(4.0 * missing.max(0) as f64, j.borrow().spec.name);
}

fn stone_joker(j: &JokerRef, ctx: &mut ScoreContext, game: &mut GameState) {
    let count = game.full_deck.iter().filter(|c| c.borrow().is_stone()).count();
    ctx.add_chips(25.0 * count as f64, j.borrow().spec.name);
}

fn drivers_license(j: &JokerRef, ctx: &mut ScoreContext, game: &mut GameState) {
    let count = game
        .full_deck
        .iter()
        .filter(|c| c.borrow().enhancement != Enhancement::None)
        .count();
    if count >= 16 {
        ctx.times_mult(3.0, j.borrow().spec.name);
    }
}

fn joker_stencil(j: &JokerRef, ctx: &mut ScoreContext, game: &mut GameState) {
    // card.lua:4203-4207 counts the stencils rather than adding one for
    // itself: X(empty slots + every Joker Stencil in the row). One stencil
    // reads the same either way, which is why adding one for itself stood;
    // two do not. Ported from the Python fix `fa9f8e0`, where U9QERIL2 held
    // two with four jokers in six slots and the game gave each X4 against
    // this X3 -- X16 against X9, 16/9 to the digit on the live run's stop.
    let stencils = game
        .jokers
        .iter()
        .filter(|other| other.borrow().spec.name == "Joker Stencil")
        .count() as i32;
    let empty = game.joker_slots() - game.jokers.len() as i32 + stencils;
    ctx.times_mult(empty as f64, j.borrow().spec.name);
}

// -- jokers that scale on what the run has done -----------------------------

fn fortune_teller(j: &JokerRef, ctx: &mut ScoreContext, game: &mut GameState) {
    ctx.add_mult(game.tarots_used as f64, j.borrow().spec.name);
}

/// `_flash_card`: +2 Mult per shop reroll.
fn flash_card(j: &JokerRef, _game: &mut GameState) {
    j.borrow_mut().counter += 2.0;
}

fn throwback(j: &JokerRef, ctx: &mut ScoreContext, game: &mut GameState) {
    // Card:update recomputes this as 1 + skips * 0.25 every frame, so the
    // joker keeps no growth of its own however much the tooltip looks like it
    // does.
    ctx.times_mult(1.0 + 0.25 * game.blinds_skipped as f64, j.borrow().spec.name);
}

/// `_glass_joker`: X0.75 for each glass card that *shattered*.
///
/// Not each glass card destroyed. The game keeps two lists when a hand
/// finishes scoring: `removed`, which is every playing card destroyed and is
/// what Canio feeds on, and `glass_shattered`, which is the subset carrying
/// `.shattered`. That flag is set in exactly one place -- a Glass Card in the
/// scoring hand, undebuffed, whose one-in-four came up. A glass card taken by
/// a Hanged Man is marked `destroyed` instead and Glass Joker gets nothing
/// for it.
fn glass_joker(j: &JokerRef, cards: &[CardRef], _game: &mut GameState) {
    j.borrow_mut().counter += 0.75 * cards.len() as f64;
}

/// The Lucky Cat trigger: grow a quarter for each Lucky trigger
/// (card.lua:3076-3081; see scoring.score_hand for when it is asked).
fn lucky_trigger(j: &JokerRef, _card: &CardRef, _ctx: &mut ScoreContext, _game: &mut GameState) {
    bump(j, 0.25, None);
}

/// Wee Joker's growth: `not context.blueprint` (card.lua:3083-3085), grown on
/// its own account only, and a copy adds the chips without growing them.
fn wee_growth(j: &JokerRef, card: &CardRef, _ctx: &mut ScoreContext, _game: &mut GameState) {
    let (rank, is_stone) = {
        let c = card.borrow();
        (c.rank, c.is_stone())
    };
    if rank == Rank::Two && !is_stone {
        bump(j, 8.0, None);
    }
}

fn spare_trousers_update(j: &JokerRef, ctx: &mut ScoreContext, _game: &mut GameState) {
    if ctx.contains.contains(HandType::TwoPair) {
        bump(j, 2.0, None);
    }
}

/// Hiker: every played card permanently gains +5 Chips when scored.
fn hiker(_j: &JokerRef, card: &CardRef, _ctx: &mut ScoreContext, _game: &mut GameState) {
    card.borrow_mut().extra_chips += 5;
}


/// `_ceremonial_dagger`: eat the joker to the right, keep twice its sell value
/// as mult.
///
/// "To the right" is the next one along in the row, so this depends on the
/// order the player has dragged them into. An eternal joker cannot be eaten
/// -- the game checks it here as well, which is another reason to have the
/// check in one place. The joker was registered with its mult but nothing
/// ever did the eating, so it sat on zero for whole runs while the row kept
/// a joker the game had taken away.
fn ceremonial_dagger(j: &JokerRef, game: &mut GameState) {
    let index = game
        .jokers
        .iter()
        .position(|o| std::rc::Rc::ptr_eq(o, j));
    let index = match index {
        Some(index) if index + 1 < game.jokers.len() => index,
        _ => return,
    };
    let victim = game.jokers[index + 1].clone();
    // The joker to the right as the row stands mid-pass, a victim of Madness
    // included: that one is getting sliced, and the Dagger eats nothing
    // rather than reaching past it (card.lua:2566). Its own victim's slot
    // comes back at once, through the buffer (card.lua:2569); Madness gives
    // none back. The `joker_buffer = 0` callback the Python queues
    // (jokers.py:1138) has no event variant; that reset is the setting_blind
    // pass's business.
    if victim.borrow().eternal || game.is_getting_sliced(&victim) {
        return;
    }
    game.joker_buffer -= 1;
    j.borrow_mut().counter += game.sell_value(&victim) as f64 * 2.0;
    game.slice_joker(&victim, "Ceremonial Dagger");
}

/// `_castle`: +3 chips for each discarded card of the round's suit.
///
/// card.lua:2814, under `context.discard` and per discarded card, skipping a
/// debuffed one. Nothing incremented this counter, so a Castle scored +0
/// chips for a whole run however much was thrown at it -- the same shape as
/// Rocket, and found the same way: the policy played the engine with a
/// simulator shadowing it, and a Full House came out 6840 against 6624.
fn castle(j: &JokerRef, cards: &[CardRef], game: &mut GameState) {
    let suit = match game.castle_suit {
        Some(suit) => suit,
        None => return,
    };
    for card in cards {
        let debuffed = card.borrow().debuffed;
        if !debuffed && suit_matches_for(card, suit, game) {
            j.borrow_mut().counter += 3.0;
        }
    }
}

/// `_obelisk_update`: grow unless the hand just played is the run's most played.
///
/// The game resets only when no *other* hand has been played at least as
/// often, so a tie keeps it growing. Reading it as a stored counter -- which
/// it is, in ability.x_mult -- misses that it is rewritten before every hand.
fn obelisk_update(j: &JokerRef, ctx: &mut ScoreContext, game: &mut GameState) {
    let mine = game.hand_levels.plays.get(&ctx.hand).copied().unwrap_or(0);
    let other_at_least = game
        .hand_levels
        .plays
        .iter()
        .any(|(hand, count)| *hand != ctx.hand && *count >= mine);
    if other_at_least {
        j.borrow_mut().counter += 0.2;
    } else if j.borrow().counter > 1.0 {
        j.borrow_mut().counter = 1.0;
    }
}

/// `_hit_the_road`: X0.5 for every Jack discarded. A debuffed Jack does not
/// count.
///
/// Nor does a Stone one: card.lua:2835-2837 asks `get_id() == 11`, and get_id
/// answers a Stone card with a random negative (card.lua:958-960).
fn hit_the_road(j: &JokerRef, cards: &[CardRef], _game: &mut GameState) {
    let jacks = cards
        .iter()
        .filter(|c| {
            let b = c.borrow();
            b.rank == Rank::Jack && !b.debuffed && !b.is_stone()
        })
        .count();
    j.borrow_mut().counter += 0.5 * jacks as f64;
}

/// `_hit_the_road_end`: back to X1 when the round ends -- "this round" is the
/// whole rule.
///
/// card.lua:3011-3017, under end_of_round, on any blind. Nothing reset it, so
/// every Jack discarded stayed in the X for the rest of the run: Seed
/// VIBC905W, Anaglyph Deck, stake 3, decision 45 on the headless engine, the
/// hand that won the round left the game's on X1 and this on X1.5.
fn hit_the_road_end(j: &JokerRef, _game: &mut GameState) {
    if j.borrow().counter > 1.0 {
        j.borrow_mut().counter = 1.0;
    }
}


// -- chance-based scoring ---------------------------------------------------

fn bloodstone(j: &JokerRef, card: &CardRef, ctx: &mut ScoreContext, game: &mut GameState) {
    if suit_matches(card, Suit::Hearts, game) && chance(game, "bloodstone", 1, 2) {
        ctx.times_mult(1.5, j.borrow().spec.name);
    }
}

fn space_joker(_j: &JokerRef, ctx: &mut ScoreContext, game: &mut GameState) {
    // card.lua:3420-3426: a `before` effect, so the upgrade pays on the very
    // hand that triggered it.
    if !chance(game, "space", 1, 4) {
        return;
    }
    game.hand_levels.level_up(ctx.hand, 1);
}

// -- jokers that name a card or hand the round chose ------------------------

fn the_idol(j: &JokerRef, card: &CardRef, ctx: &mut ScoreContext, game: &mut GameState) {
    let rank = card.borrow().rank;
    if game.idol_rank == Some(rank) {
        if let Some(suit) = game.idol_suit {
            if suit_matches(card, suit, game) {
                ctx.times_mult(2.0, j.borrow().spec.name);
            }
        }
    }
}

fn ancient_joker(j: &JokerRef, card: &CardRef, ctx: &mut ScoreContext, game: &mut GameState) {
    if let Some(suit) = game.ancient_suit {
        if suit_matches(card, suit, game) {
            ctx.times_mult(1.5, j.borrow().spec.name);
        }
    }
}

/// To Do List's `before`: `context.before`, with no `not context.blueprint`
/// (card.lua:3491): paid ahead of Bootstraps and Bull, and by a copy too.
fn to_do_list(j: &JokerRef, ctx: &mut ScoreContext, _game: &mut GameState) {
    if j.borrow().named_hand == Some(ctx.hand) {
        ctx.money_gained += 4;
    }
}

/// `_midas_mask`: turn every scoring face card to Gold before anything scores.
///
/// context.before, not per-card: the game walks the scoring hand once, up
/// front, and calls set_ability(m_gold) on each face card. That matters
/// because a card only has one enhancement -- a glass King turned gold by a
/// Midas Mask never gets to be glass, so its X2 is simply gone. Converting
/// per card as it scored let the first trigger keep the old enhancement,
/// which is a whole X2 on the hand.
///
/// Not a debuffed one: the game asks `v:is_face()` (card.lua:3446), and
/// Card:is_face answers nothing for a debuffed card unless a boss is asking
/// (card.lua:964-965), so The Plant's face cards stay what they were.
fn midas_mask(_j: &JokerRef, ctx: &mut ScoreContext, game: &mut GameState) {
    for card in &ctx.scoring {
        let debuffed = card.borrow().debuffed;
        if is_face(card, game) && !debuffed {
            game.set_enhancement(card, Enhancement::Gold);
        }
    }
}

fn seltzer_update(j: &JokerRef, _ctx: &mut ScoreContext, game: &mut GameState) {
    // Ten hands and then it is gone. The countdown runs after the hand it
    // retriggered, so the tenth hand still gets its retrigger and the joker
    // leaves with it. Nothing was counting at all, so a Seltzer bought once
    // retriggered for the rest of the run.
    decay(j, -1.0, game, 0.0);
}

fn seltzer_retrigger(
    j: &JokerRef,
    _card: &CardRef,
    _ctx: &mut ScoreContext,
    _game: &mut GameState,
) -> i32 {
    if j.borrow().counter > 0.0 {
        1
    } else {
        0
    }
}

/// `_matador_triggered`: `G.GAME.blind.triggered`, and nothing else
/// (card.lua:2737, 3720).
///
/// Not "is this a boss": a Flush into The Head debuffs no hand and triggers
/// nothing, and paying for every boss hand made the simulator eight dollars a
/// hand richer than the game. GameState._play and score_hand set it.
fn matador_triggered(game: &GameState) -> bool {
    game.blind.as_ref().map_or(false, |blind| blind.triggered)
}

fn matador(_j: &JokerRef, ctx: &mut ScoreContext, game: &mut GameState) {
    if matador_triggered(game) {
        ctx.money_gained += 8;
    }
}

/// Matador's `on_debuffed_hand`: a refused hand scores nothing but still asks
/// every joker, under context.debuffed_hand (state_events.lua:1015-1027), and
/// Matador is the one joker that answers (card.lua:2735-2745).
fn matador_debuffed(j: &JokerRef, game: &mut GameState) {
    if matador_triggered(game) {
        game.add_money(8, j.borrow().spec.name);
    }
}


fn red_card_skip(j: &JokerRef, _game: &mut GameState) {
    j.borrow_mut().counter += 3.0;
}

/// `_madness`: gain X0.5 and eat a joker -- but not on a Boss Blind.
///
/// `not context.blind.boss`, so it feeds on the Small and Big and goes quiet
/// for the one that matters. The joker it takes is drawn from the ones that
/// are neither itself nor eternal. Nothing was growing it and nothing was
/// eating, so it sat at X1 while its own drawback never arrived.
///
/// The draw is pseudorandom_element(destructable_jokers,
/// pseudoseed('madness')) (card.lua:2509), which sorts the list by sort_id
/// before indexing (misc_functions.lua:260-261) -- by age, not by where the
/// jokers sit. Row order ate Popcorn where the game ate Mystic Summit, on
/// smoke run N1OA90W1 (test_madness_eats_by_age).
fn madness(j: &JokerRef, game: &mut GameState) {
    if let Some(blind) = &game.blind {
        if blind.kind == BlindKind::Boss {
            return;
        }
    }
    j.borrow_mut().counter += 0.5;
    // Not a joker already getting sliced (card.lua:2507). The one taken is
    // only marked, and keeps its place and its slot until the pass is over.
    let mut prey: Vec<JokerRef> = game
        .jokers
        .iter()
        .filter(|o| {
            !std::rc::Rc::ptr_eq(o, j) && !o.borrow().eternal && !game.is_getting_sliced(o)
        })
        .cloned()
        .collect();
    prey.sort_by_key(|o| o.borrow().uid);
    if !prey.is_empty() {
        let chosen = game.rng.choice("madness", &prey);
        let reason = j.borrow().spec.name;
        game.slice_joker(&chosen, reason);
    }
}

/// `_burglar`: three hands, and every discard gone.
///
/// Both halves land here rather than in the round's allowance, and the
/// difference is only visible against a boss that dictates the allowance
/// itself. Burglar is ease_hands_played(+3) on setting_blind, which runs
/// *after* the blind has set the round up -- so The Needle, which allows one
/// hand, allows four with a Burglar held. Folding the +3 into the base let
/// The Needle overwrite it and the run played a single hand. Measured on the
/// engine: four.
fn burglar(_j: &JokerRef, game: &mut GameState) {
    game.hands_left += 3;
    game.discards_left = 0;
}

// -- money at the end of a round --------------------------------------------

fn cloud_9(_j: &JokerRef, game: &mut GameState) {
    let nines = game
        .full_deck
        .iter()
        .filter(|c| c.borrow().rank == Rank::Nine)
        .count() as i32;
    if nines != 0 {
        game.add_money(nines, "Cloud 9");
    }
}

/// `_rocket`: +$2 a boss, and the boss that raises it is already paying the
/// raise.
///
/// The growth is `calculate_joker({end_of_round})` (card.lua:2896, guarded on
/// `G.GAME.blind.boss`), which state_events.lua runs at line 101 -- and the
/// cash-out rows are built from calculate_dollar_bonus afterwards, at line
/// 1176. So a Rocket held through its first boss pays three dollars that
/// round, not one. Nothing incremented this counter at all, so it paid a
/// dollar a round for whole runs; found by the hand-written policy playing
/// the engine with a simulator shadowing it, which is what live.py is for.
fn rocket_end(j: &JokerRef, game: &mut GameState) {
    if game.beaten_was_boss {
        j.borrow_mut().counter += 2.0;
    }
}

fn rocket_money(j: &JokerRef, game: &mut GameState) {
    let value = j.borrow().counter;
    if value != 0.0 {
        game.add_money(value as i32, j.borrow().spec.name);
    }
}

fn satellite_money(j: &JokerRef, game: &mut GameState) {
    let value = game.unique_planets.len() as i32;
    if value != 0 {
        game.add_money(value, j.borrow().spec.name);
    }
}

fn egg_end(j: &JokerRef, _game: &mut GameState) {
    j.borrow_mut().extra_sell_value += 3.0;
}

/// `_gift_card`: a dollar on every joker and every consumable held
/// (card.lua:2993-3005).
///
/// The consumables were left out, so one held through rounds beside a Gift
/// Card sold for its bare price.
fn gift_card(_j: &JokerRef, game: &mut GameState) {
    for other in game.jokers.clone() {
        other.borrow_mut().extra_sell_value += 1.0;
    }
    for held in game.consumables.clone() {
        held.borrow_mut().extra_sell_value += 1;
    }
}

fn delayed_gratification(j: &JokerRef, game: &mut GameState) {
    if game.discards_used == 0 {
        let value = 2 * game.discards_left;
        if value != 0 {
            game.add_money(value, j.borrow().spec.name);
        }
    }
}

/// `_mail_in`: $5 for each discarded card of the round's rank -- a live, ranked
/// one.
///
/// card.lua:2825-2827, per discarded card:
///
/// ```text
///     not context.other_card.debuff and
///     context.other_card:get_id() == G.GAME.current_round.mail_card.id
/// ```
///
/// A debuffed card is skipped, and a Stone card never matches, because get_id
/// answers it with `-math.random(100, 1000000)` (card.lua:958-960). The smoke
/// test found both, $5 out each time: NXE7XRN1, UBDY5AUG and XGC81J77 threw
/// a card of the rank that The Pillar or The Club had debuffed, and WA1RMJNV
/// threw a Stone Ace in an Ace round.
fn mail_in(j: &JokerRef, cards: &[CardRef], game: &mut GameState) {
    let rank = match game.mail_rank {
        Some(rank) => rank,
        None => return,
    };
    let matched = cards
        .iter()
        .filter(|c| {
            let b = c.borrow();
            !b.debuffed && !b.is_stone() && b.rank == rank
        })
        .count() as i32;
    game.add_money(5 * matched, j.borrow().spec.name);
}


// -- shop and run structure -------------------------------------------------

/// `_invisible_sold`: sold after two rounds, a copy of a random other joker
/// (card.lua:2371-2390).
///
/// ```text
///     if invis_rounds >= extra (2, game.lua:513) and not context.blueprint
///         jokers = G.jokers.cards other than self
///         if #jokers > 0 and #G.jokers.cards <= G.jokers.config.card_limit
///             chosen = pseudorandom_element(jokers, pseudoseed('invisible'))
///             card = copy_card(chosen, ..., chosen.edition.negative)
///             if card.ability.invis_rounds then card.ability.invis_rounds = 0
/// ```
///
/// selling_self fires before the card dissolves (card.lua:1599), so the room
/// check counts this joker as still in the row, and a Negative one as still
/// giving its slot. The simulator runs this after the pop, so both go back
/// in. pseudorandom_element sorts by sort_id first; the row's order is not
/// the draw's. A debuffed joker answers no context (card.lua:2292).
fn invisible_sold(j: &JokerRef, game: &mut GameState) {
    if j.borrow().debuffed || j.borrow().counter < 2.0 {
        return;
    }
    let mut others: Vec<JokerRef> = game.jokers.iter().cloned().collect();
    others.sort_by_key(|o| o.borrow().uid);
    if others.is_empty() {
        return;
    }
    let held = game.jokers.len() as i32 + 1;
    let negative = j.borrow().edition == Edition::Negative;
    let limit = game.joker_slots() + if negative { 1 } else { 0 };
    if held > limit {
        return;
    }
    let chosen = game.rng.choice("invisible", &others);
    let reason = j.borrow().spec.name;
    let clone = game.copy_joker(&chosen, reason);
    let same_spec = std::ptr::eq(clone.borrow().spec, j.borrow().spec);
    if same_spec {
        clone.borrow_mut().counter = 0.0;
    }
}

fn diet_cola(_j: &JokerRef, game: &mut GameState) {
    game.add_tag_by_key("tag_double");
}

/// A random front out of `shop_pool.FRONTS`, as Marble Joker and Certificate
/// build it: rank and suit together from one draw, then the body.
fn random_front_card(
    game: &mut GameState,
    key: &str,
    enhancement: Enhancement,
    seal: crate::cards::Seal,
) -> CardRef {
    let front = game.rng.choice(key, &crate::shop_pool::FRONTS);
    let (suit_code, rank_code) = front
        .split_once('_')
        .unwrap_or_else(|| panic!("bad front {:?}", front));
    let rank = Rank::from_code(rank_code).unwrap_or_else(|| panic!("bad rank {:?}", rank_code));
    let suit = Suit::from_code(suit_code).unwrap_or_else(|| panic!("bad suit {:?}", suit_code));
    let card = crate::cards::make_card(rank, suit);
    {
        let mut c = card.borrow_mut();
        c.enhancement = enhancement;
        c.seal = seal;
    }
    card
}

/// `_marble`: a Stone card with a random front, into the deck.
///
/// card.lua:2583 draws the front out of the whole of G.P_CARDS, keyed by
/// string and so sorted the way shop_pool.FRONTS is -- the same draw
/// Certificate makes, under its own pool name. A Stone card scores no rank
/// or suit, but the front is what the hand shows and what orders two Stone
/// cards against each other (get_nominal, card.lua:950-955). Always making
/// an Ace of Spades showed up on seed TTL5O2HL as a 7C in the game's hand.
fn marble(_j: &JokerRef, game: &mut GameState) {
    let card = random_front_card(game, "marb_fr", Enhancement::Stone, crate::cards::Seal::None);
    game.add_card(&card);
}

/// `_certificate`: a random card with a random seal, straight into the hand.
///
/// Two draws, not three: the game picks a face out of G.P_CARDS in one go
/// -- rank and suit together, from the same pool -- and then rolls the seal
/// against thresholds. Drawing the rank and the suit separately is three
/// rolls from names the game does not have.
fn certificate(_j: &JokerRef, game: &mut GameState) {
    let roll = game.rng.pseudorandom("certsl", None, None);
    let seal = if roll > 0.75 {
        crate::cards::Seal::Red
    } else if roll > 0.5 {
        crate::cards::Seal::Blue
    } else if roll > 0.25 {
        crate::cards::Seal::Gold
    } else {
        crate::cards::Seal::Purple
    };
    let card = random_front_card(game, "cert_fr", Enhancement::None, seal);
    // Into the hand *and* into the deck. The game makes a real playing card
    // -- create_playing_card registers it in G.playing_cards -- so the run is
    // fifty-three cards from here on and every later draw comes off a
    // different deck. Putting it only in the hand loses it at the end of the
    // round.
    game.add_card_to_hand(&card);
    // The event ends with G.hand:sort() (card.lua:2476), so the card takes its
    // place by the hand's sort rather than joining the end.
    game.sort_hand("rank");
}


fn cartomancer(_j: &JokerRef, game: &mut GameState) {
    let specs = game.random_consumables(ConsumableKind::Tarot, 1, "car");
    game.add_consumables(&specs, Edition::None);
}

/// `_riff_raff`: two Common jokers, from Riff-Raff's own streams.
///
/// card.lua:2529-2543 makes them with
/// `create_card('Joker', G.jokers, nil, 0, nil, nil, nil, 'rif')`. The 'rif'
/// was missing, so the pool drawn was "Joker1<ante>" rather than
/// "Joker1rif<ante>": two believable Commons, nearly always the wrong two.
/// (The forced rarity of 0 is no roll -- Lua's 0 is truthy and below both
/// thresholds -- which Rarity.COMMON already says.)
///
/// And the count is settled before either exists: jokers_to_create is
/// min(2, card_limit - (#jokers + joker_buffer)), taken when the blind is
/// selected. A Negative first joker raises the limit as it arrives, but one
/// free slot has already been turned into one joker.
///
/// Counted against the row as it stands mid-pass: a joker getting sliced
/// still takes its slot, and joker_buffer carries what a Dagger handed back
/// and what an earlier Riff-raff, or a Blueprint's copy, already promised.
/// The jokers themselves arrive in an event, after the pass (card.lua:2532).
fn riff_raff(j: &JokerRef, game: &mut GameState) {
    let room = game.joker_slots() - (game.jokers.len() as i32 + game.joker_buffer);
    if room <= 0 {
        return;
    }
    let count = room.min(2);
    game.joker_buffer += count;
    game.after_setting_blind(BlindSelectEvent::CreateJokers {
        source_uid: j.borrow().uid,
        count,
    });
}

/// 8 Ball: 1 in 4 for each played 8 to create a Tarot card when scored.
///
/// Room first, then the roll (card.lua:3106-3107): a full row spends no draw
/// from '8ball'. Rolling anyway put the stream two draws ahead after a hand
/// of three 8s whose first made a Tarot, and 90WTJQJP missed a High
/// Priestess the game made later. A Tarot made here is added at once, so the
/// length also plays the part of G.GAME.consumeable_buffer for the next 8 in
/// the same hand.
fn eight_ball(_j: &JokerRef, card: &CardRef, _ctx: &mut ScoreContext, game: &mut GameState) {
    if (game.consumables.len() as i32) >= game.consumable_slots() {
        return;
    }
    let (rank, is_stone) = {
        let c = card.borrow();
        (c.rank, c.is_stone())
    };
    if rank != Rank::Eight || is_stone {
        return;
    }
    if !chance(game, "8ball", 1, 4) {
        return;
    }
    let specs = game.random_consumables(ConsumableKind::Tarot, 1, "8ba");
    game.add_consumables(&specs, Edition::None);
}

/// `_hallucination`: one in two to make a Tarot whenever a booster pack is
/// opened.
///
/// The odds are drawn against "halu" plus the ante, and the room check comes
/// first -- a full row of consumables costs no roll at all.
///
/// The Tarot is built after the pack's own cards (see GameState._open_pack),
/// so it is drawn from a pool with the pack's Tarots already blanked.
fn hallucination(_j: &JokerRef, game: &mut GameState) {
    if (game.consumables.len() as i32) >= game.consumable_slots() {
        return;
    }
    let key = format!("halu{}", game.ante);
    if game.rng.chance(&key, game.probability_scale(), 2.0) {
        let specs = game.random_consumables(ConsumableKind::Tarot, 1, "hal");
        game.add_consumables(&specs, Edition::None);
    }
}

fn superposition(_j: &JokerRef, ctx: &mut ScoreContext, game: &mut GameState) {
    let has_ace = ctx.scoring.iter().any(|c| c.borrow().rank == Rank::Ace);
    if ctx.contains.contains(HandType::Straight) && has_ace {
        let specs = game.random_consumables(ConsumableKind::Tarot, 1, "sup");
        game.add_consumables(&specs, Edition::None);
    }
}

fn seance(_j: &JokerRef, ctx: &mut ScoreContext, game: &mut GameState) {
    // `next(context.poker_hands[...])`, like every other joker that names a
    // hand -- not a test of what the hand *is*. The two agree on any hand
    // vanilla can make, since nothing that outranks a Straight Flush
    // contains one, but reading the top hand is the wrong shape and would be
    // wrong the moment a mod or an unusual joker made one.
    if ctx.contains.contains(HandType::StraightFlush) {
        let specs = game.random_consumables(ConsumableKind::Spectral, 1, "sea");
        game.add_consumables(&specs, Edition::None);
    }
}


/// Vagabond: `G.GAME.dollars <= extra` in joker_main (card.lua:3743-3744), and
/// the hand's own payouts are `ease_dollars` events still queued then:
/// Matador's $8 left of it did not stop the game's tarot, and read after the
/// hand it stopped this one. Seed FATMAN06, Yellow Deck, decision 23: a lone
/// Ace of Clubs into The Club with $0, the game made The Devil and this made
/// nothing.
fn vagabond(_j: &JokerRef, ctx: &mut ScoreContext, game: &mut GameState) {
    let money = game.money_at_play.unwrap_or(ctx.money());
    if money <= 4 {
        let specs = game.random_consumables(ConsumableKind::Tarot, 1, "vag");
        game.add_consumables(&specs, Edition::None);
    }
}

fn sixth_sense(_j: &JokerRef, played: &[CardRef], game: &mut GameState) {
    if played.len() == 1 && played[0].borrow().rank == Rank::Six {
        game.remove_card(&played[0], false);
        let specs = game.random_consumables(ConsumableKind::Spectral, 1, "sixth");
        game.add_consumables(&specs, Edition::None);
    }
}

fn dna(_j: &JokerRef, played: &[CardRef], game: &mut GameState) {
    if played.len() == 1 {
        let copy = crate::cards::copy_card(&played[0]);
        game.add_card_to_hand(&copy);
    }
}

fn trading_card(_j: &JokerRef, cards: &[CardRef], game: &mut GameState) {
    if cards.len() == 1 {
        // The other place the shatter flag is set before the jokers look.
        let shattered = cards[0].borrow().enhancement == Enhancement::Glass;
        game.remove_card(&cards[0], shattered);
        game.add_money(3, "Trading Card");
    }
}

fn burnt(_j: &JokerRef, cards: &[CardRef], game: &mut GameState) {
    let hand = game.evaluate_selection(cards).hand;
    game.hand_levels.level_up(hand, 1);
}


fn gros_michel_mult(j: &JokerRef, ctx: &mut ScoreContext, _game: &mut GameState) {
    ctx.add_mult(15.0, j.borrow().spec.name);
}

fn cavendish(j: &JokerRef, ctx: &mut ScoreContext, _game: &mut GameState) {
    ctx.times_mult(3.0, j.borrow().spec.name);
}


/// Turtle Bean's round end: `_decay(j, -1, g)`.
fn turtle_bean_end(j: &JokerRef, game: &mut GameState) {
    decay(j, -1.0, game, 0.0);
}

/// Invisible Joker's round end: `_bump(j, 1)` -- one round closer to its two.
fn invisible_round_end(j: &JokerRef, _game: &mut GameState) {
    bump(j, 1.0, None);
}


/// Golden Ticket: a played Gold card earns $4.
fn golden_ticket(_j: &JokerRef, card: &CardRef, ctx: &mut ScoreContext, _game: &mut GameState) {
    if card.borrow().enhancement == Enhancement::Gold {
        ctx.money_gained += 4;
    }
}


// --------------------------------------------------------------------------
// the registry
// --------------------------------------------------------------------------

/// Every registered joker, in the order the Python file registers them.
pub const SPECS: &[JokerSpec] = &[
    JokerSpec { name: "Joker", rarity: Rarity::Common, text: "+4 Mult", cost: 2,
                independent: Some(joker_indep), ..JokerSpec::DEFAULT },
    JokerSpec { name: "Greedy Joker", rarity: Rarity::Common, text: "+3 Mult per scored Diamonds", cost: 5,
                scored: Some(suit_scorer(Suit::Diamonds, 3)), ..JokerSpec::DEFAULT },
    JokerSpec { name: "Lusty Joker", rarity: Rarity::Common, text: "+3 Mult per scored Hearts", cost: 5,
                scored: Some(suit_scorer(Suit::Hearts, 3)), ..JokerSpec::DEFAULT },
    JokerSpec { name: "Wrathful Joker", rarity: Rarity::Common, text: "+3 Mult per scored Spades", cost: 5,
                scored: Some(suit_scorer(Suit::Spades, 3)), ..JokerSpec::DEFAULT },
    JokerSpec { name: "Gluttonous Joker", rarity: Rarity::Common, text: "+3 Mult per scored Clubs", cost: 5,
                scored: Some(suit_scorer(Suit::Clubs, 3)), ..JokerSpec::DEFAULT },
    JokerSpec { name: "Jolly Joker", rarity: Rarity::Common, text: "+8 Mult if hand contains a Pair", cost: 3,
                independent: Some(hand_mult(HandType::Pair, 8)), ..JokerSpec::DEFAULT },
    JokerSpec { name: "Zany Joker", rarity: Rarity::Common, text: "+12 Mult if hand contains Three of a Kind", cost: 4,
                independent: Some(hand_mult(HandType::ThreeOfAKind, 12)), ..JokerSpec::DEFAULT },
    JokerSpec { name: "Mad Joker", rarity: Rarity::Common, text: "+10 Mult if hand contains Two Pair", cost: 4,
                independent: Some(hand_mult(HandType::TwoPair, 10)), ..JokerSpec::DEFAULT },
    JokerSpec { name: "Crazy Joker", rarity: Rarity::Common, text: "+12 Mult if hand contains a Straight", cost: 4,
                independent: Some(hand_mult(HandType::Straight, 12)), ..JokerSpec::DEFAULT },
    JokerSpec { name: "Droll Joker", rarity: Rarity::Common, text: "+10 Mult if hand contains a Flush", cost: 4,
                independent: Some(hand_mult(HandType::Flush, 10)), ..JokerSpec::DEFAULT },
    JokerSpec { name: "Sly Joker", rarity: Rarity::Common, text: "+50 Chips if hand contains a Pair", cost: 3,
                independent: Some(hand_chips(HandType::Pair, 50)), ..JokerSpec::DEFAULT },
    JokerSpec { name: "Wily Joker", rarity: Rarity::Common, text: "+100 Chips if hand contains Three of a Kind", cost: 4,
                independent: Some(hand_chips(HandType::ThreeOfAKind, 100)), ..JokerSpec::DEFAULT },
    JokerSpec { name: "Clever Joker", rarity: Rarity::Common, text: "+80 Chips if hand contains Two Pair", cost: 4,
                independent: Some(hand_chips(HandType::TwoPair, 80)), ..JokerSpec::DEFAULT },
    JokerSpec { name: "Devious Joker", rarity: Rarity::Common, text: "+100 Chips if hand contains a Straight", cost: 4,
                independent: Some(hand_chips(HandType::Straight, 100)), ..JokerSpec::DEFAULT },
    JokerSpec { name: "Crafty Joker", rarity: Rarity::Common, text: "+80 Chips if hand contains a Flush", cost: 4,
                independent: Some(hand_chips(HandType::Flush, 80)), ..JokerSpec::DEFAULT },
    JokerSpec { name: "Half Joker", rarity: Rarity::Common, text: "+20 Mult if 3 or fewer cards played", cost: 5,
                independent: Some(half_joker), ..JokerSpec::DEFAULT },
    JokerSpec { name: "Banner", rarity: Rarity::Common, text: "+30 Chips per remaining discard", cost: 5,
                independent: Some(banner), ..JokerSpec::DEFAULT },
    JokerSpec { name: "Mystic Summit", rarity: Rarity::Common, text: "+15 Mult with 0 discards remaining", cost: 5,
                independent: Some(mystic_summit), ..JokerSpec::DEFAULT },
    JokerSpec { name: "Misprint", rarity: Rarity::Common, text: "+0 to +23 Mult", cost: 4,
                independent: Some(misprint), ..JokerSpec::DEFAULT },
    JokerSpec { name: "Even Steven", rarity: Rarity::Common, text: "+4 Mult per scored even card", cost: 4,
                scored: Some(rank_scorer(&EVEN, 0, 4)), ..JokerSpec::DEFAULT },
    JokerSpec { name: "Odd Todd", rarity: Rarity::Common, text: "+31 Chips per scored odd card", cost: 4,
                scored: Some(rank_scorer(&ODD, 31, 0)), ..JokerSpec::DEFAULT },
    JokerSpec { name: "Fibonacci", rarity: Rarity::Uncommon, text: "+8 Mult per scored A, 2, 3, 5 or 8", cost: 8,
                scored: Some(rank_scorer(&FIB, 0, 8)), ..JokerSpec::DEFAULT },
    JokerSpec { name: "Scholar", rarity: Rarity::Common, text: "Scored Aces give +20 Chips and +4 Mult", cost: 4,
                scored: Some(rank_scorer(&[Rank::Ace], 20, 4)), ..JokerSpec::DEFAULT },
    JokerSpec { name: "Walkie Talkie", rarity: Rarity::Common, text: "Scored 10s and 4s give +10 Chips and +4 Mult", cost: 4,
                scored: Some(rank_scorer(&[Rank::Ten, Rank::Four], 10, 4)), ..JokerSpec::DEFAULT },
    JokerSpec { name: "Scary Face", rarity: Rarity::Common, text: "+30 Chips per scored face card", cost: 4,
                scored: Some(face_scorer(30, 0)), ..JokerSpec::DEFAULT },
    JokerSpec { name: "Smiley Face", rarity: Rarity::Common, text: "+5 Mult per scored face card", cost: 4,
                scored: Some(face_scorer(0, 5)), ..JokerSpec::DEFAULT },

    JokerSpec { name: "Abstract Joker", rarity: Rarity::Common, text: "+3 Mult per Joker held", cost: 4,
                independent: Some(abstract_joker), ..JokerSpec::DEFAULT },
    JokerSpec { name: "Blue Joker", rarity: Rarity::Common, text: "+2 Chips per card left in deck", cost: 5,
                independent: Some(blue_joker), ..JokerSpec::DEFAULT },
    JokerSpec { name: "Bull", rarity: Rarity::Uncommon, text: "+2 Chips per dollar held", cost: 6,
                independent: Some(bull), ..JokerSpec::DEFAULT },
    JokerSpec { name: "Bootstraps", rarity: Rarity::Uncommon, text: "+2 Mult per $5 held", cost: 7,
                independent: Some(bootstraps), ..JokerSpec::DEFAULT },
    JokerSpec { name: "Ride the Bus", rarity: Rarity::Common,
                text: "+1 Mult per consecutive hand without a scored face card", cost: 6,
                update: Some(ride_update), update_before_scoring: true,
                independent: Some(ride_the_bus), ..JokerSpec::DEFAULT },
    JokerSpec { name: "Green Joker", rarity: Rarity::Common, text: "+1 Mult per hand played, -1 per discard", cost: 4,
                update: Some(green_update), update_before_scoring: true,
                discarded: Some(green_discard), independent: Some(green_joker), ..JokerSpec::DEFAULT },
    JokerSpec { name: "Runner", rarity: Rarity::Common, text: "+15 Chips, gains +15 Chips per Straight played", cost: 5,
                init_counter: 0.0, update: Some(runner_update), update_before_scoring: true,
                independent: Some(runner), ..JokerSpec::DEFAULT },
    JokerSpec { name: "Ice Cream", rarity: Rarity::Common, text: "+100 Chips, -5 Chips per hand played", cost: 5,
                init_counter: 100.0, independent: Some(ice_cream),
                update: Some(ice_cream_update), ..JokerSpec::DEFAULT },
    JokerSpec { name: "Square Joker", rarity: Rarity::Common, text: "+4 Chips per hand played with exactly 4 cards", cost: 4,
                update: Some(square_update), update_before_scoring: true,
                independent: Some(square_joker), ..JokerSpec::DEFAULT },
    JokerSpec { name: "Supernova", rarity: Rarity::Common, text: "+Mult equal to times this hand has been played", cost: 5,
                independent: Some(supernova), ..JokerSpec::DEFAULT },
    JokerSpec { name: "Popcorn", rarity: Rarity::Common, text: "+20 Mult, -4 Mult per round played", cost: 5,
                init_counter: 20.0, independent: Some(popcorn), round_end: Some(popcorn_end), ..JokerSpec::DEFAULT },
    JokerSpec { name: "Swashbuckler", rarity: Rarity::Common, text: "+Mult equal to sell value of other Jokers", cost: 4,
                independent: Some(swashbuckler), ..JokerSpec::DEFAULT },
    JokerSpec { name: "Golden Joker", rarity: Rarity::Common, text: "Earn $4 at end of round", cost: 6,
                round_money: Some(golden_joker), ..JokerSpec::DEFAULT },
    JokerSpec { name: "Faceless Joker", rarity: Rarity::Common, text: "Earn $5 if 3+ face cards discarded", cost: 4,
                discarded: Some(faceless), ..JokerSpec::DEFAULT },
    JokerSpec { name: "Gros Michel", rarity: Rarity::Common, text: "+15 Mult, 1 in 6 chance to be destroyed", cost: 5,
                independent: Some(gros_michel_mult), round_end: Some(gros_michel_end), ..JokerSpec::DEFAULT },
    JokerSpec { name: "Cavendish", rarity: Rarity::Common, text: "X3 Mult", cost: 4,
                independent: Some(cavendish), round_end: Some(cavendish_end), ..JokerSpec::DEFAULT },

    JokerSpec { name: "The Duo", rarity: Rarity::Rare, text: "X2 Mult if hand contains a Pair", cost: 8,
                independent: Some(hand_xmult(HandType::Pair, 2.0)), ..JokerSpec::DEFAULT },
    JokerSpec { name: "The Trio", rarity: Rarity::Rare, text: "X3 Mult if hand contains Three of a Kind", cost: 8,
                independent: Some(hand_xmult(HandType::ThreeOfAKind, 3.0)), ..JokerSpec::DEFAULT },
    JokerSpec { name: "The Family", rarity: Rarity::Rare, text: "X4 Mult if hand contains Four of a Kind", cost: 8,
                independent: Some(hand_xmult(HandType::FourOfAKind, 4.0)), ..JokerSpec::DEFAULT },
    JokerSpec { name: "The Order", rarity: Rarity::Rare, text: "X3 Mult if hand contains a Straight", cost: 8,
                independent: Some(hand_xmult(HandType::Straight, 3.0)), ..JokerSpec::DEFAULT },
    JokerSpec { name: "The Tribe", rarity: Rarity::Rare, text: "X2 Mult if hand contains a Flush", cost: 8,
                independent: Some(hand_xmult(HandType::Flush, 2.0)), ..JokerSpec::DEFAULT },
    JokerSpec { name: "Baron", rarity: Rarity::Rare, text: "Kings held in hand give X1.5 Mult", cost: 8,
                held: Some(baron), ..JokerSpec::DEFAULT },
    JokerSpec { name: "Blackboard", rarity: Rarity::Uncommon,
                text: "X3 Mult if all cards held in hand are Spades or Clubs", cost: 6,
                independent: Some(blackboard), ..JokerSpec::DEFAULT },
    JokerSpec { name: "Card Sharp", rarity: Rarity::Uncommon,
                text: "X3 Mult if this hand type was already played this round", cost: 6,
                independent: Some(card_sharp), ..JokerSpec::DEFAULT },
    JokerSpec { name: "Flower Pot", rarity: Rarity::Uncommon, text: "X3 Mult if scoring hand has all 4 suits", cost: 6,
                independent: Some(flower_pot), ..JokerSpec::DEFAULT },
    JokerSpec { name: "Stuntman", rarity: Rarity::Rare, text: "+250 Chips, -2 hand size", cost: 7, hand_size: -2,
                independent: Some(stuntman), ..JokerSpec::DEFAULT },
    JokerSpec { name: "Steel Joker", rarity: Rarity::Uncommon, text: "X0.2 Mult per Steel card in your deck", cost: 7,
                enhancement_gate: "m_steel", independent: Some(steel_joker), ..JokerSpec::DEFAULT },
    JokerSpec { name: "Vampire", rarity: Rarity::Uncommon,
                text: "X0.1 Mult per scored enhanced card, removing the enhancement", cost: 7,
                init_counter: 1.0, update: Some(vampire_update), update_before_scoring: true,
                independent: Some(times_counter), ..JokerSpec::DEFAULT },
    JokerSpec { name: "Ramen", rarity: Rarity::Uncommon, text: "X2 Mult, -X0.01 per discarded card", cost: 6,
                init_counter: 2.0, discarded: Some(ramen_discard), independent: Some(times_counter), ..JokerSpec::DEFAULT },
    JokerSpec { name: "Loyalty Card", rarity: Rarity::Uncommon, text: "X4 Mult every 6th hand played", cost: 5,
                independent: Some(loyalty_card), ..JokerSpec::DEFAULT },
    JokerSpec { name: "Hologram", rarity: Rarity::Uncommon, text: "X0.25 Mult per playing card added to your deck", cost: 7,
                init_counter: 1.0, independent: Some(times_counter), ..JokerSpec::DEFAULT },
    JokerSpec { name: "Baseball Card", rarity: Rarity::Rare, text: "Uncommon Jokers each give X1.5 Mult", cost: 8,
                other_joker: Some(baseball_card), ..JokerSpec::DEFAULT },

    JokerSpec { name: "Hack", rarity: Rarity::Uncommon, text: "Retrigger each played 2, 3, 4 or 5", cost: 6,
                retrigger_scored: Some(hack), ..JokerSpec::DEFAULT },
    JokerSpec { name: "Sock and Buskin", rarity: Rarity::Uncommon, text: "Retrigger all scored face cards", cost: 6,
                retrigger_scored: Some(sock_and_buskin), ..JokerSpec::DEFAULT },
    JokerSpec { name: "Hanging Chad", rarity: Rarity::Common, text: "Retrigger the first scored card 2 extra times", cost: 4,
                retrigger_scored: Some(hanging_chad), ..JokerSpec::DEFAULT },
    JokerSpec { name: "Dusk", rarity: Rarity::Uncommon, text: "Retrigger all scored cards on the final hand of the round", cost: 5,
                retrigger_scored: Some(dusk), ..JokerSpec::DEFAULT },
    JokerSpec { name: "Mime", rarity: Rarity::Uncommon, text: "Retrigger all card abilities held in hand", cost: 5,
                retrigger_held: Some(mime), ..JokerSpec::DEFAULT },
    JokerSpec { name: "Four Fingers", rarity: Rarity::Uncommon, text: "Flushes and Straights need only 4 cards", cost: 7,
                ..JokerSpec::DEFAULT },
    JokerSpec { name: "Shortcut", rarity: Rarity::Uncommon, text: "Straights can be made with gaps of 1 rank", cost: 7,
                ..JokerSpec::DEFAULT },
    JokerSpec { name: "Blueprint", rarity: Rarity::Rare, text: "Copies the ability of the Joker to the right", cost: 10,
                copier: Some(Copier::Right), ..JokerSpec::DEFAULT },
    JokerSpec { name: "Brainstorm", rarity: Rarity::Rare, text: "Copies the ability of the leftmost Joker", cost: 10,
                copier: Some(Copier::Leftmost), ..JokerSpec::DEFAULT },
    JokerSpec { name: "Triboulet", rarity: Rarity::Legendary, text: "Played Kings and Queens each give X2 Mult", cost: 20,
                scored: Some(triboulet), ..JokerSpec::DEFAULT },
    JokerSpec { name: "Canio", rarity: Rarity::Legendary, text: "X1 Mult, gains X1 Mult per face card destroyed", cost: 20,
                init_counter: 1.0, on_cards_destroyed: Some(canio), independent: Some(times_counter), ..JokerSpec::DEFAULT },
    JokerSpec { name: "Yorick", rarity: Rarity::Legendary, text: "X1 Mult, gains X1 Mult per 23 cards discarded", cost: 20,
                init_counter: 1.0, init_secondary: 23.0, discarded: Some(yorick),
                independent: Some(times_counter), ..JokerSpec::DEFAULT },
    JokerSpec { name: "Chicot", rarity: Rarity::Legendary, text: "Disables the effect of every Boss Blind", cost: 20,
                ..JokerSpec::DEFAULT },
    JokerSpec { name: "Perkeo", rarity: Rarity::Legendary,
                text: "Creates a Negative copy of a random consumable at the end of the shop", cost: 20,
                on_shop_end: Some(perkeo), ..JokerSpec::DEFAULT },
    JokerSpec { name: "Arrowhead", rarity: Rarity::Uncommon, text: "Played Spades give +50 Chips", cost: 7,
                scored: Some(suit_chips(Suit::Spades, 50)), ..JokerSpec::DEFAULT },
    JokerSpec { name: "Onyx Agate", rarity: Rarity::Uncommon, text: "Played Clubs give +7 Mult", cost: 7,
                scored: Some(suit_scorer(Suit::Clubs, 7)), ..JokerSpec::DEFAULT },
    JokerSpec { name: "Rough Gem", rarity: Rarity::Uncommon, text: "Played Diamonds earn $1", cost: 7,
                scored: Some(suit_money(Suit::Diamonds, 1)), ..JokerSpec::DEFAULT },
    JokerSpec { name: "Golden Ticket", rarity: Rarity::Common, text: "Played Gold cards earn $4", cost: 5,
                enhancement_gate: "m_gold", scored: Some(golden_ticket), ..JokerSpec::DEFAULT },
    JokerSpec { name: "Photograph", rarity: Rarity::Common, text: "First played face card gives X2 Mult", cost: 5,
                scored: Some(photograph), ..JokerSpec::DEFAULT },
    JokerSpec { name: "Shoot the Moon", rarity: Rarity::Common, text: "Each Queen held in hand gives +13 Mult", cost: 5,
                held: Some(shoot_the_moon), ..JokerSpec::DEFAULT },
    JokerSpec { name: "Raised Fist", rarity: Rarity::Common,
                text: "Adds double the rank of the lowest card held in hand to Mult", cost: 5,
                held: Some(raised_fist), ..JokerSpec::DEFAULT },
    JokerSpec { name: "Acrobat", rarity: Rarity::Uncommon, text: "X3 Mult on the final hand of the round", cost: 6,
                independent: Some(acrobat), ..JokerSpec::DEFAULT },
    JokerSpec { name: "Seeing Double", rarity: Rarity::Uncommon,
                text: "X2 Mult if the hand scores a Club and a card of any other suit", cost: 6,
                independent: Some(seeing_double), ..JokerSpec::DEFAULT },
    JokerSpec { name: "Erosion", rarity: Rarity::Uncommon,
                text: "+4 Mult for each card below 52 in your full deck", cost: 6,
                independent: Some(erosion), ..JokerSpec::DEFAULT },

    JokerSpec { name: "Stone Joker", rarity: Rarity::Uncommon,
                text: "+25 Chips for each Stone card in your full deck", cost: 6,
                enhancement_gate: "m_stone", independent: Some(stone_joker), ..JokerSpec::DEFAULT },
    JokerSpec { name: "Driver's License", rarity: Rarity::Rare,
                text: "X3 Mult if you have at least 16 Enhanced cards", cost: 7,
                independent: Some(drivers_license), ..JokerSpec::DEFAULT },
    JokerSpec { name: "Joker Stencil", rarity: Rarity::Uncommon,
                text: "X1 Mult for each empty Joker slot, itself included", cost: 8,
                independent: Some(joker_stencil), ..JokerSpec::DEFAULT },
    JokerSpec { name: "Fortune Teller", rarity: Rarity::Common, text: "+1 Mult per Tarot card used this run", cost: 6,
                independent: Some(fortune_teller), ..JokerSpec::DEFAULT },
    JokerSpec { name: "Constellation", rarity: Rarity::Uncommon, text: "Gains X0.1 Mult per Planet card used", cost: 6,
                init_counter: 1.0, independent: Some(times_counter), ..JokerSpec::DEFAULT },
    JokerSpec { name: "Flash Card", rarity: Rarity::Uncommon, text: "Gains +2 Mult per shop reroll", cost: 5,
                on_reroll: Some(flash_card), independent: Some(add_counter), ..JokerSpec::DEFAULT },
    JokerSpec { name: "Throwback", rarity: Rarity::Uncommon, text: "X0.25 Mult per Blind skipped this run", cost: 6,
                independent: Some(throwback), ..JokerSpec::DEFAULT },
    JokerSpec { name: "Campfire", rarity: Rarity::Rare,
                text: "Gains X0.25 Mult per card sold, resets on a defeated Boss Blind", cost: 9,
                init_counter: 1.0, independent: Some(times_counter), ..JokerSpec::DEFAULT },
    JokerSpec { name: "Glass Joker", rarity: Rarity::Uncommon, text: "Gains X0.75 Mult per Glass card destroyed", cost: 6,
                enhancement_gate: "m_glass", init_counter: 1.0, on_glass_shattered: Some(glass_joker),
                independent: Some(times_counter), ..JokerSpec::DEFAULT },
    JokerSpec { name: "Lucky Cat", rarity: Rarity::Uncommon, text: "Gains X0.25 Mult each time a Lucky card triggers", cost: 6,
                enhancement_gate: "m_lucky", init_counter: 1.0,
                lucky_trigger: Some(lucky_trigger), independent: Some(times_counter), ..JokerSpec::DEFAULT },
    JokerSpec { name: "Wee Joker", rarity: Rarity::Rare, text: "Gains +8 Chips when each played 2 scores", cost: 8,
                scored_growth: Some(wee_growth), independent: Some(chips_counter), ..JokerSpec::DEFAULT },
    JokerSpec { name: "Spare Trousers", rarity: Rarity::Uncommon,
                text: "Gains +2 Mult if the played hand contains a Two Pair", cost: 6,
                update: Some(spare_trousers_update), update_before_scoring: true,
                independent: Some(add_counter), ..JokerSpec::DEFAULT },
    JokerSpec { name: "Hiker", rarity: Rarity::Uncommon,
                text: "Every played card permanently gains +5 Chips when scored", cost: 5,
                scored: Some(hiker), ..JokerSpec::DEFAULT },
    JokerSpec { name: "Ceremonial Dagger", rarity: Rarity::Uncommon,
                text: "When Blind is selected, destroy the Joker to the right and permanently add double its sell value to Mult",
                cost: 6, on_blind_select: Some(ceremonial_dagger),
                independent: Some(add_counter), ..JokerSpec::DEFAULT },
    JokerSpec { name: "Castle", rarity: Rarity::Uncommon,
                text: "Gains +3 Chips per discarded card of a suit that changes each round", cost: 6,
                discarded: Some(castle), independent: Some(chips_counter), ..JokerSpec::DEFAULT },
    JokerSpec { name: "Obelisk", rarity: Rarity::Rare,
                text: "Gains X0.2 Mult per consecutive hand played without playing your most played poker hand", cost: 8,
                init_counter: 1.0, update: Some(obelisk_update), update_before_scoring: true,
                independent: Some(times_counter), ..JokerSpec::DEFAULT },
    JokerSpec { name: "Hit the Road", rarity: Rarity::Rare,
                text: "Gains X0.5 Mult for every Jack discarded this round", cost: 8,
                init_counter: 1.0, discarded: Some(hit_the_road), independent: Some(times_counter),
                round_end: Some(hit_the_road_end), ..JokerSpec::DEFAULT },
    JokerSpec { name: "Splash", rarity: Rarity::Common, text: "Every played card counts in scoring", cost: 3,
                ..JokerSpec::DEFAULT },
    JokerSpec { name: "Pareidolia", rarity: Rarity::Uncommon, text: "All cards are considered face cards", cost: 5,
                ..JokerSpec::DEFAULT },
    JokerSpec { name: "Smeared Joker", rarity: Rarity::Uncommon,
                text: "Hearts and Diamonds count as the same suit, as do Spades and Clubs", cost: 7,
                ..JokerSpec::DEFAULT },
    JokerSpec { name: "Oops! All 6s", rarity: Rarity::Uncommon, text: "Doubles all listed probabilities", cost: 4,
                ..JokerSpec::DEFAULT },
    JokerSpec { name: "Bloodstone", rarity: Rarity::Uncommon,
                text: "1 in 2 chance for played Hearts to give X1.5 Mult", cost: 7,
                scored: Some(bloodstone), ..JokerSpec::DEFAULT },
    JokerSpec { name: "Business Card", rarity: Rarity::Common,
                text: "Played face cards have a 1 in 2 chance to give $2", cost: 4,
                scored: Some(pays_on_face(2, "business")), ..JokerSpec::DEFAULT },
    JokerSpec { name: "Reserved Parking", rarity: Rarity::Common,
                text: "Each face card held in hand has a 1 in 2 chance to give $1", cost: 6,
                held: Some(pays_on_face(1, "parking")), ..JokerSpec::DEFAULT },
    JokerSpec { name: "Space Joker", rarity: Rarity::Uncommon,
                text: "1 in 4 chance to upgrade the level of the played poker hand", cost: 5,
                before: Some(space_joker), ..JokerSpec::DEFAULT },

    JokerSpec { name: "The Idol", rarity: Rarity::Uncommon,
                text: "Each played card of a rank and suit that changes each round gives X2 Mult", cost: 6,
                scored: Some(the_idol), ..JokerSpec::DEFAULT },
    JokerSpec { name: "Ancient Joker", rarity: Rarity::Rare,
                text: "Each played card of a suit that changes each round gives X1.5 Mult", cost: 8,
                scored: Some(ancient_joker), ..JokerSpec::DEFAULT },
    JokerSpec { name: "To Do List", rarity: Rarity::Common,
                text: "Earn $4 if the poker hand is one that changes each round", cost: 4,
                rerolls_a_hand: true, before: Some(to_do_list), ..JokerSpec::DEFAULT },
    JokerSpec { name: "Midas Mask", rarity: Rarity::Uncommon,
                text: "All played face cards become Gold cards when scored", cost: 7,
                update: Some(midas_mask), update_before_scoring: true, ..JokerSpec::DEFAULT },
    JokerSpec { name: "Seltzer", rarity: Rarity::Uncommon,
                text: "Retrigger all played cards for the next 10 hands", cost: 6,
                init_counter: 10.0, update: Some(seltzer_update),
                retrigger_scored: Some(seltzer_retrigger), ..JokerSpec::DEFAULT },
    JokerSpec { name: "Matador", rarity: Rarity::Uncommon,
                text: "Earn $8 if the played hand triggers the Boss Blind ability", cost: 7,
                independent: Some(matador), on_debuffed_hand: Some(matador_debuffed), ..JokerSpec::DEFAULT },
    JokerSpec { name: "Red Card", rarity: Rarity::Common,
                text: "Gains +3 Mult when any Booster Pack is skipped", cost: 5,
                on_pack_skip: Some(red_card_skip), independent: Some(add_counter), ..JokerSpec::DEFAULT },
    JokerSpec { name: "Madness", rarity: Rarity::Uncommon,
                text: "Gains X0.5 Mult when a Small or Big Blind is selected, and destroys a random Joker", cost: 7,
                init_counter: 1.0, on_blind_select: Some(madness),
                independent: Some(times_counter), ..JokerSpec::DEFAULT },
    JokerSpec { name: "Juggler", rarity: Rarity::Common, text: "+1 hand size", cost: 4, hand_size: 1,
                ..JokerSpec::DEFAULT },
    JokerSpec { name: "Drunkard", rarity: Rarity::Common, text: "+1 discard each round", cost: 4,
                extra_discards: 1, ..JokerSpec::DEFAULT },
    JokerSpec { name: "Merry Andy", rarity: Rarity::Uncommon, text: "+3 discards each round, -1 hand size", cost: 7,
                hand_size: -1, extra_discards: 3, ..JokerSpec::DEFAULT },
    JokerSpec { name: "Troubadour", rarity: Rarity::Uncommon, text: "+2 hand size, -1 hand each round", cost: 6,
                hand_size: 2, extra_hands: -1, ..JokerSpec::DEFAULT },
    JokerSpec { name: "Turtle Bean", rarity: Rarity::Uncommon, text: "+5 hand size, reduced by 1 every round", cost: 6,
                init_counter: 5.0, hand_size_from_counter: true,
                round_end: Some(turtle_bean_end), ..JokerSpec::DEFAULT },
    JokerSpec { name: "Burglar", rarity: Rarity::Uncommon,
                text: "When Blind is selected, gain +3 Hands and lose all discards", cost: 6,
                on_blind_select: Some(burglar), ..JokerSpec::DEFAULT },
    JokerSpec { name: "Cloud 9", rarity: Rarity::Uncommon,
                text: "Earn $1 for each 9 in your full deck at end of round", cost: 7,
                round_money: Some(cloud_9), ..JokerSpec::DEFAULT },
    JokerSpec { name: "Rocket", rarity: Rarity::Uncommon,
                text: "Earn $1 at end of round, increasing by $2 per Boss Blind defeated", cost: 6,
                init_counter: 1.0, round_end: Some(rocket_end), round_money: Some(rocket_money), ..JokerSpec::DEFAULT },
    JokerSpec { name: "Satellite", rarity: Rarity::Uncommon,
                text: "Earn $1 at end of round per unique Planet card used this run", cost: 6,
                round_money: Some(satellite_money), ..JokerSpec::DEFAULT },
    JokerSpec { name: "Egg", rarity: Rarity::Common, text: "Gains $3 of sell value at end of round", cost: 4,
                round_end: Some(egg_end), ..JokerSpec::DEFAULT },
    JokerSpec { name: "Gift Card", rarity: Rarity::Uncommon,
                text: "Adds $1 of sell value to every Joker and Consumable at end of round", cost: 6,
                round_end: Some(gift_card), ..JokerSpec::DEFAULT },
    JokerSpec { name: "Delayed Gratification", rarity: Rarity::Common,
                text: "Earn $2 per discard if no discards are used by end of the round", cost: 4,
                round_money: Some(delayed_gratification), ..JokerSpec::DEFAULT },
    JokerSpec { name: "Mail-In Rebate", rarity: Rarity::Common,
                text: "Earn $5 for each discarded card of a rank that changes every round", cost: 4,
                discarded: Some(mail_in), ..JokerSpec::DEFAULT },
    JokerSpec { name: "To the Moon", rarity: Rarity::Uncommon,
                text: "Earn an extra $1 of interest for every $5 at end of round", cost: 5,
                interest_bonus: 1, ..JokerSpec::DEFAULT },

    JokerSpec { name: "Chaos the Clown", rarity: Rarity::Common, text: "1 free Reroll per shop", cost: 4,
                free_rerolls: 1, ..JokerSpec::DEFAULT },
    JokerSpec { name: "Credit Card", rarity: Rarity::Common, text: "Go up to -$20 in debt", cost: 1,
                debt_limit: 20, ..JokerSpec::DEFAULT },
    JokerSpec { name: "Astronomer", rarity: Rarity::Uncommon,
                text: "All Planet cards and Celestial Packs in the shop are free", cost: 8,
                free_planets: true, ..JokerSpec::DEFAULT },
    JokerSpec { name: "Showman", rarity: Rarity::Uncommon,
                text: "Joker, Tarot, Planet and Spectral cards may appear multiple times", cost: 5,
                allows_duplicates: true, ..JokerSpec::DEFAULT },
    JokerSpec { name: "Mr. Bones", rarity: Rarity::Uncommon,
                text: "Prevents death if chips scored are at least 25% of the requirement, then self destructs",
                cost: 5, prevents_death: true, ..JokerSpec::DEFAULT },
    JokerSpec { name: "Luchador", rarity: Rarity::Uncommon,
                text: "Sell this card to disable the current Boss Blind", cost: 5,
                disables_boss_on_sell: true, ..JokerSpec::DEFAULT },
    JokerSpec { name: "Invisible Joker", rarity: Rarity::Rare,
                text: "After 2 rounds, sell this card to duplicate a random Joker", cost: 8,
                round_end: Some(invisible_round_end), on_sell: Some(invisible_sold), ..JokerSpec::DEFAULT },
    JokerSpec { name: "Diet Cola", rarity: Rarity::Uncommon, text: "Sell this card to create a free Double Tag", cost: 6,
                on_sell: Some(diet_cola), ..JokerSpec::DEFAULT },
    JokerSpec { name: "Marble Joker", rarity: Rarity::Uncommon,
                text: "Adds one Stone card to the deck when Blind is selected", cost: 6,
                on_blind_select: Some(marble), ..JokerSpec::DEFAULT },
    JokerSpec { name: "Cartomancer", rarity: Rarity::Uncommon,
                text: "Create a Tarot card when Blind is selected", cost: 6,
                on_blind_select: Some(cartomancer), ..JokerSpec::DEFAULT },
    JokerSpec { name: "Certificate", rarity: Rarity::Uncommon,
                text: "When the round begins, add a random playing card with a random seal to your hand", cost: 6,
                on_round_start: Some(certificate), ..JokerSpec::DEFAULT },
    JokerSpec { name: "Riff-Raff", rarity: Rarity::Common, text: "When Blind is selected, create 2 Common Jokers", cost: 6,
                on_blind_select: Some(riff_raff), ..JokerSpec::DEFAULT },
    JokerSpec { name: "8 Ball", rarity: Rarity::Common,
                text: "1 in 4 chance for each played 8 to create a Tarot card when scored", cost: 5,
                scored: Some(eight_ball), ..JokerSpec::DEFAULT },
    JokerSpec { name: "Hallucination", rarity: Rarity::Common,
                text: "1 in 2 chance to create a Tarot card when a Booster Pack is opened", cost: 4,
                on_pack_open: Some(hallucination), ..JokerSpec::DEFAULT },
    JokerSpec { name: "Superposition", rarity: Rarity::Common,
                text: "Create a Tarot card if the poker hand contains an Ace and a Straight", cost: 4,
                after_hand: Some(superposition), ..JokerSpec::DEFAULT },
    JokerSpec { name: "Séance", rarity: Rarity::Uncommon,
                text: "If the poker hand contains a Straight Flush, create a Spectral card", cost: 6,
                after_hand: Some(seance), ..JokerSpec::DEFAULT },
    JokerSpec { name: "Vagabond", rarity: Rarity::Rare,
                text: "Create a Tarot card if a hand is played with $4 or less", cost: 8,
                after_hand: Some(vagabond), ..JokerSpec::DEFAULT },
    JokerSpec { name: "Sixth Sense", rarity: Rarity::Uncommon,
                text: "If the first hand of a round is a single 6, destroy it and create a Spectral card",
                cost: 6, before_hand: Some(sixth_sense), ..JokerSpec::DEFAULT },
    JokerSpec { name: "DNA", rarity: Rarity::Rare,
                text: "If the first hand of a round has only 1 card, add a permanent copy to the deck and draw it to hand",
                cost: 8, before_hand: Some(dna), ..JokerSpec::DEFAULT },
    JokerSpec { name: "Trading Card", rarity: Rarity::Uncommon,
                text: "If the first discard of a round has only 1 card, destroy it and earn $3", cost: 6,
                on_first_discard: Some(trading_card), ..JokerSpec::DEFAULT },
    JokerSpec { name: "Burnt Joker", rarity: Rarity::Rare,
                text: "Upgrade the level of the first discarded poker hand each round", cost: 8,
                on_first_discard: Some(burnt), ..JokerSpec::DEFAULT },
];

