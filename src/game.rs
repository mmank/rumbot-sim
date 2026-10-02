//! Run state machine: blinds, playing, cash out, shop, packs.
//!
//! `GameState::legal_actions()` returns every action available in the current
//! phase, which doubles as the action mask for the RL environment.
//! `GameState::step()` applies one. Nothing in here knows about neural networks
//! or observations.
//!
//! This file currently holds the vocabulary the rest of the machine is built
//! from -- the phase enum, the action table and the skip tags. The state machine
//! itself is the remaining work; see `rust/PORTING.md`.

use std::collections::HashMap;

/// The phases a run moves through.
///
/// `ROUND_EVAL` is deliberately a phase of its own: between beating a blind and
/// entering the shop the game sits on the cash-out screen, with the payout shown
/// but not yet paid and the deck already restored. Collapsing that into the
/// winning hand made the simulator richer than the engine and its deck shorter,
/// at the same instant, for the whole of that gap.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum Phase {
    #[default]
    BlindSelect,
    Playing,
    RoundEval,
    Shop,
    Pack,
    GameOver,
    Won,
}

impl Phase {
    /// The phase's own name, as the engine spells it.
    pub fn as_str(self) -> &'static str {
        match self {
            Phase::BlindSelect => "blind_select",
            Phase::Playing => "playing",
            Phase::RoundEval => "round_eval",
            Phase::Shop => "shop",
            Phase::Pack => "pack",
            Phase::GameOver => "game_over",
            Phase::Won => "won",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum ActionType {
    CashOut,
    SelectBlind,
    SkipBlind,
    Play,
    Discard,
    UseConsumable,
    SellJoker,
    SwapJokerLeft,
    SellConsumable,
    Buy,
    BuyAndUse,
    RerollBoss,
    BuyVoucher,
    Reroll,
    BuyPack,
    PickPack,
    SkipPack,
    LeaveShop,
}

impl ActionType {
    pub fn as_str(self) -> &'static str {
        match self {
            ActionType::CashOut => "cash_out",
            ActionType::SelectBlind => "select_blind",
            ActionType::SkipBlind => "skip_blind",
            ActionType::Play => "play",
            ActionType::Discard => "discard",
            ActionType::UseConsumable => "use_consumable",
            ActionType::SellJoker => "sell_joker",
            ActionType::SwapJokerLeft => "swap_joker_left",
            ActionType::SellConsumable => "sell_consumable",
            ActionType::Buy => "buy",
            ActionType::BuyAndUse => "buy_and_use",
            ActionType::RerollBoss => "reroll_boss",
            ActionType::BuyVoucher => "buy_voucher",
            ActionType::Reroll => "reroll",
            ActionType::BuyPack => "buy_pack",
            ActionType::PickPack => "pick_pack",
            ActionType::SkipPack => "skip_pack",
            ActionType::LeaveShop => "leave_shop",
        }
    }
}

/// One move. `index` addresses a slot; `cards` addresses positions in the hand.
#[derive(Clone, PartialEq, Eq, Hash, Debug, Default)]
pub struct Action {
    pub r#type: ActionType,
    pub index: i32,
    pub cards: Vec<usize>,
}

impl Action {
    pub fn new(r#type: ActionType) -> Self {
        Action {
            r#type,
            index: -1,
            cards: Vec::new(),
        }
    }

    pub fn at(r#type: ActionType, index: i32) -> Self {
        Action {
            r#type,
            index,
            cards: Vec::new(),
        }
    }

    pub fn with_cards(r#type: ActionType, cards: Vec<usize>) -> Self {
        Action {
            r#type,
            index: -1,
            cards,
        }
    }
}

impl Default for ActionType {
    fn default() -> Self {
        ActionType::CashOut
    }
}

impl std::fmt::Display for Action {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut bits = vec![self.r#type.as_str().to_string()];
        if self.index >= 0 {
            bits.push(format!("#{}", self.index));
        }
        if !self.cards.is_empty() {
            bits.push(format!("{:?}", self.cards));
        }
        write!(f, "{}", bits.join(" "))
    }
}

/// Reward for skipping a blind; applied when the next shop opens.
///
/// The order of the variants is the game's pool order, and it is load-bearing:
/// the draw picks an index into this pool, so a reordered enum offers different
/// tags from the same seed while every rule still looks right.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Tag {
    Uncommon,
    Rare,
    Charm,
    Meteor,
    Buffoon,
    Boss,
    Double,
    Ethereal,
    Standard,
    Foil,
    Holographic,
    Polychrome,
    Negative,
    Coupon,
    Investment,
    Economy,
    Juggle,
    Handy,
    Garbage,
    Speed,
    TopUp,
    Orbital,
    Voucher,
    DSix,
}

impl Tag {
    /// The game's own name for the tag, which is also its display name.
    pub fn label(self) -> &'static str {
        match self {
            Tag::Uncommon => "Uncommon Tag",
            Tag::Rare => "Rare Tag",
            Tag::Charm => "Charm Tag",
            Tag::Meteor => "Meteor Tag",
            Tag::Buffoon => "Buffoon Tag",
            Tag::Boss => "Boss Tag",
            Tag::Double => "Double Tag",
            Tag::Ethereal => "Ethereal Tag",
            Tag::Standard => "Standard Tag",
            Tag::Foil => "Foil Tag",
            Tag::Holographic => "Holographic Tag",
            Tag::Polychrome => "Polychrome Tag",
            Tag::Negative => "Negative Tag",
            Tag::Coupon => "Coupon Tag",
            Tag::Investment => "Investment Tag",
            Tag::Economy => "Economy Tag",
            Tag::Juggle => "Juggle Tag",
            Tag::Handy => "Handy Tag",
            Tag::Garbage => "Garbage Tag",
            Tag::Speed => "Speed Tag",
            Tag::TopUp => "Top-up Tag",
            Tag::Orbital => "Orbital Tag",
            Tag::Voucher => "Voucher Tag",
            Tag::DSix => "D6 Tag",
        }
    }
}

/// Every tag, in the game's pool order. `TAG_POOL = list(Tag)`.
pub const TAG_POOL: [Tag; 24] = [
    Tag::Uncommon,
    Tag::Rare,
    Tag::Charm,
    Tag::Meteor,
    Tag::Buffoon,
    Tag::Boss,
    Tag::Double,
    Tag::Ethereal,
    Tag::Standard,
    Tag::Foil,
    Tag::Holographic,
    Tag::Polychrome,
    Tag::Negative,
    Tag::Coupon,
    Tag::Investment,
    Tag::Economy,
    Tag::Juggle,
    Tag::Handy,
    Tag::Garbage,
    Tag::Speed,
    Tag::TopUp,
    Tag::Orbital,
    Tag::Voucher,
    Tag::DSix,
];

/// The game's key for each tag, all twenty-four of them.
///
/// Seven used to be missing -- Handy, Garbage, Speed, Top-up, Orbital, Voucher
/// and D6 -- and a missing tag was not an inert one: the pool still drew it, the
/// skip still happened, and a lookup returning nothing made the reward evaporate.
/// A run could skip a blind for a Top-up Tag and get two fewer jokers than the
/// game would have given it.
pub fn tag_by_key(key: &str) -> Option<Tag> {
    Some(match key {
        "tag_uncommon" => Tag::Uncommon,
        "tag_rare" => Tag::Rare,
        "tag_charm" => Tag::Charm,
        "tag_meteor" => Tag::Meteor,
        "tag_buffoon" => Tag::Buffoon,
        "tag_boss" => Tag::Boss,
        "tag_double" => Tag::Double,
        "tag_ethereal" => Tag::Ethereal,
        "tag_standard" => Tag::Standard,
        "tag_foil" => Tag::Foil,
        "tag_holo" => Tag::Holographic,
        "tag_polychrome" => Tag::Polychrome,
        "tag_negative" => Tag::Negative,
        "tag_coupon" => Tag::Coupon,
        "tag_investment" => Tag::Investment,
        "tag_economy" => Tag::Economy,
        "tag_juggle" => Tag::Juggle,
        "tag_handy" => Tag::Handy,
        "tag_garbage" => Tag::Garbage,
        "tag_skip" => Tag::Speed,
        "tag_top_up" => Tag::TopUp,
        "tag_orbital" => Tag::Orbital,
        "tag_voucher" => Tag::Voucher,
        "tag_d_six" => Tag::DSix,
        _ => return None,
    })
}

/// The key for a tag, the inverse of `tag_by_key`.
pub fn tag_key(tag: Tag) -> &'static str {
    match tag {
        Tag::Uncommon => "tag_uncommon",
        Tag::Rare => "tag_rare",
        Tag::Charm => "tag_charm",
        Tag::Meteor => "tag_meteor",
        Tag::Buffoon => "tag_buffoon",
        Tag::Boss => "tag_boss",
        Tag::Double => "tag_double",
        Tag::Ethereal => "tag_ethereal",
        Tag::Standard => "tag_standard",
        Tag::Foil => "tag_foil",
        Tag::Holographic => "tag_holo",
        Tag::Polychrome => "tag_polychrome",
        Tag::Negative => "tag_negative",
        Tag::Coupon => "tag_coupon",
        Tag::Investment => "tag_investment",
        Tag::Economy => "tag_economy",
        Tag::Juggle => "tag_juggle",
        Tag::Handy => "tag_handy",
        Tag::Garbage => "tag_garbage",
        Tag::Speed => "tag_skip",
        Tag::TopUp => "tag_top_up",
        Tag::Orbital => "tag_orbital",
        Tag::Voucher => "tag_voucher",
        Tag::DSix => "tag_d_six",
    }
}

/// The five that pay out the instant the blind is skipped, rather than waiting
/// for a shop or a blind choice. `G.GAME.tags` is walked for `immediate` inside
/// `skip_blind` itself, after the skip has been counted and the new tag added --
/// so a Speed Tag counts the very skip that produced it.
pub const IMMEDIATE_TAGS: [Tag; 5] = [
    Tag::Handy,
    Tag::Garbage,
    Tag::Speed,
    Tag::TopUp,
    Tag::Orbital,
];

/// The table `tag_by_key` is built from, for tests and for iteration.
pub fn tag_key_map() -> HashMap<&'static str, Tag> {
    TAG_POOL.iter().map(|t| (tag_key(*t), *t)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_tag_has_a_unique_key_that_round_trips() {
        let map = tag_key_map();
        assert_eq!(map.len(), 24);
        for tag in TAG_POOL {
            let key = tag_key(tag);
            assert_eq!(tag_by_key(key), Some(tag), "{}", key);
        }
        assert_eq!(tag_by_key("tag_nonsense"), None);
    }

    #[test]
    fn the_pool_order_is_the_games_own_enum_order() {
        // Python: [t.name for t in TAG_POOL] -- the draw indexes into this list,
        // so the order is part of the distribution and not a display detail.
        let keys: Vec<&str> = TAG_POOL.iter().map(|t| tag_key(*t)).collect();
        assert_eq!(
            keys,
            vec![
                "tag_uncommon",
                "tag_rare",
                "tag_charm",
                "tag_meteor",
                "tag_buffoon",
                "tag_boss",
                "tag_double",
                "tag_ethereal",
                "tag_standard",
                "tag_foil",
                "tag_holo",
                "tag_polychrome",
                "tag_negative",
                "tag_coupon",
                "tag_investment",
                "tag_economy",
                "tag_juggle",
                "tag_handy",
                "tag_garbage",
                "tag_skip",
                "tag_top_up",
                "tag_orbital",
                "tag_voucher",
                "tag_d_six",
            ]
        );
    }

    #[test]
    fn the_immediate_five_are_all_real_tags() {
        for tag in IMMEDIATE_TAGS {
            assert!(TAG_POOL.contains(&tag), "{:?}", tag);
        }
    }

    #[test]
    fn an_action_renders_the_way_the_python_repr_does() {
        assert_eq!(Action::new(ActionType::CashOut).to_string(), "cash_out");
        assert_eq!(Action::at(ActionType::Buy, 2).to_string(), "buy #2");
        assert_eq!(
            Action::with_cards(ActionType::Play, vec![0, 1, 2]).to_string(),
            "play [0, 1, 2]"
        );
    }
}

// --------------------------------------------------------------------------
// run state
// --------------------------------------------------------------------------

use std::collections::HashSet;

use crate::blinds::{Blind, BossEffect};
use crate::cards::{CardRef, Edition, Rank, Suit};
use crate::consumables::ConsumableRef;
use crate::deck_data::deck_config;
use crate::hands::{HandLevels, HandType};
use crate::jokers::JokerRef;
use crate::rng::RunRng;
use crate::shop::{PackSpec, Shop, Voucher};

pub const MAX_PLAYED: usize = 5;
pub const BASE_JOKER_SLOTS: i32 = 5;
pub const BASE_CONSUMABLE_SLOTS: i32 = 2;
pub const BASE_HAND_SIZE: i32 = 8;
pub const BASE_HANDS: i32 = 4;
pub const BASE_DISCARDS: i32 = 3;
pub const BASE_INTEREST_CAP: i32 = 5;
/// G.GAME.perishable_rounds and G.GAME.rental_rate.
pub const PERISHABLE_ROUNDS: i32 = 5;
/// What the Director's Cut / Retcon button charges to re-roll the boss.
pub const BOSS_REROLL_COST: i32 = 10;
pub const RENTAL_RATE: i32 = 3;
pub const WIN_ANTE: i32 = 8;

/// The consumables whose use is gated on something other than how many cards are
/// selected. Grouped by what they need, from Card:can_use_consumeable.
pub const FREE_JOKER_NEEDED: [&str; 3] = ["Judgement", "The Soul", "Wraith"];
pub const FREE_CONSUMABLE_NEEDED: [&str; 3] = ["The Emperor", "The High Priestess", "The Fool"];
pub const PLAIN_JOKER_NEEDED: [&str; 3] = ["The Wheel of Fortune", "Ectoplasm", "Hex"];
/// These destroy a card picked at random, and want one to spare.
pub const SPARE_CARD_NEEDED: [&str; 6] = [
    "Familiar",
    "Grim",
    "Incantation",
    "Immolate",
    "Sigil",
    "Ouija",
];

/// A joker's deferred blind-select action.
///
/// Python queues *closures* here (`blind_select_events: list | None`), each
/// capturing the run so it can act after the pass has finished. Rust closures
/// cannot capture the run they are stored inside, so the queue holds data and the
/// pass performs it -- same order, same effect:
///
/// ```text
///     events, self.blind_select_events = self.blind_select_events, None
///     for event in events:
///         event()
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BlindSelectEvent {
    /// Riff-raff makes its jokers in an event, so nothing in the pass sees them
    /// (card.lua:2532-2543).
    CreateJokers { source_uid: u64, count: i32 },
    /// Madness and Ceremonial Dagger dissolve their victim in an event
    /// (card.lua:2170-2175); the dissolve's `remove()` is queued behind
    /// everything the pass queued.
    DestroyJoker { victim_uid: u64, reason: String },
}

/// One entry on a booster's shelf.
///
/// Python's `pack_options` is a heterogeneous list -- a `JokerInstance`, a
/// `Card` or a `ConsumableSpec` -- and `_pick_pack` tells them apart with
/// `isinstance`. Rust says the same thing with an enum, which also removes the
/// chance of a Joker being handled as a Card.
#[derive(Clone, Debug)]
pub enum PackChoice {
    Joker(JokerRef),
    Card(CardRef),
    /// A consumable taken from a pack is used there and then: the game's pack
    /// screen calls use_card, not buy, so it never reaches a slot.
    Consumable(&'static crate::consumables::ConsumableSpec),
}

/// A run.
///
/// The Python original's fields, one for one. Sea of comments carried across,
/// because almost every one of them records a bug that a differential replay
/// found -- they are the specification, not decoration.
#[derive(Debug)]
pub struct GameState {
    pub seed: String,
    /// The back the run is played with. Not decoration: Red Deck grants an extra
    /// discard every round, Blue an extra hand, Black a joker slot at the cost of
    /// a hand. A run that ignores the deck is a different run.
    pub deck: String,
    pub rng: RunRng,

    pub ante: i32,
    /// 0 small, 1 big, 2 boss
    pub blind_index: i32,
    pub round_number: i32,
    pub money: i32,
    /// The money as the hand being played found it. The game's dollars move
    /// through `ease_dollars` events that run after evaluate_play, so a joker
    /// reading G.GAME.dollars mid-hand -- Vagabond -- sees this and not what the
    /// hand pays (Matador's $8, a Gold Seal's $3).
    pub money_at_play: Option<i32>,

    pub full_deck: Vec<CardRef>,
    /// G.GAME.starting_deck_size: the full deck as the run dealt it, 40 on an
    /// Abandoned Deck. Erosion counts the cards missing below this, not below
    /// 52 -- DM46XNV1 / Abandoned / stake 8 bought Erosion in ante 5 and the
    /// shadow scored +48 Mult the game never paid, cleared a blind the game
    /// had not, and waited on a cash-out that never came.
    pub starting_deck_size: usize,
    pub draw_pile: Vec<CardRef>,
    pub hand: Vec<CardRef>,
    pub discard_pile: Vec<CardRef>,

    pub jokers: Vec<JokerRef>,
    pub consumables: Vec<ConsumableRef>,
    pub vouchers: Vec<Voucher>,
    pub tags: Vec<Tag>,
    /// Skip rewards [small, big].
    pub ante_tags: Vec<String>,
    /// Whether the consumable being applied right now came straight out of a
    /// booster rather than a slot. See `consumables::wheel_of_fortune`.
    pub using_from_pack: bool,
    /// The key of the consumable being applied right now. G.FUNCS.use_card takes
    /// the card out of its area but does not remove it until it dissolves, after
    /// its effect, so its used_jokers entry stands for the whole effect: The
    /// Emperor cannot draw another Emperor. See `seen_centers`.
    pub using_key: String,
    /// G.STATES.PLAY_TAROT: true while a consumable's effect runs. use_card
    /// switches G.STATE for the whole effect and puts it back only in an event
    /// queued behind it, so a hand size a Hex or a Judgement raises is not dealt
    /// into. See `hand_size_changed`.
    pub playing_tarot: bool,
    /// The setting_blind pass while it runs: the events its jokers queue for
    /// after it, and the jokers Madness and Ceremonial Dagger have marked
    /// getting_sliced. None outside the pass, when there is nothing to wait for.
    pub blind_select_events: Option<Vec<BlindSelectEvent>>,
    pub getting_sliced: Vec<(u64, String)>,
    pub joker_buffer: i32,
    /// The jokers that get a money row on the cash-out screen, settled when the
    /// round is evaluated -- before the beaten blind lets go of the joker Crimson
    /// Heart held. None outside a cash-out. See `beat_blind`.
    ///
    /// The jokers themselves, not their uids: the rows are paid when the button
    /// is pressed, and a joker sold on the cash-out screen before that still
    /// pays. Resolving uids against `self.jokers` at cash-out lost exactly that
    /// -- a Delayed Gratification sold in ROUND_EVAL went unpaid.
    pub dollar_rows: Option<Vec<JokerRef>>,
    /// The same two as the game's keys, kept because a tag this simulator has no
    /// effect for is still the tag the run was offered.
    pub ante_tag_keys: Vec<String>,

    pub hand_levels: HandLevels,

    pub base_hand_size: i32,
    pub base_joker_slots: i32,
    /// The Nebula Deck takes a consumable slot away, the Painted Deck a joker
    /// slot; both are set once from the deck rather than derived.
    pub extra_consumable_slots: i32,

    pub blind: Option<Blind>,
    /// Held between beating a blind and cashing out: the game shows the payout on
    /// that screen before any of it is paid.
    pub beaten_blind: Option<Blind>,
    pub pending_payout: i64,
    pub beaten_was_boss: bool,
    /// Which sort the player last asked for. The game keeps this on the hand's
    /// CardArea and reapplies it to every draw.
    pub hand_sort: String,
    /// The poker hand most recently played, which is what the game shows and what
    /// a recording carries. Cheap to keep and the fastest way to see that two
    /// engines played different cards from the same choice.
    pub last_hand: String,
    /// total for the run, as G.GAME.hands_played
    pub hands_played: i32,
    /// Targets the game rerolls each round, which the jokers that name a card or
    /// a suit read. Kept on the run because that is where the game keeps them --
    /// two Idols name the same card.
    pub idol_rank: Option<Rank>,
    pub idol_suit: Option<Suit>,
    pub ancient_suit: Option<Suit>,
    pub mail_rank: Option<Rank>,
    pub castle_suit: Option<Suit>,

