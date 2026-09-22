//! Present a simulator run in the shape `BOT.state()` returns.
//!
//! One state shape for every backend: the simulator, the headless engine and
//! the running game all report in the engine's shape, so whatever reads a
//! state -- an encoder, a comparison, a report -- cannot tell which produced
//! it.
//!
//! The observation encoder is the contract between the agent and the game, and
//! it is written against the engine's state dictionary. Rather than write a
//! second encoder for the simulator -- which is how the two would drift -- this
//! builds the same dictionary out of a `GameState`, so `encode` and
//! `legal_actions` cannot tell which engine produced it.
//!
//! Only the keys the encoder and the action mask actually read are built. The
//! engine's own state carries a good deal more (`last_refusal`, `shop_ready`,
//! `stop_use`, the selected indices) that exists for the socket client driving
//! the real game, and inventing simulator answers for those would be fiction
//! nobody reads.
//!
//! # The id tables are the engine's
//!
//! The id tables below are the engine's, not the simulator's, and they are not
//! the same. `SUIT_IDS` in `bot_api.lua` is Spades, Hearts, Clubs, Diamonds --
//! Clubs before Diamonds -- while the simulator's `Suit` enum runs Spades,
//! Hearts, Diamonds, Clubs. Mapping by enum position would silently swap the
//! two suits in every observation, which is the sort of mistake that trains a
//! policy on a game nobody is playing. [`suit_id`] therefore spells the table
//! out the engine's way rather than deriving it from the enum.
//!
//! # No serde
//!
//! `state_dict` returns a JSON-shaped dictionary. With no serde available it is
//! modelled as a [`StateValue`] tree: the consumer is an encoder that walks it
//! by key, and a typed struct would hide which keys exist. `StateValue::Map`
//! is a `BTreeMap`, so a caller iterates the keys in sorted order.

use std::collections::BTreeMap;

use crate::cards::{
    debuffed_of, edition_of, enhancement_of, extra_chips_of, rank_of, seal_of, suit_of, CardRef,
    Edition, Enhancement, Rank, Seal, Suit,
};
use crate::consumables::{ConsumableKind, ConsumableSpec};
use crate::game::{GameState, PackChoice, Phase};
use crate::hands::HandType;
use crate::jokers::{JokerRef, Rarity};
use crate::shop::{PackKind, PackSpec, ShopSlot};
use crate::vocabulary::centres;

/// A JSON-shaped value, so the encoder can walk the state by key.
///
/// With no serde crate, `state_dict` builds this directly. `Map` is a
/// `BTreeMap` so the keys come out in sorted order, which is what makes a
/// fixture comparison deterministic.
#[derive(Clone, Debug, PartialEq)]
pub enum StateValue {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    List(Vec<StateValue>),
    Map(BTreeMap<String, StateValue>),
}

impl From<i32> for StateValue {
    fn from(v: i32) -> Self {
        StateValue::Int(v as i64)
    }
}

impl From<i64> for StateValue {
    fn from(v: i64) -> Self {
        StateValue::Int(v)
    }
}

impl From<f64> for StateValue {
    fn from(v: f64) -> Self {
        StateValue::Float(v)
    }
}

impl From<bool> for StateValue {
    fn from(v: bool) -> Self {
        StateValue::Bool(v)
    }
}

impl From<&str> for StateValue {
    fn from(v: &str) -> Self {
        StateValue::Str(v.to_string())
    }
}

impl From<String> for StateValue {
    fn from(v: String) -> Self {
        StateValue::Str(v)
    }
}

/// A map from owned key/value pairs; the keys are sorted by `BTreeMap`.
fn map(pairs: Vec<(&str, StateValue)>) -> StateValue {
    StateValue::Map(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

// ----------------------------------------------------------------------
// the engine's id tables
// ----------------------------------------------------------------------

/// The engine's `SUIT_IDS`: Spades, Hearts, Clubs, Diamonds.
///
/// The engine orders the suits this way and the simulator's `Suit` enum does
/// not -- it runs Spades, Hearts, Diamonds, Clubs. This spells the engine's
/// table out rather than deriving it from the enum position, because deriving
/// it would swap Clubs and Diamonds in every observation.
pub fn suit_id(suit: Suit) -> i32 {
    match suit {
        Suit::Spades => 1,
        Suit::Hearts => 2,
        Suit::Clubs => 3,
        Suit::Diamonds => 4,
    }
}

/// The engine's `RANK_IDS`: `Two..Ace`, one-based in `Rank` order.
pub fn rank_id(rank: Rank) -> i32 {
    rank.value() as i32 - 1
}

/// The engine's `EDITION_IDS`.
pub fn edition_id(edition: Edition) -> i32 {
    match edition {
        Edition::None => 0,
        Edition::Foil => 1,
        Edition::Holographic => 2,
        Edition::Polychrome => 3,
        Edition::Negative => 4,
    }
}

/// The engine's `SEAL_IDS`.
pub fn seal_id(seal: Seal) -> i32 {
    match seal {
        Seal::None => 0,
        Seal::Gold => 1,
        Seal::Red => 2,
        Seal::Blue => 3,
        Seal::Purple => 4,
    }
}

/// The centre key an enhancement is named by, `ENHANCEMENT_KEYS`.
pub fn enhancement_key(enhancement: Enhancement) -> &'static str {
    match enhancement {
        Enhancement::None => "c_base",
        Enhancement::Bonus => "m_bonus",
        Enhancement::Mult => "m_mult",
        Enhancement::Wild => "m_wild",
        Enhancement::Glass => "m_glass",
        Enhancement::Steel => "m_steel",
        Enhancement::Stone => "m_stone",
        Enhancement::Gold => "m_gold",
        Enhancement::Lucky => "m_lucky",
    }
}

/// The order `bot_api.lua` declares, which the encoder indexes directly.
///
/// Keyed on the enhancement key rather than the enum, because the encoder maps
/// a card's centre through `vocab.enhancement_index`, which is keyed the same
/// way.
pub fn enhancement_id(key: &str) -> i32 {
    match key {
        "c_base" => 0,
        "m_bonus" => 1,
        "m_mult" => 2,
        "m_wild" => 3,
        "m_glass" => 4,
        "m_steel" => 5,
        "m_stone" => 6,
        "m_gold" => 7,
        "m_lucky" => 8,
        _ => 0,
    }
}

/// The engine's `RARITY_IDS`, one-based in `Rarity` order.
pub fn rarity_id(rarity: Rarity) -> i32 {
    rarity as i32
}

/// `SHOP_SETS` in `encoding.py`, one-based when it reaches the encoder.
pub fn shop_set_id(name: &str) -> i32 {
    match name {
        "Joker" => 1,
        "Tarot" => 2,
        "Planet" => 3,
        "Spectral" => 4,
        "Voucher" => 5,
        "Booster" => 6,
        "Default" => 7,
        "Enhanced" => 8,
        _ => 1,
    }
}

/// The engine's state name for a phase, `STATE_BY_PHASE`.
///
/// A phase with no engine equivalent would break the state one-hot, so every
/// one is mapped. `pack` is absent from the table: a `PACK` phase names itself
/// from the pack kind in [`state_dict`], and a `PACK` phase with no pack to
/// read falls back the way Python's `dict.get(..., "SELECTING_HAND")` does.
pub fn state_name(phase: Phase) -> &'static str {
    match phase {
        Phase::BlindSelect => "BLIND_SELECT",
        Phase::Playing => "SELECTING_HAND",
        Phase::RoundEval => "ROUND_EVAL",
        Phase::Shop => "SHOP",
        Phase::GameOver => "GAME_OVER",
        Phase::Won => "GAME_OVER",
        Phase::Pack => "SELECTING_HAND",
    }
}

