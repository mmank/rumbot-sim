//! The registry's spec table: one entry per `register(...)` in the Python
//! `consumables.py`, in the order the Python file registers them.
//!
//! The registry itself lives in `consumables.rs`; this is only the data. Every
//! `apply` here is a plain `fn` (Python's `_planet_apply`, `_enhance`,
//! `_to_suit` and `_seal` return closures, but a closure over an immutable
//! argument is just a `fn` with one more parameter, so the wrappers below pin
//! the argument and hand the engine the two-argument hook it expects).
//!
//! The file order is the registry's own insertion order, which matters because
//! a run that draws from a pool draws by index into it. Python registers the
//! twelve planets first, then the tarots, then the spectrals that precede the
//! "never registered" block, then The Fool and the remaining spectrals.

use crate::cards::{CardRef, Edition, Enhancement, Rank, Seal, Suit};
use crate::consumables::{ConsumableKind, ConsumableSpec};
use crate::game::GameState;
use crate::hands::{planet_for_hand, HandType};
use crate::jokers::JokerRef;
use std::rc::Rc;

pub const SPECS: &[ConsumableSpec] = &[
    // -- planets, in PLANET_FOR_HAND's own order ------------------------------
    ConsumableSpec {
        name: "Pluto",
        kind: ConsumableKind::Planet,
        text: "Level up High Card",
        apply: Some(pluto),
        ..ConsumableSpec::DEFAULT
    },
    ConsumableSpec {
        name: "Mercury",
        kind: ConsumableKind::Planet,
        text: "Level up Pair",
        apply: Some(mercury),
        ..ConsumableSpec::DEFAULT
    },
    ConsumableSpec {
        name: "Uranus",
        kind: ConsumableKind::Planet,
        text: "Level up Two Pair",
        apply: Some(uranus),
        ..ConsumableSpec::DEFAULT
    },
    ConsumableSpec {
        name: "Venus",
        kind: ConsumableKind::Planet,
        text: "Level up Three of a Kind",
        apply: Some(venus),
        ..ConsumableSpec::DEFAULT
    },
    ConsumableSpec {
        name: "Saturn",
        kind: ConsumableKind::Planet,
        text: "Level up Straight",
        apply: Some(saturn),
        ..ConsumableSpec::DEFAULT
    },
    ConsumableSpec {
        name: "Jupiter",
        kind: ConsumableKind::Planet,
        text: "Level up Flush",
        apply: Some(jupiter),
        ..ConsumableSpec::DEFAULT
    },
    ConsumableSpec {
        name: "Earth",
        kind: ConsumableKind::Planet,
        text: "Level up Full House",
        apply: Some(earth),
        ..ConsumableSpec::DEFAULT
    },
    ConsumableSpec {
        name: "Mars",
        kind: ConsumableKind::Planet,
        text: "Level up Four of a Kind",
        apply: Some(mars),
        ..ConsumableSpec::DEFAULT
    },
    ConsumableSpec {
        name: "Neptune",
        kind: ConsumableKind::Planet,
        text: "Level up Straight Flush",
        apply: Some(neptune),
        ..ConsumableSpec::DEFAULT
    },
    ConsumableSpec {
        name: "Planet X",
        kind: ConsumableKind::Planet,
        text: "Level up Five of a Kind",
        apply: Some(planet_x),
        ..ConsumableSpec::DEFAULT
    },
    ConsumableSpec {
        name: "Ceres",
        kind: ConsumableKind::Planet,
        text: "Level up Flush House",
        apply: Some(ceres),
        ..ConsumableSpec::DEFAULT
    },
    ConsumableSpec {
        name: "Eris",
        kind: ConsumableKind::Planet,
        text: "Level up Flush Five",
        apply: Some(eris),
        ..ConsumableSpec::DEFAULT
    },
    // -- tarots ---------------------------------------------------------------
    ConsumableSpec {
        name: "The Magician",
        kind: ConsumableKind::Tarot,
        text: "Enhance 2 cards into Lucky Cards",
        targets: 1,
        max_targets: Some(2),
        apply: Some(the_magician),
        ..ConsumableSpec::DEFAULT
    },
    ConsumableSpec {
        name: "The Empress",
        kind: ConsumableKind::Tarot,
        text: "Enhance 2 cards into Mult Cards",
        targets: 1,
        max_targets: Some(2),
        apply: Some(the_empress),
        ..ConsumableSpec::DEFAULT
    },
    ConsumableSpec {
        name: "The Hierophant",
        kind: ConsumableKind::Tarot,
        text: "Enhance 2 cards into Bonus Cards",
        targets: 1,
        max_targets: Some(2),
        apply: Some(the_hierophant),
        ..ConsumableSpec::DEFAULT
    },
    ConsumableSpec {
        name: "The Lovers",
        kind: ConsumableKind::Tarot,
        text: "Enhance 1 card into a Wild Card",
        targets: 1,
        apply: Some(the_lovers),
        ..ConsumableSpec::DEFAULT
    },
    ConsumableSpec {
        name: "The Chariot",
        kind: ConsumableKind::Tarot,
        text: "Enhance 1 card into a Steel Card",
        targets: 1,
        apply: Some(the_chariot),
        ..ConsumableSpec::DEFAULT
    },
    ConsumableSpec {
        name: "Justice",
        kind: ConsumableKind::Tarot,
        text: "Enhance 1 card into a Glass Card",
        targets: 1,
        apply: Some(justice),
        ..ConsumableSpec::DEFAULT
    },
    ConsumableSpec {
        name: "The Devil",
        kind: ConsumableKind::Tarot,
        text: "Enhance 1 card into a Gold Card",
        targets: 1,
        apply: Some(the_devil),
        ..ConsumableSpec::DEFAULT
    },
    ConsumableSpec {
        name: "The Tower",
        kind: ConsumableKind::Tarot,
        text: "Enhance 1 card into a Stone Card",
        targets: 1,
        apply: Some(the_tower),
        ..ConsumableSpec::DEFAULT
    },
    ConsumableSpec {
        name: "The Star",
        kind: ConsumableKind::Tarot,
        text: "Convert up to 3 cards to Diamonds",
        targets: 1,
        max_targets: Some(3),
        apply: Some(the_star),
        ..ConsumableSpec::DEFAULT
    },
    ConsumableSpec {
        name: "The Moon",
        kind: ConsumableKind::Tarot,
        text: "Convert up to 3 cards to Clubs",
        targets: 1,
        max_targets: Some(3),
        apply: Some(the_moon),
        ..ConsumableSpec::DEFAULT
    },
    ConsumableSpec {
        name: "The Sun",
        kind: ConsumableKind::Tarot,
        text: "Convert up to 3 cards to Hearts",
        targets: 1,
        max_targets: Some(3),
        apply: Some(the_sun),
        ..ConsumableSpec::DEFAULT
    },
    ConsumableSpec {
        name: "The World",
        kind: ConsumableKind::Tarot,
        text: "Convert up to 3 cards to Spades",
        targets: 1,
        max_targets: Some(3),
        apply: Some(the_world),
        ..ConsumableSpec::DEFAULT
    },
    ConsumableSpec {
        name: "Strength",
        kind: ConsumableKind::Tarot,
        text: "Increase the rank of up to 2 cards",
        targets: 1,
        max_targets: Some(2),
        apply: Some(strength),
        ..ConsumableSpec::DEFAULT
    },
    ConsumableSpec {
        name: "The Hanged Man",
        kind: ConsumableKind::Tarot,
        text: "Destroy up to 2 cards",
        targets: 1,
        max_targets: Some(2),
        apply: Some(the_hanged_man),
        ..ConsumableSpec::DEFAULT
    },
    ConsumableSpec {
        name: "Death",
        kind: ConsumableKind::Tarot,
        text: "Convert the left card into the right card",
        targets: 2,
        apply: Some(death),
        ..ConsumableSpec::DEFAULT
    },
    ConsumableSpec {
        name: "The Hermit",
        kind: ConsumableKind::Tarot,
        text: "Double your money (max $20)",
        apply: Some(the_hermit),
        ..ConsumableSpec::DEFAULT
    },
    ConsumableSpec {
        name: "Temperance",
        kind: ConsumableKind::Tarot,
        text: "Gain the total sell value of your Jokers (max $50)",
        apply: Some(temperance),
        ..ConsumableSpec::DEFAULT
    },
    ConsumableSpec {
        name: "The High Priestess",
        kind: ConsumableKind::Tarot,
        text: "Create 2 random Planet cards",
        apply: Some(the_high_priestess),
        ..ConsumableSpec::DEFAULT
    },
    ConsumableSpec {
        name: "The Emperor",
        kind: ConsumableKind::Tarot,
        text: "Create 2 random Tarot cards",
        apply: Some(the_emperor),
        ..ConsumableSpec::DEFAULT
    },
    ConsumableSpec {
        name: "Judgement",
        kind: ConsumableKind::Tarot,
        text: "Create a random Joker",
        apply: Some(judgement),
        ..ConsumableSpec::DEFAULT
    },
    ConsumableSpec {
        name: "The Wheel of Fortune",
        kind: ConsumableKind::Tarot,
        text: "1 in 4 chance to add an edition to a random Joker",
        apply: Some(the_wheel_of_fortune),
        ..ConsumableSpec::DEFAULT
    },
    // -- spectrals (the ones registered before the "never registered" block) --
    ConsumableSpec {
        name: "Talisman",
        kind: ConsumableKind::Spectral,
        text: "Add a Gold Seal to 1 card",
        targets: 1,
        cost: 4,
        apply: Some(talisman),
        ..ConsumableSpec::DEFAULT
    },
    ConsumableSpec {
        name: "Deja Vu",
        kind: ConsumableKind::Spectral,
        text: "Add a Red Seal to 1 card",
        targets: 1,
        cost: 4,
        apply: Some(deja_vu),
        ..ConsumableSpec::DEFAULT
    },
    ConsumableSpec {
        name: "Trance",
        kind: ConsumableKind::Spectral,
        text: "Add a Blue Seal to 1 card",
        targets: 1,
        cost: 4,
        apply: Some(trance),
        ..ConsumableSpec::DEFAULT
    },
    ConsumableSpec {
        name: "Medium",
        kind: ConsumableKind::Spectral,
        text: "Add a Purple Seal to 1 card",
        targets: 1,
        cost: 4,
        apply: Some(medium),
        ..ConsumableSpec::DEFAULT
    },
    ConsumableSpec {
        name: "Aura",
        kind: ConsumableKind::Spectral,
        text: "Add a random edition to 1 card in hand",
        targets: 1,
        cost: 4,
        apply: Some(aura),
        ..ConsumableSpec::DEFAULT
    },
    ConsumableSpec {
        name: "Black Hole",
        kind: ConsumableKind::Spectral,
        text: "Level up every poker hand",
        cost: 4,
        apply: Some(black_hole),
        ..ConsumableSpec::DEFAULT
    },
    ConsumableSpec {
        name: "Immolate",
        kind: ConsumableKind::Spectral,
        text: "Destroy 5 random cards in deck, gain $20",
        cost: 4,
        apply: Some(immolate),
        ..ConsumableSpec::DEFAULT
    },
    ConsumableSpec {
        name: "Ectoplasm",
        kind: ConsumableKind::Spectral,
        text: "Add Negative to a random Joker, -1 hand size",
        cost: 4,
        apply: Some(ectoplasm),
        ..ConsumableSpec::DEFAULT
    },
    ConsumableSpec {
        name: "Familiar",
        kind: ConsumableKind::Spectral,
        text: "Destroy 1 random card in hand, add 3 random Enhanced face cards",
        cost: 4,
        apply: Some(familiar),
        ..ConsumableSpec::DEFAULT
    },
    // -- the "never registered" block: The Fool and the rest of the spectrals -
    ConsumableSpec {
        name: "The Fool",
        kind: ConsumableKind::Tarot,
        text: "Copy the last Tarot or Planet card used this run",
        apply: Some(the_fool),
        ..ConsumableSpec::DEFAULT
    },
    ConsumableSpec {
        name: "Ankh",
        kind: ConsumableKind::Spectral,
        text: "Copy a random Joker, destroy the others",
        cost: 4,
        apply: Some(ankh),
        ..ConsumableSpec::DEFAULT
    },
    ConsumableSpec {
        name: "Cryptid",
        kind: ConsumableKind::Spectral,
        text: "Create 2 copies of a selected card",
        targets: 1,
        cost: 4,
        apply: Some(cryptid),
        ..ConsumableSpec::DEFAULT
    },
    ConsumableSpec {
        name: "Grim",
        kind: ConsumableKind::Spectral,
        text: "Destroy 1 random card in hand, add 2 random Enhanced Aces",
        cost: 4,
        apply: Some(grim),
        ..ConsumableSpec::DEFAULT
    },
    ConsumableSpec {
        name: "Hex",
        kind: ConsumableKind::Spectral,
        text: "Add Polychrome to a random Joker, destroy the others",
        cost: 4,
        apply: Some(hex),
        ..ConsumableSpec::DEFAULT
    },
    ConsumableSpec {
        name: "Incantation",
        kind: ConsumableKind::Spectral,
        text: "Destroy 1 random card in hand, add 4 random Enhanced numbered cards",
        cost: 4,
        apply: Some(incantation),
        ..ConsumableSpec::DEFAULT
    },
    ConsumableSpec {
        name: "Ouija",
        kind: ConsumableKind::Spectral,
        text: "Convert all cards in hand to a single random rank, -1 hand size",
        cost: 4,
        apply: Some(ouija),
        ..ConsumableSpec::DEFAULT
    },
    ConsumableSpec {
        name: "Sigil",
        kind: ConsumableKind::Spectral,
        text: "Convert all cards in hand to a single random suit",
        cost: 4,
        apply: Some(sigil),
        ..ConsumableSpec::DEFAULT
    },
    ConsumableSpec {
        name: "The Soul",
        kind: ConsumableKind::Spectral,
        text: "Create a Legendary Joker",
        cost: 4,
        apply: Some(the_soul),
        ..ConsumableSpec::DEFAULT
    },
    ConsumableSpec {
        name: "Wraith",
        kind: ConsumableKind::Spectral,
        text: "Create a random Rare Joker, set money to $0",
        cost: 4,
        apply: Some(wraith),
        ..ConsumableSpec::DEFAULT
    },
];
// --------------------------------------------------------------------------
// planets
// --------------------------------------------------------------------------
//
// Python's `_planet_apply(hand)` returns a closure that levels the hand and
// logs the new level. The twelve fns below are that closure with `hand` pinned
// as the one extra argument of `planet_apply`.