    /// this round, for Delayed Gratification
    pub discards_used: i32,
    /// How often each boss has been drawn. The game narrows the eligible set to
    /// the least-used before rolling, so this is part of the selection rather
    /// than bookkeeping.
    pub bosses_used: HashMap<String, i32>,
    pub tarots_used: i32,
    pub planets_used: i32,
    pub unique_planets: HashSet<String>,
    /// G.GAME.pool_flags: the run's history as the pools read it. One flag in the
    /// whole game -- 'gros_michel_extinct', set when Gros Michel dies -- and it
    /// swaps Gros Michel out of every later pool and Cavendish in. Every call
    /// that rolls a joker has to pass it.
    pub pool_flags: HashSet<String>,
    pub rerolls: i32,
    pub blinds_skipped: i32,
    /// G.GAME.unused_discards, which the Garbage Tag pays a dollar each for. A
    /// run total, banked at the end of every round, not this round's leftovers.
    pub unused_discards: i32,
    /// G.GAME.orbital_choices[ante][blind]. Rolled once per ante and blind and
    /// remembered, so a Double Tag's copy levels the same hand as the original.
    pub orbital_choices: HashMap<(i32, i32), HandType>,
    /// G.GAME.round_resets.temp_handsize -- the Juggle Tag's three cards. It lasts
    /// one round and is handed back when that round ends, and it stacks:
    /// round_start_bonus is applied to every tag held, without breaking.
    pub temp_hand_size: i32,
    /// G.GAME.current_round.most_played_poker_hand, which is what The Ox reads. A
    /// snapshot taken when a boss round ends, not a live count -- it starts at
    /// High Card and is only ever rewritten there.
    pub most_played_hand: HandType,
    /// G.GAME.round_scores.hand.amt -- the biggest single hand scored this run.
    pub best_hand: i64,
    /// G.GAME.current_round.reroll_cost, which survives the shop closing: the
    /// game resets it at the start of a round, not when the shop is left, so a
    /// shop rerolled twice still reads its climbed price afterwards.
    pub free_rerolls_carried: i32,
    pub reroll_price_carried: i32,
    /// round_resets.temp_reroll_cost, the D6 Tag's $0 start. Not the shop's alone:
    /// it stands until the end of the round after it, so the round's reroll price
    /// reads 0 too.
    pub temp_reroll_cost: bool,
    /// round_resets.blind_states, for the ante in progress: which of this ante's
    /// three blinds were skipped rather than beaten.
    pub skipped_this_ante: HashSet<i32>,
    pub cards_sold: i32,
    pub glass_destroyed: i32,
    /// Cards that have entered the deck, counted for the same reason Hologram
    /// counts them: it is *additions*, not deck size.
    pub cards_created: i32,
    pub lucky_triggers: i32,
    pub chips_scored: i64,
    pub hands_left: i32,
    pub discards_left: i32,
    pub hands_played_this_round: HashSet<HandType>,
    /// Blind.only_hand: The Mouth's hand for the round, which is the type of the
    /// first hand it let through and nothing else.
    pub mouth_only_hand: Option<HandType>,

    pub phase: Phase,
    pub shop: Option<Shop>,
    pub pack: Option<PackSpec>,
    /// The game gives every run a Buffoon pack in its first shop, short-circuiting
    /// before any roll -- see get_pack. This remembers whether that has happened,
    /// which is G.GAME.first_shop_buffoon.
    pub first_shop_buffoon: bool,
    /// The voucher this round's shop will offer, drawn when the round starts.
    pub round_voucher: String,
    /// Whether the hand on screen was dealt by a pack rather than by a round,
    /// which decides whether closing the pack takes it away again.
    pub pack_dealt_hand: bool,
    /// G.GAME.shop_free -- the Coupon Tag, which lasts for the one shop.
    pub shop_free: bool,
    /// G.GAME.last_tarot_planet -- the key The Fool copies.
    pub last_tarot_planet: String,
    /// G.GAME.ecto_minus. Ectoplasm's hand-size cost is not a flat one: it starts
    /// at one and rises by one with every Ectoplasm used in the run.
    pub ecto_minus: i32,
    /// round_resets.blind_choices.Boss: this ante's boss, drawn at its start.
    pub ante_boss: String,
    /// The card Cerulean Bell nominates: every hand must include it.
    pub forced_card: Option<CardRef>,
    /// round_resets.boss_rerolled: Director's Cut allows one reroll an ante and
    /// this is what remembers that it has been spent. Reset when a boss falls.
    pub boss_rerolled: bool,
    /// The stake, one to eight. It is not a difficulty label: it changes the chips
    /// every ante asks for, the discards a round starts with, whether the Small
    /// Blind pays, and what stickers the shop puts on its jokers.
    ///
    /// `new` also takes a *negative* stake: -n is stake n in every respect but
    /// the stickers, which are rolled as on Gold -- eternal, perishable and
    /// rental all on. Stored here as n with `all_stickers` set, so nothing
    /// below this line knows the difference. It is for tuning the sticker
    /// rules on a stake that does not kill the run first; a real run cannot
    /// be at stake -3, and the rolls move the RNG, so it is not a stake-3 run
    /// with stickers added but its own run. There is no -8: Gold already
    /// rolls every sticker, so it would be stake 8 under another name.
    pub stake: i32,
    /// Put every sticker on every stake, whatever the stake would allow.
    /// Set by a negative stake (above). It moves the RNG, so a measurement
    /// against the real stake leaves it off.
    pub all_stickers: bool,
    /// Endless: keep playing past the ante-eight boss instead of ending there.
    pub endless: bool,
    pub pack_options: Vec<PackChoice>,
    pub pack_picks_left: i32,

    /// `preview_money`'s scratch value: what a previewed play earns in
    /// expectation. Set by `_preview`, read the instant after.
    pub preview_expected: f64,

    /// Whether `_preview` sets `blind.triggered` for the play it scores, as
    /// `_play_hand_score` does, rather than keeping the flag the last real
    /// play left. Off (the old behaviour, which the differential fixtures
    /// were recorded under), the flag is read stale: after a hand that set
    /// the boss off every preview pays Matador's $8, The Arm's and The Ox's
    /// half is never set, and a refused hand previews $0. On, the flag is
    /// cleared first, The Arm (a hand above level 1) and The Ox (its
    /// most-played hand) set it, and a hand the boss refuses runs
    /// `on_debuffed_hand` so Matador's $8 shows in the dollars. The Ox's
    /// money going to $0 is still not previewed. Matador is the only reader
    /// of the flag, so nothing moves without it. Set by the policy under
    /// `fix_preview_trigger`; the simulator never sets it.
    pub preview_trigger: bool,

    pub logs: Vec<String>,
    pub verbose: bool,
}

impl GameState {
    /// A run.
    ///
    /// Seed-faithful: the deck, the money, the deck's own grants, the draw pile
    /// and every draw `__post_init__` makes -- the boss, the round's voucher, the
    /// two skip tags, the named cards and suits, and the opening blind -- all land
    /// in the same order the Python original performs them.
    pub fn new<S: ToString>(seed: S, deck: &str, stake: i32) -> Self {
        // -8 would be Gold with Gold's own stickers: the same game as 8, row
        // for row, under a second name that read as a separate measurement.
        assert!(
            stake >= -7,
            "stake {stake}: a negative stake runs -1 to -7 (-8 is stake 8)"
        );
        let mut game = GameState {
            seed: seed.to_string(),
            deck: deck.to_string(),
            rng: RunRng::new(seed.to_string()),
            ante: 1,
            blind_index: 0,
            round_number: 0,
            money: 4,
            money_at_play: None,
            full_deck: Vec::new(),
            starting_deck_size: 52,
            draw_pile: Vec::new(),
            hand: Vec::new(),
            discard_pile: Vec::new(),
            jokers: Vec::new(),
            consumables: Vec::new(),
            vouchers: Vec::new(),
            tags: Vec::new(),
            ante_tags: Vec::new(),
            using_from_pack: false,
            using_key: String::new(),
            playing_tarot: false,
            blind_select_events: None,
            getting_sliced: Vec::new(),
            joker_buffer: 0,
            dollar_rows: None,
            ante_tag_keys: Vec::new(),
            hand_levels: HandLevels::new(),
            base_hand_size: BASE_HAND_SIZE,
            base_joker_slots: BASE_JOKER_SLOTS,
            extra_consumable_slots: 0,
            blind: None,
            beaten_blind: None,
            pending_payout: 0,
            beaten_was_boss: false,
            hand_sort: "rank".to_string(),
            last_hand: String::new(),
            hands_played: 0,
            idol_rank: None,
            idol_suit: None,
            ancient_suit: None,
            mail_rank: None,
            castle_suit: None,
            discards_used: 0,
            bosses_used: HashMap::new(),
            tarots_used: 0,
            planets_used: 0,
            unique_planets: HashSet::new(),
            pool_flags: HashSet::new(),
            rerolls: 0,
            blinds_skipped: 0,
            unused_discards: 0,
            orbital_choices: HashMap::new(),
            temp_hand_size: 0,
            most_played_hand: HandType::HighCard,
            best_hand: 0,
            free_rerolls_carried: 0,
            reroll_price_carried: 5,
            temp_reroll_cost: false,
            skipped_this_ante: HashSet::new(),
            cards_sold: 0,
            glass_destroyed: 0,
            cards_created: 0,
            lucky_triggers: 0,
            chips_scored: 0,
            hands_left: 0,
            discards_left: 0,
            hands_played_this_round: HashSet::new(),
            mouth_only_hand: None,
            phase: Phase::BlindSelect,
            shop: None,
            pack: None,
            first_shop_buffoon: false,
            round_voucher: String::new(),
            pack_dealt_hand: false,
            shop_free: false,
            last_tarot_planet: String::new(),
            ecto_minus: 1,
            ante_boss: String::new(),
            forced_card: None,
            boss_rerolled: false,
            stake: stake.abs(),
            all_stickers: stake < 0,
            endless: false,
            pack_options: Vec::new(),
            pack_picks_left: 0,
            preview_expected: 0.0,
            preview_trigger: false,
            logs: Vec::new(),
            verbose: false,
        };
        let config = game.deck_config();
        game.money += config.dollars;
        game.full_deck = crate::cards::standard_deck(
            config.remove_faces,
            if config.randomize_rank_suit {
                Some(&mut game.rng)
            } else {
                None
            },
        );
        game.starting_deck_size = game.full_deck.len();
        game.apply_deck_config();
        // G:start_run's own deck:shuffle(), under the bare pool name. Almost
        // invisible, because pseudoshuffle sorts by id before it shuffles, so the
        // round-start shuffle washes this order out and every dealt hand matches
        // without it.
        //
        // It shows when something deals *before* the first round: a Charm Tag
        // taken off the opening blind select opens an Arcana pack, and that pack
        // deals a hand from the deck as it stands.
        game.draw_pile = game.full_deck.clone();
        game.draw_pile.sort_by_key(|c| crate::cards::uid_of(c));
        game.rng.shuffle(&mut game.draw_pile, "shuffle");
        // The order the game starts a run in: the boss, then the voucher, then the
        // two skip tags. Every one of them draws, so the order is part of the seed.
        game._roll_boss();
        game._roll_voucher();
        game._roll_ante_tags();
        game._reset_round_cards();
        game.checker_the_deck();
        game._reroll_todo_hands();
        // The counters, not the cards: start_run puts round_resets on the HUD
        // before anything is dealt.
        let (hands, discards) = game.round_allowance(false);
        game.hands_left = hands;
        game.discards_left = discards;
        game._next_blind();
        game
    }

    /// What the chosen deck starts the run holding.
    ///
    /// The deck's numbers were being read where they were needed -- hand size,
    /// ante scaling, the spectral rate -- but the things it *gives* a run were not
    /// applied at all. The Ghost Deck starts with a Hex, the Magic Deck with two
    /// Fools and Crystal Ball already redeemed, the Nebula Deck with Telescope.
    /// Missing the cards is worse than missing the effect: every consumable slot
    /// after the first is numbered one place out, so "use the first consumable"
    /// uses the wrong card for the rest of the run.
    pub fn apply_deck_config(&mut self) {
        let config = self.deck_config();
        for key in config.consumables {
            if let Some(row) = crate::consumable_data::consumable_row_by_key(key) {
                if crate::consumables::spec(row.name).is_some() {
                    let spec = crate::consumables::spec_or_panic(row.name);
                    self.consumables.push(crate::consumables::make_ref(
                        crate::consumables::ConsumableInstance::new(spec, Edition::None),
                    ));
                }
            }
        }
        let mut starting: Vec<&str> = config.vouchers.to_vec();
        if !config.voucher.is_empty() {
            starting.push(config.voucher);
        }
        for key in starting {
            if let Some(voucher) = crate::shop::voucher_by_key(key) {
                self.vouchers.push(voucher);
            }
        }
        self.base_joker_slots += config.joker_slot;
        self.extra_consumable_slots += config.consumable_slot;
    }

    /// The Checkered Deck's suits: Clubs to Spades, Diamonds to Hearts.
    ///
    /// By walking the deck it has just built rather than by building a different
    /// one -- so a card keeps its place, and its id, and changes suit where it
    /// stands.
    pub fn checker_the_deck(&mut self) {
        if self.deck != "Checkered Deck" {
            return;
        }
        for card in &self.full_deck {
            let suit = crate::cards::suit_of(card);
            if suit == Suit::Clubs {
                crate::cards::set_suit(card, Suit::Spades);
            } else if suit == Suit::Diamonds {
                crate::cards::set_suit(card, Suit::Hearts);
            }
        }
    }
}

/// The read-only questions every other module asks of a run: what the deck is,
/// what the boss is doing, how big the hand is, which jokers are still awake.
impl GameState {
    pub fn is_over(&self) -> bool {
        matches!(self.phase, Phase::GameOver | Phase::Won)
    }

    pub fn log(&mut self, message: impl Into<String>) {
        let message = message.into();
        if self.verbose {
            println!("{}", message);
        }
        self.logs.push(message);
    }

    /// The deck's own config table.
    pub fn deck_config(&self) -> &'static crate::deck_data::DeckConfig {
        deck_config(&self.deck)
    }

    /// The boss in force: None on the blind select screen, where `blind` is only
    /// on offer, and None when Chicot is held or the blind has been disabled.
    ///
    /// Not the one on deck (`Blind.on_deck`): the game's G.GAME.blind is the empty
    /// one Blind:defeat left behind, and the boss does nothing until set_blind.
    pub fn boss(&self) -> Option<&'static BossEffect> {
        let blind = self.blind.as_ref()?;
        if blind.kind != BlindKind::Boss || blind.on_deck || blind.disabled {
            return None;
        }
        // Chicot disables the boss from setting_blind and add_to_deck, neither of
        // which a debuffed one runs.
        if self
            .active_jokers()
            .iter()
            .any(|j| j.borrow().name() == "Chicot")
        {
            return None;
        }
        blind.boss
    }

    /// The blind's target, which is nothing outside a round.
    ///
    /// `blind` holds the next blind through the select screen and the shop so
    /// that it can be offered and skipped, but it is not in force until the round
    /// starts -- the game reports no target until then, and reading the pending
    /// one as active made the simulator look like it was mid-round while sitting
    /// in a shop.
    pub fn blind_target(&self) -> i64 {
        let blind = match &self.blind {
            Some(blind) => blind,
            None => return 0,
        };
        // A run that ends does so *during* a blind, and the game still reports
        // that blind's target on the game-over screen.
        if !matches!(self.phase, Phase::Playing | Phase::GameOver) {
            return 0;
        }
        blind.target
    }

    /// The blind in force, which is nothing outside a round.
    pub fn blind_name(&self) -> String {
        let blind = match &self.blind {
            Some(blind) => blind,
            None => return String::new(),
        };
        if !matches!(self.phase, Phase::Playing | Phase::GameOver) {
            return String::new();
        }
        blind.name()
    }

    /// The jokers that still do anything.
    ///
    /// A perishable joker is switched off once its five rounds are up -- the game
    /// debuffs it, which leaves it sitting in the row taking a slot and
    /// contributing nothing. Reading `jokers` for effects therefore keeps a dead
    /// joker working: a debuffed Stuntman went on taking two off the hand size, so
    /// the run dealt seven cards where the game dealt nine.
    pub fn active_jokers(&self) -> Vec<JokerRef> {
        self.jokers
            .iter()
            .filter(|j| !j.borrow().debuffed)
            .cloned()
            .collect()
    }

    /// The row as calculate_joker walks it: a debuffed joker is skipped.
    ///
    /// Card:calculate_joker opens `if self.debuff then return nil end`
    /// (card.lua:2291-2292), so a debuffed joker answers no context at all -- not
    /// a discard, not the end of the round, not a reroll, a sale, a pack opened
    /// or skipped, a card added or destroyed, a consumable used.
    pub fn calculating_jokers(&self) -> Vec<JokerRef> {
        self.active_jokers()
    }

    /// Every card counts as a face card.
    pub fn has_pareidolia(&self) -> bool {
        self.active_jokers()
            .iter()
            .any(|j| j.borrow().name() == "Pareidolia")
    }

    /// Hearts count as Diamonds and Spades as Clubs, both ways.
    pub fn has_smeared(&self) -> bool {
        self.active_jokers()
            .iter()
            .any(|j| j.borrow().name() == "Smeared Joker")
    }

    /// Oops! All 6s doubles every listed probability, and stacks.
    ///
    /// A pessimistic preview does *not* zero this: `PessimisticRng` overrides
    /// `pseudorandom` instead, so the scaled ratio faces a draw of `1 - 1e-9`
    /// exactly as Python does (and a doubled 1-in-2 still comes up). See
    /// `RunRng::pessimistic`.
    pub fn probability_scale(&self) -> f64 {
        let count = self
            .active_jokers()
            .iter()
            .filter(|j| j.borrow().name() == "Oops! All 6s")
            .count();
        2f64.powi(count as i32)
    }

    pub fn splash(&self) -> bool {
        self.active_jokers()
            .iter()
            .any(|j| j.borrow().name() == "Splash")
    }

    pub fn four_fingers(&self) -> bool {
        self.active_jokers()
            .iter()
            .any(|j| j.borrow().name() == "Four Fingers")
    }

    pub fn shortcut_joker(&self) -> bool {
        self.active_jokers()
            .iter()
            .any(|j| j.borrow().name() == "Shortcut")
    }

    /// G.GAME.modifiers.scaling: 1, 2 from Green stake, 3 from Purple.
    pub fn blind_scaling(&self) -> i32 {
        if self.stake >= 6 {
            3
        } else if self.stake >= 3 {
            2
        } else {
            1
        }
    }

    /// G.GAME.edition_rate, which Hone and Glow Up raise.
    pub fn edition_rate(&self) -> f64 {
        self.vouchers
            .iter()
            .filter(|v| v.edition_rate != 0.0)
            .map(|v| v.edition_rate)
            .fold(f64::MIN, f64::max)
            .max(1.0)
    }

    /// How many jokers the row holds.
    ///
    /// A negative joker does not take a slot -- add_to_deck raises the limit by
    /// one for it and remove_from_deck lowers it again -- so a row of five with a
    /// negative among them has room for a sixth. The simulator had no idea, so a
    /// Judgement that should have made a joker made nothing, and the run went on a
    /// joker short.
    pub fn joker_slots(&self) -> i32 {
        self.base_joker_slots
            + self
                .jokers
                .iter()
                .filter(|j| j.borrow().edition == Edition::Negative)
                .count() as i32
            + self.vouchers.iter().map(|v| v.joker_slots).sum::<i32>()
    }

    /// Whether this particular joker can be taken, full row or not.
    ///
    /// `joker_slots` counts the Negatives already held, because add_to_deck raises
    /// the limit as one arrives. The one being offered has not arrived, so it has
    /// to be asked about separately -- a Negative needs no slot, and a full row
    /// does not stop it.
    pub fn room_for_joker(&self, joker: &JokerRef) -> bool {
        (self.jokers.len() as i32) < self.joker_slots()
            || joker.borrow().edition == Edition::Negative
    }

    /// How many consumables the row holds.
    ///
    /// A Negative one does not take a slot. Perkeo's whole point is that its copy
    /// is free, and without this it was taking a slot like any other card.
    pub fn consumable_slots(&self) -> i32 {
        BASE_CONSUMABLE_SLOTS
            + self.extra_consumable_slots
            + self
                .vouchers
                .iter()
                .map(|v| v.consumable_slots)
                .sum::<i32>()
            + self
                .consumables
                .iter()
                .filter(|c| c.borrow().edition == Edition::Negative)
                .count() as i32
    }

    pub fn hand_size(&self) -> i32 {
        let mut size = self.base_hand_size;
        size += self.vouchers.iter().map(|v| v.hand_size).sum::<i32>();
        // Turtle Bean's contribution is its counter, which shrinks by one every
        // round, rather than a number fixed on the spec -- summing the spec gave
        // nothing at all, so a run holding one dealt five cards fewer than the
        // game did.
        for joker in self.active_jokers() {
            let j = joker.borrow();
            size += if j.spec.hand_size_from_counter {
                j.counter as i32
            } else {
                j.spec.hand_size
            };
        }
        size += self.deck_config().hand_size;
        size += self.temp_hand_size;
        if let Some(boss) = self.boss() {
            size += boss.hand_size_delta;
        }
        // CardArea:update floors this at *zero*, not one -- math.max(0,
        // real_card_limit). Flooring it at one made the hand-size loss
        // unreachable, which matters because reaching zero is a real way to end a
        // run: Troubadour and Stuntman are -2 each, Merry Andy -1, a decayed
        // Turtle Bean another, and The Manacle one more on top.
        size.max(0)
    }

    pub fn interest_cap(&self) -> i32 {
        self.vouchers
            .iter()
            .map(|v| v.interest_cap)
            .filter(|c| *c != 0)
            .max()
            .unwrap_or(BASE_INTEREST_CAP)
    }

    pub fn price_multiplier(&self) -> f64 {
        self.vouchers
            .iter()
            .fold(1.0, |mult, v| mult * v.price_multiplier())
    }

    /// What a voucher or a booster on the shelf costs.
    ///
    /// The same `Card:set_cost` every other price goes through: a voucher and a
    /// pack are Cards like any other.
    pub fn price(&self, base: i32) -> i32 {
        self.card_cost(base, Edition::None)
    }

    /// Hands and discards for a round, from the deck, vouchers and boss.
    ///
    /// The boss is optional because the counters exist before it applies. On the
    /// blind select screen the game shows the plain allowance -- the boss's effect
    /// lands in set_blind, when the blind is actually taken -- so a Water Blind on
    /// offer still reads three discards there and zero the moment it is selected.
    pub fn round_allowance(&self, with_boss: bool) -> (i32, i32) {
        let config = self.deck_config();
        let mut hands = BASE_HANDS + self.vouchers.iter().map(|v| v.extra_hands).sum::<i32>();
        hands += self
            .active_jokers()
            .iter()
            .map(|j| j.borrow().spec.extra_hands)
            .sum::<i32>();
        hands += config.hands;
        // Blue stake and up start a round with one discard fewer.
        let mut discards = BASE_DISCARDS - if self.stake >= 5 { 1 } else { 0 };
        discards += self.vouchers.iter().map(|v| v.extra_discards).sum::<i32>();
        discards += self
            .active_jokers()
            .iter()
            .map(|j| j.borrow().spec.extra_discards)
            .sum::<i32>();
        discards += config.discards;
        if with_boss {
            if let Some(boss) = self.boss() {
                hands = if boss.hands_delta > -50 {
                    (hands + boss.hands_delta).max(1)
                } else {
                    1
                };
                discards = if boss.discards_delta > -50 {
                    (discards + boss.discards_delta).max(0)
                } else {
                    0
                };
            }
        }
        (hands, discards)
    }

    /// The hands the run knows about.
    ///
    /// Five of a Kind, Flush House and Flush Five start `visible = false` and are
    /// switched on in evaluate_play the first time one is made. Anything choosing
    /// a poker hand at random draws from the visible ones only, so the difference
    /// is not cosmetic: it is a nine-entry pool rather than a twelve-entry one,
    /// which changes both the hand picked and where the stream lands afterwards.
    ///
    /// In the order the game's `pairs(G.GAME.hands)` walks them
    /// (`hands::GAME_PAIRS_ORDER`), because every caller is a draw that
    /// indexes into a list built by that walk.
    pub fn visible_hands(&self) -> Vec<HandType> {
        crate::hands::GAME_PAIRS_ORDER
            .into_iter()
            .filter(|h| !crate::hands::is_secret(*h) || self.hand_levels.played(*h) > 0)
            .collect()
    }

    /// Classify the cards the player has selected.
    pub fn evaluate_selection(&self, cards: &[CardRef]) -> crate::hands::HandResult {
        crate::hands::evaluate(
            cards,
            crate::hands::EvalFlags {
                four_fingers: self.four_fingers(),
                shortcut: self.shortcut_joker(),
                splash: self.splash(),
                smeared: self.has_smeared(),
            },
        )
    }
}