/// `PACK_STATE_BY_KIND`.
pub fn pack_state(kind: PackKind) -> &'static str {
    match kind {
        PackKind::Arcana => "TAROT_PACK",
        PackKind::Celestial => "PLANET_PACK",
        PackKind::Spectral => "SPECTRAL_PACK",
        PackKind::Standard => "STANDARD_PACK",
        PackKind::Buffoon => "BUFFOON_PACK",
    }
}

/// `_rank_id`: `RANK_IDS`, with zero for "no such value yet".
pub fn rank_id_of(rank: Option<Rank>) -> i32 {
    match rank {
        None => 0,
        Some(rank) => rank_id(rank),
    }
}

/// `_suit_id`: `SUIT_IDS`, with zero for "no such value yet".
pub fn suit_id_of(suit: Option<Suit>) -> i32 {
    match suit {
        None => 0,
        Some(suit) => suit_id(suit),
    }
}

/// `_card_row`: one played card as the encoder reads it.
pub fn card_row(card: &CardRef, highlighted: bool) -> StateValue {
    let key = enhancement_key(enhancement_of(card));
    let mut m = BTreeMap::new();
    m.insert("rank".to_string(), rank_id(rank_of(card)).into());
    // The engine's suit id, not the enum position: see `suit_id`.
    m.insert("suit".to_string(), suit_id(suit_of(card)).into());
    // The encoder maps this through `vocab.enhancement_index`, which is keyed
    // on the *centre* id, not on `ENHANCEMENT_IDS`.
    m.insert("center".to_string(), centres().of(key).into());
    // `card.base.nominal` -- the rank's own chip value, ten for a face card
    // and eleven for an ace. Reporting the Hiker bonus instead left every card
    // in every observation worth zero chips.
    m.insert("chips".to_string(), rank_of(card).chips().into());
    // What Hiker and friends have added to this card for good, kept beside the
    // nominal rather than folded into it.
    m.insert("extra_chips".to_string(), extra_chips_of(card).into());
    m.insert(
        "highlighted".to_string(),
        (if highlighted { 1 } else { 0 }).into(),
    );
    m.insert(
        "debuffed".to_string(),
        (if debuffed_of(card) { 1 } else { 0 }).into(),
    );
    m.insert("edition".to_string(), edition_id(edition_of(card)).into());
    m.insert("seal".to_string(), seal_id(seal_of(card)).into());
    StateValue::Map(m)
}

/// `_joker_row`: one joker, with the state that decides what it is worth.
pub fn joker_row(game: &GameState, joker: &JokerRef) -> StateValue {
    let j = joker.borrow();
    let key = crate::shop_pool::key_by_joker_name(j.name()).unwrap_or("");
    let mut m = BTreeMap::new();
    m.insert("center".to_string(), centres().of(key).into());
    m.insert(
        "sellable".to_string(),
        (if j.eternal { 0 } else { 1 }).into(),
    );
    m.insert("sell_cost".to_string(), game.sell_value(joker).into());
    m.insert("rarity".to_string(), rarity_id(j.spec.rarity).into());
    m.insert("edition".to_string(), edition_id(j.edition).into());
    m.insert("counter".to_string(), j.counter.into());
    m.insert("secondary".to_string(), j.secondary.into());
    m.insert(
        "debuffed".to_string(),
        (if j.debuffed { 1 } else { 0 }).into(),
    );
    m.insert(
        "eternal".to_string(),
        (if j.eternal { 1 } else { 0 }).into(),
    );
    m.insert(
        "perishable".to_string(),
        (if j.perishable { 1 } else { 0 }).into(),
    );
    m.insert("perish_tally".to_string(), j.perish_tally.into());
    m.insert("rental".to_string(), (if j.rental { 1 } else { 0 }).into());
    m.insert(
        "hands_held".to_string(),
        (game.hands_played - j.hands_at_create).into(),
    );
    StateValue::Map(m)
}

/// `_consumable_row`: one held consumable.
pub fn consumable_row(
    _game: &GameState,
    held: &crate::consumables::ConsumableRef,
    usable: bool,
) -> StateValue {
    let h = held.borrow();
    let key = crate::shop_pool::key_by_consumable_name(h.spec.name).unwrap_or("");
    map(vec![
        ("center", centres().of(key).into()),
        ("sellable", 1.into()),
        ("usable", (if usable { 1 } else { 0 }).into()),
        ("edition", edition_id(h.edition).into()),
    ])
}

