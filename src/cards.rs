//! Playing cards, enhancements, editions and seals.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

// Shared by cards, jokers and consumables; see `next_sort_id` below.
thread_local! {
    static NEXT_SORT_ID: Cell<u64> = const { Cell::new(0) };
}

/// The game's G.sort_id: one counter for everything it makes.
///
/// Card:init does
///
/// ```text
///     G.sort_id = (G.sort_id or 0) + 1
///     self.sort_id = G.sort_id
/// ```
///
/// and pseudorandom_element sorts a table by sort_id before indexing into it,
/// so a random draw over jokers picks by age and never by where they sit in the
/// row. Shared with the cards because the game shares it.
///
/// Thread-local rather than global: a `GameState` is used from one thread, and a
/// per-thread counter keeps two tests from interleaving their ids and shuffling
/// each other's decks. Within a thread the order is exactly the game's.
pub fn next_sort_id() -> u64 {
    NEXT_SORT_ID.with(|c| {
        let v = c.get();
        c.set(v + 1);
        v
    })
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum Suit {
    Spades,
    Hearts,
    Diamonds,
    Clubs,
}

impl Suit {
    pub const ALL: [Suit; 4] = [Suit::Spades, Suit::Hearts, Suit::Diamonds, Suit::Clubs];

    /// The game's one-letter name, `S`, `H`, `D`, `C`.
    pub fn as_str(self) -> &'static str {
        match self {
            Suit::Spades => "S",
            Suit::Hearts => "H",
            Suit::Diamonds => "D",
            Suit::Clubs => "C",
        }
    }

    /// Parse the game's one-letter code.
    pub fn from_code(code: &str) -> Option<Suit> {
        Some(match code {
            "S" => Suit::Spades,
            "H" => Suit::Hearts,
            "D" => Suit::Diamonds,
            "C" => Suit::Clubs,
            _ => return None,
        })
    }

    pub fn is_red(self) -> bool {
        matches!(self, Suit::Hearts | Suit::Diamonds)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
#[repr(u8)]
pub enum Rank {
    Two = 2,
    Three = 3,
    Four = 4,
    Five = 5,
    Six = 6,
    Seven = 7,
    Eight = 8,
    Nine = 9,
    Ten = 10,
    Jack = 11,
    Queen = 12,
    King = 13,
    Ace = 14,
}

impl Rank {
    pub const ALL: [Rank; 13] = [
        Rank::Two,
        Rank::Three,
        Rank::Four,
        Rank::Five,
        Rank::Six,
        Rank::Seven,
        Rank::Eight,
        Rank::Nine,
        Rank::Ten,
        Rank::Jack,
        Rank::Queen,
        Rank::King,
        Rank::Ace,
    ];

    pub fn value(self) -> u8 {
        self as u8
    }

    pub fn chips(self) -> i32 {
        if self == Rank::Ace {
            return 11;
        }
        (self as i32).min(10)
    }

    pub fn is_face(self) -> bool {
        matches!(self, Rank::Jack | Rank::Queen | Rank::King)
    }

    pub fn short(self) -> &'static str {
        match self {
            Rank::Ten => "T",
            Rank::Jack => "J",
            Rank::Queen => "Q",
            Rank::King => "K",
            Rank::Ace => "A",
            Rank::Two => "2",
            Rank::Three => "3",
            Rank::Four => "4",
            Rank::Five => "5",
            Rank::Six => "6",
            Rank::Seven => "7",
            Rank::Eight => "8",
            Rank::Nine => "9",
        }
    }

    /// The game's centre code, `2`..`9`, `T`, `J`, `Q`, `K`, `A`.
    pub fn code(self) -> &'static str {
        self.short()
    }

    pub fn from_code(code: &str) -> Option<Rank> {
        Some(match code {
            "2" => Rank::Two,
            "3" => Rank::Three,
            "4" => Rank::Four,
            "5" => Rank::Five,
            "6" => Rank::Six,
            "7" => Rank::Seven,
            "8" => Rank::Eight,
            "9" => Rank::Nine,
            "T" => Rank::Ten,
            "J" => Rank::Jack,
            "Q" => Rank::Queen,
            "K" => Rank::King,
            "A" => Rank::Ace,
            _ => return None,
        })
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum Enhancement {
    #[default]
    None,
    Bonus, // +30 chips
    Mult,  // +4 mult
    Wild,  // counts as every suit
    Glass, // x2 mult, 1 in 4 chance to shatter
    Steel, // x1.5 mult while held in hand
    Stone, // +50 chips, no rank/suit
    Gold,  // +$3 if held at end of round
    Lucky, // 1 in 5 for +20 mult, 1 in 15 for $20
}

impl Enhancement {
    pub fn as_str(self) -> &'static str {
        match self {
            Enhancement::None => "none",
            Enhancement::Bonus => "bonus",
            Enhancement::Mult => "mult",
            Enhancement::Wild => "wild",
            Enhancement::Glass => "glass",
            Enhancement::Steel => "steel",
            Enhancement::Stone => "stone",
            Enhancement::Gold => "gold",
            Enhancement::Lucky => "lucky",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum Edition {
    #[default]
    None,
    Foil,        // +50 chips
    Holographic, // +10 mult
    Polychrome,  // x1.5 mult
    Negative,    // +1 joker slot (jokers only)
}

impl Edition {
    pub fn as_str(self) -> &'static str {
        match self {
            Edition::None => "none",
            Edition::Foil => "foil",
            Edition::Holographic => "holo",
            Edition::Polychrome => "polychrome",
            Edition::Negative => "negative",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum Seal {
    #[default]
    None,
    Gold,   // +$3 when scored
    Red,    // retrigger the card once
    Blue,   // creates a Planet card for the played hand if held
    Purple, // creates a Tarot card when discarded
}

impl Seal {
    pub fn as_str(self) -> &'static str {
        match self {
            Seal::None => "none",
            Seal::Gold => "gold",
            Seal::Red => "red",
            Seal::Blue => "blue",
            Seal::Purple => "purple",
        }
    }
}

/// From the game's own card bases. Suits order Diamonds < Clubs < Hearts <
/// Spades, and face_nominal separates the cards that all count as ten chips.
pub fn suit_nominal(suit: Suit) -> f64 {
    match suit {
        Suit::Diamonds => 0.01,
        Suit::Clubs => 0.02,
        Suit::Hearts => 0.03,
        Suit::Spades => 0.04,
    }
}

/// get_nominal adds `suit_nominal_original * 0.0001`, which is the suit the card
/// was *built* as. Card:set_base carries it across a suit change, so a Club
/// turned into a Spade by the Checkered Deck still remembers being a Club, and
/// sorts behind a Spade that was always one. Two identical-looking Jacks of
/// Spades are therefore not tied at all -- and the difference is larger than the
/// unique_val term that comes after it, so it decides.
pub const ORIGINAL_SUIT_WEIGHT: f64 = 0.0001;

pub fn face_nominal(rank: Rank) -> f64 {
    match rank {
        Rank::Ace => 0.4,
        Rank::King => 0.3,
        Rank::Queen => 0.2,
        Rank::Jack => 0.1,
        _ => 0.0,
    }
}

/// A playing card.
///
/// `PartialEq` is by `uid`, which is what Python's `eq=False` dataclass gives:
/// identity, not value. Every code path that asks "is this the same card?" --
/// `list.remove`, `card in cards`, the `uid` sets in `hands::evaluate` -- wants
/// that, and two cards that print the same are still two cards.
#[derive(Clone, Debug)]
pub struct Card {
    pub rank: Rank,
    pub suit: Suit,
    pub enhancement: Enhancement,
    pub edition: Edition,
    pub seal: Seal,
    /// Permanent bonus from Hiker etc.
    pub extra_chips: i32,
    /// Set only when something changes the card's suit; see `original_suit`.
    pub original_suit: Option<Suit>,
    /// Whether this card has already been played this ante, which is what The
    /// Pillar debuffs. It lives on the card rather than on the run because
    /// changing the card's *enhancement* wipes it -- see `set_enhancement`.
    pub played_this_ante: bool,
    pub uid: u64,
    pub debuffed: bool,
}

impl PartialEq for Card {
    fn eq(&self, other: &Self) -> bool {
        self.uid == other.uid
    }
}
impl Eq for Card {}

impl std::hash::Hash for Card {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.uid.hash(state);
    }
}

impl Card {
    pub fn new(rank: Rank, suit: Suit) -> Self {
        Card {
            rank,
            suit,
            enhancement: Enhancement::None,
            edition: Edition::None,
            seal: Seal::None,
            extra_chips: 0,
            original_suit: None,
            played_this_ante: false,
            uid: next_sort_id(),
            debuffed: false,
        }
    }

    pub fn is_stone(&self) -> bool {
        self.enhancement == Enhancement::Stone
    }

    pub fn base_chips(&self) -> i32 {
        if self.is_stone() {
            return 50;
        }
        self.rank.chips() + self.extra_chips
    }

    /// The suit this card was built as, which a conversion does not clear.
    ///
    /// base.suit_nominal_original in the game. It only ever differs on a deck or
    /// an effect that changes a card's suit, and it is invisible until two cards
    /// look identical -- then it is what separates them.
    ///
    /// A field and a method of the same name: `self.original_suit` reads the
    /// stored override, `self.original_suit()` the effective suit. Rust keeps
    /// the two namespaces apart, so this mirrors the Python property exactly.
    pub fn original_suit(&self) -> Suit {
        self.original_suit.unwrap_or(self.suit)
    }

    /// The game's get_nominal, which is what orders a hand on screen.
    ///
    /// The order matters beyond looks: actions address cards by position, so a
    /// simulator holding the same eight cards in a different order plays
    /// different ones for the same choice. Ranks come first, then face cards
    /// separate within the tens (Ace .4, King .3, Queen .2, Jack .1), then the
    /// suit breaks what is left -- Diamonds lowest, Spades highest -- and
    /// finally the suit the card was originally built as.
    pub fn sort_value(&self) -> f64 {
        let original = ORIGINAL_SUIT_WEIGHT * suit_nominal(self.original_suit());
        if self.is_stone() {
            // The game multiplies the suit term by -1000 for stone cards, which
            // sinks them below everything else.
            return (self.rank.chips() as f64)
                - 1000.0 * suit_nominal(self.suit)
                - 1000.0 * original;
        }
        (self.rank.chips() as f64)
            + suit_nominal(self.suit)
            + original
            + face_nominal(self.rank)
    }

    /// get_nominal('suit'), which is what the sort-by-suit button uses.
    ///
    /// The game multiplies the suit term by a thousand for this, so the suit
    /// dominates and the rank only breaks ties within it. Everything else is the
    /// same as the ordinary sort.
    pub fn suit_sort_value(&self) -> f64 {
        let original = 1000.0 * ORIGINAL_SUIT_WEIGHT * suit_nominal(self.original_suit());
        if self.is_stone() {
            return -1000.0 * suit_nominal(self.suit) - original + self.rank.chips() as f64;
        }
        1000.0 * suit_nominal(self.suit)
            + original
            + self.rank.chips() as f64
            + face_nominal(self.rank)
    }

    /// Change the suit, remembering what it was.
    pub fn set_suit(&mut self, suit: Suit) {
        if self.original_suit.is_none() {
            self.original_suit = Some(self.suit);
        }
        self.suit = suit;
    }

    /// Wild cards count as every suit; stone cards have no suit.
    pub fn counts_as_suit(&self, suit: Suit) -> bool {
        if self.debuffed || self.is_stone() {
            return false;
        }
        if self.enhancement == Enhancement::Wild {
            return true;
        }
        self.suit == suit
    }

    /// A copy in the game's sense: same card, next sort_id.
    pub fn copy(&self) -> Card {
        let mut out = self.clone();
        out.uid = next_sort_id();
        out
    }

    pub fn label(&self) -> String {
        if self.is_stone() {
            return "Stone".to_string();
        }
        format!("{}{}", self.rank.short(), self.suit.as_str())
    }
}

/// One card, shared by every pile that holds it -- Python object identity.
///
/// The Python `Card` is a mutable object and a run keeps *references* to the
/// same object in several lists at once: `add_card` puts one card in both
/// `full_deck` and `draw_pile`, `add_card_to_hand` in `full_deck` and `hand`,
/// and `remove_card` walks all four piles removing *the same object* from each.
/// A value type cannot express that: a clone in each pile would let a Hanged
/// Man or a shatter mutate one pile and leave the others stale. The handle is
/// how the port holds the reference.
///
/// Equality and hashing are by `uid`, the same identity `Card` already gives:
/// `Rc<RefCell<Card>>` inherits `Card`'s uid-based `PartialEq`/`Eq`/`Hash`, so
/// `vec.contains(&card)` and `HashSet<CardRef>` behave like Python's
/// identity-based `in` and sets, and two handles that print the same card are
/// still two cards.
pub type CardRef = Rc<RefCell<Card>>;

/// A fresh card with a fresh `uid` -- `Card(rank, suit)`.
pub fn make_card(rank: Rank, suit: Suit) -> CardRef {
    wrap(Card::new(rank, suit))
}

/// Take ownership of a card value and hand back the shared handle.
pub fn wrap(card: Card) -> CardRef {
    Rc::new(RefCell::new(card))
}

/// A copy in the game's sense: same fields, next `uid` -- `Card.copy`.
pub fn copy_card(card: &CardRef) -> CardRef {
    wrap(card.borrow().copy())
}

/// The card's identity, which is what every pile walks it by.
pub fn uid_of(card: &CardRef) -> u64 {
    card.borrow().uid
}

pub fn rank_of(card: &CardRef) -> Rank {
    card.borrow().rank
}

pub fn suit_of(card: &CardRef) -> Suit {
    card.borrow().suit
}

pub fn is_stone(card: &CardRef) -> bool {
    card.borrow().is_stone()
}

pub fn label_of(card: &CardRef) -> String {
    card.borrow().label()
}

pub fn base_chips(card: &CardRef) -> i32 {
    card.borrow().base_chips()
}

pub fn sort_value(card: &CardRef) -> f64 {
    card.borrow().sort_value()
}

pub fn suit_sort_value(card: &CardRef) -> f64 {
    card.borrow().suit_sort_value()
}

pub fn counts_as_suit(card: &CardRef, suit: Suit) -> bool {
    card.borrow().counts_as_suit(suit)
}

pub fn original_suit(card: &CardRef) -> Suit {
    card.borrow().original_suit()
}

/// Change the suit, remembering what it was. See `Card::set_suit`.
pub fn set_suit(card: &CardRef, suit: Suit) {
    card.borrow_mut().set_suit(suit);
}

/// Change a card's enhancement.
///
/// Card:set_ability rebuilds the whole ability table from the new centre and
/// carries exactly two things across: perma_bonus, so a Hiker's chips survive
/// (`extra_chips` here), and forced_selection. Everything else is rebuilt --
/// including `played_this_ante`, which is what The Pillar debuffs. So changing
/// an *enhancement* launders a card that has already been played this ante,
/// while changing its suit, rank, edition or seal does not.
pub fn set_enhancement(card: &CardRef, enhancement: Enhancement) {
    let mut c = card.borrow_mut();
    c.enhancement = enhancement;
    c.played_this_ante = false;
}

pub fn enhancement_of(card: &CardRef) -> Enhancement {
    card.borrow().enhancement
}

pub fn edition_of(card: &CardRef) -> Edition {
    card.borrow().edition
}

pub fn seal_of(card: &CardRef) -> Seal {
    card.borrow().seal
}

pub fn debuffed_of(card: &CardRef) -> bool {
    card.borrow().debuffed
}

pub fn set_debuffed(card: &CardRef, debuffed: bool) {
    card.borrow_mut().debuffed = debuffed;
}

pub fn extra_chips_of(card: &CardRef) -> i32 {
    card.borrow().extra_chips
}

pub fn played_this_ante_of(card: &CardRef) -> bool {
    card.borrow().played_this_ante
}

pub fn set_played_this_ante(card: &CardRef, value: bool) {
    card.borrow_mut().played_this_ante = value;
}

impl std::fmt::Display for Card {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut marks = String::new();
        if !matches!(self.enhancement, Enhancement::None | Enhancement::Stone) {
            marks.push_str(&format!("+{}", self.enhancement.as_str()));
        }
        if self.edition != Edition::None {
            marks.push_str(&format!("+{}", self.edition.as_str()));
        }
        if self.seal != Seal::None {
            marks.push_str(&format!("+{}seal", self.seal.as_str()));
        }
        write!(f, "<{}{}>", self.label(), marks)
    }
}

/// The order the game builds a deck in, which is also its sort_id order. It is
/// not a playing order at all: the game keys its cards "C_2", "C_A", "C_T" and
/// so on, and lays them out in the alphabetical order of those keys. So suits
/// run Clubs, Diamonds, Hearts, Spades -- and within a suit the ranks run 2..9,
/// then Ace, Jack, King, Queen, Ten, because that is A, J, K, Q, T.
///
/// This matters because the round's shuffle runs over this list. A deck built in
/// any other order shuffles reproducibly into a different deck, and every hand
/// of the run is then wrong from the same seed while every rule stays right --
/// the failure that is hardest to see.
pub const DECK_SUIT_ORDER: [Suit; 4] =
    [Suit::Clubs, Suit::Diamonds, Suit::Hearts, Suit::Spades];

pub const DECK_RANK_ORDER: [Rank; 13] = [
    Rank::Two,
    Rank::Three,
    Rank::Four,
    Rank::Five,
    Rank::Six,
    Rank::Seven,
    Rank::Eight,
    Rank::Nine,
    Rank::Ace,
    Rank::Jack,
    Rank::King,
    Rank::Queen,
    Rank::Ten,
];

/// The suit's position in the alphabetical centre-key order the game sorts by.
fn deck_suit_rank(suit: Suit) -> u8 {
    match suit {
        Suit::Clubs => 0,
        Suit::Diamonds => 1,
        Suit::Hearts => 2,
        Suit::Spades => 3,
    }
}

/// The 52-card deck, in the game's own build order.
///
/// Two decks change the build rather than what happens afterwards, and both have
/// to be done here because a card's place in this list is the id the game gives
/// it.
///
/// The Abandoned Deck drops the Kings, Queens and Jacks before the protos are
/// sorted, so it is a forty card deck and every id after the first Jack shifts.
/// The Erratic Deck replaces each card's face with a draw from G.P_CARDS under
/// the pool name "erratic" -- fifty-two draws, keeping duplicates -- so pass the
/// run's RNG to get one.
pub fn standard_deck(no_faces: bool, erratic: Option<&mut crate::rng::RunRng>) -> Vec<CardRef> {
    if let Some(erratic) = erratic {
        let mut fronts: Vec<(Suit, Rank)> = Vec::with_capacity(52);
        for suit in [Suit::Clubs, Suit::Diamonds, Suit::Hearts, Suit::Spades] {
            for rank in DECK_RANK_ORDER {
                fronts.push((suit, rank));
            }
        }
        // Draw fifty-two, then put them in order.
        //
        // The game builds card_protos and then sorts them by the card's own
        // letters -- `table.sort(card_protos, s..r..e..d..g)` -- which is what
        // gives a deck a deterministic build order at all, since the protos come
        // out of `pairs(P_CARDS)` in no defined order. For an ordinary deck the
        // sort is invisible because the result is the order it was already in.
        // For an Erratic Deck it is the whole difference: fifty-two random
        // draws, then sorted, so C2 C4 C4 C6 rather than the order they were
        // rolled in.
        //
        // The draws themselves were right all along -- the two decks held the
        // same fifty-two cards -- but a card's place in this list is the id it
        // gets, and the ids are what The Hook and every other positional effect
        // read.
        let mut drawn: Vec<(Suit, Rank)> = (0..52)
            .map(|_| {
                let i = erratic.random_element_index(fronts.len(), "erratic");
                fronts[i]
            })
            .collect();
        drawn.sort_by_key(|&(suit, rank)| (deck_suit_rank(suit), rank.code()));
        return drawn.into_iter().map(|(s, r)| make_card(r, s)).collect();
    }
    let mut out = Vec::with_capacity(52);
    for suit in DECK_SUIT_ORDER {
        for rank in DECK_RANK_ORDER {
            if no_faces && rank.is_face() {
                continue;
            }
            out.push(make_card(rank, suit));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_standard_deck_is_built_in_the_games_order() {
        let deck = standard_deck(false, None);
        assert_eq!(deck.len(), 52);
        assert_eq!(label_of(&deck[0]), "2C");
        assert_eq!(label_of(&deck[1]), "3C");
        assert_eq!(label_of(&deck[8]), "AC");
        assert_eq!(label_of(&deck[9]), "JC");
        assert_eq!(label_of(&deck[10]), "KC");
        assert_eq!(label_of(&deck[11]), "QC");
        assert_eq!(label_of(&deck[12]), "TC");
        assert_eq!(label_of(&deck[51]), "TS");
    }

    #[test]
    fn the_abandoned_deck_has_forty_cards() {
        let deck = standard_deck(true, None);
        assert_eq!(deck.len(), 40);
        assert!(!deck.iter().any(|c| rank_of(c).is_face()));
    }

    #[test]
    fn a_suit_change_remembers_what_it_was() {
        let card = make_card(Rank::Jack, Suit::Clubs);
        set_suit(&card, Suit::Spades);
        assert_eq!(suit_of(&card), Suit::Spades);
        assert_eq!(original_suit(&card), Suit::Clubs);
        // A natural Spade has no remembered Club, so it sorts differently.
        let natural = make_card(Rank::Jack, Suit::Spades);
        assert!(sort_value(&card) < sort_value(&natural));
    }

    #[test]
    fn stone_sinks_below_everything() {
        let stone = make_card(Rank::Ace, Suit::Spades);
        set_enhancement(&stone, Enhancement::Stone);
        let two = make_card(Rank::Two, Suit::Diamonds);
        assert!(sort_value(&stone) < sort_value(&two));
        assert_eq!(base_chips(&stone), 50);
    }

    #[test]
    fn a_destroyed_card_leaves_every_pile() {
        // The point of the handle: one object in two Vecs. A mutation through
        // one handle is seen through the other, exactly as Python's reference
        // would see it.
        let card = make_card(Rank::King, Suit::Hearts);
        let mut full_deck = vec![card.clone()];
        let mut hand = vec![card.clone()];
        assert!(Rc::ptr_eq(&full_deck[0], &hand[0]));

        set_enhancement(&hand[0], Enhancement::Glass);
        assert_eq!(enhancement_of(&full_deck[0]), Enhancement::Glass);

        // remove_card walks the piles and takes the card out of each. `contains`
        // and `retain` go by uid, as Python's `card in pile` goes by identity.
        let mut gone = false;
        for pile in [&mut full_deck, &mut hand] {
            if pile.contains(&card) {
                pile.retain(|c| c != &card);
                gone = true;
            }
        }
        assert!(gone);
        assert!(full_deck.is_empty());
        assert!(hand.is_empty());
    }

    #[test]
    fn a_shattered_card_is_removed_from_all_piles() {
        // The glass shattering that runs through remove_card: a card can sit in
        // the draw pile and the hand at once, and one shatter clears both.
        let card = make_card(Rank::Ace, Suit::Spades);
        set_enhancement(&card, Enhancement::Glass);
        let mut full_deck = vec![card.clone()];
        let mut draw_pile = vec![card.clone()];
        let mut hand = vec![card.clone()];

        let mut gone = false;
        for pile in [&mut full_deck, &mut draw_pile, &mut hand] {
            if pile.contains(&card) {
                pile.retain(|c| c != &card);
                gone = true;
            }
        }
        assert!(gone);
        assert!(full_deck.is_empty());
        assert!(draw_pile.is_empty());
        assert!(hand.is_empty());
    }
}
