//! Joker definitions.
//!
//! Jokers are data, not engine code: each one is a `JokerSpec` with optional
//! hooks that the scoring pipeline calls. Adding a joker means adding a
//! `register(...)` call here -- the engine never needs to change.
//!
//! ## Why the hooks take a `JokerRef`
//!
//! The Python hooks are closures over their joker's numbers, and the scoring
//! pipeline calls them while walking the row. Rust cannot lend out a row element
//! mutably and the whole run mutably at the same time, so instances live behind
//! `Rc<RefCell<..>>` and a hook is handed a cheap handle plus the run -- which is
//! exactly what Python's object reference means. A hook grows its own counter
//! with `joker.borrow_mut().counter += 1`, and every other joker sees it, because
//! there is only one instance.

use std::cell::RefCell;
use std::rc::Rc;

use crate::cards::{CardRef, Edition, Enhancement, Suit};
use crate::effects::ScoreContext;
use crate::game::GameState;
use crate::hands::HandType;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
#[repr(u8)]
pub enum Rarity {
    Common = 1,
    Uncommon = 2,
    Rare = 3,
    Legendary = 4,
}

impl Rarity {
    pub fn from_index(index: u8) -> Option<Rarity> {
        Some(match index {
            1 => Rarity::Common,
            2 => Rarity::Uncommon,
            3 => Rarity::Rare,
            4 => Rarity::Legendary,
            _ => return None,
        })
    }
}

/// BASE_COST.
pub const fn base_cost(rarity: Rarity) -> i32 {
    match rarity {
        Rarity::Common => 4,
        Rarity::Uncommon => 6,
        Rarity::Rare => 8,
        Rarity::Legendary => 20,
    }
}

/// What an edition adds to a card's price, and so to half of it.
///
/// From Card:set_cost, where the same numbers serve buying and selling.
pub fn edition_value(edition: Edition) -> i32 {
    match edition {
        Edition::None => 0,
        Edition::Foil => 2,
        Edition::Holographic => 3,
        Edition::Polychrome => 5,
        Edition::Negative => 5,
    }
}

pub type JokerRef = Rc<RefCell<JokerInstance>>;

pub fn make_ref(instance: JokerInstance) -> JokerRef {
    Rc::new(RefCell::new(instance))
}

/// `context.other_joker`: asked about each joker in the row, straight after that
/// joker's own effect and before its polychrome (state_events.lua:918-930).
/// Called as (joker, other joker, ctx). Baseball Card.
pub type OtherJokerHook = fn(&JokerRef, &JokerRef, &mut ScoreContext, &mut GameState);

pub type ScoredHook = fn(&JokerRef, &CardRef, &mut ScoreContext, &mut GameState);
pub type HeldHook = fn(&JokerRef, &CardRef, &mut ScoreContext, &mut GameState);
pub type IndepHook = fn(&JokerRef, &mut ScoreContext, &mut GameState);
pub type UpdateHook = fn(&JokerRef, &mut ScoreContext, &mut GameState);
pub type RoundHook = fn(&JokerRef, &mut GameState);
/// Cards leaving the deck, whatever took them. Canio, Glass Joker.
pub type CardsHook = fn(&JokerRef, &[CardRef], &mut GameState);
pub type DiscardHook = fn(&JokerRef, &[CardRef], &mut GameState);
pub type RetriggerHook = fn(&JokerRef, &CardRef, &mut ScoreContext, &mut GameState) -> i32;

/// Which direction a copier looks. `"right"` (Blueprint) or `"leftmost"`
/// (Brainstorm).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Copier {
    Right,
    Leftmost,
}