/// `_card_key_and_set`: a playing card named by its enhancement's centre --
/// `c_base` for a plain one -- whose set is `Default`, or `Enhanced`.
pub fn card_key_and_set(card: &CardRef) -> (String, &'static str) {
    let enhancement = enhancement_of(card);
    let key = enhancement_key(enhancement);
    let set = if enhancement == Enhancement::None {
        "Default"
    } else {
        "Enhanced"
    };
    (key.to_string(), set)
}

/// `_shop_key_and_set`.
pub fn shop_key_and_set(slot: &ShopSlot) -> (String, &'static str) {
    if let Some(joker) = &slot.joker {
        let key = crate::shop_pool::key_by_joker_name(joker.borrow().name()).unwrap_or("");
        return (key.to_string(), "Joker");
    }
    if let Some(spec) = slot.consumable {
        let key = crate::shop_pool::key_by_consumable_name(spec.name).unwrap_or("");
        return (key.to_string(), spec.kind.set_name());
    }
    if let Some(card) = &slot.card {
        return card_key_and_set(card);
    }
    (String::new(), "Joker")
}

/// `_stickers`: eternal, perishable and rental for a joker that may not be one.
///
/// A row that omitted the keys would encode as a joker with no stickers rather
/// than as something that cannot carry them. Zero for all three is the same
/// vector either way; spelling it out keeps the row shape constant.
fn push_stickers(m: &mut BTreeMap<String, StateValue>, joker: Option<&JokerRef>) {
    let (eternal, perishable, rental) = match joker {
        None => (0, 0, 0),
        Some(joker) => {
            let j = joker.borrow();
            (
                if j.eternal { 1 } else { 0 },
                if j.perishable { 1 } else { 0 },
                if j.rental { 1 } else { 0 },
            )
        }
    };
    m.insert("eternal".to_string(), eternal.into());
    m.insert("perishable".to_string(), perishable.into());
    m.insert("rental".to_string(), rental.into());
}

/// `_has_room`: `check_for_buy_space` -- a joker needs a joker slot, a
/// consumable a consumable one.
fn has_room(game: &GameState, slot: &ShopSlot) -> bool {
    if let Some(joker) = &slot.joker {
        return game.room_for_joker(joker);
    }
    if slot.consumable.is_some() {
        return (game.consumables.len() as i32) < game.consumable_slots();
    }
    true // a playing card goes to the deck
}

/// `_shop_row`: one card on the shop's main shelf.
///
/// `slot_price`, `pack_price` and `can_use_consumable` mirror the Python
/// `GameState` methods of the same name. They are not on the Rust `GameState`
/// yet, so the observation builds the same answers here; when they land there,
/// these become thin calls.
fn shop_row(game: &GameState, slot: &ShopSlot, picked: &[CardRef]) -> BTreeMap<String, StateValue> {
    let (key, kind) = shop_key_and_set(slot);
    let price = slot_price(game, slot);
    let card = &slot.card;
    let mut m = BTreeMap::new();
    m.insert("area".to_string(), "shop_jokers".into());
    m.insert("index".to_string(), 0.into()); // renumbered by shop_rows
    m.insert("center".to_string(), centres().of(&key).into());
    m.insert("set".to_string(), shop_set_id(kind).into());
    m.insert("cost".to_string(), price.into());
    m.insert(
        "buyable".to_string(),
        (if game.affords(price) && has_room(game, slot) {
            1
        } else {
            0
        })
        .into(),
    );
    let buy_and_usable = slot
        .consumable
        .is_some_and(|spec| game.affords(price) && can_use_consumable(game, spec, picked));
    m.insert(
        "buy_and_usable".to_string(),
        (if buy_and_usable { 1 } else { 0 }).into(),
    );
    let edition = if let Some(joker) = &slot.joker {
        edition_id(joker.borrow().edition)
    } else if let Some(card) = card {
        edition_id(card.borrow().edition)
    } else {
        0
    };
    m.insert("edition".to_string(), edition.into());
    let seal = match card {
        Some(card) => seal_id(card.borrow().seal),
        None => 0,
    };
    m.insert("seal".to_string(), seal.into());
    push_stickers(&mut m, slot.joker.as_ref());
    m
}

/// `shop_rows`: the whole shop, in the engine's own area order.
///
/// The order is `BOT.state`'s, which is *not* `headless_api`'s `SHOP_AREAS`:
/// jokers, vouchers, boosters against jokers, boosters, vouchers. The
/// observation is built from the first, so this follows the first.
pub fn shop_rows(game: &GameState, picked: &[CardRef]) -> StateValue {
    let shop = match &game.shop {
        Some(shop) => shop,
        None => return StateValue::List(Vec::new()),
    };

    let mut rows: Vec<BTreeMap<String, StateValue>> = shop
        .slots
        .iter()
        .map(|slot| shop_row(game, slot, picked))
        .collect();

    for voucher in shop.vouchers_on_offer() {
        let price = game.price(voucher.cost);
        let mut m = BTreeMap::new();
        m.insert("area".to_string(), "shop_vouchers".into());
        m.insert("index".to_string(), 0.into());
        m.insert("center".to_string(), centres().of(voucher.key).into());
        m.insert("set".to_string(), shop_set_id("Voucher").into());
        m.insert("cost".to_string(), price.into());
        m.insert(
            "buyable".to_string(),
            (if game.affords(price) { 1 } else { 0 }).into(),
        );
        m.insert("buy_and_usable".to_string(), 0.into());
        m.insert("edition".to_string(), 0.into());
        m.insert("seal".to_string(), 0.into());
        push_stickers(&mut m, None);
        rows.push(m);
    }

    for pack in &shop.packs {
        let price = pack_price(game, pack);
        let mut m = BTreeMap::new();
        m.insert("area".to_string(), "shop_booster".into());
        m.insert("index".to_string(), 0.into());
        m.insert("center".to_string(), centres().of(pack.key).into());
        m.insert("set".to_string(), shop_set_id("Booster").into());
        m.insert("cost".to_string(), price.into());
        // Opened rather than stored, so no slot has to be free for it -- what
        // comes out is what needs room, and that is checked when a card is
        // picked.
        m.insert(
            "buyable".to_string(),
            (if game.affords(price) { 1 } else { 0 }).into(),
        );
        m.insert("buy_and_usable".to_string(), 0.into());
        m.insert("edition".to_string(), 0.into());
        m.insert("seal".to_string(), 0.into());
        push_stickers(&mut m, None);
        rows.push(m);
    }

    // 1-based within each area, as the engine numbers them.
    let mut counters: BTreeMap<String, i64> = BTreeMap::new();
    for row in &mut rows {
        let area = match row.get("area") {
            Some(StateValue::Str(area)) => area.clone(),
            _ => String::new(),
        };
        let count = counters.entry(area).or_insert(0);
        *count += 1;
        row.insert("index".to_string(), (*count).into());
    }

    StateValue::List(rows.into_iter().map(StateValue::Map).collect())
}