// ==========================================================================
// The run state machine.
//
// Everything below is the remaining `game.py`: run setup, the blind flow, the
// playing pipeline, the shop, packs, the action interface and the previews.
// The comments are the Python's, carried across because nearly every one of
// them records a divergence that differential replay found against the real
// game, and each is a test case.
// ==========================================================================

use std::rc::Rc;

use crate::blinds::{boss_by_name, make_blind, BlindKind};
use crate::boss_data::{boss_row, eligible_bosses};
use crate::cards::{enhancement_of, seal_of, uid_of, Enhancement, Seal};
use crate::consumables::{ConsumableKind, ConsumableSpec};
use crate::effects::ScoreContext;
use crate::hands::{planet_for_hand, HandResult, HANDLIST};
use crate::scoring::{
    after_hand_pass, calculating_specs, effective_specs, held_triggers, score_hand, shattered_glass,
};
use crate::shop::{pack_from_key, pack_from_row, voucher_by_key, PackKind, ShopSlot};

/// The game's names for a polled edition.
fn edition_from_name(name: &str) -> Edition {
    match name {
        "foil" => Edition::Foil,
        "holo" => Edition::Holographic,
        "polychrome" => Edition::Polychrome,
        "negative" => Edition::Negative,
        _ => Edition::None,
    }
}

/// `Enhancement(key[2:])` -- a centre key like `m_bonus`, or a bare name.
fn enhancement_from_key(key: &str) -> Enhancement {
    let name = key.strip_prefix("m_").unwrap_or(key);
    match name {
        "bonus" => Enhancement::Bonus,
        "mult" => Enhancement::Mult,
        "wild" => Enhancement::Wild,
        "glass" => Enhancement::Glass,
        "steel" => Enhancement::Steel,
        "stone" => Enhancement::Stone,
        "gold" => Enhancement::Gold,
        "lucky" => Enhancement::Lucky,
        _ => Enhancement::None,
    }
}

fn seal_from_name(name: &str) -> Seal {
    match name {
        "red" => Seal::Red,
        "blue" => Seal::Blue,
        "gold" => Seal::Gold,
        "purple" => Seal::Purple,
        _ => Seal::None,
    }
}

/// `_RANK_BY_CODE` / `_SUIT_BY_CODE` over the game's `S_T` centre keys.
fn front_to_card(front: &str) -> crate::cards::CardRef {
    let (suit, rank) = front.split_once('_').expect("front is suit_rank");
    crate::cards::make_card(
        Rank::from_code(rank).unwrap_or(Rank::Two),
        Suit::from_code(suit).unwrap_or(Suit::Spades),
    )
}

// `_NOT_COPIED`, by hook: the copied joker names whose branch does not run for a
// copy. `None` shuts the whole context to copies.
const NO_COPY_DISCARDED: &[&str] = &["Ramen", "Yorick", "Castle", "Hit the Road", "Green Joker"];
const NO_COPY_FIRST_DISCARD: &[&str] = &["Trading Card"];
const NO_COPY_BEFORE_HAND: &[&str] = &["Sixth Sense"];
const NO_COPY_EMPTY: &[&str] = &[];

fn not_copied(hook: &str) -> Option<&'static [&'static str]> {
    match hook {
        "discarded" => Some(NO_COPY_DISCARDED),
        "on_first_discard" => Some(NO_COPY_FIRST_DISCARD),
        "before_hand" => Some(NO_COPY_BEFORE_HAND),
        "after_hand" | "on_round_start" | "on_pack_open" | "on_shop_end" => Some(NO_COPY_EMPTY),
        // end_of_round's joker branch is `elseif not context.blueprint`.
        "round_end" => None,
        "on_reroll" => Some(&["Flash Card"]),
        "on_pack_skip" => Some(&["Red Card"]),
        "on_cards_destroyed" => Some(&["Canio"]),
        "on_glass_shattered" => Some(&["Glass Joker"]),
        "on_sell" => Some(&["Invisible Joker"]),
        _ => Some(NO_COPY_EMPTY),
    }
}

const PACK_TAGS: [Tag; 5] = [
    Tag::Charm,
    Tag::Meteor,
    Tag::Buffoon,
    Tag::Ethereal,
    Tag::Standard,
];

const NOT_COPIED_ON_BLIND_SELECT: [&str; 3] = ["Madness", "Ceremonial Dagger", "Chicot"];

const EDITION_TAGS: [(Tag, Edition); 4] = [
    (Tag::Foil, Edition::Foil),
    (Tag::Holographic, Edition::Holographic),
    (Tag::Polychrome, Edition::Polychrome),
    (Tag::Negative, Edition::Negative),
];

/// All index subsets of `0..n` of size 1..=max_size, in itertools' order:
/// ascending size, and lexicographic within a size.
fn card_index_subsets(n: usize, max_size: usize) -> Vec<Vec<usize>> {
    fn rec(n: usize, k: usize, start: usize, cur: &mut Vec<usize>, out: &mut Vec<Vec<usize>>) {
        if cur.len() == k {
            out.push(cur.clone());
            return;
        }
        for i in start..n {
            cur.push(i);
            rec(n, k, i + 1, cur, out);
            cur.pop();
        }
    }
    let mut out = Vec::new();
    for size in 1..=max_size.min(n) {
        let mut cur = Vec::new();
        rec(n, size, 0, &mut cur, &mut out);
    }
    out
}

/// `random_element` over an already-sorted list, named by the game's pool.
fn pick_from<T: Clone>(rng: &mut RunRng, key: &str, items: &[T]) -> Option<T> {
    if items.is_empty() {
        None
    } else {
        Some(rng.choice(key, items))
    }
}

// --------------------------------------------------------------------------
// run setup (__post_init__'s remaining steps)
// --------------------------------------------------------------------------
impl GameState {
    /// Draw the boss for this ante and hold on to it.
    ///
    /// The game decides it at the start of the ante and keeps it in
    /// round_resets.blind_choices.Boss, which is what makes a Boss Tag able to
    /// re-roll it -- there is something to replace. Drawing it lazily when the
    /// boss blind comes up left nothing for the tag to act on and made the run
    /// one draw short on that pool.
    pub fn _roll_boss(&mut self) {
        let pool = eligible_bosses(self.ante, &self.bosses_used, WIN_ANTE);
        if pool.is_empty() {
            self.ante_boss = String::new();
            return;
        }
        let key = self.rng.choice("boss", &pool);
        *self.bosses_used.entry(key.to_string()).or_insert(0) += 1;
        self.ante_boss = key.to_string();
    }

    /// The voucher every shop this ante will offer.
    ///
    /// Once per ante, not once per shop: the game rolls it as the boss falls and
    /// all three shops of the next ante show the same one. Rolling per shop gave
    /// a run three vouchers an ante and put every later voucher draw in the wrong
    /// place.
    pub fn _roll_voucher(&mut self) {
        let redeemed: Vec<&str> = self.vouchers.iter().map(|v| v.key).collect();
        self.round_voucher = crate::shop_pool::draw_voucher(
            &mut self.rng,
            self.ante,
            &redeemed,
            &[] as &[&str],
            false,
        );
    }

    /// Both skip rewards for the ante, rolled together.
    ///
    /// The game shows them both on the blind select screen -- deciding whether to
    /// skip the Small Blind means knowing what skipping the Big one would pay --
    /// so both are drawn at once, at run start and again when a boss falls.
    pub fn _roll_ante_tags(&mut self) {
        self.ante_tag_keys = (0..2)
            .map(|_| crate::shop_pool::draw_tag(&mut self.rng, self.ante, None::<&[&str]>, ""))
            .collect();
        self.ante_tags = self
            .ante_tag_keys
            .iter()
            .map(|k| {
                tag_by_key(k)
                    .map(|t| t.label().to_string())
                    .unwrap_or_default()
            })
            .collect();
    }

    /// Re-roll the card and the suits that some jokers name.
    ///
    /// Four of these, all declared on the run and none of them ever set, so The
    /// Idol, Ancient Joker, Mail-In Rebate and Castle scored nothing at all. The
    /// game rolls them from G.playing_cards -- the whole deck, not the hand --
    /// sorted by sort_id, skipping Stone cards, once at run start and again at
    /// the end of every round. Ancient Joker draws a suit from the three it is
    /// *not* already on, so it never repeats itself.
    pub fn _reset_round_cards(&mut self) {
        let mut pool: Vec<CardRef> = self
            .full_deck
            .iter()
            .filter(|c| enhancement_of(c) != Enhancement::Stone)
            .cloned()
            .collect();
        pool.sort_by_key(|c| uid_of(c));
        if !pool.is_empty() {
            let idol_key = format!("idol{}", self.ante);
            let idol = self.rng.choice(&idol_key, &pool);
            self.idol_rank = Some(crate::cards::rank_of(&idol));
            self.idol_suit = Some(crate::cards::suit_of(&idol));
            let mail_key = format!("mail{}", self.ante);
            let mail = self.rng.choice(&mail_key, &pool);
            self.mail_rank = Some(crate::cards::rank_of(&mail));
            let cas_key = format!("cas{}", self.ante);
            let cas = self.rng.choice(&cas_key, &pool);
            self.castle_suit = Some(crate::cards::suit_of(&cas));
        }

        let suits: Vec<Suit> = [Suit::Spades, Suit::Hearts, Suit::Clubs, Suit::Diamonds]
            .into_iter()
            .filter(|s| Some(*s) != self.ancient_suit)
            .collect();
        let anc_key = format!("anc{}", self.ante);
        self.ancient_suit = Some(self.rng.choice(&anc_key, &suits));
    }
}

// --------------------------------------------------------------------------
// blind flow
// --------------------------------------------------------------------------
impl GameState {
    /// To Do List names a poker hand, and picks a new one every round.
    ///
    /// Drawn from the visible hands it is *not* already on, so it never repeats
    /// itself two rounds running. The hand belongs to the joker -- the game keeps
    /// it in ability.to_do_poker_hand -- so two To Do Lists name two different
    /// hands and roll separately.
    pub fn _reroll_todo_hands(&mut self) {
        let visible = self.visible_hands();
        // An end_of_round branch (card.lua:2975): a debuffed To Do List keeps
        // its hand and takes no 'to_do' draw.
        for joker in self.calculating_jokers() {
            let spec = joker.borrow().spec;
            if !spec.rerolls_a_hand {
                continue;
            }
            let named = joker.borrow().named_hand;
            let pool: Vec<HandType> = visible
                .iter()
                .cloned()
                .filter(|h| Some(*h) != named)
                .collect();
            if let Some(choice) = pick_from(&mut self.rng, "to_do", &pool) {
                joker.borrow_mut().named_hand = Some(choice);
            }
        }
    }

    /// A joker has just been built: what Card:set_ability does with it.
    ///
    /// For a To Do List that is rolling its hand (card.lua:311-322) -- from every
    /// visible hand, on the same 'to_do' stream as the round-end roll, and for
    /// every card built whether or not anyone buys it.
    pub fn _made_joker(&mut self, joker: JokerRef) -> JokerRef {
        if joker.borrow().spec.rerolls_a_hand {
            let visible = self.visible_hands();
            if let Some(choice) = pick_from(&mut self.rng, "to_do", &visible) {
                joker.borrow_mut().named_hand = Some(choice);
            }
        }
        joker
    }

    /// The blind that comes up next, offered on the select screen.
    pub fn _next_blind(&mut self) {
        let kind = [BlindKind::Small, BlindKind::Big, BlindKind::Boss][self.blind_index as usize];
        let boss = if kind == BlindKind::Boss {
            self._pick_boss()
        } else {
            None
        };
        let ante_scaling = self.deck_config().ante_scaling;
        let no_reward = kind == BlindKind::Small && self.stake >= 2;
        let mut blind = make_blind(
            kind,
            self.ante,
            boss,
            ante_scaling,
            self.blind_scaling(),
            no_reward,
        );
        blind.on_deck = true;
        self.blind = Some(blind);
        self.phase = Phase::BlindSelect;
        self._roll_orbital_choices();
        self._apply_blind_select_tags();
    }

    /// The boss in force's BossEffect, from the ante's held key.
    pub fn _pick_boss(&self) -> Option<&'static BossEffect> {
        if self.ante_boss.is_empty() {
            return None;
        }
        boss_row(&self.ante_boss).and_then(|row| boss_by_name(row.name))
    }

    /// Draw a new boss and put it in force if the boss blind is on deck.
    ///
    /// Rolling alone is not enough. `blind` is built when the blind comes up and
    /// holds its boss, so a re-roll that only moves ante_boss leaves the run
    /// facing the old one.
    pub fn _reroll_boss_blind(&mut self) {
        self._roll_boss();
        if let Some(blind) = &self.blind {
            if blind.kind == BlindKind::Boss {
                let on_deck = blind.on_deck;
                let boss = self._pick_boss();
                let ante_scaling = self.deck_config().ante_scaling;
                let mut fresh = make_blind(
                    BlindKind::Boss,
                    self.ante,
                    boss,
                    ante_scaling,
                    self.blind_scaling(),
                    false,
                );
                fresh.on_deck = on_deck;
                self.blind = Some(fresh);
            }
        }
        // G.FUNCS.reroll_boss ends by offering the tags a new blind choice.
        self._new_blind_choice();
    }

    /// The tags that fire on a new blind choice: the first one only.
    ///
    /// The game walks G.GAME.tags and breaks at the first that fires, from four
    /// places: the blind-select screen being built, a skip, a boss re-roll and a
    /// pack closing. So two Charm Tags open one pack, and the second opens when
    /// that pack closes; two Boss Tags re-roll twice.
    pub fn _new_blind_choice(&mut self) {
        let snapshot = self.tags.clone();
        for tag in snapshot {
            if tag == Tag::Boss {
                // Re-rolls the boss, free -- the paid reroll is the Director's
                // Cut button, which costs ten.
                if let Some(i) = self.tags.iter().position(|t| *t == Tag::Boss) {
                    self.tags.remove(i);
                }
                self.log("Boss Tag: the boss is re-rolled");
                self._reroll_boss_blind();
                return;
            }
            if PACK_TAGS.contains(&tag) {
                if let Some(i) = self.tags.iter().position(|t| *t == tag) {
                    self.tags.remove(i);
                }
                let key = self._tag_pack_key(tag);
                self._open_pack(pack_from_key(&key), true);
                return;
            }
        }
    }

    /// The Economy Tag fires on the blind-select screen; then the choice.
    pub fn _apply_blind_select_tags(&mut self) {
        let snapshot = self.tags.clone();
        for tag in snapshot {
            if tag == Tag::Economy {
                self.add_money(self.money.max(0).min(40), tag.label());
                if let Some(i) = self.tags.iter().position(|t| *t == Tag::Economy) {
                    self.tags.remove(i);
                }
            }
        }
        self._new_blind_choice();
    }
}