#[derive(Clone, Copy, Debug)]
pub struct JokerSpec {
    pub name: &'static str,
    pub rarity: Rarity,
    pub text: &'static str,
    pub cost: i32,
    pub init_counter: f64,
    pub init_secondary: f64,
    pub update: Option<UpdateHook>,
    /// When `update` runs relative to the hand it is part of. The game is not
    /// consistent about this and the difference is visible on the very first
    /// hand: Ice Cream, Runner and Square Joker grow under context.after, so
    /// they score their old value, while Green Joker increments while the last
    /// played card is scoring and so pays its new one immediately.
    pub update_before_scoring: bool,
    pub scored: Option<ScoredHook>,
    /// A scoring Lucky card whose +Mult or $ roll hit on this trigger, asked
    /// right after `scored` and never on a copy's behalf. Lucky Cat.
    pub lucky_trigger: Option<ScoredHook>,
    /// Growth in the per-card branch under `not context.blueprint`, asked right
    /// after `scored` and never on a copy's behalf. Wee Joker.
    pub scored_growth: Option<ScoredHook>,
    pub held: Option<HeldHook>,
    pub independent: Option<IndepHook>,
    pub other_joker: Option<OtherJokerHook>,
    pub round_end: Option<RoundHook>,
    pub discarded: Option<DiscardHook>,
    pub retrigger_scored: Option<RetriggerHook>,
    pub retrigger_held: Option<RetriggerHook>,
    // Triggers outside the scoring of a hand. The game fires these at points the
    // scoring pipeline never reaches, and a joker whose whole effect lives here
    // scores nothing -- which is why they were invisible to the differential and
    // had to be listed as unbuilt rather than assumed done.
    pub on_blind_select: Option<RoundHook>,
    pub on_round_start: Option<RoundHook>,
    pub on_sell: Option<RoundHook>,
    /// Money listed on the cash-out screen, as against `round_end` which is decay
    /// and growth the moment the round closes.
    pub round_money: Option<RoundHook>,
    pub on_reroll: Option<RoundHook>,
    pub on_pack_skip: Option<RoundHook>,
    pub on_pack_open: Option<RoundHook>,
    pub on_cards_destroyed: Option<CardsHook>,
    /// Narrower than the above on purpose: only glass cards that shattered while
    /// scoring, which is the game's own separate list.
    pub on_glass_shattered: Option<CardsHook>,
    pub on_shop_end: Option<RoundHook>,
    pub rerolls_a_hand: bool,
    pub before_hand: Option<CardsHook>,
    /// context.before, for a branch without `not context.blueprint`: the whole
    /// pass runs, in row order, ahead of the cards and of every joker's main
    /// effect (state_events.lua:628-638), and a Blueprint or a Brainstorm
    /// repeats it.
    pub before: Option<IndepHook>,
    /// context.debuffed_hand: a hand the boss refused still asks every joker,
    /// after scoring nothing (state_events.lua:1015-1027).
    pub on_debuffed_hand: Option<RoundHook>,
    pub after_hand: Option<IndepHook>,
    pub on_first_discard: Option<DiscardHook>,
    pub copier: Option<Copier>,
    /// The shop will not offer these unless the run already has a card with that
    /// enhancement -- no Lucky Cat without a lucky card. Taken from the game's
    /// own enhancement_gate field.
    pub enhancement_gate: &'static str,
    pub hand_size: i32,
    /// True when the hand size this joker gives is its counter rather than a
    /// fixed number -- Turtle Bean starts at five and loses one a round.
    pub hand_size_from_counter: bool,
    pub extra_hands: i32,
    pub extra_discards: i32,
    // Jokers that change the shape of a run rather than the score of a hand.
    // These are read by the shop and the round, not by the scoring pipeline, so
    // they are declared rather than hooked -- a hook that never fires during
    // scoring is indistinguishable from a joker that does nothing.
    pub free_rerolls: i32,
    pub debt_limit: i32,
    pub interest_bonus: i32,
    pub free_planets: bool,
    pub allows_duplicates: bool,
    pub prevents_death: bool,
    pub disables_boss_on_sell: bool,
}