/// `_pack_row`: one card on offer inside an opened pack.
///
/// `usable` is what gates `PICK_PACK`, and it is the game's own
/// `can_select_card`: a consumable taken from a pack is used the instant it is
/// taken -- the pack screen calls `use_card`, not `buy` -- so it faces the same
/// test as using one from the slots; a playing card always goes to the deck; a
/// joker needs a free slot unless it is Negative.
pub fn pack_row(game: &GameState, option: &PackChoice, picked: &[CardRef]) -> StateValue {
    match option {
        PackChoice::Joker(joker) => {
            let key = crate::shop_pool::key_by_joker_name(joker.borrow().name()).unwrap_or("");
            let mut m = BTreeMap::new();
            m.insert("center".to_string(), centres().of(key).into());
            m.insert("set".to_string(), shop_set_id("Joker").into());
            m.insert(
                "edition".to_string(),
                edition_id(joker.borrow().edition).into(),
            );
            m.insert("seal".to_string(), 0.into());
            m.insert(
                "usable".to_string(),
                (if game.room_for_joker(joker) { 1 } else { 0 }).into(),
            );
            push_stickers(&mut m, Some(joker));
            StateValue::Map(m)
        }
        PackChoice::Card(card) => {
            // Its own set, as the engine reports it (`SET_IDS` in
            // `bot_api.lua`): this said Joker, so every card in a Standard pack
            // read as one.
            let (key, kind) = card_key_and_set(card);
            map(vec![
                ("center", centres().of(&key).into()),
                ("set", shop_set_id(kind).into()),
                ("edition", edition_id(card.borrow().edition).into()),
                ("seal", seal_id(card.borrow().seal).into()),
                ("usable", 1.into()),
                ("eternal", 0.into()),
                ("perishable", 0.into()),
                ("rental", 0.into()),
            ])
        }
        PackChoice::Consumable(spec) => {
            let key = crate::shop_pool::key_by_consumable_name(spec.name).unwrap_or("");
            let usable = can_use_consumable(game, spec, picked);
            map(vec![
                ("center", centres().of(key).into()),
                ("set", shop_set_id(spec.kind.set_name()).into()),
                ("edition", 0.into()),
                ("seal", 0.into()),
                ("usable", (if usable { 1 } else { 0 }).into()),
                ("eternal", 0.into()),
                ("perishable", 0.into()),
                ("rental", 0.into()),
            ])
        }
    }
}

/// Whether the run holds a joker that makes Planets free (Astronomer).
fn has_free_planets(game: &GameState) -> bool {
    game.active_jokers()
        .iter()
        .any(|j| j.borrow().spec.free_planets)
}

/// `GameState.slot_price` -- what a shop slot costs right now.
///
/// The game recomputes a card's cost whenever anything that touches it
/// changes, so a price is not fixed when the shop is stocked: the discount,
/// then Astronomer's free planets, then a rental's flat dollar, then a coupon.
fn slot_price(game: &GameState, slot: &ShopSlot) -> i32 {
    if slot.couponed {
        return 0;
    }
    if let Some(spec) = slot.consumable {
        if spec.kind == ConsumableKind::Planet && has_free_planets(game) {
            return 0;
        }
    }
    if let Some(joker) = &slot.joker {
        if joker.borrow().rental {
            return 1;
        }
    }
    let edition = if let Some(joker) = &slot.joker {
        joker.borrow().edition
    } else if let Some(card) = &slot.card {
        card.borrow().edition
    } else {
        Edition::None
    };
    game.card_cost(slot.base_cost, edition)
}

/// `GameState.pack_price` -- what a booster costs right now.
///
/// `Card:set_cost` zeroes a Celestial booster while Astronomer is held, exactly
/// as it zeroes a Planet.
fn pack_price(game: &GameState, pack: &PackSpec) -> i32 {
    if game.shop_free {
        return 0;
    }
    if pack.kind == PackKind::Celestial && has_free_planets(game) {
        return 0;
    }
    game.price(pack.cost)
}