impl GameState {
    /// set_blind and the deal: the round begins.
    pub fn _start_round(&mut self) {
        // set_blind: the blind on offer is the one in force from here.
        if let Some(blind) = &mut self.blind {
            blind.on_deck = false;
        }
        self.round_number += 1;
        self.chips_scored = 0;
        self.discards_used = 0;
        self.hands_played_this_round.clear();
        self.mouth_only_hand = None;
        // The counters first, then the jokers that react to the blind being
        // taken -- Burglar's whole drawback is ease_discard(-discards_left),
        // which needs a number to take away.
        let (hands, discards) = self.round_allowance(true);
        self.hands_left = hands;
        self.discards_left = discards;
        self.free_rerolls_carried = self
            .active_jokers()
            .iter()
            .map(|j| j.borrow().spec.free_rerolls)
            .sum();
        self.reroll_price_carried = if self.temp_reroll_cost {
            0
        } else {
            (5 - self.vouchers.iter().map(|v| v.reroll_discount).sum::<i32>()).max(0)
        };

        // round_start_bonus, applied to every tag held rather than the first:
        // three Juggle Tags are nine cards, not three.
        while let Some(i) = self.tags.iter().position(|t| *t == Tag::Juggle) {
            self.tags.remove(i);
            self.temp_hand_size += 3;
            self.log("Juggle Tag: +3 hand size for this round");
        }

        // set_blind leaves the blind prepped, which is what lets Crimson Heart
        // take a joker on the opening deal -- all but The Fish, which it
        // unpreps (blind.lua:176), so the opening deal is dealt face up.
        let fish = self.boss().is_some_and(|b| b.face_down_after_play);
        if let Some(blind) = &mut self.blind {
            blind.prepped = !fish;
        }
        // Amber Acorn turns the row over in set_blind itself (blind.lua:190),
        // before any joker hears setting_blind: a joker Riff-raff makes is
        // shuffled in below face up. Chicot turns them back at once
        // (Blind:disable), which is `boss()` already answering None.
        if self.boss().is_some_and(|b| b.shuffles_jokers) {
            for joker in &self.jokers {
                joker.borrow_mut().face_down = true;
            }
        }
        self._apply_debuffs();
        self._setting_blind();

        // Amber Acorn shuffles the joker row under its own pool name -- three
        // times, each sorted by card id first (blind.lua:195-201). Chicot does
        // not stop it: set_blind queues the shuffle before Chicot's
        // setting_blind queues the disable, so the row is shuffled and then
        // turned face up (the game's own Lua, three seeds: `aajk` advanced and
        // the row reordered). Hence the blind's boss, not `boss()`, which
        // answers None beside Chicot.
        let shuffles = self
            .blind
            .as_ref()
            .and_then(|b| b.boss)
            .is_some_and(|b| b.shuffles_jokers);
        if shuffles && self.jokers.len() > 1 {
            for _ in 0..3 {
                self.jokers.sort_by_key(|j| j.borrow().uid);
                self.rng.shuffle(&mut self.jokers, "aajk");
            }
        }

        self.draw_pile = self.full_deck.clone();
        // The game shuffles with pseudoseed("nr" .. ante) at the start of a
        // round, and draws from the end. Sorted by card id first, because
        // pseudoshuffle does it itself.
        self.draw_pile.sort_by_key(|c| uid_of(c));
        let shuffle_key = format!("nr{}", self.ante);
        self.rng.shuffle(&mut self.draw_pile, &shuffle_key);
        self.hand = Vec::new();
        self.discard_pile = Vec::new();
        self._draw_to_hand_size();
        if self.phase == Phase::GameOver {
            // A run whose hand size has reached zero dies on the deal itself.
            return;
        }

        // After the deal, not before: the game fires these on
        // `first_hand_drawn`, so Certificate's card lands on top of a hand that
        // is already full.
        let hooks = self.calculating_hooks("on_round_start");
        for (joker, spec) in hooks {
            if let Some(answer) = spec.on_round_start {
                answer(&joker, self);
            }
        }

        self.phase = Phase::Playing;
        let target = self.blind.as_ref().map(|b| b.target).unwrap_or(0);
        let name = self.blind.as_ref().map(|b| b.name()).unwrap_or_default();
        self.log(format!(
            "--- Ante {} {}: need {} ---",
            self.ante, name, target
        ));
        self._drawn_to_hand();
    }

    /// calculate_joker({setting_blind}) down the row.
    ///
    /// Nothing leaves the row or joins it while the pass runs. Madness and
    /// Ceremonial Dagger only mark their victim getting_sliced and dissolve it
    /// in an event, and Riff-raff makes its jokers in one.
    pub fn _setting_blind(&mut self) {
        let row = self.jokers.clone();
        let effective = effective_specs(&row);
        self.blind_select_events = Some(Vec::new());
        self.getting_sliced = Vec::new();
        self.joker_buffer = 0;
        for (i, joker) in row.iter().enumerate() {
            let (spec, source) = &effective[i];
            let hook = match spec.on_blind_select {
                Some(hook) => hook,
                None => continue,
            };
            if joker.borrow().debuffed || source.borrow().debuffed {
                continue;
            }
            if !Rc::ptr_eq(source, joker) && NOT_COPIED_ON_BLIND_SELECT.contains(&spec.name) {
                continue;
            }
            if self.is_getting_sliced(joker) || self.is_getting_sliced(source) {
                continue;
            }
            hook(source, self);
        }
        let events = self.blind_select_events.take().unwrap_or_default();
        for event in events {
            self.after_setting_blind(event);
        }
        let sliced = std::mem::take(&mut self.getting_sliced);
        for (uid, reason) in sliced {
            let victim = self.jokers.iter().find(|j| j.borrow().uid == uid).cloned();
            if let Some(victim) = victim {
                self.destroy_joker(&victim, &reason);
            }
        }
        self.joker_buffer = 0;
    }
}

impl GameState {
    /// Blind:drawn_to_hand, after every deal into the round.
    ///
    /// Crimson Heart's half. The pick: the jokers not already debuffed are the
    /// candidates, every joker is let go, and one candidate is taken with
    /// pseudorandom_element, which sorts by sort_id first.
    pub fn _drawn_to_hand(&mut self) {
        let (prepped, is_playing) = match &self.blind {
            Some(blind) => (blind.prepped, self.phase == Phase::Playing),
            None => return,
        };
        if !is_playing {
            return;
        }
        let boss = self.boss();
        if boss.is_some_and(|b| b.debuff_a_joker) && prepped && !self.jokers.is_empty() {
            let count = self.jokers.len();
            let mut eligible: Vec<JokerRef> = self
                .jokers
                .iter()
                .filter(|j| !j.borrow().debuffed || count < 2)
                .cloned()
                .collect();
            for joker in self.jokers.clone() {
                self.set_joker_debuff(&joker, false);
            }
            eligible.sort_by_key(|j| j.borrow().uid);
            if let Some(chosen) = pick_from(&mut self.rng, "crimson_heart", &eligible) {
                self.set_joker_debuff(&chosen, true);
            }
        }
        if let Some(blind) = &mut self.blind {
            blind.prepped = false;
        }
    }

    /// Cerulean Bell picks a card the player must always include.
    ///
    /// Chosen when the hand is dealt, and only when the hand does not already
    /// hold the one it chose -- so it survives a discard that leaves it in place
    /// and is replaced when it goes.
    pub fn _nominate_forced_card(&mut self) {
        let boss = self.boss();
        match boss {
            None => self.forced_card = None,
            Some(boss) => {
                if !boss.forces_a_card || self.hand.is_empty() {
                    return;
                }
                let held = self
                    .forced_card
                    .as_ref()
                    .is_some_and(|f| self.hand.iter().any(|c| uid_of(c) == uid_of(f)));
                if held {
                    return;
                }
                let mut pool = self.hand.clone();
                pool.sort_by_key(|c| uid_of(c));
                if let Some(card) = pick_from(&mut self.rng, "cerulean_bell", &pool) {
                    self.forced_card = Some(card);
                }
            }
        }
    }

    /// The whole deck is asked its debuff when the blind is set.
    pub fn _apply_debuffs(&mut self) {
        for card in self.full_deck.clone() {
            self.debuff_card(&card);
        }
    }
}

// --------------------------------------------------------------------------
// playing: sorting, dealing, the blind's own hooks
// --------------------------------------------------------------------------
impl GameState {
    /// Keep the hand in the order the game shows it.
    ///
    /// G.hand is sorted "desc" by get_nominal, and every action addresses cards
    /// by position, so an unsorted hand turns the same choice into a different
    /// play. The id breaks ties, ascending.
    pub fn _sort_hand(&mut self) {
        let suit = self.hand_sort == "suit";
        self.hand.sort_by(|a, b| {
            let va = if suit {
                crate::cards::suit_sort_value(a)
            } else {
                crate::cards::sort_value(a)
            };
            let vb = if suit {
                crate::cards::suit_sort_value(b)
            } else {
                crate::cards::sort_value(b)
            };
            vb.partial_cmp(&va)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(uid_of(a).cmp(&uid_of(b)))
        });
    }

    /// Reorder the hand the way the sort buttons do, and keep doing it.
    ///
    /// The choice sticks, as it does in the game: CardArea keeps the method on
    /// itself and applies it to every later draw.
    pub fn sort_hand(&mut self, by: &str) {
        self.hand_sort = if by == "suit" {
            "suit".to_string()
        } else {
            "rank".to_string()
        };
        self._sort_hand();
    }

    /// Deal `count` cards regardless of the hand limit.
    ///
    /// Sorted as each card lands only if one did: a hand the player dragged into
    /// an order keeps it when nothing is dealt.
    pub fn _draw_cards(&mut self, count: i32) {
        let mut drawn = 0;
        for _ in 0..count {
            match self.draw_pile.pop() {
                Some(card) => {
                    let flipped = self._stay_flipped(&card);
                    card.borrow_mut().face_down = flipped;
                    self.hand.push(card);
                    drawn += 1;
                }
                None => break,
            }
        }
        if drawn > 0 {
            self._sort_hand();
        }
    }

    /// Top the hand up to the hand limit, and end the round if there is none.
    pub fn _draw_to_hand_size(&mut self) {
        // Checked before anything is dealt, and it is the one loss that does not
        // go through the end of the round: no target check, no Mr. Bones, no
        // cash-out. An Arcana or Spectral pack suspends it.
        if self.hand_size() <= 0 && self.hand.is_empty() && !self._in_hand_pack() {
            self.phase = Phase::GameOver;
            self.log("No hand size left");
            return;
        }

        let boss = self.boss();
        if boss.is_some_and(|b| b.always_draw_three)
            && (!self.hands_played_this_round.is_empty() || self.discards_used > 0)
        {
            // The Serpent: after the first play or discard of the round the hand
            // is topped up with exactly three cards, whatever room there was.
            let count = 3.min(self.draw_pile.len() as i32);
            self._draw_cards(count);
            self._nominate_forced_card();
            return;
        }
        let mut drawn = 0;
        while (self.hand.len() as i32) < self.hand_size() && !self.draw_pile.is_empty() {
            let card = self.draw_pile.pop().unwrap();
            let flipped = self._stay_flipped(&card);
            card.borrow_mut().face_down = flipped;
            self.hand.push(card);
            drawn += 1;
        }
        if drawn > 0 {
            self._sort_hand();
        }
        self._nominate_forced_card();
        if self.hand.is_empty() && self.draw_pile.is_empty() && self.phase == Phase::Playing {
            // Hand and deck both empty ends the round where it stands.
            self.phase = Phase::GameOver;
            self.log("Ran out of cards");
        }
    }

    /// Blind:stay_flipped (blind.lua:605-620), asked of each card dealt into
    /// the hand: is it dealt face down?
    ///
    /// Asked one card at a time, in the order they are dealt, because The
    /// Wheel's answer is a draw on its own pool -- `pseudorandom(
    /// pseudoseed('wheel')) < normal/7` -- taken for every card while the
    /// Wheel is live and for none otherwise, so a hand of eight is eight draws
    /// and no other pool moves. A disabled boss (Chicot, Luchador) turns
    /// nothing over, and neither does a pack's deal, which happens with no
    /// blind in force.
    pub fn _stay_flipped(&mut self, card: &CardRef) -> bool {
        let boss = match self.boss() {
            Some(boss) => boss,
            None => return false,
        };
        if boss.face_down_odds > 0 {
            let normal = self.probability_scale();
            if self.rng.chance("wheel", normal, boss.face_down_odds as f64) {
                return true;
            }
        }
        if boss.face_down_first_hand
            && self.hands_played_this_round.is_empty()
            && self.discards_used == 0
        {
            return true;
        }
        if boss.face_down_faces && crate::jokers::is_face_for(card, self, true) {
            return true;
        }
        if boss.face_down_after_play && self.blind.as_ref().is_some_and(|b| b.prepped) {
            return true;
        }
        false
    }

    /// Turn every card in the hand face up -- `Blind:disable`'s loop over
    /// `G.hand.cards` (blind.lua:364-372), and the end of a round.
    pub fn _hand_face_up(&mut self) {
        for card in &self.hand {
            card.borrow_mut().face_down = false;
        }
    }

    /// Turn the joker row face up: `Blind:defeat` and `Blind:disable`
    /// (blind.lua:338, 358).
    pub fn _jokers_face_up(&mut self) {
        for joker in &self.jokers {
            joker.borrow_mut().face_down = false;
        }
    }

    /// Is an Arcana or Spectral pack open? Those two deal a hand.
    pub fn _in_hand_pack(&self) -> bool {
        self.phase == Phase::Pack
            && self
                .pack
                .is_some_and(|p| matches!(p.kind, PackKind::Arcana | PackKind::Spectral))
    }

    /// Turn the boss off mid-round, the way Blind:disable does it.
    ///
    /// Not simply a flag. A blind that was made larger is divided back down to
    /// the ordinary boss size, a hand or a discard the blind took away is handed
    /// back to the counter that is already running, and The Manacle -- which took
    /// a card out when the blind began -- gives back two.
    pub fn disable_blind(&mut self, source: &str) {
        let boss = match &self.blind {
            Some(blind) if blind.boss.is_some() && !blind.disabled => blind.boss.unwrap(),
            _ => return,
        };
        if let Some(blind) = &mut self.blind {
            blind.disabled = true;
        }
        // Everything face down turns over, the row for any boss and the hand
        // for the four that dealt it that way.
        self._jokers_face_up();
        self._hand_face_up();
        if boss.chip_mult != 2.0 && boss.chip_mult != 0.0 {
            if let Some(blind) = &mut self.blind {
                blind.target = (blind.target as f64 * 2.0 / boss.chip_mult) as i64;
            }
        }
        let (hands, discards) = self.round_allowance(true);
        if boss.hands_delta != 0 {
            self.hands_left = self.hands_left.max(hands);
        }
        if boss.discards_delta != 0 {
            self.discards_left = self.discards_left.max(discards);
        }
        if boss.hand_size_delta < 0 {
            // change_size(1) deals its card and draw_from_deck_to_hand(1)
            // another. Both are the delta rather than a top-up.
            self._hand_size_changed(-boss.hand_size_delta);
            self._draw_cards(-boss.hand_size_delta);
        }
        for card in self.full_deck.clone() {
            self.debuff_card(&card);
        }
        for joker in self.jokers.clone() {
            self.set_joker_debuff(&joker, false);
        }
        self.log(format!("{}: {} is disabled", source, boss.name));
    }
}

// --------------------------------------------------------------------------
// playing: the hand itself
// --------------------------------------------------------------------------
impl GameState {
    /// The card at `index` trades places with the one on its left.
    ///
    /// Hand order is scoring order -- play_cards_from_highlighted sorts the
    /// selection by screen position -- so this is a decision, not presentation.
    pub fn swap_card_left(&mut self, index: usize) {
        if index > 0 && index < self.hand.len() {
            self.hand.swap(index - 1, index);
        }
    }

    /// Put the named cards leftmost, in the order named, and renumber.
    pub fn _arrange_play(&mut self, indices: &[usize]) -> Vec<usize> {
        let mut sorted = indices.to_vec();
        sorted.sort_unstable();
        if indices == sorted.as_slice() {
            return indices.to_vec();
        }
        let chosen: Vec<CardRef> = indices.iter().map(|&i| self.hand[i].clone()).collect();
        let rest: Vec<CardRef> = self
            .hand
            .iter()
            .enumerate()
            .filter(|(i, _)| !indices.contains(i))
            .map(|(_, c)| c.clone())
            .collect();
        self.hand = chosen.iter().cloned().chain(rest).collect();
        (0..chosen.len()).collect()
    }

    /// A play: set money_at_play, resolve the hand, put it back.
    pub fn _play(&mut self, indices: Vec<usize>) {
        self.money_at_play = Some(self.money);
        self._play_hand(indices);
        self.money_at_play = None;
    }

    /// The heart: evaluate, score, shatter, deal, and end the round or not.
    pub fn _play_hand(&mut self, indices: Vec<usize>) {
        // Which cards the boss debuffs is decided again every time, not once
        // when the round began. The game re-evaluates it in Card:update.
        self._apply_debuffs();
        if self.boss().is_some_and(|b| b.debuff_a_joker) && !self.jokers.is_empty() {
            // Crimson Heart's press_play only preps the blind. The joker it takes
            // is picked on the draw that follows the hand.
            if let Some(blind) = &mut self.blind {
                blind.prepped = true;
            }
        }
        // The Fish's press_play preps it too (blind.lua:494), so the draw that
        // follows the hand is dealt face down.
        if self.boss().is_some_and(|b| b.face_down_after_play) {
            if let Some(blind) = &mut self.blind {
                blind.prepped = true;
            }
        }

        let arranged = self._arrange_play(&indices);
        let played: Vec<CardRef> = arranged.iter().map(|&i| self.hand[i].clone()).collect();
        let result = self.evaluate_selection(&played);

        // Played this ante from the moment they are played, before anything
        // scores.
        for card in &played {
            crate::cards::set_played_this_ante(card, true);
            // Into G.play, which turns a face-down card over (cardarea.lua:38).
            card.borrow_mut().face_down = false;
        }

        // The Hook takes its two cards *before* the hand scores. By creation
        // order, not by what is on screen.
        if let Some(boss) = self.boss() {
            if boss.discard_random_on_play > 0 {
                let mut pool: Vec<CardRef> = self
                    .hand
                    .iter()
                    .filter(|c| !played.iter().any(|p| uid_of(p) == uid_of(c)))
                    .cloned()
                    .collect();
                pool.sort_by_key(|c| uid_of(c));
                let count = (boss.discard_random_on_play as usize).min(pool.len());
                let idx = self.rng.sample_index("hook", pool.len(), count);
                let taken: Vec<CardRef> = idx.iter().map(|&i| pool[i].clone()).collect();
                if !taken.is_empty() {
                    self.discard_cards(&taken, true);
                }
            }
        }

        // DNA and Sixth Sense act on the played cards before they score, and only
        // on the round's first hand. Neither runs on a hand the boss refuses.
        let debuffed = self.hand_is_debuffed(result.hand, &played);
        if !debuffed && self.hands_played_this_round.is_empty() {
            let hooks = self.calculating_hooks("before_hand");
            for (joker, spec) in hooks {
                if let Some(answer) = spec.before_hand {
                    answer(&joker, &played, self);
                }
            }
        }

        // The held cards are read *after* them, because DNA puts its copy in hand
        // and the game scores that copy as a held card like any other.
        let held: Vec<CardRef> = self
            .hand
            .iter()
            .filter(|c| !played.iter().any(|p| uid_of(p) == uid_of(c)))
            .cloned()
            .collect();
        self.hands_left -= 1;
        *self.hand_levels.plays.entry(result.hand).or_insert(0) += 1;
        self.last_hand = result.hand.label().to_string();

        self._play_hand_score(&result, &played, &held, debuffed);
    }
}