/// The shared body of every Planet card.
///
/// Python:
///
/// ```text
///     game.hand_levels.level_up(hand)
///     game.log(f"{PLANET_FOR_HAND[hand]}: {hand.label} to level "
///              f"{game.hand_levels.levels[hand]}")
/// ```
fn planet_apply(game: &mut GameState, hand: HandType) {
    game.hand_levels.level_up(hand, 1);
    game.log(format!(
        "{}: {} to level {}",
        planet_for_hand(hand),
        hand.label(),
        game.hand_levels.level(hand)
    ));
}

fn pluto(game: &mut GameState, _cards: &[CardRef]) {
    planet_apply(game, HandType::HighCard);
}
fn mercury(game: &mut GameState, _cards: &[CardRef]) {
    planet_apply(game, HandType::Pair);
}
fn uranus(game: &mut GameState, _cards: &[CardRef]) {
    planet_apply(game, HandType::TwoPair);
}
fn venus(game: &mut GameState, _cards: &[CardRef]) {
    planet_apply(game, HandType::ThreeOfAKind);
}
fn saturn(game: &mut GameState, _cards: &[CardRef]) {
    planet_apply(game, HandType::Straight);
}
fn jupiter(game: &mut GameState, _cards: &[CardRef]) {
    planet_apply(game, HandType::Flush);
}
fn earth(game: &mut GameState, _cards: &[CardRef]) {
    planet_apply(game, HandType::FullHouse);
}
fn mars(game: &mut GameState, _cards: &[CardRef]) {
    planet_apply(game, HandType::FourOfAKind);
}
fn neptune(game: &mut GameState, _cards: &[CardRef]) {
    planet_apply(game, HandType::StraightFlush);
}
fn planet_x(game: &mut GameState, _cards: &[CardRef]) {
    planet_apply(game, HandType::FiveOfAKind);
}
fn ceres(game: &mut GameState, _cards: &[CardRef]) {
    planet_apply(game, HandType::FlushHouse);
}
fn eris(game: &mut GameState, _cards: &[CardRef]) {
    planet_apply(game, HandType::FlushFive);
}