impl JokerSpec {
    /// The base every registry entry spreads from: `..JokerSpec::DEFAULT`.
    ///
    /// A `const` rather than `Default`, because the registry is built in const
    /// context and `..Default::default()` is not const. Every field here is the
    /// Python dataclass's own default.
    pub const DEFAULT: JokerSpec = JokerSpec {
        name: "",
        rarity: Rarity::Common,
        text: "",
        cost: 0,
        init_counter: 0.0,
        init_secondary: 0.0,
        update: None,
        update_before_scoring: false,
        scored: None,
        lucky_trigger: None,
        scored_growth: None,
        held: None,
        independent: None,
        other_joker: None,
        round_end: None,
        discarded: None,
        retrigger_scored: None,
        retrigger_held: None,
        on_blind_select: None,
        on_round_start: None,
        on_sell: None,
        round_money: None,
        on_reroll: None,
        on_pack_skip: None,
        on_pack_open: None,
        on_cards_destroyed: None,
        on_glass_shattered: None,
        on_shop_end: None,
        rerolls_a_hand: false,
        before_hand: None,
        before: None,
        on_debuffed_hand: None,
        after_hand: None,
        on_first_discard: None,
        copier: None,
        enhancement_gate: "",
        hand_size: 0,
        hand_size_from_counter: false,
        extra_hands: 0,
        extra_discards: 0,
        free_rerolls: 0,
        debt_limit: 0,
        interest_bonus: 0,
        free_planets: false,
        allows_duplicates: false,
        prevents_death: false,
        disables_boss_on_sell: false,
    };
}

#[derive(Debug)]
pub struct JokerInstance {
    pub spec: &'static JokerSpec,
    /// Age, for the random draws that sort by it -- see `cards::next_sort_id`.
    /// Stamped here, when the joker is built, because that is where Card:init
    /// stamps sort_id. A shop builds its shelf in slot order and buying moves
    /// that same card into the row, so a joker bought second out of an earlier
    /// slot is the older one.
    pub uid: u64,
    pub edition: Edition,
    pub counter: f64,
    pub eternal: bool,
    /// The stake's stickers. Perishable counts rounds down and debuffs the joker
    /// at zero; rental takes three dollars at the end of every round.
    pub perishable: bool,
    pub perish_tally: i32,
    pub rental: bool,
    pub debuffed: bool,
    /// Jokers that count hands measure from when they were acquired, not from
    /// the start of the run -- the game stores this as hands_played_at_create.
    pub hands_at_create: i32,
    /// A second counter for the jokers that keep two numbers -- Yorick's
    /// countdown to its next X1, Invisible Joker's rounds held.
    pub secondary: f64,
    /// Egg grows this on its own; Gift Card grows every joker's.
    pub extra_sell_value: f64,
    /// To Do List's poker hand. The game keeps it in the joker's own ability
    /// table -- ability.to_do_poker_hand -- so two of them name two hands.
    pub named_hand: Option<HandType>,
}

impl JokerInstance {
    pub fn new(spec: &'static JokerSpec) -> Self {
        JokerInstance {
            spec,
            uid: crate::cards::next_sort_id(),
            edition: Edition::None,
            counter: spec.init_counter,
            eternal: false,
            perishable: false,
            perish_tally: 0,
            rental: false,
            debuffed: false,
            hands_at_create: 0,
            secondary: spec.init_secondary,
            extra_sell_value: 0.0,
            named_hand: None,
        }
    }

    pub fn name(&self) -> &'static str {
        self.spec.name
    }

    /// Half the price, and the price includes the edition.
    ///
    /// Ignoring the edition made a polychrome joker sell for what a plain one
    /// sells for. It is not a rounding difference: a Hex turns a joker
    /// polychrome, which is five dollars on its price and two on its sell value,
    /// and Temperance pays out the sell value of every joker held.
    ///
    /// A rental costs a dollar however expensive the joker is, so it sells for
    /// one. The list price here ignores the run's discount --
    /// `GameState::sell_value` is the one that knows about Liquidation and should
    /// be preferred wherever the run is at hand.
    pub fn sell_value(&self) -> i32 {
        let cost = if self.rental {
            1
        } else {
            self.spec.cost + edition_value(self.edition)
        };
        (cost / 2).max(1) + self.extra_sell_value as i32
    }
}