impl GameState {
    /// The scoring half of a played hand, split only to keep this readable.
    pub fn _play_hand_score(
        &mut self,
        result: &HandResult,
        played: &[CardRef],
        held: &[CardRef],
        debuffed: bool,
    ) {
        // The Arm takes the level off *before* the hand scores.
        let mut arm_triggered = false;
        let mut ox_triggered = false;
        if let Some(boss) = self.boss() {
            if boss.level_down_played_hand {
                let level = self.hand_levels.level(result.hand);
                arm_triggered = level > 1;
                self.hand_levels
                    .levels
                    .insert(result.hand, (level - 1).max(1));
            }
        }

        // And the two bosses that move money do it here as well, for the same
        // reason -- Bootstraps reads the money already gone.
        if let Some(boss) = self.boss() {
            if boss.money_per_card_played != 0 {
                self.add_money(boss.money_per_card_played * played.len() as i32, boss.name);
            }
            if boss.zero_money_on_most_played && self._is_most_played(result.hand) {
                ox_triggered = true;
                self.money = 0;
                self.log(format!("{}: money set to $0", boss.name));
            }
        }

        // The Mouth's write, made only by the real call and only when the hand
        // got through.
        if !debuffed
            && self.boss().is_some_and(|b| b.lock_first_hand_type)
            && self.mouth_only_hand.is_none()
        {
            self.mouth_only_hand = Some(result.hand);
        }

        // G.GAME.blind.triggered, which is all Matador reads.
        if self.blind.is_some() {
            let triggered = self.boss().is_some() && (debuffed || arm_triggered || ox_triggered);
            if let Some(blind) = &mut self.blind {
                blind.triggered = triggered;
            }
        }

        let mut ctx_opt: Option<ScoreContext> = None;
        if debuffed {
            self.log(format!(
                "{} debuffed by {}: scores nothing",
                result.hand.label(),
                self.boss().map(|b| b.name).unwrap_or("the blind")
            ));
            let pairs = calculating_specs(&self.jokers);
            for (spec, source) in pairs {
                if let Some(answer) = spec.on_debuffed_hand {
                    answer(&source, self);
                }
            }
            self._refused_hand_after_pass(result, played, held);
        } else {
            ctx_opt = Some(score_hand(self, result, played, held));
        }

        // The run's hand count goes up once the hand has scored, not before.
        self.hands_played += 1;

        // Jokers that make a card off the back of a hand run once it has resolved.
        let hooks = self.calculating_hooks("after_hand");
        for (joker, spec) in hooks {
            if let (Some(answer), Some(ctx)) = (spec.after_hand, ctx_opt.as_mut()) {
                answer(&joker, ctx, self);
            }
        }

        let gained = ctx_opt.as_ref().map(|c| c.score()).unwrap_or(0);
        self.chips_scored += gained;
        self.best_hand = self.best_hand.max(gained);
        if let Some(ctx) = &ctx_opt {
            self.log(format!(
                "{} scored {} ({} x {}) -> {}",
                result.hand.label(),
                gained,
                ctx.chips,
                ctx.mult,
                self.chips_scored
            ));
            if ctx.money_gained != 0 {
                let money = ctx.money_gained;
                self.add_money(money, "cards");
            }
        }
        self.hands_played_this_round.insert(result.hand);

        // The glass roll is in the destroying pass, inside the block a refused
        // hand skips.
        let shattered = if debuffed {
            Vec::new()
        } else {
            shattered_glass(self, &result.scoring)
        };
        for card in shattered {
            self.remove_card(&card, true);
            self.log(format!("{} shattered", crate::cards::label_of(&card)));
        }

        for card in played.to_vec() {
            if let Some(i) = self.hand.iter().position(|c| uid_of(c) == uid_of(&card)) {
                self.hand.remove(i);
                self.discard_pile.push(card.clone());
            }
        }

        let target = self.blind.as_ref().map(|b| b.target).unwrap_or(0);
        if self.chips_scored >= target {
            self._beat_blind(true);
        } else if self.hands_left <= 0 {
            self._lose_round();
        } else {
            self._draw_to_hand_size();
            self._drawn_to_hand();
        }
    }
}

impl GameState {
    /// Fix the hand The Ox will punish, as a boss round closes.
    ///
    /// The game keeps `v.played > _played` with a tie-break whose order is not
    /// reproducible across processes -- so a tie is a coin flip. Measured, the
    /// order runs strongest-first, which leaves the *weakest* of the tied hands
    /// standing. Walking HANDLIST backwards reproduces that.
    pub fn _snapshot_most_played(&mut self) {
        let mut best = HandType::HighCard;
        let mut best_plays = i32::MIN;
        for hand in HANDLIST.iter().rev() {
            let plays = self.hand_levels.played(*hand);
            if plays > best_plays {
                best_plays = plays;
                best = *hand;
            }
        }
        self.most_played_hand = best;
    }

    /// The Ox compares against the snapshot, not against a live count.
    pub fn _is_most_played(&self, hand: HandType) -> bool {
        hand == self.most_played_hand
    }

    /// A discard the player asked for.
    pub fn _discard(&mut self, indices: &[usize]) {
        let cards: Vec<CardRef> = indices.iter().map(|&i| self.hand[i].clone()).collect();
        self.discards_left -= 1;
        self.discard_cards(&cards, false);
        self.discards_used += 1;
        self._draw_to_hand_size();
        self._drawn_to_hand();
    }

    /// The discard itself: seals, joker hooks, and the pile.
    ///
    /// Split out because a discard the player did not ask for goes through all of
    /// it: The Hook calls this with `hook` true and still fires the seals and the
    /// jokers and still fills the discard pile -- it just costs no discard and
    /// draws nothing back.
    pub fn discard_cards(&mut self, cards: &[CardRef], hook: bool) {
        let first = self.discards_used == 0;
        // Copies included: a Blueprint on a Mail-In Rebate pays twice.
        let hooks = self.calculating_hooks("discarded");
        for (joker, spec) in hooks {
            if let Some(answer) = spec.discarded {
                answer(&joker, cards, self);
            }
        }
        if first {
            let hooks = self.calculating_hooks("on_first_discard");
            for (joker, spec) in hooks {
                // The copied joker, so a copy of Burnt Joker skips too.
                if hook && joker.borrow().name() == "Burnt Joker" {
                    continue;
                }
                if let Some(answer) = spec.on_first_discard {
                    answer(&joker, cards, self);
                }
            }
        }
        for card in cards {
            // Card:calculate_seal opens `if self.debuff then return nil end`: a
            // debuffed Purple Seal makes nothing.
            if seal_of(card) == Seal::Purple && !crate::cards::debuffed_of(card) {
                let specs = self.random_consumables(ConsumableKind::Tarot, 1, "8ba");
                self.add_consumables(&specs, Edition::None);
            }
            // A joker may have eaten the card on its way out -- Trading Card
            // destroys a lone first discard -- and a destroyed card is not
            // discarded.
            if !self.hand.iter().any(|c| uid_of(c) == uid_of(card)) {
                continue;
            }
            if let Some(i) = self.hand.iter().position(|c| uid_of(c) == uid_of(card)) {
                self.hand.remove(i);
                card.borrow_mut().face_down = false;
                self.discard_pile.push(card.clone());
            }
        }
    }
}

impl GameState {
    /// context.after for a hand the boss refused, over self.jokers.
    ///
    /// score_hand runs the pass for a scored hand; a refused one never gets
    /// there, and evaluate_play asks it anyway. One place, so _play and
    /// preview_play cannot drift apart on it.
    pub fn _refused_hand_after_pass(
        &mut self,
        result: &HandResult,
        played: &[CardRef],
        held: &[CardRef],
    ) {
        let pairs = calculating_specs(&self.jokers);
        let mut ctx = ScoreContext::new(
            result.hand,
            result.scoring.clone(),
            played.to_vec(),
            held.to_vec(),
            result.contains,
            self.money,
            self.probability_scale(),
        );
        after_hand_pass(&pairs, &mut ctx, self);
    }

    /// Blind:debuff_hand -- the boss zeroing a hand it dislikes.
    ///
    /// The Psychic wants five cards, The Eye a hand type not yet played this
    /// round, The Mouth the same type as the round's first. Failing any of them
    /// is allowed; it just scores nothing.
    pub fn hand_is_debuffed(&self, hand: HandType, cards: &[CardRef]) -> bool {
        let boss = match self.boss() {
            Some(boss) => boss,
            None => return false,
        };
        if self.blind.as_ref().is_some_and(|b| b.disabled) {
            return false;
        }
        if boss.min_cards_played > 0 && (cards.len() as i32) < boss.min_cards_played {
            return true;
        }
        if boss.no_repeat_hand && self.hands_played_this_round.contains(&hand) {
            return true;
        }
        if boss.lock_first_hand_type
            && self.mouth_only_hand.is_some()
            && self.mouth_only_hand != Some(hand)
        {
            return true;
        }
        false
    }

    /// Whether a play is *legal*, which is narrower than it looks.
    ///
    /// Only Cerulean Bell belongs here. The Psychic, The Eye and The Mouth are not
    /// legality at all: a debuffed hand is played, consumes a hand, counts as
    /// played, and scores nothing.
    pub fn _restriction_ok(&self, cards: &[usize]) -> bool {
        let boss = match self.boss() {
            Some(boss) => boss,
            None => return true,
        };
        if boss.forces_a_card {
            if let Some(forced) = &self.forced_card {
                let holds = cards
                    .iter()
                    .any(|&i| i < self.hand.len() && uid_of(&self.hand[i]) == uid_of(forced));
                if !holds {
                    return false;
                }
            }
        }
        true
    }

    /// Whether any subset satisfies the boss restriction (see `_play_actions`).
    pub fn _restriction_satisfiable(&self) -> bool {
        let boss = match self.boss() {
            Some(boss) => boss,
            None => return true,
        };
        if !boss.forces_a_card {
            return true;
        }
        self._card_subsets(MAX_PLAYED)
            .iter()
            .any(|s| self._restriction_ok(s))
    }

    /// Hand indices a selection may name.
    ///
    /// `ordered` is the usual case: naming the set ascending is the one spelling
    /// of it. A consumable whose targets are *positional* is the exception.
    pub fn _valid_indices(&self, cards: &[usize], max_size: usize, ordered: bool) -> bool {
        if cards.is_empty() || cards.len() > max_size || cards.iter().any(|&i| i >= self.hand.len())
        {
            return false;
        }
        if ordered {
            return cards.windows(2).all(|w| w[0] < w[1]);
        }
        let mut seen = std::collections::HashSet::new();
        cards.iter().all(|i| seen.insert(*i))
    }

    /// All index subsets of the hand of size 1..=max_size.
    pub fn _card_subsets(&self, max_size: usize) -> Vec<Vec<usize>> {
        card_index_subsets(self.hand.len(), max_size)
    }
}

// --------------------------------------------------------------------------
// ending a round: lose, beat, cash out
// --------------------------------------------------------------------------
impl GameState {
    /// The round ended short of the target.
    ///
    /// Mr. Bones is the only way back. He needs a quarter of the target, and what
    /// he buys is specific: the engine dissolves him, marks the blind defeated so
    /// the run moves on to the next one rather than replaying it, and stops at the
    /// cash-out screen -- but pays no blind reward, because the blind was not
    /// beaten.
    pub fn _lose_round(&mut self) {
        let target = self.blind.as_ref().map(|b| b.target).unwrap_or(0);
        if target > 0 {
            for joker in self.jokers.clone() {
                let saves = {
                    let j = joker.borrow();
                    j.spec.prevents_death && !j.debuffed
                };
                if saves && (self.chips_scored as f64) / (target as f64) >= 0.25 {
                    let name = joker.borrow().name();
                    self.log(format!("{} saved the run", name));
                    self._beat_blind(false);
                    self.destroy_joker(&joker, "");
                    return;
                }
            }
        }
        self.phase = Phase::GameOver;
        let name = self.blind.as_ref().map(|b| b.name()).unwrap_or_default();
        self.log(format!("Lost on ante {} {}", self.ante, name));
    }

    /// Close the round and stop on the cash-out screen.
    ///
    /// `reward` is False when Mr. Bones brought the run here rather than the
    /// score. The payout is worked out here but not paid: the game shows it and
    /// waits. The deck comes back now, though.
    pub fn _beat_blind(&mut self, reward: bool) {
        // end_round lets the D6 Tag's $0 start go, and the price is the round's
        // own again.
        if self.temp_reroll_cost {
            self.temp_reroll_cost = false;
            self.reroll_price_carried =
                (5 - self.vouchers.iter().map(|v| v.reroll_discount).sum::<i32>()).max(0);
        }
        // A gold card pays the moment the round ends -- ease_dollars, right there
        // in the hand loop -- rather than as a row on the cash-out screen.
        let mut gold = 0;
        for card in self.hand.clone() {
            if crate::cards::enhancement_of(&card) == Enhancement::Gold {
                gold += 3 * held_triggers(self, &card);
            }
        }
        if gold != 0 {
            self.add_money(gold, "gold cards");
        }

        // A blue seal makes the Planet for the *last hand played this round*,
        // once, at the end of it, for each sealed card still in hand.
        if !self.last_hand.is_empty() {
            let hand = HANDLIST.into_iter().find(|h| h.label() == self.last_hand);
            for card in self.hand.clone() {
                if crate::cards::seal_of(&card) == Seal::Blue {
                    if let Some(hand) = hand {
                        let triggers = held_triggers(self, &card);
                        for _ in 0..triggers {
                            let spec = crate::consumables::spec_or_panic(planet_for_hand(hand));
                            self.add_consumables(&[spec], Edition::None);
                        }
                    }
                }
            }
        }

        let config = *self.deck_config();
        let per_hand = config.extra_hand_bonus;
        let per_discard = config.extra_discard_bonus;
        // G.GAME.unused_discards accumulates over the whole run.
        self.unused_discards += self.discards_left.max(0);

        let blind = self.blind.clone().expect("a round has a blind");
        self.pending_payout = (if reward { blind.reward } else { 0 }) as i64
            + (self.hands_left.max(0) * per_hand) as i64
            + (self.discards_left.max(0) * per_discard) as i64;
        self._beat_blind_deck(blind, config.double_tag_after_boss);
    }
}

impl GameState {
    /// The deck-return and tag half of `_beat_blind`, split for readability.
    fn _beat_blind_deck(&mut self, blind: crate::blinds::Blind, double_tag: bool) {
        // Every card returns to the deck as the round closes -- but in the game's
        // order: the hand to the discard from the front, the discard to the deck
        // from the back, each inserted at the deck's front.
        // Blind:defeat turns the row back over; the hand goes back to the deck,
        // where nothing is face up or down until it is dealt again.
        self._jokers_face_up();
        self._hand_face_up();
        self.discard_pile.extend(self.hand.clone());
        self.hand = Vec::new();
        let mut fresh = self.discard_pile.clone();
        fresh.extend(self.draw_pile.clone());
        self.draw_pile = fresh;
        self.discard_pile = Vec::new();

        if blind.kind == BlindKind::Boss {
            // The Ox's target for the ante ahead is fixed here, as the boss round
            // closes, and nowhere else.
            self._snapshot_most_played();
        }
        if blind.kind == BlindKind::Boss && double_tag {
            self.add_tag_by_key("tag_double");
        }
        if blind.kind == BlindKind::Boss {
            while let Some(i) = self.tags.iter().position(|t| *t == Tag::Investment) {
                self.tags.remove(i);
                self.pending_payout += 25;
                self.log("Investment Tag: +$25 on the cash-out");
            }
        }

        // The Juggle Tag's cards go back as the round closes.
        self.temp_hand_size = 0;
        self.beaten_blind = Some(blind.clone());
        // Beating a boss puts Campfire back to X1.
        if blind.kind == BlindKind::Boss {
            for joker in self.calculating_jokers() {
                if joker.borrow().name() == "Campfire" {
                    joker.borrow_mut().counter = 1.0;
                }
            }
        }
        self.beaten_was_boss = blind.kind == BlindKind::Boss;
        self.blind = None;
        self.phase = Phase::RoundEval;

        // The ante turns over the moment the boss round closes.
        if self.beaten_was_boss {
            self.blind_index = 0;
            self.ante += 1;
            self.skipped_this_ante.clear();
            // After the ante turns over, not before: the voucher for the ante
            // about to start is drawn from that ante's pool.
            self._roll_voucher();
        } else {
            self.blind_index += 1;
        }
        self._reset_round_cards();
        self._reroll_todo_hands();

        // calculate_joker({end_of_round}) -- decay, growth and destruction, all of
        // it the instant the round closes and before the cash-out screen appears.
        // No copies: the branch is `elseif not context.blueprint`.
        let in_row = self.jokers.clone();
        let hooks = self.calculating_hooks("round_end");
        for (joker, spec) in hooks {
            if let Some(answer) = spec.round_end {
                answer(&joker, self);
            }
        }

        // The stake's stickers are paid for when the round ends, not when the money
        // is taken. Over the row the pass began with, not what is left of it.
        for joker in in_row {
            let (rental, perishable, tally, name) = {
                let j = joker.borrow();
                (j.rental, j.perishable, j.perish_tally, j.name())
            };
            if rental {
                self.add_money(-RENTAL_RATE, &format!("{} rental", name));
            }
            if perishable && tally > 0 {
                let mut j = joker.borrow_mut();
                j.perish_tally -= 1;
                if j.perish_tally == 0 {
                    j.debuffed = true;
                }
            }
            if perishable && tally == 1 {
                self.log(format!("{} perished", name));
            }
        }

        // And now the interest, on what is left after the rent. The multiplier
        // applies after the cap has bitten.
        let per_block = 1 + self
            .active_jokers()
            .iter()
            .map(|j| j.borrow().spec.interest_bonus)
            .sum::<i32>();
        if !self.deck_config().no_interest {
            let blocks = (self.money.max(0) / 5).min(self.interest_cap());
            self.pending_payout += (per_block * blocks) as i64;
        }

        // Which jokers get a money row is settled now, while Crimson Heart still
        // holds its joker. Blind:defeat then releases every joker it held. The
        // handles are kept, as the Python original keeps the instances: a joker
        // sold on the cash-out screen still pays the row it was given here.
        self.dollar_rows = Some(
            self.active_jokers()
                .into_iter()
                .filter(|j| j.borrow().spec.round_money.is_some())
                .collect(),
        );
        for joker in self.jokers.clone() {
            self.set_joker_debuff(&joker, false);
        }
    }

    /// Take the payout and move on, as pressing Cash Out does.
    pub fn _cash_out(&mut self) {
        let beaten = self.beaten_blind.clone().expect("cash out has a blind");
        // Pressing Cash Out shuffles the deck, under its own pool name. The next
        // thing to draw from this deck is the hand an Arcana or Spectral pack
        // deals in the shop, so without this the pack deals off the round's order.
        self.draw_pile.sort_by_key(|c| uid_of(c));
        let shuffle_key = format!("cashout{}", self.ante);
        self.rng.shuffle(&mut self.draw_pile, &shuffle_key);

        let payout = self.pending_payout as i32;
        self.add_money(payout, &format!("{} payout", beaten.name()));
        self.pending_payout = 0;

        // A beaten boss ends the ante, and the next one's two skip tags are rolled
        // here, on the cash-out screen -- after the voucher.
        if beaten.kind == BlindKind::Boss {
            self._roll_ante_tags();
            // reset_blinds runs after the tags: it draws the next boss and gives
            // Director's Cut its reroll back.
            self.boss_rerolled = false;
            self._roll_boss();
        }

        // The money rows on the cash-out screen are paid when the button is
        // pressed. A debuffed joker has no row. The rows were settled at
        // `beat_blind` and are paid as they stand, even if one of those jokers
        // has since been sold -- the Python original holds the instances.
        let rows: Vec<JokerRef> = match self.dollar_rows.take() {
            Some(rows) => rows,
            None => self
                .active_jokers()
                .into_iter()
                .filter(|j| j.borrow().spec.round_money.is_some())
                .collect(),
        };
        for joker in rows {
            let hook = joker.borrow().spec.round_money;
            if let Some(hook) = hook {
                hook(&joker, self);
            }
        }

        // Cashing out is also where the round's counters go back.
        self.chips_scored = 0;
        let (hands, discards) = self.round_allowance(true);
        self.hands_left = hands;
        self.discards_left = discards;
        // Cleared as the shop opens, before any tag runs.
        self.shop_free = false;

        let was_boss = self.beaten_was_boss;
        self.beaten_blind = None;
        // Blind:defeat resets the blind, which asks every playing card again with
        // no rule to catch it, releasing it.
        for card in self.full_deck.clone() {
            crate::cards::set_debuffed(&card, false);
        }
        if was_boss {
            for card in self.full_deck.clone() {
                crate::cards::set_played_this_ante(&card, false);
            }
            // The ante has already moved on, so the run is won once it reads
            // *past* WIN_ANTE, not at it.
            if self.ante > WIN_ANTE && !self.endless {
                self.phase = Phase::Won;
                self.log(format!("Run won after ante {}", WIN_ANTE));
                return;
            }
        }

        self._open_shop();
    }
}

// --------------------------------------------------------------------------
// packs
// --------------------------------------------------------------------------
impl GameState {
    /// Which pack each pack tag hands over.
    pub fn _tag_pack_key(&mut self, tag: Tag) -> String {
        if tag == Tag::Charm {
            let n = self.rng.math_random(Some(1.0), Some(2.0)) as i64;
            return format!("p_arcana_mega_{}", n);
        }
        if tag == Tag::Meteor {
            let n = self.rng.math_random(Some(1.0), Some(2.0)) as i64;
            return format!("p_celestial_mega_{}", n);
        }
        match tag {
            Tag::Ethereal => "p_spectral_normal_1".to_string(),
            Tag::Standard => "p_standard_mega_1".to_string(),
            Tag::Buffoon => "p_buffoon_mega_1".to_string(),
            _ => "p_arcana_normal_1".to_string(),
        }
    }