// --------------------------------------------------------------------------
// the shared tarot facts
// --------------------------------------------------------------------------

/// Python `_enhance(enh)`: set the enhancement on every selected card.
///
/// The shared body; the eight enhancement tarots below pin `enh`.
fn enhance(game: &mut GameState, cards: &[CardRef], enh: Enhancement) {
    for card in cards {
        game.set_enhancement(card, enh);
    }
}

/// Python `_to_suit(suit)`: the four suit tarots.
///
/// `set_suit`, because a converted card remembers what it was, and the game
/// re-asks the boss afterwards (`Card:change_suit` ends
/// `G.GAME.blind:debuff_card(self)`, card.lua:561) -- so under The Window a
/// card turned into a Diamond is debuffed at once, and one turned out of
/// Diamonds is released at once.
fn to_suit(game: &mut GameState, cards: &[CardRef], suit: Suit) {
    for card in cards {
        crate::cards::set_suit(card, suit);
        game.debuff_card(card);
    }
}

/// Python `_seal(seal)`: the four seal spectrals.
fn set_seal(_game: &mut GameState, cards: &[CardRef], seal: Seal) {
    for card in cards {
        card.borrow_mut().seal = seal;
    }
}

fn the_magician(game: &mut GameState, cards: &[CardRef]) {
    enhance(game, cards, Enhancement::Lucky);
}
fn the_empress(game: &mut GameState, cards: &[CardRef]) {
    enhance(game, cards, Enhancement::Mult);
}
fn the_hierophant(game: &mut GameState, cards: &[CardRef]) {
    enhance(game, cards, Enhancement::Bonus);
}
fn the_lovers(game: &mut GameState, cards: &[CardRef]) {
    enhance(game, cards, Enhancement::Wild);
}
fn the_chariot(game: &mut GameState, cards: &[CardRef]) {
    enhance(game, cards, Enhancement::Steel);
}
fn justice(game: &mut GameState, cards: &[CardRef]) {
    enhance(game, cards, Enhancement::Glass);
}
fn the_devil(game: &mut GameState, cards: &[CardRef]) {
    enhance(game, cards, Enhancement::Gold);
}
fn the_tower(game: &mut GameState, cards: &[CardRef]) {
    enhance(game, cards, Enhancement::Stone);
}