impl std::fmt::Display for JokerInstance {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let tag = if self.edition == Edition::None {
            String::new()
        } else {
            format!("[{}]", self.edition.as_str())
        };
        let num = if self.counter == self.spec.init_counter {
            String::new()
        } else {
            format!("({})", self.counter)
        };
        write!(f, "{}{}{}", self.spec.name, tag, num)
    }
}

/// Smeared Joker collapses the four suits into two.
pub fn smeared_pairs(suit: Suit) -> Suit {
    match suit {
        Suit::Hearts => Suit::Diamonds,
        Suit::Diamonds => Suit::Hearts,
        Suit::Spades => Suit::Clubs,
        Suit::Clubs => Suit::Spades,
    }
}

/// Does this card count as that suit, for this run?
///
/// Smeared Joker makes Hearts and Diamonds one suit and Spades and Clubs
/// another, which no property on the card can know about -- so every suit test
/// a joker makes has to come through here. Takes the run rather than a scoring
/// context, because the tests a joker makes while *discarding* have no context
/// to hand.
pub fn suit_matches_for(card: &CardRef, suit: Suit, game: &GameState) -> bool {
    if card.borrow().counts_as_suit(suit) {
        return true;
    }
    if game.has_smeared() {
        return card.borrow().counts_as_suit(smeared_pairs(suit));
    }
    false
}

/// The same question, asked from inside scoring.
///
/// Python reads the run off `ctx.game`; the port passes it explicitly, because
/// the context no longer borrows the run -- see `effects.rs`.
pub fn suit_matches(card: &CardRef, suit: Suit, game: &GameState) -> bool {
    suit_matches_for(card, suit, game)
}

/// Card:is_suit(suit, bypass_debuff), the ordinary branch (card.lua:4076-4087):
///
/// ```text
///     if self.debuff and not bypass_debuff then return end
///     if self.ability.effect == 'Stone Card' then return false end
///     if self.ability.name == "Wild Card" then return true end
///     if next(find_joker('Smeared Joker')) and <same colour> then return true end
///     return self.base.suit == suit
/// ```
///
/// Without bypass_debuff this is `suit_matches_for`. Flower Pot is the one joker
/// that passes it (card.lua:3816-3819), so a debuffed card still counts its suit
/// there.
pub fn is_suit_for(card: &CardRef, suit: Suit, game: &GameState, bypass_debuff: bool) -> bool {
    let (debuffed, is_stone, wild, card_suit) = {
        let c = card.borrow();
        (
            c.debuffed,
            c.is_stone(),
            c.enhancement == Enhancement::Wild,
            c.suit,
        )
    };
    if debuffed && !bypass_debuff {
        return false;
    }
    if is_stone {
        return false;
    }
    if wild {
        return true;
    }
    if game.has_smeared() && card_suit == smeared_pairs(suit) {
        return true;
    }
    card_suit == suit
}

/// Card:is_face(from_boss), card.lua:964-970:
///
/// ```text
///     if self.debuff and not from_boss then return end
///     local id = self:get_id()
///     if id == 11 or id == 12 or id == 13 or next(find_joker("Pareidolia"))
/// ```
///
/// A debuffed card is no face card unless a boss is asking: The Plant passes
/// from_boss (blind.lua:630), the jokers never do. get_id answers a Stone card
/// with a random negative (card.lua:958-960), so a Stone King is no face card --
/// but the Pareidolia test does not look at the id, so beside Pareidolia every
/// card is one, Stone included. find_joker leaves out a debuffed Pareidolia
/// (misc_functions.lua:903-907), as has_pareidolia does.
pub fn is_face_for(card: &CardRef, game: &GameState, from_boss: bool) -> bool {
    let (debuffed, is_stone, rank) = {
        let c = card.borrow();
        (c.debuffed, c.is_stone(), c.rank)
    };
    if debuffed && !from_boss {
        return false;
    }
    if game.has_pareidolia() {
        return true;
    }
    !is_stone && rank.is_face()
}