    /// Fill a pack the way Card:open fills it.
    pub fn _open_pack(&mut self, spec: PackSpec, _free: bool) {
        self.pack = Some(spec);
        self.pack_picks_left = spec.picks;
        let played: Vec<String> = self
            .hand_levels
            .plays
            .iter()
            .filter(|(_, n)| **n > 0)
            .map(|(h, _)| h.label().to_string())
            .collect();
        let owned: Vec<String> = self
            .full_deck
            .iter()
            .map(|c| format!("m_{}", crate::cards::enhancement_of(c).as_str()))
            .collect();
        let seen = self.seen_centers();
        let seen_vec: Vec<String> = seen.iter().cloned().collect();
        let showman = self
            .active_jokers()
            .iter()
            .any(|j| j.borrow().spec.allows_duplicates);
        let flags: Vec<String> = self.pool_flags.iter().cloned().collect();
        let stickers = self.sticker_rules();
        let sticker_opts = crate::shop_pool::StickerOptions {
            eternals: stickers.eternals,
            perishables: stickers.perishables,
            rentals: stickers.rentals,
        };
        let telescope = self.vouchers.iter().any(|v| v.key == "v_telescope");
        let omen_globe = self.vouchers.iter().any(|v| v.key == "v_omen_globe");
        let most_played_planet = self._most_played_planet();
        let soul_used = seen.contains("c_soul");
        let black_hole_used = seen.contains("c_black_hole");
        let edition_rate = self.edition_rate();
        let ante = self.ante;
        let contents = crate::shop_pool::pack_contents(
            &mut self.rng,
            spec.kind.title(),
            spec.options,
            ante,
            &played,
            &seen_vec,
            showman,
            &owned,
            &seen_vec,
            &flags,
            soul_used,
            black_hole_used,
            telescope,
            omen_globe,
            most_played_planet.as_deref(),
            Some(&sticker_opts),
            edition_rate,
        );

        self.pack_options = contents
            .iter()
            .map(|entry| self._pack_card(entry))
            .collect();

        // The open_booster jokers come after the pack is filled. A copied
        // Hallucination rolls again.
        let hooks = self.calculating_hooks("on_pack_open");
        for (joker, spec) in hooks {
            if let Some(answer) = spec.on_pack_open {
                answer(&joker, self);
            }
        }
        self.phase = Phase::Pack;

        // An Arcana or a Spectral pack deals a hand. Its cards need targets.
        if matches!(spec.kind, PackKind::Arcana | PackKind::Spectral) && self.hand.is_empty() {
            self.pack_dealt_hand = true;
            self._draw_to_hand_size();
        }
    }

    /// The Planet for the hand this run has played most, for Telescope.
    pub fn _most_played_planet(&self) -> Option<String> {
        // Every hand is scanned. This used `.any()`, which stops at the first
        // hand with a play -- and HANDLIST runs strongest-first, so a run whose
        // strongest played hand was not its most played got the wrong planet:
        // recording 16 played Pair nine times and Four of a Kind once, and
        // Telescope forced Mars where the game forced Mercury. Python's `max`
        // over HANDLIST keeps the first maximum, which the strict `>` below is.
        let mut best = HandType::HighCard;
        let mut best_plays = -1;
        let mut any = false;
        for h in HANDLIST {
            let n = self.hand_levels.played(h);
            if n > 0 {
                any = true;
            }
            if n > best_plays {
                best_plays = n;
                best = h;
            }
        }
        if !any {
            return None;
        }
        let name = planet_for_hand(best);
        crate::shop_pool::key_by_consumable_name(name).map(|s| s.to_string())
    }
}

impl GameState {
    /// One entry from `shop_pool.pack_contents`, as a simulator object.
    pub fn _pack_card(&mut self, entry: &crate::shop_pool::PackCard) -> PackChoice {
        if entry.set == "Joker" {
            let name = entry
                .key
                .as_ref()
                .and_then(|key| crate::shop_pool::name_by_joker_key(key))
                .unwrap_or("Joker");
            let spec = crate::jokers::spec_or_panic(name);
            let edition = edition_from_name(entry.edition.unwrap_or("none"));
            let mut instance = crate::jokers::JokerInstance::new(spec);
            instance.edition = edition;
            instance.eternal = entry.eternal;
            instance.perishable = entry.perishable;
            instance.rental = entry.rental;
            if instance.perishable {
                instance.perish_tally = PERISHABLE_ROUNDS;
            }
            let joker = self._made_joker(crate::jokers::make_ref(instance));
            return PackChoice::Joker(joker);
        }
        if entry.set == "Playing" {
            let suit = entry.suit.clone().unwrap_or_else(|| "S".to_string());
            let rank = entry.rank.clone().unwrap_or_else(|| "2".to_string());
            let card = front_to_card(&format!("{}_{}", suit, rank));
            if let Some(enh) = &entry.enhancement {
                crate::cards::set_enhancement(&card, enhancement_from_key(enh));
            }
            card.borrow_mut().edition = edition_from_name(entry.edition.unwrap_or("none"));
            if let Some(seal) = &entry.seal {
                card.borrow_mut().seal = seal_from_name(&seal.to_lowercase());
            }
            return PackChoice::Card(card);
        }
        let key = entry.key.as_deref().unwrap_or("");
        let name = crate::shop_pool::name_by_consumable_key(key).unwrap_or("");
        PackChoice::Consumable(crate::consumables::spec_or_panic(name))
    }

    /// Close a pack, sending a dealt hand back to the deck.
    pub fn _close_pack(&mut self) {
        self.pack = None;
        self.pack_options = Vec::new();
        self.pack_picks_left = 0;
        if self.pack_dealt_hand {
            // draw_from_hand_to_deck: the hand goes back a card at a time from
            // the front, each emplaced at the deck's front.
            for card in self.hand.clone() {
                self.draw_pile.insert(0, card);
            }
            self.hand = Vec::new();
            self.pack_dealt_hand = false;
        }
        self.phase = if self.shop.is_some() {
            Phase::Shop
        } else {
            Phase::BlindSelect
        };
        // end_consumeable offers the tags a new blind choice as the pack closes.
        self._new_blind_choice();
    }

    /// Take one card out of a pack.
    pub fn _pick_pack(&mut self, index: usize, card_indices: &[usize]) {
        let choice = self.pack_options.remove(index);
        match choice {
            PackChoice::Joker(joker) => {
                self.gain_joker(&joker);
                self.log(format!("Pack: took {}", joker.borrow().name()));
            }
            PackChoice::Card(card) => {
                self.add_card(&card);
                self.log(format!(
                    "Pack: added {} to deck",
                    crate::cards::label_of(&card)
                ));
            }
            PackChoice::Consumable(spec) => {
                // A consumable taken from a pack is used there and then.
                let targets: Vec<CardRef> = card_indices
                    .iter()
                    .filter(|&&i| i < self.hand.len())
                    .map(|&i| self.hand[i].clone())
                    .collect();
                self.use_consumable(spec, &targets, true);
            }
        }
        self.pack_picks_left -= 1;
        if self.pack_picks_left <= 0 || self.pack_options.is_empty() {
            self._close_pack();
        }
    }
}

// --------------------------------------------------------------------------
// shop
// --------------------------------------------------------------------------
impl GameState {
    pub fn _shop_slot_count(&self) -> i32 {
        2 + self.vouchers.iter().map(|v| v.shop_slots).sum::<i32>()
    }

    /// The run's card-type rates, which the deck and vouchers move.
    ///
    /// A voucher *sets* its rate rather than adding to it, so the last one
    /// redeemed wins, which for an upgrade is always the bigger.
    pub fn _shop_rates(&self) -> HashMap<String, f64> {
        let mut rates = crate::shop_pool::base_rates();
        let spectral = self.deck_config().spectral_rate as f64;
        *rates.entry("Spectral".to_string()).or_insert(0.0) += spectral;
        for voucher in &self.vouchers {
            if voucher.tarot_rate != 0.0 {
                rates.insert("Tarot".to_string(), voucher.tarot_rate);
            }
            if voucher.planet_rate != 0.0 {
                rates.insert("Planet".to_string(), voucher.planet_rate);
            }
            if voucher.playing_card_rate != 0.0 {
                rates.insert("Base".to_string(), voucher.playing_card_rate);
            }
        }
        rates
    }

    /// One shop slot, rolled the way the game rolls it.
    pub fn _roll_slot(&mut self) -> ShopSlot {
        let played: Vec<String> = self
            .hand_levels
            .plays
            .iter()
            .filter(|(_, n)| **n > 0)
            .map(|(h, _)| h.label().to_string())
            .collect();
        let owned: Vec<String> = self
            .full_deck
            .iter()
            .map(|c| format!("m_{}", crate::cards::enhancement_of(c).as_str()))
            .collect();
        let seen = self.seen_centers();
        let seen_vec: Vec<String> = seen.iter().cloned().collect();
        let showman = self
            .active_jokers()
            .iter()
            .any(|j| j.borrow().spec.allows_duplicates);
        let flags: Vec<String> = self.pool_flags.iter().cloned().collect();
        let rates = self._shop_rates();
        let ante = self.ante;
        let (kind, key) = crate::shop_pool::draw_shop_card(
            &mut self.rng,
            ante,
            Some(&rates),
            &owned,
            &seen_vec,
            showman,
            &flags,
            &played,
        );

        // Illusion's first roll costs a draw on *every* slot, whatever the slot
        // turns out to be. The game decides Enhanced-or-Base inside the table of
        // candidate types, which Lua builds in full before picking from it.
        let illusion = self.vouchers.iter().any(|v| v.key == "v_illusion");
        let enhanced = illusion && self.rng.pseudorandom("illusion", None, None) > 0.6;

        if kind == "Joker" {
            let name = crate::shop_pool::name_by_joker_key(&key)
                .unwrap_or_else(|| panic!("no joker key {:?} in the registry", key));
            let spec = crate::jokers::spec_or_panic(name);
            // "edi" + the append + the ante.
            let edition_key = format!("edi{}{}", crate::shop_pool::SHOP_APPEND, ante);
            let rate = self.edition_rate();
            let edition = edition_from_name(crate::shop_pool::poll_edition(
                &mut self.rng,
                &edition_key,
                1.0,
                false,
                rate,
                false,
            ));
            let mut instance = crate::jokers::JokerInstance::new(spec);
            instance.edition = edition;
            let joker = self._made_joker(crate::jokers::make_ref(instance));
            self._apply_stickers(&joker, false);
            let mut slot = ShopSlot::new("joker", spec.cost);
            slot.joker = Some(joker);
            return slot;
        }
        if kind == "Tarot" || kind == "Planet" || kind == "Spectral" {
            let name = crate::shop_pool::name_by_consumable_key(&key)
                .unwrap_or_else(|| panic!("no consumable key {:?} in the registry", key));
            let spec = crate::consumables::spec_or_panic(name);
            let mut slot = ShopSlot::new("consumable", spec.cost);
            slot.consumable = Some(spec);
            return slot;
        }
        // A playing card, which only appears once Magic Trick or Illusion has
        // raised the playing card rate.
        let mut enhancement = Enhancement::None;
        if enhanced {
            let enh_key = format!("Enhanced{}{}", crate::shop_pool::SHOP_APPEND, ante);
            let chosen = self.rng.choice(&enh_key, &crate::shop_pool::ENHANCEMENTS);
            enhancement = enhancement_from_key(chosen);
        }
        let front_key = format!("front{}{}", crate::shop_pool::SHOP_APPEND, ante);
        let front = self.rng.choice(&front_key, &crate::shop_pool::FRONTS);
        let card = front_to_card(front);
        crate::cards::set_enhancement(&card, enhancement);
        if illusion && self.rng.pseudorandom("illusion", None, None) > 0.8 {
            let roll = self.rng.pseudorandom("illusion", None, None);
            let edition = if roll > 1.0 - 0.15 {
                Edition::Polychrome
            } else if roll > 0.5 {
                Edition::Holographic
            } else {
                Edition::Foil
            };
            card.borrow_mut().edition = edition;
        }
        let mut slot = ShopSlot::new("card", 1);
        slot.card = Some(card);
        slot
    }
}

impl GameState {
    /// Let an edition tag claim a shop card as it is made.
    ///
    /// The tag puts the edition on and marks the card couponed, which sets its
    /// price to nothing. Only the first tag that applies fires, and only on a
    /// joker with no edition of its own.
    pub fn _modify_shop_slot(&mut self, mut slot: ShopSlot) -> ShopSlot {
        let eligible = match &slot.joker {
            Some(joker) => joker.borrow().edition == Edition::None,
            None => false,
        };
        if !eligible {
            return slot;
        }
        for tag in self.tags.clone() {
            let edition = EDITION_TAGS
                .iter()
                .find(|(t, _)| *t == tag)
                .map(|(_, e)| *e);
            let Some(edition) = edition else { continue };
            if let Some(i) = self.tags.iter().position(|t| *t == tag) {
                self.tags.remove(i);
            }
            if let Some(joker) = &slot.joker {
                joker.borrow_mut().edition = edition;
                let name = joker.borrow().name();
                self.log(format!(
                    "{}: {} is {}, free",
                    tag.label(),
                    name,
                    edition.as_str()
                ));
            }
            slot.couponed = true;
            break;
        }
        slot
    }

    /// Stock every slot of the shop.
    pub fn _fill_shop(&mut self) {
        if self.shop.is_none() {
            return;
        }
        let count = self._shop_slot_count();
        // Empty in place and push as each slot is made, not into a local it is
        // swapped in at the end: `seen_centers` blanks a pool with what exists,
        // and a slot that has just been built exists. Deferring the swap let a
        // reroll draw the same consumable twice (`c_star, c_star` where the game
        // had `c_star, c_heirophant`); the opening shop only escaped because
        // `_open_shop` sets the empty shelf before filling it. See game.py:3509.
        if let Some(shop) = &mut self.shop {
            shop.slots.clear();
        }
        for _ in 0..count {
            let slot = match self._forced_shop_slot() {
                Some(forced) => forced,
                None => self._roll_slot(),
            };
            let slot = self._modify_shop_slot(slot);
            if let Some(shop) = &mut self.shop {
                shop.slots.push(slot);
            }
        }
    }

    /// One shop pack, rolled from the game's own Booster pool.
    pub fn _roll_pack(&mut self) -> PackSpec {
        let first = !self.first_shop_buffoon;
        self.first_shop_buffoon = true;
        let ante = self.ante;
        let row = crate::shop_pool::draw_pack(&mut self.rng, ante, first, "shop_pack");
        pack_from_row(row)
    }
}

impl GameState {
    /// A shop slot an Uncommon or Rare Tag fills instead of a roll.
    ///
    /// The tag does not hand the player a joker -- it puts a free one in the shop,
    /// in place of a card that is then never rolled.
    pub fn _forced_shop_slot(&mut self) -> Option<ShopSlot> {
        for tag in self.tags.clone() {
            if tag != Tag::Uncommon && tag != Tag::Rare {
                continue;
            }
            let (rarity, append) = if tag == Tag::Uncommon {
                (2u8, "uta")
            } else {
                (3u8, "rta")
            };
            if let Some(i) = self.tags.iter().position(|t| *t == tag) {
                self.tags.remove(i);
            }
            let owned: Vec<String> = self
                .full_deck
                .iter()
                .map(|c| format!("m_{}", crate::cards::enhancement_of(c).as_str()))
                .collect();
            let seen = self.seen_centers();
            let seen_vec: Vec<String> = seen.iter().cloned().collect();
            let showman = self
                .active_jokers()
                .iter()
                .any(|j| j.borrow().spec.allows_duplicates);
            let flags: Vec<String> = self.pool_flags.iter().cloned().collect();
            let ante = self.ante;
            let key = crate::shop_pool::draw_joker(
                &mut self.rng,
                ante,
                &owned,
                &seen_vec,
                showman,
                Some(rarity),
                &flags,
                append,
            );
            let name = crate::shop_pool::name_by_joker_key(&key)
                .unwrap_or_else(|| panic!("no joker key {:?} in the registry", key));
            let spec = crate::jokers::spec_or_panic(name);
            let edition_key = format!("edi{}{}", append, ante);
            let rate = self.edition_rate();
            let edition = edition_from_name(crate::shop_pool::poll_edition(
                &mut self.rng,
                &edition_key,
                1.0,
                false,
                rate,
                false,
            ));
            let mut instance = crate::jokers::JokerInstance::new(spec);
            instance.edition = edition;
            let joker = self._made_joker(crate::jokers::make_ref(instance));
            self._apply_stickers(&joker, false);
            let mut slot = ShopSlot::new("joker", spec.cost);
            slot.couponed = true;
            slot.joker = Some(joker);
            return Some(slot);
        }
        None
    }

    /// The tags that fire as the shop opens; none are modelled here yet.
    pub fn _apply_shop_tags(&mut self) {}