fn the_star(game: &mut GameState, cards: &[CardRef]) {
    to_suit(game, cards, Suit::Diamonds);
}
fn the_moon(game: &mut GameState, cards: &[CardRef]) {
    to_suit(game, cards, Suit::Clubs);
}
fn the_sun(game: &mut GameState, cards: &[CardRef]) {
    to_suit(game, cards, Suit::Hearts);
}
fn the_world(game: &mut GameState, cards: &[CardRef]) {
    to_suit(game, cards, Suit::Spades);
}

fn talisman(game: &mut GameState, cards: &[CardRef]) {
    set_seal(game, cards, Seal::Gold);
}
fn deja_vu(game: &mut GameState, cards: &[CardRef]) {
    set_seal(game, cards, Seal::Red);
}
fn trance(game: &mut GameState, cards: &[CardRef]) {
    set_seal(game, cards, Seal::Blue);
}
fn medium(game: &mut GameState, cards: &[CardRef]) {
    set_seal(game, cards, Seal::Purple);
}
// --------------------------------------------------------------------------
// bespoke tarots
// --------------------------------------------------------------------------

/// Increase each selected card's rank by one, wrapping Ace back to Two.
///
/// Python:
///
/// ```text
///     order = list(Rank)
///     for card in cards:
///         card.rank = order[(order.index(card.rank) + 1) % len(order)]
///         game.debuff_card(card)   # set_base re-asks the boss (card.lua:143)
/// ```
fn strength(game: &mut GameState, cards: &[CardRef]) {
    let order = Rank::ALL;
    for card in cards {
        let rank = card.borrow().rank;
        let index = order
            .iter()
            .position(|r| *r == rank)
            .expect("rank in Rank::ALL");
        card.borrow_mut().rank = order[(index + 1) % order.len()];
        game.debuff_card(card);
    }
}