/// `GameState.can_use_consumable` -- `Card:can_use_consumeable`, branch for
/// branch.
///
/// Only the count of selected cards is checked for most cards; the grouped
/// names need more (a free slot, a joker, a hand to work on). The branch that
/// matters most is the last: anything that selects cards is gated on the phase
/// being `SELECTING_HAND` or one of the pack states, so a targeting Tarot
/// cannot be used in a shop or on the cash-out screen at all.
fn can_use_consumable(game: &GameState, spec: &ConsumableSpec, targets: &[CardRef]) -> bool {
    use crate::game::{
        FREE_CONSUMABLE_NEEDED, FREE_JOKER_NEEDED, PLAIN_JOKER_NEEDED, SPARE_CARD_NEEDED,
    };

    // A hand to work on: in a round, or inside a pack that dealt one.
    let hand_dealt = matches!(game.phase, Phase::Playing | Phase::Pack);
    let plain_jokers = game
        .jokers
        .iter()
        .filter(|j| j.borrow().edition == Edition::None)
        .count();

    if FREE_JOKER_NEEDED.contains(&spec.name) {
        return (game.jokers.len() as i32) < game.joker_slots();
    }
    if FREE_CONSUMABLE_NEEDED.contains(&spec.name) {
        // Using it frees the slot it is sitting in, so holding it is always
        // enough. Compared by centre rather than by card, so this answers the
        // same whether it was handed the registry entry or the held copy.
        let room = (game.consumables.len() as i32) < game.consumable_slots()
            || game
                .consumables
                .iter()
                .any(|c| std::ptr::eq(c.borrow().spec, spec));
        if spec.name != "The Fool" {
            return room;
        }
        return room && !game.last_tarot_planet.is_empty() && game.last_tarot_planet != "c_fool";
    }
    if PLAIN_JOKER_NEEDED.contains(&spec.name) {
        return plain_jokers > 0;
    }
    if spec.name == "Ankh" {
        // Deliberately not a free slot -- that is `check_use`, and the
        // disagreement between the two is the Ankh bug. See `refuses_use`.
        return !game.jokers.is_empty() && game.joker_slots() > 1;
    }
    if spec.name == "Aura" {
        return hand_dealt
            && targets.len() == 1
            && edition_of(&targets[0]) == Edition::None
            && takes_forced_card(game, targets);
    }
    if SPARE_CARD_NEEDED.contains(&spec.name) {
        // They destroy a card at random, and the game will not let the hand go
        // empty that way.
        return hand_dealt && game.hand.len() > 1;
    }
    if spec.targets > 0 {
        return hand_dealt
            && spec.accepts(targets.len() as i32)
            && takes_forced_card(game, targets);
    }
    true
}

/// `held_forced_card`: Cerulean Bell's card, while it is in force and in hand.
fn held_forced_card(game: &GameState) -> Option<CardRef> {
    let boss = game.boss()?;
    let card = game.forced_card.clone()?;
    if game.phase != Phase::Playing || !boss.forces_a_card {
        return None;
    }
    if !game
        .hand
        .iter()
        .any(|c| crate::cards::uid_of(c) == crate::cards::uid_of(&card))
    {
        return None;
    }
    Some(card)
}

/// `_takes_forced_card`: whether a selection holds Cerulean Bell's card, when
/// one is forced.
fn takes_forced_card(game: &GameState, targets: &[CardRef]) -> bool {
    match held_forced_card(game) {
        None => true,
        Some(card) => targets
            .iter()
            .any(|t| crate::cards::uid_of(t) == crate::cards::uid_of(&card)),
    }
}

/// `_reroll_cost`: what a reroll would cost, in or out of a shop.
fn reroll_cost(game: &GameState) -> i32 {
    if let Some(shop) = &game.shop {
        let discount: i32 = game.vouchers.iter().map(|v| v.reroll_discount).sum();
        return shop.reroll_cost(discount);
    }
    if game.free_rerolls_carried > 0 {
        0
    } else {
        game.reroll_price_carried
    }
}

/// The blind's tier title, `kind.value.title()`.
fn blind_title(kind: crate::blinds::BlindKind) -> &'static str {
    match kind {
        crate::blinds::BlindKind::Small => "Small",
        crate::blinds::BlindKind::Big => "Big",
        crate::blinds::BlindKind::Boss => "Boss",
    }
}

/// `_blind_row`: one row of the run info screen -- what this blind asks and
/// pays.
///
/// Built rather than read off the run, because the whole point of the row is
/// that it is knowable *before* the blind is in force: which boss is coming
/// decides what to build for while there is still a shop to spend in.
fn blind_row(
    game: &GameState,
    kind: crate::blinds::BlindKind,
    index: i32,
    boss_key: &str,
) -> StateValue {
    use crate::blinds::{ante_base_chips, boss_by_name, BlindKind};

    let key = if kind == BlindKind::Boss {
        boss_key
    } else if kind == BlindKind::Small {
        "bl_small"
    } else {
        "bl_big"
    };
    let mut mult = kind.mult();
    let mut effect = None;
    if kind == BlindKind::Boss && !boss_key.is_empty() {
        effect = crate::boss_data::boss_row(boss_key).and_then(|row| boss_by_name(row.name));
        if let Some(effect) = effect {
            mult = effect.chip_mult;
        }
    }
    let target = (ante_base_chips(game.ante, game.blind_scaling()) as f64
        * mult
        * game.deck_config().ante_scaling) as i64;
    let reward = crate::blinds::reward_for(kind, effect);
    // Defeated and skipped are different states on the run info screen, and a
    // blind that was skipped was never beaten.
    let defeated = index < game.blind_index && !game.skipped_this_ante.contains(&index);
    let skipped = game.skipped_this_ante.contains(&index);
    // "Current" is the game's own `blind_states` value, set while the blind is
    // being played -- not while it merely sits next in line.
    let current = index == game.blind_index && game.blind_target() != 0;
    map(vec![
        ("kind", blind_title(kind).into()),
        ("blind", centres().blind(key).into()),
        ("chips", target.into()),
        ("reward", reward.into()),
        ("defeated", (if defeated { 1 } else { 0 }).into()),
        ("skipped", (if skipped { 1 } else { 0 }).into()),
        ("current", (if current { 1 } else { 0 }).into()),
    ])
}