    /// What a shop slot costs right now.
    pub fn slot_price(&self, slot: &ShopSlot) -> i32 {
        if slot.couponed {
            return 0;
        }
        if let Some(spec) = slot.consumable {
            if spec.kind == ConsumableKind::Planet
                && self
                    .active_jokers()
                    .iter()
                    .any(|j| j.borrow().spec.free_planets)
            {
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
        self.card_cost(slot.base_cost, edition)
    }

    /// What a booster costs right now -- the price every caller must use.
    ///
    /// Card:set_cost zeroes a Celestial booster while Astronomer is held, exactly
    /// as it zeroes a Planet.
    pub fn pack_price(&self, pack: &PackSpec) -> i32 {
        if self.shop_free {
            return 0;
        }
        if pack.kind == PackKind::Celestial
            && self
                .active_jokers()
                .iter()
                .any(|j| j.borrow().spec.free_planets)
        {
            return 0;
        }
        self.price(pack.cost)
    }
}

impl GameState {
    /// Open the shop: the row, its packs, its voucher, and the tags.
    pub fn _open_shop(&mut self) {
        let mut shop = Shop::default();
        shop.free_rerolls = self
            .active_jokers()
            .iter()
            .map(|j| j.borrow().spec.free_rerolls)
            .sum();
        // Set before it is filled, not after: seen_centers blanks a pool with what
        // exists, and a card that has just been built exists.
        self.shop = Some(shop);
        self._fill_shop();

        // The Coupon Tag, over the stocked shop and only as it opens.
        if let Some(i) = self.tags.iter().position(|t| *t == Tag::Coupon) {
            self.tags.remove(i);
            self.shop_free = true;
            if let Some(shop) = &mut self.shop {
                for slot in &mut shop.slots {
                    slot.couponed = true;
                }
            }
            self.log("Coupon Tag: the shop is free");
        }

        let packs: Vec<PackSpec> = (0..2).map(|_| self._roll_pack()).collect();
        if let Some(shop) = &mut self.shop {
            shop.packs = packs;
        }

        // The ante's voucher, unless it has already been taken.
        let redeemed: Vec<String> = self.vouchers.iter().map(|v| v.key.to_string()).collect();
        let mut vs: Vec<crate::shop::Voucher> = Vec::new();
        if !self.round_voucher.is_empty() && !redeemed.contains(&self.round_voucher) {
            if let Some(v) = voucher_by_key(&self.round_voucher) {
                vs.push(v);
            }
        }
        if let Some(shop) = &mut self.shop {
            shop.vouchers = vs;
        }

        // Every Voucher Tag held adds one more, drawn under 'Voucher_fromtag'.
        while let Some(i) = self.tags.iter().position(|t| *t == Tag::Voucher) {
            self.tags.remove(i);
            let redeemed: Vec<String> = self.vouchers.iter().map(|v| v.key.to_string()).collect();
            let on_offer: Vec<String> = self
                .shop
                .as_ref()
                .map(|s| s.vouchers.iter().map(|v| v.key.to_string()).collect())
                .unwrap_or_default();
            let key = crate::shop_pool::draw_voucher(
                &mut self.rng,
                self.ante,
                &redeemed,
                &on_offer,
                true,
            );
            if let Some(v) = voucher_by_key(&key) {
                let name = v.name;
                if let Some(shop) = &mut self.shop {
                    shop.vouchers.push(v);
                }
                self.log(format!("Voucher Tag: {} as well", name));
            }
        }

        // The D6 Tag makes this shop's rerolls free from the first one.
        if let Some(i) = self.tags.iter().position(|t| *t == Tag::DSix) {
            self.tags.remove(i);
            if let Some(shop) = &mut self.shop {
                shop.free_reroll_cost = true;
            }
            self.temp_reroll_cost = true;
            self.log("D6 Tag: rerolls start at nothing");
        }
        self.phase = Phase::Shop;
        self._apply_shop_tags();
    }

    /// What redeeming does beyond the fields read off `self.vouchers`.
    pub fn _redeem_voucher(&mut self, voucher: crate::shop::Voucher) {
        if voucher.shop_slots != 0 && self.shop.is_some() {
            // change_shop_size fills *every* empty slot, not just the one it
            // added.
            loop {
                let current = self.shop.as_ref().map(|s| s.slots.len()).unwrap_or(0);
                if current >= self._shop_slot_count().max(0) as usize {
                    break;
                }
                let slot = match self._forced_shop_slot() {
                    Some(forced) => forced,
                    None => self._roll_slot(),
                };
                let slot = self._modify_shop_slot(slot);
                if let Some(shop) = &mut self.shop {
                    shop.slots.push(slot);
                }
            }
        }
        if voucher.ante_shift != 0 {
            // No floor. ease_ante is a bare addition, and the ante really does go
            // to zero and below -- two Hieroglyphs at ante one leave it at minus
            // one, with get_blind_amount returning 100 for anything under one.
            self.ante += voucher.ante_shift;
        }
        self.hands_left += voucher.extra_hands;
        self.discards_left += voucher.extra_discards;
    }

    /// Leave the shop: the shop-end pass, then the next blind choice.
    pub fn _leave_shop(&mut self) {
        let hooks = self.calculating_hooks("on_shop_end");
        for (joker, spec) in hooks {
            if let Some(answer) = spec.on_shop_end {
                answer(&joker, self);
            }
        }
        if let Some(shop) = &self.shop {
            self.free_rerolls_carried = shop.free_rerolls;
            let discount: i32 = self.vouchers.iter().map(|v| v.reroll_discount).sum();
            self.reroll_price_carried = shop.reroll_price(discount);
        }
        self.shop = None;
        self._next_blind();
    }
}

// --------------------------------------------------------------------------
// consumables: use, buy-and-use, forced cards
// --------------------------------------------------------------------------
impl GameState {
    /// Cerulean Bell's card, while it is in force and in the hand.
    pub fn held_forced_card(&self) -> Option<CardRef> {
        if self.phase != Phase::Playing {
            return None;
        }
        let boss = self.boss()?;
        if !boss.forces_a_card {
            return None;
        }
        let card = self.forced_card.clone()?;
        if !self.hand.iter().any(|c| uid_of(c) == uid_of(&card)) {
            return None;
        }
        Some(card)
    }

    /// Whether a selection holds Cerulean Bell's card, when one is forced.
    pub fn _takes_forced_card(&self, targets: &[CardRef]) -> bool {
        match self.held_forced_card() {
            None => true,
            Some(card) => targets.iter().any(|t| uid_of(t) == uid_of(&card)),
        }
    }

    /// Card:check_use -- the one card the game refuses at the last moment.
    ///
    /// Ankh is the only entry, and it disagrees with the check that enables the
    /// button: can_use_consumeable asks only for a joker and a limit above one,
    /// while this asks for a *free slot*.
    pub fn refuses_use(&self, spec: &ConsumableSpec) -> bool {
        spec.name == "Ankh" && self.jokers.len() as i32 >= self.joker_slots()
    }

    /// Whether pressing use actually uses the card: the button, then check_use.
    pub fn _usable_now(&self, spec: &ConsumableSpec, targets: &[CardRef]) -> bool {
        self.can_use_consumable(spec, targets) && !self.refuses_use(spec)
    }

    /// Card:can_use_consumeable, branch for branch.
    pub fn can_use_consumable(&self, spec: &ConsumableSpec, targets: &[CardRef]) -> bool {
        // A hand to work on: in a round, or inside a pack that dealt one.
        let hand_dealt = self.phase == Phase::Playing || self.phase == Phase::Pack;
        let plain_jokers = self
            .jokers
            .iter()
            .filter(|j| j.borrow().edition == Edition::None)
            .count();

        if crate::game::FREE_JOKER_NEEDED.contains(&spec.name) {
            return (self.jokers.len() as i32) < self.joker_slots();
        }
        if crate::game::FREE_CONSUMABLE_NEEDED.contains(&spec.name) {
            // `or self.area == G.consumeables`: using it frees the slot it is
            // sitting in, so holding it is always enough.
            let room = (self.consumables.len() as i32) < self.consumable_slots()
                || self
                    .consumables
                    .iter()
                    .any(|c| std::ptr::eq(c.borrow().spec, spec));
            if spec.name != "The Fool" {
                return room;
            }
            return room
                && !self.last_tarot_planet.is_empty()
                && self.last_tarot_planet != "c_fool";
        }
        if crate::game::PLAIN_JOKER_NEEDED.contains(&spec.name) {
            return plain_jokers > 0;
        }
        if spec.name == "Ankh" {
            // Deliberately not a free slot -- that is check_use, and the
            // disagreement between the two is the Ankh bug.
            return !self.jokers.is_empty() && self.joker_slots() > 1;
        }
        if spec.name == "Aura" {
            return hand_dealt
                && targets.len() == 1
                && targets[0].borrow().edition == Edition::None
                && self._takes_forced_card(targets);
        }
        if crate::game::SPARE_CARD_NEEDED.contains(&spec.name) {
            // They destroy a card at random, and the game will not let the hand go
            // empty that way.
            return hand_dealt && self.hand.len() > 1;
        }
        if spec.targets > 0 {
            return hand_dealt
                && spec.accepts(targets.len() as i32)
                && self._takes_forced_card(targets);
        }
        true
    }

    /// Apply a consumable and remember it if it was a Tarot or a Planet.
    pub fn use_consumable(
        &mut self,
        spec: &'static ConsumableSpec,
        targets: &[CardRef],
        from_pack: bool,
    ) {
        if let Some(apply) = spec.apply {
            // An effect that needs to know reads it off the game.
            self.using_from_pack = from_pack;
            self.using_key = crate::shop_pool::key_by_consumable_name(spec.name)
                .unwrap_or("")
                .to_string();
            self.playing_tarot = true;
            apply(self, targets);
            self.using_from_pack = false;
            self.using_key = String::new();
            self.playing_tarot = false;
        }
        self.log(format!("Used {}", spec.name));
        if matches!(spec.kind, ConsumableKind::Tarot | ConsumableKind::Planet) {
            self.last_tarot_planet = crate::shop_pool::key_by_consumable_name(spec.name)
                .unwrap_or("")
                .to_string();
        }

        // G.GAME.consumeable_usage_total. Both counters were declared and never
        // written, so Fortune Teller and Constellation grew on nothing.
        if spec.kind == ConsumableKind::Tarot {
            self.tarots_used += 1;
        } else if spec.kind == ConsumableKind::Planet {
            self.planets_used += 1;
            self.unique_planets.insert(spec.name.to_string());
            for joker in self.calculating_jokers() {
                if joker.borrow().name() == "Constellation" {
                    joker.borrow_mut().counter += 0.1;
                }
            }
        }
    }
}

impl GameState {
    /// The shop's buy-and-use button, quirk included.
    ///
    /// The game charges for the card, takes it out of the shop, and -- on this
    /// path only -- never files it anywhere. If the use is then refused, the card
    /// has been paid for and belongs to no card area at all. It is gone.
    pub fn buy_and_use(&mut self, index: usize) {
        let slot = match self.shop.as_mut() {
            Some(shop) => shop.slots.remove(index),
            None => return,
        };
        let label = slot.label();
        let price = self.slot_price(&slot);
        self.add_money(-price, &format!("bought {}", label));
        match slot.consumable {
            None => {
                if let Some(joker) = slot.joker {
                    self.gain_joker(&joker);
                }
            }
            Some(spec) => {
                if self.refuses_use(spec) {
                    self.log(format!(
                        "{}: No Room -- bought, used by nothing, lost",
                        spec.name
                    ));
                    return;
                }
                self.use_consumable(spec, &[], false);
            }
        }
    }

    /// Buy one shop slot.
    pub fn _buy(&mut self, index: usize) {
        let slot = match self.shop.as_mut() {
            Some(shop) => shop.slots.remove(index),
            None => return,
        };
        let label = slot.label();
        let price = self.slot_price(&slot);
        self.add_money(-price, &format!("bought {}", label));
        if let Some(joker) = slot.joker {
            self.gain_joker(&joker);
        } else if let Some(spec) = slot.consumable {
            let held = self.hold_consumable(spec, Edition::None);
            held.borrow_mut().uid = slot.sort_id;
            self.consumables.push(held);
        } else if let Some(card) = slot.card {
            self.add_card(&card);
        }
    }
}

// --------------------------------------------------------------------------
// actions
// --------------------------------------------------------------------------
impl GameState {
    /// Moving a joker one place to its left.
    pub fn _swap_actions(&self) -> Vec<Action> {
        (1..self.jokers.len())
            .map(|i| Action::at(ActionType::SwapJokerLeft, i as i32))
            .collect()
    }

    /// Playable subsets, honouring boss restrictions where possible.
    ///
    /// A boss must never leave the player with nothing to do, so if its restriction
    /// rules out every hand the restriction is dropped for that decision rather
    /// than deadlocking the run.
    pub fn _play_actions(&self) -> Vec<Action> {
        let subsets = self._card_subsets(MAX_PLAYED);
        let strict: Vec<Action> = subsets
            .iter()
            .filter(|s| self._restriction_ok(s))
            .map(|s| Action::with_cards(ActionType::Play, s.clone()))
            .collect();
        if !strict.is_empty() {
            return strict;
        }
        subsets
            .into_iter()
            .map(|s| Action::with_cards(ActionType::Play, s))
            .collect()
    }

    /// The actions a consumable in a pack offers.
    pub fn _pack_consumable_actions(&self, index: usize, spec: &ConsumableSpec) -> Vec<Action> {
        if spec.targets == 0 {
            // A consumable in a pack gets its button from can_use_consumeable, so a
            // Judgement is not takeable into a full joker row.
            if !self._usable_now(spec, &[]) {
                return Vec::new();
            }
            return vec![Action::at(ActionType::PickPack, index as i32)];
        }
        if self.phase == Phase::Pack && self.hand.is_empty() {
            // Opened from the shop: bank it instead of applying it now.
            if (self.consumables.len() as i32) < self.consumable_slots() {
                return vec![Action::at(ActionType::PickPack, index as i32)];
            }
            return Vec::new();
        }
        let max = spec.max_targets.unwrap_or(spec.targets).max(0) as usize;
        self._card_subsets(max)
            .into_iter()
            .filter(|s| spec.accepts(s.len() as i32))
            .map(|s| Action {
                r#type: ActionType::PickPack,
                index: index as i32,
                cards: s,
            })
            .collect()
    }

    /// The actions a held consumable offers.
    pub fn _consumable_actions(&self) -> Vec<Action> {
        let mut actions = Vec::new();
        for (i, held) in self.consumables.iter().enumerate() {
            let spec = held.borrow().spec;
            if spec.targets == 0 {
                if self._usable_now(spec, &[]) {
                    actions.push(Action::at(ActionType::UseConsumable, i as i32));
                }
                continue;
            }
            if self.hand.is_empty() {
                continue;
            }
            let max = spec.max_targets.unwrap_or(spec.targets).max(0) as usize;
            for subset in self._card_subsets(max) {
                if !spec.accepts(subset.len() as i32) {
                    continue;
                }
                let targets: Vec<CardRef> = subset.iter().map(|&j| self.hand[j].clone()).collect();
                if self._usable_now(spec, &targets) {
                    actions.push(Action {
                        r#type: ActionType::UseConsumable,
                        index: i as i32,
                        cards: subset,
                    });
                }
            }
        }
        actions
    }

    /// Membership test for a consumable action.
    pub fn _consumable_legal(&self, index: usize, cards: &[usize]) -> bool {
        if index >= self.consumables.len() {
            return false;
        }
        let spec = self.consumables[index].borrow().spec;
        if !spec.accepts(cards.len() as i32) {
            return false;
        }
        if spec.targets == 0 {
            if !cards.is_empty() {
                return false;
            }
        } else {
            let max = spec.max_targets.unwrap_or(spec.targets).max(0) as usize;
            if !self._valid_indices(cards, max, false) {
                return false;
            }
        }
        let targets: Vec<CardRef> = cards.iter().map(|&i| self.hand[i].clone()).collect();
        self._usable_now(spec, &targets)
    }
}

impl GameState {
    /// Every action available in the current phase -- the RL action mask.
    pub fn legal_actions(&self) -> Vec<Action> {
        match self.phase {
            Phase::RoundEval => {
                // Take the money, or spend what you are holding first.
                let mut actions = vec![Action::new(ActionType::CashOut)];
                actions.extend(self._consumable_actions());
                actions.extend(
                    (0..self.consumables.len())
                        .map(|i| Action::at(ActionType::SellConsumable, i as i32)),
                );
                actions.extend(
                    self.jokers
                        .iter()
                        .enumerate()
                        .filter(|(_, j)| !j.borrow().eternal)
                        .map(|(i, _)| Action::at(ActionType::SellJoker, i as i32)),
                );
                actions.extend(self._swap_actions());
                actions
            }
            Phase::BlindSelect => {
                let mut actions = vec![Action::new(ActionType::SelectBlind)];
                if self
                    .blind
                    .as_ref()
                    .is_some_and(|b| b.kind != BlindKind::Boss)
                {
                    actions.push(Action::new(ActionType::SkipBlind));
                }
                // The Director's Cut / Retcon button.
                if self.can_reroll_boss() {
                    actions.push(Action::new(ActionType::RerollBoss));
                }
                actions
            }
            Phase::Playing => {
                let mut actions = self._play_actions();
                if self.discards_left > 0 {
                    // Cerulean Bell keeps its card highlighted, so a *discard*
                    // cannot go without it either. No escape here, unlike the
                    // plays: a hand that cannot be discarded can always be played.
                    actions.extend(
                        self._card_subsets(MAX_PLAYED)
                            .iter()
                            .filter(|s| self._restriction_ok(s))
                            .map(|s| Action::with_cards(ActionType::Discard, s.clone())),
                    );
                }
                actions.extend(self._consumable_actions());
                actions.extend(
                    self.jokers
                        .iter()
                        .enumerate()
                        .filter(|(_, j)| !j.borrow().eternal)
                        .map(|(i, _)| Action::at(ActionType::SellJoker, i as i32)),
                );
                actions.extend(self._swap_actions());
                actions
            }
            Phase::Shop => self._shop_actions(),
            Phase::Pack => self._pack_actions(),
            _ => Vec::new(),
        }
    }
}

impl GameState {
    /// The shop's action list, split out of `legal_actions`.
    fn _shop_actions(&self) -> Vec<Action> {
        let mut actions = vec![Action::new(ActionType::LeaveShop)];
        if let Some(shop) = &self.shop {
            for (i, slot) in shop.slots.iter().enumerate() {
                if !self.affords(self.slot_price(slot)) {
                    continue;
                }
                if slot.kind == "joker"
                    && !slot.joker.as_ref().is_some_and(|j| self.room_for_joker(j))
                {
                    continue;
                }
                if slot.kind == "consumable"
                    && (self.consumables.len() as i32) >= self.consumable_slots()
                {
                    continue;
                }
                actions.push(Action::at(ActionType::Buy, i as i32));
            }
            for (i, pack) in shop.packs.iter().enumerate() {
                if self.affords(self.pack_price(pack)) {
                    actions.push(Action::at(ActionType::BuyPack, i as i32));
                }
            }
            for (i, voucher) in shop.vouchers_on_offer().iter().enumerate() {
                if self.affords(self.price(voucher.cost)) {
                    actions.push(Action::at(ActionType::BuyVoucher, i as i32));
                }
            }
            let discount: i32 = self.vouchers.iter().map(|v| v.reroll_discount).sum();
            if self.affords(shop.reroll_cost(discount)) {
                actions.push(Action::new(ActionType::Reroll));
            }
        }
        actions.extend(self._consumable_actions());
        actions.extend(
            self.jokers
                .iter()
                .enumerate()
                .filter(|(_, j)| !j.borrow().eternal)
                .map(|(i, _)| Action::at(ActionType::SellJoker, i as i32)),
        );
        actions.extend(self._swap_actions());
        actions.extend(
            (0..self.consumables.len()).map(|i| Action::at(ActionType::SellConsumable, i as i32)),
        );
        actions
    }

    /// The pack's action list, split out of `legal_actions`.
    fn _pack_actions(&self) -> Vec<Action> {
        let mut actions = vec![Action::new(ActionType::SkipPack)];
        for (i, option) in self.pack_options.iter().enumerate() {
            match option {
                PackChoice::Joker(joker) => {
                    if self.room_for_joker(joker) {
                        actions.push(Action::at(ActionType::PickPack, i as i32));
                    }
                }
                PackChoice::Card(_) => {
                    actions.push(Action::at(ActionType::PickPack, i as i32));
                }
                PackChoice::Consumable(spec) => {
                    actions.extend(self._pack_consumable_actions(i, spec));
                }
            }
        }
        // An open booster does not lock the row.
        actions.extend(
            self.jokers
                .iter()
                .enumerate()
                .filter(|(_, j)| !j.borrow().eternal)
                .map(|(i, _)| Action::at(ActionType::SellJoker, i as i32)),
        );
        actions.extend(
            (0..self.consumables.len()).map(|i| Action::at(ActionType::SellConsumable, i as i32)),
        );
        actions.extend(self._swap_actions());
        actions
    }
}

impl GameState {
    /// Exact membership test for `legal_actions()` without building the list.
    pub fn is_legal(&self, action: &Action) -> bool {
        let t = action.r#type;
        let index = action.index;
        let cards = &action.cards;

        // Before the phases, because the row can be rearranged in any of them.
        if t == ActionType::SwapJokerLeft {
            return index >= 1 && (index as usize) < self.jokers.len();
        }

        match self.phase {
            Phase::RoundEval => match t {
                ActionType::CashOut => true,
                ActionType::UseConsumable => self._consumable_legal(index as usize, cards),
                ActionType::SellConsumable => {
                    index >= 0 && (index as usize) < self.consumables.len()
                }
                ActionType::SellJoker => {
                    index >= 0
                        && (index as usize) < self.jokers.len()
                        && !self.jokers[index as usize].borrow().eternal
                }
                _ => false,
            },
            Phase::BlindSelect => match t {
                ActionType::SelectBlind => cards.is_empty(),
                ActionType::SkipBlind => self
                    .blind
                    .as_ref()
                    .is_some_and(|b| b.kind != BlindKind::Boss),
                ActionType::RerollBoss => self.can_reroll_boss(),
                _ => false,
            },
            Phase::Playing => match t {
                ActionType::Play => {
                    // In any order: a play names its cards in the order they score.
                    let mut sorted = cards.clone();
                    sorted.sort_unstable();
                    if !self._valid_indices(&sorted, MAX_PLAYED, true) {
                        return false;
                    }
                    self._restriction_ok(cards) || !self._restriction_satisfiable()
                }
                ActionType::Discard => {
                    self.discards_left > 0
                        && self._valid_indices(cards, MAX_PLAYED, true)
                        && self._restriction_ok(cards)
                }
                ActionType::UseConsumable => self._consumable_legal(index as usize, cards),
                ActionType::SellJoker => {
                    index >= 0
                        && (index as usize) < self.jokers.len()
                        && !self.jokers[index as usize].borrow().eternal
                }
                _ => false,
            },
            Phase::Shop => self._is_legal_shop(t, index, cards),
            Phase::Pack => self._is_legal_pack(t, index, cards),
            _ => false,
        }
    }
}

impl GameState {
    fn _is_legal_shop(&self, t: ActionType, index: i32, cards: &[usize]) -> bool {
        let shop = match &self.shop {
            Some(shop) => shop,
            None => return false,
        };
        match t {
            ActionType::LeaveShop => true,
            ActionType::Buy => {
                if index < 0 || (index as usize) >= shop.slots.len() {
                    return false;
                }
                let slot = &shop.slots[index as usize];
                if !self.affords(self.slot_price(slot)) {
                    return false;
                }
                if slot.kind == "joker" {
                    return slot.joker.as_ref().is_some_and(|j| self.room_for_joker(j));
                }
                if slot.kind == "consumable" {
                    return (self.consumables.len() as i32) < self.consumable_slots();
                }
                true
            }
            ActionType::BuyPack => {
                index >= 0
                    && (index as usize) < shop.packs.len()
                    && self.affords(self.pack_price(&shop.packs[index as usize]))
            }
            ActionType::BuyVoucher => {
                let offered = shop.vouchers_on_offer();
                index >= 0
                    && (index as usize) < offered.len()
                    && self.affords(self.price(offered[index as usize].cost))
            }
            ActionType::Reroll => {
                let discount: i32 = self.vouchers.iter().map(|v| v.reroll_discount).sum();
                self.affords(shop.reroll_cost(discount))
            }
            ActionType::UseConsumable => self._consumable_legal(index as usize, cards),
            ActionType::SellJoker => {
                index >= 0
                    && (index as usize) < self.jokers.len()
                    && !self.jokers[index as usize].borrow().eternal
            }
            ActionType::SellConsumable => index >= 0 && (index as usize) < self.consumables.len(),
            _ => false,
        }
    }

    fn _is_legal_pack(&self, t: ActionType, index: i32, cards: &[usize]) -> bool {
        match t {
            ActionType::SkipPack => true,
            ActionType::SellJoker => {
                index >= 0
                    && (index as usize) < self.jokers.len()
                    && !self.jokers[index as usize].borrow().eternal
            }
            ActionType::SellConsumable => index >= 0 && (index as usize) < self.consumables.len(),
            ActionType::PickPack => {
                if index < 0 || (index as usize) >= self.pack_options.len() {
                    return false;
                }
                match &self.pack_options[index as usize] {
                    PackChoice::Joker(joker) => cards.is_empty() && self.room_for_joker(joker),
                    PackChoice::Card(_) => cards.is_empty(),
                    PackChoice::Consumable(spec) => self
                        ._pack_consumable_actions(index as usize, spec)
                        .iter()
                        .any(|a| a.cards == cards),
                }
            }
            _ => false,
        }
    }
}

impl GameState {
    /// Apply one action.
    pub fn step(&mut self, action: &Action) {
        match action.r#type {
            ActionType::SelectBlind => self._start_round(),
            ActionType::SkipBlind => {
                let idx = self.blind_index as usize;
                let tag = self.ante_tags.get(idx).cloned().unwrap_or_default();
                let key = self.ante_tag_keys.get(idx).cloned().unwrap_or_default();
                // Counted before the tag is handed over, which is the game's own
                // order and the reason a Speed Tag pays for its own skip.
                self.blinds_skipped += 1;
                self.skipped_this_ante.insert(self.blind_index);
                self.add_tag_by_key(&key);
                let name = self.blind.as_ref().map(|b| b.name()).unwrap_or_default();
                let reward = if tag.is_empty() {
                    key.clone()
                } else {
                    tag.clone()
                };
                self.log(format!("Skipped {}, gained {}", name, reward));
                self._fire_immediate_tags();
                self.blind_index += 1;
                // _next_blind offers the new blind choice, which is the skip's own.
                self._next_blind();
            }
            ActionType::Play => self._play(action.cards.clone()),
            ActionType::Discard => self._discard(&action.cards),
            ActionType::UseConsumable => {
                let spec = self.consumables.remove(action.index as usize).borrow().spec;
                // Named out of hand order, the targets are dragged into it first,
                // exactly as an ordered play is.
                let arranged = self._arrange_play(&action.cards);
                let targets: Vec<CardRef> =
                    arranged.iter().map(|&i| self.hand[i].clone()).collect();
                self.use_consumable(spec, &targets, false);
            }
            ActionType::SwapJokerLeft => {
                let i = action.index as usize;
                self.jokers.swap(i - 1, i);
            }
            ActionType::SellJoker => {
                let pairs = effective_specs(&self.jokers);
                let (spec, source) = pairs[action.index as usize].clone();
                let joker = self.jokers.remove(action.index as usize);
                let answers = !joker.borrow().debuffed
                    && (Rc::ptr_eq(&source, &joker)
                        || !not_copied("on_sell").unwrap().contains(&spec.name));
                self._move_joker_counters(&joker, false);
                let value = self.sell_value(&joker);
                self.add_money(value, &format!("sold {}", joker.borrow().name()));
                self.note_card_sold();
                // Both only while the boss is in force.
                let in_round = self.phase == Phase::Playing;
                if spec.disables_boss_on_sell && answers && in_round {
                    let name = joker.borrow().name();
                    self.disable_blind(name);
                }
                if in_round && self.boss().is_some_and(|b| b.debuff_until_sale) {
                    // Verdant Leaf lifts the moment any joker is sold.
                    self.disable_blind("a joker was sold");
                }
                if let Some(hook) = spec.on_sell {
                    if answers {
                        hook(&source, self);
                    }
                }
            }
            ActionType::SellConsumable => {
                // Priced while it is still held: sell_card pays sell_cost before
                // the card leaves.
                let held = self.consumables[action.index as usize].clone();
                let price = self.consumable_sell_value(&held);
                self.consumables.remove(action.index as usize);
                self.add_money(price, &format!("sold {}", held.borrow().spec.name));
                self.note_card_sold();
            }
            ActionType::Buy => self._buy(action.index as usize),
            ActionType::BuyAndUse => self.buy_and_use(action.index as usize),
            ActionType::RerollBoss => {
                self.boss_rerolled = true;
                self.add_money(-BOSS_REROLL_COST, "boss reroll");
                self._reroll_boss_blind();
            }
            ActionType::BuyVoucher => self._step_buy_voucher(action.index as usize),
            ActionType::Reroll => self._step_reroll(),
            ActionType::BuyPack => self._step_buy_pack(action.index as usize),
            ActionType::PickPack => self._pick_pack(action.index as usize, &action.cards),
            ActionType::SkipPack => {
                let hooks = self.calculating_hooks("on_pack_skip");
                for (joker, spec) in hooks {
                    if let Some(answer) = spec.on_pack_skip {
                        answer(&joker, self);
                    }
                }
                self._close_pack();
            }
            ActionType::CashOut => self._cash_out(),
            ActionType::LeaveShop => self._leave_shop(),
        }
    }
}

impl GameState {
    fn _step_buy_voucher(&mut self, index: usize) {
        let voucher = match self.shop.as_mut() {
            Some(shop) => shop.vouchers.remove(index),
            None => return,
        };
        self.add_money(
            -self.price(voucher.cost),
            &format!("bought {}", voucher.name),
        );
        self.vouchers.push(voucher);
        // Any voucher redeemed takes the ante's off the later shelves.
        self.round_voucher = String::new();
        if let Some(shop) = &mut self.shop {
            shop.cut_until_reroll += voucher.reroll_discount;
        }
        self._redeem_voucher(voucher);
    }