/// Destroy the selected cards, paying Glass Joker by a side door.
///
/// Every other tarot that destroys leaves Glass Joker with nothing, because the
/// shatter is queued behind the joker check -- see
/// `GameState::note_cards_destroyed`. The Hanged Man is the one the game
/// patched around, with a second handler that fires on the consumable being
/// used and counts the selected Glass Cards directly, so it pays whether or not
/// the flag has caught up.
///
/// Python:
///
/// ```text
///     glass = [c for c in cards if c.enhancement is Enhancement.GLASS]
///     if glass:
///         for joker, answer in game.calculating_hooks("on_glass_shattered"):
///             answer(joker, list(glass), game)
///     for card in cards:
///         game.remove_card(card)
/// ```
fn the_hanged_man(game: &mut GameState, cards: &[CardRef]) {
    let glass: Vec<CardRef> = cards
        .iter()
        .filter(|c| c.borrow().enhancement == Enhancement::Glass)
        .cloned()
        .collect();
    if !glass.is_empty() {
        for (joker, answer) in game.calculating_card_hooks("on_glass_shattered") {
            answer(&joker, &glass, game);
        }
    }
    for card in cards {
        // `shattered = false`: the tarot notifies *before* the shatter flag is
        // set, which is the whole reason for the handler above.
        game.remove_card(card, false);
    }
}

/// Convert the left selected card into the right one.
///
/// Exactly two, never more: `c_death` is `max_highlighted = 2,
/// min_highlighted = 2`, and `can_use_consumeable` will not let the card be
/// used at any other count.
///
/// Which one is "right" is decided by screen position rather than by the order
/// the cards were clicked in, so it is resolved against the hand here rather
/// than trusting the order the caller passed them in.
///
/// Python copies `copy_card`'s whole ability table, of which two fields matter
/// beyond what is printed on the card: `perma_bonus` (the chips a Hiker left,
/// here `extra_chips`) and `played_this_ante`, which is exactly what The Pillar
/// debuffs -- so a Death can debuff a fresh card by copying a spent one onto
/// it, and launder a spent one by copying a fresh one the other way. The debuff
/// itself comes across verbatim too (`new_card.debuff = other.debuff`).
fn death(game: &mut GameState, cards: &[CardRef]) {
    if cards.len() != 2 {
        return;
    }
    // `order = {id(c): i for i, c in enumerate(game.hand)}`; cards not in hand
    // get -1 and therefore sort first, as Python's `order.get(id(c), -1)`.
    let position = |card: &CardRef| -> i32 {
        let uid = crate::cards::uid_of(card);
        game.hand
            .iter()
            .position(|c| crate::cards::uid_of(c) == uid)
            .map(|i| i as i32)
            .unwrap_or(-1)
    };
    let mut ordered: Vec<CardRef> = cards.to_vec();
    ordered.sort_by_key(|c| position(c));
    let (left, right) = (&ordered[0], &ordered[1]);
    if Rc::ptr_eq(left, right) {
        return;
    }
    let source = right.borrow();
    let mut target = left.borrow_mut();
    target.rank = source.rank;
    // `set_suit`, inlined so the source is not borrowed twice.
    if target.original_suit.is_none() {
        target.original_suit = Some(target.suit);
    }
    target.suit = source.suit;
    target.enhancement = source.enhancement;
    target.edition = source.edition;
    target.seal = source.seal;
    target.extra_chips = source.extra_chips;
    target.played_this_ante = source.played_this_ante;
    target.debuffed = source.debuffed;
}