/// `_deck_counts`: the whole deck's composition, in engine index order.
///
/// A hot loop in Python -- the profile showed 1,956,574 rank/suit lookups in
/// six thousand steps, because `Enum.__hash__` is a Python call -- so it stays
/// a plain loop over fixed-length vectors. The vectors are indexed by the
/// *engine's* ids: the rank vector is `RANK_IDS` order (`Two..Ace`), the suit
/// vector is `SUIT_IDS` order (Spades, Hearts, Clubs, Diamonds -- Clubs before
/// Diamonds), the enhancements are `ENHANCEMENT_IDS` order, the seals
/// `SEAL_IDS` and the editions `EDITION_IDS`. The ids are the engine's, not the
/// simulator's, and mapping them by enum position would swap two suits in every
/// observation.
pub fn deck_counts(cards: &[CardRef]) -> StateValue {
    let mut ranks = [0i64; 13];
    let mut suits = [0i64; 4];
    let mut enhancements = [0i64; 9];
    let mut seals = [0i64; 5];
    let mut editions = [0i64; 5];
    let mut extra_sum = 0i64;
    let mut extra_nonzero = 0i64;
    for card in cards {
        ranks[(rank_id(rank_of(card)) - 1) as usize] += 1;
        suits[(suit_id(suit_of(card)) - 1) as usize] += 1;
        enhancements[enhancement_id(enhancement_key(enhancement_of(card))) as usize] += 1;
        seals[seal_id(seal_of(card)) as usize] += 1;
        editions[edition_id(edition_of(card)) as usize] += 1;
        let extra = extra_chips_of(card) as i64;
        extra_sum += extra;
        if extra != 0 {
            extra_nonzero += 1;
        }
    }
    let len = cards.len() as f64;
    let mean = if cards.is_empty() {
        0.0
    } else {
        extra_sum as f64 / len
    };
    let share = if cards.is_empty() {
        0.0
    } else {
        extra_nonzero as f64 / len
    };
    let list =
        |values: &[i64]| StateValue::List(values.iter().map(|v| StateValue::Int(*v)).collect());
    map(vec![
        ("extra_chips_mean", mean.into()),
        ("extra_chips_share", share.into()),
        ("ranks", list(&ranks)),
        ("suits", list(&suits)),
        ("enhancements", list(&enhancements)),
        ("seals", list(&seals)),
        ("editions", list(&editions)),
    ])
}

/// `state_dict`: a simulator run, in the shape the observation encoder reads.
///
/// `selection` is the picking the environment is holding on the agent's
/// behalf. The simulator has no notion of a highlighted card -- it takes the
/// indices with the action -- so the environment owns that and hands it in.
///
/// `toggles_used` and `joker_swaps_used` are the caller's UI counters, exactly
/// as Python's `state_dict(game, selection, toggles_used=0,
/// joker_swaps_used=0)` takes them: the two numbers come *from the caller*,
/// not from the run, and must reach the output unchanged. They default to zero
/// in Python because a scripted driver has no buttons to press (see
/// `sorted_rank`/`sorted_suit`) -- but a constant can neither be compared nor
/// proven to arrive, so the replay fixture varies both per step and the state
/// fixture pins the values. Neither argument is derived from the game.
pub fn state_dict(
    game: &GameState,
    selection: &[usize],
    toggles_used: i32,
    joker_swaps_used: i32,
) -> StateValue {
    use crate::blinds::BlindKind;

    let hand = &game.hand;
    let chosen: Vec<usize> = selection
        .iter()
        .cloned()
        .filter(|index| *index < hand.len())
        .collect();
    let picked: Vec<CardRef> = chosen.iter().map(|index| hand[*index].clone()).collect();

    let state = if game.phase == Phase::Pack {
        match &game.pack {
            Some(pack) => pack_state(pack.kind),
            None => state_name(game.phase),
        }
    } else {
        state_name(game.phase)
    };

    let mut levels = BTreeMap::new();
    for hand_type in HandType::ALL {
        let (chips, mult) = game.hand_levels.values(hand_type);
        levels.insert(
            hand_type.label().to_string(),
            map(vec![
                ("level", game.hand_levels.level(hand_type).into()),
                ("played", game.hand_levels.played(hand_type).into()),
                ("chips", chips.into()),
                ("mult", mult.into()),
            ]),
        );
    }

    let mut made = map(vec![
        ("name", "".into()),
        ("level", 0.into()),
        ("chips", 0.into()),
        ("mult", 0.into()),
        ("cards", 0.into()),
        ("estimate", 0.into()),
    ]);
    if !chosen.is_empty() {
        let result = game.evaluate_selection(&picked);
        let (chips, mult) = game.hand_levels.values(result.hand);
        // The line the game shows while a player is choosing: the hand's own
        // chips *plus the nominals of the cards that will actually score*, and
        // a count of those rather than of the ones highlighted.
        let card_chips: i32 = result.scoring.iter().map(|c| rank_of(c).chips()).sum();
        made = map(vec![
            ("name", result.hand.label().into()),
            ("level", game.hand_levels.level(result.hand).into()),
            ("chips", (chips + card_chips).into()),
            ("mult", mult.into()),
            ("cards", (result.scoring.len() as i32).into()),
            ("estimate", 0.into()),
        ]);
    }

    let deck = deck_counts(&game.full_deck);
    let boss_blind = game.ante_boss.clone();

    let mut rows = Vec::new();
    for (index, kind) in [BlindKind::Small, BlindKind::Big, BlindKind::Boss]
        .into_iter()
        .enumerate()
    {
        rows.push(blind_row(game, kind, index as i32, &boss_blind));
    }

    // The tag for the blind *on deck*, which is knowable in the shop and on the
    // select screen -- the whole point of it, since skipping trades the blind's
    // money and chips for exactly this.
    let tag_key: &str = {
        let index = game.blind_index;
        if index >= 0 && (index as usize) < game.ante_tag_keys.len() {
            game.ante_tag_keys[index as usize].as_str()
        } else {
            ""
        }
    };

    let mut vouchers: Vec<String> = game.vouchers.iter().map(|v| v.key.to_string()).collect();
    vouchers.sort();

    let hand_rows = StateValue::List(
        hand.iter()
            .enumerate()
            .map(|(index, card)| card_row(card, chosen.contains(&index)))
            .collect(),
    );
    let jokers = StateValue::List(game.jokers.iter().map(|j| joker_row(game, j)).collect());
    let consumables = StateValue::List(
        game.consumables
            .iter()
            .map(|held| {
                let usable = can_use_consumable(game, held.borrow().spec, &picked);
                consumable_row(game, held, usable)
            })
            .collect(),
    );
    let shop = shop_rows(game, &picked);
    let pack = StateValue::List(
        game.pack_options
            .iter()
            .map(|option| pack_row(game, option, &picked))
            .collect(),
    );

    // In force, not merely on deck -- same rule as `blind_chips`.
    let boss = game
        .blind
        .as_ref()
        .is_some_and(|blind| blind.kind == BlindKind::Boss)
        && game.blind_target() != 0;

    map(vec![
        ("state_name", state.into()),
        // A literal in Python (state.py:471): the observation is only built
        // for a live run, so it is pinned, not derived.
        ("in_run", 1.into()),
        // What the run was started with. Constant for its whole length.
        ("deck", game.deck.clone().into()),
        ("stake", game.stake.into()),
        ("ante", game.ante.into()),
        ("round", game.round_number.into()),
        ("dollars", game.money.into()),
        ("chips", game.chips_scored.into()),
        // Zero outside a round: the blind is held through the select screen and
        // the shop so it can be offered, but it is not in force.
        ("blind_chips", game.blind_target().into()),
        ("hands_left", game.hands_left.into()),
        ("discards_left", game.discards_left.into()),
        ("joker_limit", game.joker_slots().into()),
        ("consumable_limit", game.consumable_slots().into()),
        // current_round.reroll_cost, which the game keeps between shops.
        ("reroll_cost", reroll_cost(game).into()),
        // How far into debt the run may go (Credit Card).
        ("bankrupt_at", game.bankrupt_at().into()),
        ("won", (if game.phase == Phase::Won { 1 } else { 0 }).into()),
        ("selection_size", (chosen.len() as i32).into()),
        // A literal in Python (state.py:499), so it is pinned, not derived.
        ("highlight_limit", 5.into()),
        // The simulator has no sort buttons, so the mask never offers the
        // action and both are reported already sorted. Literals in Python
        // (state.py:504-505), so they are pinned, not derived.
        ("sorted_rank", 1.into()),
        ("sorted_suit", 1.into()),
        // Caller-supplied, not derived from the game (see the doc comment).
        ("toggles_used", toggles_used.into()),
        ("joker_swaps_used", joker_swaps_used.into()),
        ("boss", (if boss { 1 } else { 0 }).into()),
        (
            "skippable",
            (if game.blind_index < 2 { 1 } else { 0 }).into(),
        ),
        ("offered_tag", centres().tag(tag_key).into()),
        ("best_hand", game.best_hand.into()),
        ("deck_size", (game.full_deck.len() as i32).into()),
        ("deck_cards", deck),
        // The four values the run holds on behalf of a joker, by id because
        // that is what the encoder's tables are keyed on.
        ("idol_rank", rank_id_of(game.idol_rank).into()),
        ("idol_suit", suit_id_of(game.idol_suit).into()),
        ("ancient_suit", suit_id_of(game.ancient_suit).into()),
        ("mail_rank", rank_id_of(game.mail_rank).into()),
        ("castle_suit", suit_id_of(game.castle_suit).into()),
        (
            "vouchers",
            StateValue::List(vouchers.into_iter().map(StateValue::Str).collect()),
        ),
        ("blinds", StateValue::List(rows)),
        ("hand_levels", StateValue::Map(levels)),
        ("selected_hand", made),
        ("hand", hand_rows),
        ("jokers", jokers),
        ("consumables", consumables),
        ("shop", shop),
        ("pack", pack),
    ])
}
// ----------------------------------------------------------------------
// the canonical rendering and digest
// ----------------------------------------------------------------------
//
// These mirror `tools/state_render.py` byte for byte. The observation fixture
// (`tests/state_fixture.rs`) and the replay fixture (`tests/replay_fixture.rs`)
// both compare through this code, so the leaf rendering and the digest can
// never disagree about what a value is. The Python module's docstring carries
// the format; the anchors at the bottom pin the hash itself.