    fn _step_reroll(&mut self) {
        if self.shop.is_none() {
            return;
        }
        let discount: i32 = self.vouchers.iter().map(|v| v.reroll_discount).sum();
        let cost = self.shop.as_ref().unwrap().reroll_cost(discount);
        self.add_money(-cost, "reroll");
        if let Some(shop) = &mut self.shop {
            if shop.free_rerolls > 0 {
                shop.free_rerolls -= 1;
            } else {
                shop.rerolls += 1;
            }
            shop.cut_until_reroll = 0;
        }
        // The jokers that count rerolls are told before the new cards are made.
        let hooks = self.calculating_hooks("on_reroll");
        for (joker, spec) in hooks {
            if let Some(answer) = spec.on_reroll {
                answer(&joker, self);
            }
        }
        self._fill_shop();
    }

    fn _step_buy_pack(&mut self, index: usize) {
        let pack = match self.shop.as_mut() {
            Some(shop) => shop.packs.remove(index),
            None => return,
        };
        let cost = self.pack_price(&pack);
        self.add_money(-cost, &format!("bought {}", pack.name()));
        self._open_pack(pack, false);
    }

    /// A one-line summary of the run, as Python's `summary`.
    pub fn summary(&self) -> String {
        let blind = self
            .blind
            .as_ref()
            .map(|b| b.name())
            .unwrap_or_else(|| "-".to_string());
        let target = self.blind.as_ref().map(|b| b.target).unwrap_or(0);
        format!(
            "ante {} {} | {} | ${} | {}/{} | hands {} discards {} | jokers {}",
            self.ante,
            blind,
            self.phase.as_str(),
            self.money,
            self.chips_scored,
            target,
            self.hands_left,
            self.discards_left,
            self.jokers.len()
        )
    }
}

// --------------------------------------------------------------------------
// previews -- read-only
// --------------------------------------------------------------------------

/// A deep copy of a joker instance, uid and all: Python's `copy.copy(j)`.
fn clone_joker_instance(inst: &crate::jokers::JokerInstance) -> crate::jokers::JokerInstance {
    let mut copy = crate::jokers::JokerInstance::new(inst.spec);
    copy.uid = inst.uid;
    copy.edition = inst.edition;
    copy.counter = inst.counter;
    copy.eternal = inst.eternal;
    copy.perishable = inst.perishable;
    copy.perish_tally = inst.perish_tally;
    copy.rental = inst.rental;
    copy.debuffed = inst.debuffed;
    copy.hands_at_create = inst.hands_at_create;
    copy.secondary = inst.secondary;
    copy.extra_sell_value = inst.extra_sell_value;
    copy.named_hand = inst.named_hand;
    copy.face_down = inst.face_down;
    copy
}

impl GameState {
    /// Score a candidate play without advancing the run.
    ///
    /// `mode` is how chance is treated. "roll" draws every chance off one
    /// throwaway stream, the same for every preview. "pessimistic" misses every
    /// chance and takes every range at its bottom. "expected" is the mean of four
    /// rolled previews off different streams.
    pub fn preview_score(&mut self, indices: &[usize], mode: &str) -> i64 {
        self.preview_play(indices, mode).0
    }

    /// The score and the joker row as scoring left it.
    pub fn preview_play(&mut self, indices: &[usize], mode: &str) -> (i64, Vec<JokerRef>) {
        let (score, jokers, _, _) = self._preview_mode(indices, mode);
        (score, jokers)
    }

    /// Rolled previews averaged for mode="expected".
    pub const EXPECTED_ROLLS: i32 = 4;

    pub fn _preview_mode(
        &mut self,
        indices: &[usize],
        mode: &str,
    ) -> (i64, Vec<JokerRef>, i64, i32) {
        if mode == "expected" {
            let mut runs = Vec::new();
            for k in 0..Self::EXPECTED_ROLLS {
                let salt = format!("_preview{}", k);
                runs.push(self._preview(indices, &salt, false, None));
            }
            let n = runs.len() as f64;
            let score = runs.iter().map(|r| r.0 as f64).sum::<f64>() / n;
            let dollars = runs.iter().map(|r| r.2 as f64).sum::<f64>() / n;
            return (
                score.round() as i64,
                runs[0].1.clone(),
                dollars.round() as i64,
                runs[0].3,
            );
        }
        if mode == "pessimistic" {
            return self._preview(indices, "_preview", true, None);
        }
        self._preview(indices, "_preview", false, None)
    }

    /// The score, the dollars earned while scoring, and the Lucky rolls.
    pub fn preview_outcome(&mut self, indices: &[usize]) -> (i64, i64, i32) {
        let (score, _, dollars, rolls) = self._preview(indices, "_preview", false, None);
        (score, dollars, rolls)
    }

    /// The score, and the money the play earns in expectation.
    pub fn preview_money(&mut self, indices: &[usize]) -> (i64, f64) {
        self.preview_expected = 0.0;
        let score = self._preview(indices, "_preview", false, None).0;
        (score, self.preview_expected)
    }

    /// The score, and the dollars the play earns while it scores.
    pub fn preview_value(&mut self, indices: &[usize], mode: &str) -> (i64, i64) {
        let (score, _, dollars, _) = self._preview_mode(indices, mode);
        (score, dollars)
    }
}

impl GameState {
    /// Score a candidate play without advancing the run.
    ///
    /// Joker counters and card enhancements that scoring would mutate are
    /// snapshotted and restored, and a throwaway RNG stands in so previewing does
    /// not consume the run's random stream. The row that comes back is the copies
    /// scoring ran on; the run's own row is put back untouched.
    pub fn _preview(
        &mut self,
        indices: &[usize],
        salt: &str,
        pessimistic: bool,
        hook_taken: Option<Vec<CardRef>>,
    ) -> (i64, Vec<JokerRef>, i64, i32) {
        let played: Vec<CardRef> = indices.iter().map(|&i| self.hand[i].clone()).collect();
        let mut held: Vec<CardRef> = self
            .hand
            .iter()
            .enumerate()
            .filter(|(i, _)| !indices.contains(i))
            .map(|(_, c)| c.clone())
            .collect();

        // The Hook takes two held cards before the hand scores, as `_play` does.
        let hook = self.boss().is_some_and(|b| b.discard_random_on_play > 0) && !held.is_empty();
        let mut hook_taken = hook_taken;
        if hook && hook_taken.is_none() {
            let mut pool = held.clone();
            pool.sort_by_key(|c| uid_of(c));
            let count = (self.boss().unwrap().discard_random_on_play as usize).min(pool.len());
            if pessimistic {
                // The pair whose loss costs the play most.
                let mut worst: Option<(i64, Vec<JokerRef>, i64, i32)> = None;
                for combo in card_index_subsets(pool.len(), count) {
                    if combo.len() != count {
                        continue;
                    }
                    let pair: Vec<CardRef> = combo.iter().map(|&i| pool[i].clone()).collect();
                    let result = self._preview(indices, salt, true, Some(pair));
                    if worst.as_ref().is_none_or(|w| result.0 < w.0) {
                        worst = Some(result);
                    }
                }
                return worst.unwrap_or((0, self.jokers.clone(), 0, 0));
            }
            let mut chooser = RunRng::new(format!("{}{}", self.seed, salt));
            let idx = chooser.sample_index("hook", pool.len(), count);
            hook_taken = Some(idx.iter().map(|&i| pool[i].clone()).collect());
        }
        if let Some(taken) = &hook_taken {
            held = held
                .into_iter()
                .filter(|c| !taken.iter().any(|t| uid_of(t) == uid_of(c)))
                .collect();
        }

        // Everything a scoring hook can write to that is not the joker it runs on.
        let cards: Vec<(CardRef, Enhancement, i32, Seal, Edition, bool)> = played
            .iter()
            .chain(held.iter())
            .map(|c| {
                let b = c.borrow();
                (
                    c.clone(),
                    b.enhancement,
                    b.extra_chips,
                    b.seal,
                    b.edition,
                    b.debuffed,
                )
            })
            .collect();
        let levels = self.hand_levels.levels.clone();
        let plays = self.hand_levels.plays.clone();
        let consumables = self.consumables.clone();
        let money = self.money;
        let triggered = self.blind.as_ref().map(|b| b.triggered).unwrap_or(false);
        // The play's own trigger, not the last real play's (`preview_trigger`).
        // Scoring sets The Flint's and a debuffed scoring card's half.
        if self.preview_trigger {
            if let Some(blind) = &mut self.blind {
                blind.triggered = false;
            }
        }

        let real_jokers = self.jokers.clone();
        self.jokers = real_jokers
            .iter()
            .map(|j| Rc::new(std::cell::RefCell::new(clone_joker_instance(&j.borrow()))))
            .collect();
        let mut throwaway = RunRng::new(format!("{}{}", self.seed, salt));
        // Python swaps `RunRng` for `PessimisticRng`; here the flag on the
        // throwaway reproduces its `pseudorandom` override. See `RunRng`.
        throwaway.pessimistic = pessimistic;
        let real_rng = std::mem::replace(&mut self.rng, throwaway);

        let result = self.evaluate_selection(&played);
        let out: (i64, Vec<JokerRef>, i64, i32);
        if self.hand_is_debuffed(result.hand, &played) {
            // A hand the boss zeroes scores nothing and runs no scoring joker. But
            // the `after` pass is outside that block and asked of every hand.
            let mut dollars = 0;
            if self.preview_trigger {
                // A refused hand sets the boss off and asks every joker under
                // context.debuffed_hand, as `_play_hand_score` does: Matador's
                // $8. The log lines the hooks write are not the run's.
                let logged = self.logs.len();
                if self.boss().is_some() {
                    if let Some(blind) = &mut self.blind {
                        blind.triggered = true;
                    }
                }
                for (spec, source) in calculating_specs(&self.jokers) {
                    if let Some(answer) = spec.on_debuffed_hand {
                        answer(&source, self);
                    }
                }
                self.logs.truncate(logged);
                dollars = (self.money - money) as i64;
                self.preview_expected = dollars as f64;
            }
            self._refused_hand_after_pass(&result, &played, &held);
            out = (0, self.jokers.clone(), dollars, 0);
        } else {
            // Counted before it scores, as the play counts it.
            *self.hand_levels.plays.entry(result.hand).or_insert(0) += 1;
            // The Arm takes the level off before the hand scores, as `_play` does.
            let mut arm = false;
            if self.boss().is_some_and(|b| b.level_down_played_hand) {
                let level = self.hand_levels.level(result.hand);
                arm = level > 1;
                self.hand_levels
                    .levels
                    .insert(result.hand, (level - 1).max(1));
            }
            // The Arm's and The Ox's half of the trigger, as `_play_hand_score`
            // sets it (The Ox's $0 itself is not previewed).
            if self.preview_trigger {
                let ox = self.boss().is_some_and(|b| {
                    b.zero_money_on_most_played && self._is_most_played(result.hand)
                });
                if arm || ox {
                    if let Some(blind) = &mut self.blind {
                        blind.triggered = true;
                    }
                }
            }
            let ctx = score_hand(self, &result, &played, &held);
            let dollars = (ctx.money_gained as i64) + ((self.money - money) as i64);
            self.preview_expected =
                (dollars as f64) - (ctx.chance_paid as f64) + ctx.chance_expected;
            out = (ctx.score(), self.jokers.clone(), dollars, ctx.lucky_rolls);
        }

        // Restore everything, deliberately -- Python's `finally`.
        for (card, enhancement, extra, seal, edition, debuffed) in &cards {
            let mut b = card.borrow_mut();
            b.enhancement = *enhancement;
            b.extra_chips = *extra;
            b.seal = *seal;
            b.edition = *edition;
            b.debuffed = *debuffed;
        }
        self.hand_levels.levels = levels;
        self.hand_levels.plays = plays;
        self.consumables = consumables;
        self.money = money;
        if let Some(blind) = &mut self.blind {
            blind.triggered = triggered;
        }
        self.jokers = real_jokers;
        self.rng = real_rng;
        out
    }
}

impl GameState {
    /// `(joker, spec)` for each joker that answers `hook`, copies included.
    ///
    /// The game calls the copied joker's own calculate_joker, so this yields the
    /// *copied* joker -- the one whose state the hook reads -- and drops a copy
    /// when the copied joker's branch says `not context.blueprint`.
    pub fn calculating_hooks(
        &self,
        hook: &str,
    ) -> Vec<(JokerRef, &'static crate::jokers::JokerSpec)> {
        let row = self.jokers.clone();
        let effective = effective_specs(&row);
        let shut = not_copied(hook);
        let mut out = Vec::new();
        for (owner, (spec, source)) in row.iter().zip(effective.iter()) {
            if owner.borrow().debuffed {
                continue;
            }
            if !Rc::ptr_eq(source, owner) {
                match shut {
                    None => continue,
                    Some(names) => {
                        if names.contains(&spec.name) {
                            continue;
                        }
                    }
                }
            }
            out.push((source.clone(), *spec));
        }
        out
    }

    /// Whether the Director's Cut / Retcon button is live.
    ///
    /// Without either voucher there is no button at all. Director's Cut allows one
    /// reroll an ante -- boss_rerolled remembers it, and reset_blinds clears it
    /// when a boss falls. Retcon allows any number. Either way you must be able to
    /// afford the ten dollars, measured against the debt floor rather than zero.
    pub fn can_reroll_boss(&self) -> bool {
        let owned: Vec<&str> = self.vouchers.iter().map(|v| v.key).collect();
        if !owned.contains(&"v_retcon") {
            if !(owned.contains(&"v_directors_cut") && !self.boss_rerolled) {
                return false;
            }
        }
        (self.money - self.bankrupt_at()) - BOSS_REROLL_COST >= 0
    }
}