/// The same question, asked from inside scoring.
pub fn is_face(card: &CardRef, game: &GameState) -> bool {
    is_face_for(card, game, false)
}

/// `is_suit(suit, nil, true)` -- the flush_calc branch (card.lua:4065).
///
/// The game asks its suit question two ways and they differ on a debuffed card.
/// The ordinary test refuses one outright; the flush_calc one reads the printed
/// suit anyway, and only a *wild* card loses its everything-suit to a debuff.
/// That is what a flush is judged on, and what Blackboard is judged on: The Goad
/// debuffs the Queen of Spades held in hand and Blackboard still counts it black,
/// which took a flush from 2320 to 6960 in the game while the simulator left it
/// at 2320.
///
/// One implementation, shared with hand detection -- see `hands::flush_suit`.
pub fn counts_for_flush(card: &CardRef, suit: Suit, game: &GameState) -> bool {
    crate::hands::flush_suit(card, suit, game.has_smeared())
}

// --------------------------------------------------------------------------
// the registry
// --------------------------------------------------------------------------
//
// Python keeps a dict keyed by name, filled by `register(...)` calls at import
// time. Rust keeps the specs in a `const` array in `joker_specs.rs` (the
// translation of those same calls) and looks them up through a lazily built
// index, so `spec(name)` is O(1) like the dict.

/// Every registered joker, in the order the Python file registers them.
pub fn all_specs() -> &'static [JokerSpec] {
    crate::joker_specs::SPECS
}

fn index() -> &'static std::collections::HashMap<&'static str, &'static JokerSpec> {
    static INDEX: std::sync::OnceLock<std::collections::HashMap<&'static str, &'static JokerSpec>> =
        std::sync::OnceLock::new();
    INDEX.get_or_init(|| {
        let mut map = std::collections::HashMap::with_capacity(all_specs().len());
        for spec in all_specs() {
            if map.insert(spec.name, spec).is_some() {
                panic!("joker {:?} is registered twice", spec.name);
            }
        }
        map
    })
}

/// The spec of this name, or `None`. The Python `REGISTRY.get`.
pub fn spec(name: &str) -> Option<&'static JokerSpec> {
    index().get(name).copied()
}

/// The spec of this name; panics with the name if it is not registered.
///
/// Python's `REGISTRY[name]`, which raises `KeyError`. A silent miss would be far
/// worse here than a panic: a joker whose spec is missing scores nothing, and
/// "contributed nothing at all rather than contributing wrongly" is how several
/// of these bugs stayed invisible.
pub fn spec_or_panic(name: &str) -> &'static JokerSpec {
    spec(name).unwrap_or_else(|| panic!("no joker named {:?} is registered", name))
}

/// A new instance of a registered joker. Python's `make(name)`.
pub fn make(name: &str) -> JokerRef {
    make_ref(JokerInstance::new(spec_or_panic(name)))
}

/// Jokers of that rarity, in the game's pool order (Python `by_rarity`).
pub fn by_rarity(rarity: Rarity) -> Vec<&'static JokerSpec> {
    let mut out: Vec<&'static JokerSpec> = all_specs()
        .iter()
        .filter(|s| s.rarity == rarity)
        .collect();
    out.sort_by_key(|s| spec_order(s.name));
    out
}

/// The game's own pool position, from the generated `joker_data` table. The
/// shop picks an index into a rarity's pool, so this order is part of the
/// distribution rather than a display detail.
pub fn spec_order(name: &str) -> i32 {
    crate::joker_data::joker_row(name)
        .map(|row| row.order)
        .unwrap_or(i32::MAX)
}