/// FNV-1a, 64-bit: the published parameters.
pub const FNV_OFFSET_BASIS: u64 = 0xCBF2_9CE4_8422_2325;
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01B3;

/// FNV-1a 64-bit over `data`.
pub fn fnv1a64(data: &[u8]) -> u64 {
    fnv1a64_update(FNV_OFFSET_BASIS, data)
}

/// Continue an FNV-1a-64 digest over more bytes.
///
/// FNV-1a is a byte-at-a-time fold, so `fnv1a64_update(fnv1a64(a), b)` is
/// exactly `fnv1a64(a ++ b)` for any split of the input. The sweep fixture
/// (`tests/fuzz_sweep.rs`) relies on that: one running digest per seed over
/// every step's canonical bytes, rather than hashing the concatenation at the
/// end, which is what keeps the fixture one line per seed. The unit test below
/// pins the identity against the one-shot hash, and `tools/state_render.py`
/// carries the same function, so the streaming form cannot drift.
pub fn fnv1a64_update(digest: u64, data: &[u8]) -> u64 {
    let mut digest = digest;
    for &byte in data {
        digest ^= byte as u64;
        digest = digest.wrapping_mul(FNV_PRIME);
    }
    digest
}

/// One typed value, as `tools/state_render.py` writes it.
///
/// Bools render `i:0`/`i:1`, floats as their IEEE-754 bits so the comparison
/// is exact, `None` as `n:`. An empty list or map is a leaf of its own.
pub fn render_value(value: &StateValue) -> String {
    match value {
        StateValue::Null => "n:".to_string(),
        StateValue::Bool(v) => format!("i:{}", if *v { 1 } else { 0 }),
        StateValue::Int(v) => format!("i:{}", v),
        StateValue::Float(v) => format!("f:{}", v.to_bits()),
        StateValue::Str(v) => format!("s:{}", v),
        StateValue::List(items) if items.is_empty() => "[]".to_string(),
        StateValue::Map(m) if m.is_empty() => "{}".to_string(),
        StateValue::List(_) | StateValue::Map(_) => unreachable!("not a leaf"),
    }
}