/// Double your money, capped at twenty.
fn the_hermit(game: &mut GameState, _cards: &[CardRef]) {
    game.add_money(game.money.clamp(0, 20), "The Hermit");
}

/// Gain the total sell value of every Joker held, capped at fifty.
fn temperance(game: &mut GameState, _cards: &[CardRef]) {
    let jokers = game.jokers.clone();
    let total: i32 = jokers.iter().map(|j| game.sell_value(j)).sum();
    game.add_money(total.min(50), "Temperance");
}

/// Create two random Planet cards.
fn the_high_priestess(game: &mut GameState, _cards: &[CardRef]) {
    let made = game.random_consumables(ConsumableKind::Planet, 2, "pri");
    game.add_consumables(&made, Edition::None);
}

/// Create two random Tarot cards.
fn the_emperor(game: &mut GameState, _cards: &[CardRef]) {
    let made = game.random_consumables(ConsumableKind::Tarot, 2, "emp");
    game.add_consumables(&made, Edition::None);
}

/// Create a random Joker.
fn judgement(game: &mut GameState, _cards: &[CardRef]) {
    game.add_random_joker("Judgement", None, false, "jud", false);
}

/// The jokers with no edition, oldest first.
///
/// Order decides the answer: the caller draws from this by index, and the game
/// draws with `pseudorandom_element`, which sorts by sort_id before indexing.
/// Building it in row order returns the wrong joker as soon as anything has been
/// dragged -- reproducibly, from the right stream, which is what made it
/// invisible.
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

/// The game's polled edition name, as an `Edition`.
fn edition_from_name(name: &str) -> Edition {
    match name {
        "foil" => Edition::Foil,
        "holo" => Edition::Holographic,
        "polychrome" => Edition::Polychrome,
        "negative" => Edition::Negative,
        _ => Edition::None,
    }
}

/// One in four to put an edition on a joker that has none.
///
/// Three draws, all against the same pool name -- `"wheel_of_fortune"` -- so
/// they come out of one stream in order: the chance, then which joker, then
/// which edition. The game polls the ordinary edition bands widened twenty-five
/// times so that something always lands (the guaranteed form of `poll_edition`).
fn the_wheel_of_fortune(game: &mut GameState, _cards: &[CardRef]) {
    let plain = editionless(game);
    if plain.is_empty() {
        return;
    }
    let scale = game.probability_scale();
    if !game.rng.chance("wheel_of_fortune", 1.0 * scale, 4.0) {
        return;
    }
    let joker = game.rng.choice("wheel_of_fortune", &plain);
    let name = crate::shop_pool::poll_edition(
        &mut game.rng,
        "wheel_of_fortune",
        1.0,
        true,
        1.0,
        true,
    );
    let edition = edition_from_name(name);
    joker.borrow_mut().edition = edition;
    game.log(format!(
        "Wheel of Fortune: {} is now {}",
        joker.borrow().name(),
        edition.as_str()
    ));
}
// --------------------------------------------------------------------------
// bespoke spectrals
// --------------------------------------------------------------------------

/// A random edition on one card in hand.
///
/// `poll_edition('aura', nil, true, true)` -- the guaranteed form, bands widened
/// twenty-five times so something always lands, and no negative. Picking
/// uniformly from three gives polychrome far more often than the game does.
fn aura(game: &mut GameState, cards: &[CardRef]) {
    for card in cards {
        let name =
            crate::shop_pool::poll_edition(&mut game.rng, "aura", 1.0, true, 1.0, true);
        card.borrow_mut().edition = edition_from_name(name);
    }
}

/// Level up every poker hand.
fn black_hole(game: &mut GameState, _cards: &[CardRef]) {
    for hand in HandType::ALL {
        game.hand_levels.level_up(hand, 1);
    }
}