/// Flatten `value` to `path<TAB>value` leaves, sorted by path.
///
/// Dicts are walked in sorted key order (a `StateValue::Map` is already a
/// `BTreeMap`) and lists in index order, so the leaves come out sorted. An
/// empty container is itself a leaf (`jokers\t[]`), which is how a key that is
/// empty in every state is still compared.
pub fn flatten_state(value: &StateValue, path: &str, out: &mut Vec<(String, String)>) {
    match value {
        StateValue::Map(m) => {
            if m.is_empty() {
                out.push((path.to_string(), render_value(value)));
                return;
            }
            for (key, child) in m {
                let child_path = if path.is_empty() {
                    key.clone()
                } else {
                    format!("{}.{}", path, key)
                };
                flatten_state(child, &child_path, out);
            }
        }
        StateValue::List(items) => {
            if items.is_empty() {
                out.push((path.to_string(), render_value(value)));
                return;
            }
            for (i, item) in items.iter().enumerate() {
                flatten_state(item, &format!("{}[{}]", path, i), out);
            }
        }
        leaf => out.push((path.to_string(), render_value(leaf))),
    }
}

/// The top-level key a leaf path belongs to.
pub fn top_key(path: &str) -> &str {
    let end = path.find(['.', '[']).unwrap_or(path.len());
    &path[..end]
}

/// FNV-1a-64 of a subtree's leaves written `path<TAB>value`, joined by `\n`.
pub fn digest_leaves(leaves: &[(String, String)]) -> u64 {
    fnv1a64(&leaves_to_bytes(leaves))
}

/// The exact bytes `digest_leaves` hashes: `path<TAB>value` joined by `\n`.
///
/// Exposed separately because the fuzz sweep feeds *one step's* bytes into a
/// running digest; it needs the bytes, not just their hash.
pub fn leaves_to_bytes(leaves: &[(String, String)]) -> Vec<u8> {
    let mut bytes: Vec<u8> = Vec::new();
    for (i, (path, value)) in leaves.iter().enumerate() {
        if i > 0 {
            bytes.push(b'\n');
        }
        bytes.extend_from_slice(path.as_bytes());
        bytes.push(b'\t');
        bytes.extend_from_slice(value.as_bytes());
    }
    bytes
}

/// One digest per top-level key of `state`.
///
/// Per key rather than one for the whole dictionary: a mismatch names the key
/// immediately, and the generator's `--dump` mode turns it into the exact
/// differing leaf.
pub fn key_digests(state: &StateValue) -> BTreeMap<String, u64> {
    let mut leaves = Vec::new();
    flatten_state(state, "", &mut leaves);
    let mut grouped: BTreeMap<String, Vec<(String, String)>> = BTreeMap::new();
    for (path, value) in leaves {
        grouped
            .entry(top_key(&path).to_string())
            .or_default()
            .push((path, value));
    }
    grouped
        .into_iter()
        .map(|(key, leaves)| (key, digest_leaves(&leaves)))
        .collect()
}
#[cfg(test)]
mod tests {
    use super::*;

    /// The same anchors `tools/state_render.py` pins, so a mismatch in the hash
    /// cannot be mistaken for a state divergence. `python tools/state_render.py`
    /// prints the same three numbers.
    #[test]
    fn fnv1a64_matches_the_python_side() {
        assert_eq!(fnv1a64(b""), 0xCBF2_9CE4_8422_2325);
        assert_eq!(fnv1a64(b"a"), 0xAF63_DC4C_8601_EC8C);
        assert_eq!(fnv1a64(b"hello, world\n"), 0xE60E_7EE6_4882_9675);
    }

    /// The streaming form the sweep fixture depends on.
    ///
    /// `fnv1a64_update` fed every step's bytes in turn must equal the one-shot
    /// hash of the concatenation, for arbitrary splits -- verified here against
    /// `fnv1a64`, not assumed. `tools/state_render.py`'s `fnv1a64_update` is the
    /// same fold, and its anchor `fnv1a64("hello, world\n")` is asserted above.
    #[test]
    fn fnv1a64_update_is_incremental() {
        let whole: Vec<u8> = (0u16..4096).map(|i| (i % 251) as u8).collect();
        let one_shot = fnv1a64(&whole);
        for split in [0usize, 1, 2, 7, 1000, 4095, 4096] {
            let mut digest = FNV_OFFSET_BASIS;
            digest = fnv1a64_update(digest, &whole[..split]);
            digest = fnv1a64_update(digest, &whole[split..]);
            assert_eq!(digest, one_shot, "split at {}", split);
        }
        // Many small chunks, the shape the sweep actually uses (one call per
        // step), must land on the same digest as the concatenation.
        let mut digest = FNV_OFFSET_BASIS;
        for chunk in whole.chunks(13) {
            digest = fnv1a64_update(digest, chunk);
        }
        assert_eq!(digest, one_shot);
        // The offset basis is FNV-1a of the empty string, so an empty update
        // is a no-op rather than a reset.
        assert_eq!(fnv1a64_update(FNV_OFFSET_BASIS, b""), FNV_OFFSET_BASIS);
    }

    /// The renderer and digest over a small tree whose bytes are spelled out,
    /// so the Python and Rust versions are pinned to the same input, not just
    /// to each other on a run.
    #[test]
    fn digest_of_a_known_tree_is_stable() {
        let mut inner = BTreeMap::new();
        inner.insert("b".to_string(), StateValue::Int(2));
        inner.insert("a".to_string(), StateValue::Str("x".to_string()));
        let tree = StateValue::Map({
            let mut m = BTreeMap::new();
            m.insert(
                "k".to_string(),
                StateValue::List(vec![StateValue::Bool(true)]),
            );
            m.insert("m".to_string(), StateValue::Map(inner));
            m.insert("z".to_string(), StateValue::List(Vec::new()));
            m
        });
        let mut leaves = Vec::new();
        flatten_state(&tree, "", &mut leaves);
        assert_eq!(
            leaves,
            vec![
                ("k[0]".to_string(), "i:1".to_string()),
                ("m.a".to_string(), "s:x".to_string()),
                ("m.b".to_string(), "i:2".to_string()),
                ("z".to_string(), "[]".to_string()),
            ]
        );
        // FNV-1a-64 of "k[0]\ti:1\nm.a\ts:x\nm.b\ti:2\nz\t[]".
        assert_eq!(digest_leaves(&leaves), 0xAE15_FC1A_7FF7_8BF1);
    }
}