/// Destroy 5 random cards *in hand*, and gain twenty dollars.
///
/// Not five from the deck: the game copies `G.hand.cards`, shuffles that copy
/// under the pool name `"immolate"` and takes the first five. Cards still in the
/// deck are never at risk, so the card the player is looking at is exactly the
/// card that can burn.
fn immolate(game: &mut GameState, _cards: &[CardRef]) {
    let mut doomed: Vec<CardRef> = game.hand.clone();
    doomed.sort_by_key(|c| crate::cards::uid_of(c));
    game.rng.shuffle(&mut doomed, "immolate");
    for card in doomed.iter().take(5) {
        game.remove_card(card, false);
    }
    game.add_money(20, "Immolate");
}

/// Negative onto a joker with no edition, and a growing bite out of the hand.
///
/// The cost is not the flat -1 the card prints. `G.GAME.ecto_minus` starts at
/// one and rises by one after every use, so a run's second Ectoplasm costs two
/// hand size and its third costs three.
///
/// A run whose jokers all carry an edition cannot use it at all -- see
/// `GameState::can_use_consumable` -- so the empty case here is belt and
/// braces.
fn ectoplasm(game: &mut GameState, _cards: &[CardRef]) {
    let plain = editionless(game);
    if plain.is_empty() {
        return;
    }
    let joker = game.rng.choice("ectoplasm", &plain);
    joker.borrow_mut().edition = Edition::Negative;
    game.base_hand_size -= game.ecto_minus;
    game.ecto_minus += 1;
}

/// The enhancements a card these three spectrals make may carry, in the game's
/// own order. Every enhancement but Stone: the cards these three make are always
/// enhanced, and Stone is excluded because it has no rank or suit to give.
const SPE_POOL: [Enhancement; 7] = [
    Enhancement::Bonus,
    Enhancement::Mult,
    Enhancement::Wild,
    Enhancement::Glass,
    Enhancement::Steel,
    Enhancement::Gold,
    Enhancement::Lucky,
];

/// Familiar, Grim and Incantation: one card out, several in.
///
/// The card destroyed is a *random* one from the hand, not a card the player
/// selected -- `pseudorandom_element(G.hand.cards, 'random_destroy')`. The cards
/// made go into the *hand*, not into the deck to be drawn later, and they are
/// always enhanced.
///
/// Rank and suit come from the same pool name for both halves (Familiar and
/// Incantation) so the two draws come out of one stream in that order. Grim
/// draws no rank at all: it sets `_rank = 'A'` and only the suit goes through
/// `pseudorandom_element` -- a draw from a one-element list is not free, so
/// `rank_key = None` says there is no draw.
fn destroy_and_make(
    game: &mut GameState,
    count: usize,
    ranks: &[&str],
    rank_key: Option<&str>,
    suit_key: &str,
) {
    if !game.hand.is_empty() {
        let mut pool: Vec<CardRef> = game.hand.clone();
        pool.sort_by_key(|c| crate::cards::uid_of(c));
        let doomed = game.rng.choice("random_destroy", &pool);
        game.remove_card(&doomed, false);
    }
    for _ in 0..count {
        let rank_letter = match rank_key {
            None => ranks[0],
            Some(key) => {
                let items: Vec<&str> = ranks.to_vec();
                game.rng.choice(key, &items)
            }
        };
        let rank = Rank::from_code(rank_letter).expect("a rank letter");
        let suit_letters = ["S", "H", "D", "C"];
        let suit_letter = game.rng.choice(suit_key, &suit_letters);
        let suit = Suit::from_code(suit_letter).expect("a suit letter");
        let card = crate::cards::make_card(rank, suit);
        let enhancement = game.rng.choice("spe_card", &SPE_POOL);
        card.borrow_mut().enhancement = enhancement;
        game.add_card_to_hand(&card);
    }
}
/// Destroy 1 random card in hand, add 3 random Enhanced face cards.
fn familiar(game: &mut GameState, _cards: &[CardRef]) {
    destroy_and_make(
        game,
        3,
        &["J", "Q", "K"],
        Some("familiar_create"),
        "familiar_create",
    );
}

/// Destroy 1 random card in hand, add 2 random Enhanced Aces.
fn grim(game: &mut GameState, _cards: &[CardRef]) {
    destroy_and_make(game, 2, &["A"], None, "grim_create");
}

/// Destroy 1 random card in hand, add 4 random Enhanced numbered cards.
fn incantation(game: &mut GameState, _cards: &[CardRef]) {
    destroy_and_make(
        game,
        4,
        &["2", "3", "4", "5", "6", "7", "8", "9", "T"],
        Some("incantation_create"),
        "incantation_create",
    );
}

/// Two copies of a chosen card, into the hand.
///
/// `G.hand:emplace`, not into the deck to be drawn later -- the copies are there
/// to be played with the hand they came from. They join the deck too, so they
/// come round again in later rounds.
fn cryptid(game: &mut GameState, cards: &[CardRef]) {
    for card in cards {
        for _ in 0..2 {
            let copy = crate::cards::copy_card(card);
            game.add_card_to_hand(&copy);
        }
    }
}

/// Copy a random joker, destroy the others.
///
/// The copy is chosen from every joker held, but only the ones that can be
/// destroyed are destroyed -- an eternal joker survives even when it was not the
/// one chosen.
fn ankh(game: &mut GameState, _cards: &[CardRef]) {
    if game.jokers.is_empty() {
        return;
    }
    // Oldest first: pseudorandom_element sorts by sort_id before it draws.
    let mut pool: Vec<JokerRef> = game.jokers.clone();
    pool.sort_by_key(|j| j.borrow().uid);
    let chosen = game.rng.choice("ankh_choice", &pool);
    let row = game.jokers.clone();
    for joker in row {
        if !Rc::ptr_eq(&joker, &chosen) {
            game.destroy_joker(&joker, "Ankh");
        }
    }
    game.add_joker_copy(&chosen, "Ankh");
}

/// Make one joker Polychrome and destroy the rest.
///
/// The one chosen is drawn from the jokers with no edition yet, not from all of
/// them, so a run whose jokers are already editioned loses nothing and gains
/// nothing.
fn hex(game: &mut GameState, _cards: &[CardRef]) {
    let plain = editionless(game);
    if plain.is_empty() {
        return;
    }
    let chosen = game.rng.choice("hex", &plain);
    chosen.borrow_mut().edition = Edition::Polychrome;
    let row = game.jokers.clone();
    for joker in row {
        if !Rc::ptr_eq(&joker, &chosen) {
            game.destroy_joker(&joker, "Hex");
        }
    }
    game.log(format!("Hex: {} is now polychrome", chosen.borrow().name()));
}

/// The game's `Enum.name.title()` for a rank, which is what Ouija logs.
fn rank_title(rank: Rank) -> &'static str {
    match rank {
        Rank::Two => "Two",
        Rank::Three => "Three",
        Rank::Four => "Four",
        Rank::Five => "Five",
        Rank::Six => "Six",
        Rank::Seven => "Seven",
        Rank::Eight => "Eight",
        Rank::Nine => "Nine",
        Rank::Ten => "Ten",
        Rank::Jack => "Jack",
        Rank::Queen => "Queen",
        Rank::King => "King",
        Rank::Ace => "Ace",
    }
}

/// The game's `Enum.name.title()` for a suit, which is what Sigil logs.
fn suit_title(suit: Suit) -> &'static str {
    match suit {
        Suit::Spades => "Spades",
        Suit::Hearts => "Hearts",
        Suit::Diamonds => "Diamonds",
        Suit::Clubs => "Clubs",
    }
}

/// Every card in hand becomes one random rank. Costs a hand size.
fn ouija(game: &mut GameState, _cards: &[CardRef]) {
    let rank = game.rng.choice("ouija", &Rank::ALL);
    let hand = game.hand.clone();
    for card in hand {
        card.borrow_mut().rank = rank;
        game.debuff_card(&card); // set_base, card.lua:1256 -> 143
    }
    game.base_hand_size -= 1;
    game.log(format!("Ouija: the hand is all {}s", rank_title(rank)));
}

/// Every card in hand becomes one random suit.
fn sigil(game: &mut GameState, _cards: &[CardRef]) {
    let suit = game.rng.choice("sigil", &Suit::ALL);
    let hand = game.hand.clone();
    for card in hand {
        crate::cards::set_suit(&card, suit);
        game.debuff_card(&card); // set_base, card.lua:1242 -> 143
    }
    game.log(format!("Sigil: the hand is all {}s", suit_title(suit)));
}

/// Create a Legendary Joker.
fn the_soul(game: &mut GameState, _cards: &[CardRef]) {
    game.add_random_joker("The Soul", None, true, "sou", false);
}

/// A random Rare joker, and every dollar you had.
///
/// The rarity is not rolled: the game passes 0.99, which lands in the rare band,
/// so this is always rare and never legendary.
fn wraith(game: &mut GameState, _cards: &[CardRef]) {
    game.add_random_joker("Wraith", Some(crate::jokers::Rarity::Rare), false, "wra", false);
    let money = game.money;
    game.add_money(-money, "Wraith");
}

/// Copy the last Tarot or Planet used this run, itself excepted.
fn the_fool(game: &mut GameState, _cards: &[CardRef]) {
    let last = game.last_tarot_planet.clone();
    if last.is_empty() || last == "c_fool" {
        return;
    }
    if let Some(name) = crate::shop_pool::name_by_consumable_key(&last) {
        if crate::consumables::spec(name).is_some() {
            let spec = crate::consumables::spec_or_panic(name);
            game.add_consumables(&[spec], Edition::None);
        }
    }
}
