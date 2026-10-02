//! Poker hand detection and hand-level bookkeeping.

use crate::cards::{is_stone, rank_of, uid_of, CardRef, Enhancement, Rank, Suit};
use std::collections::HashMap;

/// The twelve poker hands, weakest first, as the game numbers them.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
#[repr(u8)]
pub enum HandType {
    HighCard = 0,
    Pair = 1,
    TwoPair = 2,
    ThreeOfAKind = 3,
    Straight = 4,
    Flush = 5,
    FullHouse = 6,
    FourOfAKind = 7,
    StraightFlush = 8,
    FiveOfAKind = 9,
    FlushHouse = 10,
    FlushFive = 11,
}

impl HandType {
    pub const ALL: [HandType; 12] = [
        HandType::HighCard,
        HandType::Pair,
        HandType::TwoPair,
        HandType::ThreeOfAKind,
        HandType::Straight,
        HandType::Flush,
        HandType::FullHouse,
        HandType::FourOfAKind,
        HandType::StraightFlush,
        HandType::FiveOfAKind,
        HandType::FlushHouse,
        HandType::FlushFive,
    ];

    pub fn value(self) -> u8 {
        self as u8
    }

    /// The game's own name for the hand, from G.handlist.
    ///
    /// Title-casing the enum gives "Five Of A Kind" where the game says "Five of
    /// a Kind", and the name is not cosmetic: it is the key into G.GAME.hands,
    /// so anything comparing levels or plays by name misses. The Python original
    /// title-cases and then lower-cases `Of` and `A`; this is that result.
    pub fn label(self) -> &'static str {
        match self {
            HandType::HighCard => "High Card",
            HandType::Pair => "Pair",
            HandType::TwoPair => "Two Pair",
            HandType::ThreeOfAKind => "Three of a Kind",
            HandType::Straight => "Straight",
            HandType::Flush => "Flush",
            HandType::FullHouse => "Full House",
            HandType::FourOfAKind => "Four of a Kind",
            HandType::StraightFlush => "Straight Flush",
            HandType::FiveOfAKind => "Five of a Kind",
            HandType::FlushHouse => "Flush House",
            HandType::FlushFive => "Flush Five",
        }
    }

    /// Parse the game's name, which is how levels and plays are keyed.
    pub fn from_label(label: &str) -> Option<HandType> {
        HandType::ALL.into_iter().find(|h| h.label() == label)
    }
}

/// A set of hand types, as a bitset.
///
/// Python keeps a `frozenset`; the jokers that ask "does this hand contain a
/// Pair?" read it constantly, and a twelve-bit mask answers without hashing.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub struct HandSet(u16);

impl HandSet {
    pub fn new() -> Self {
        HandSet(0)
    }
    pub fn insert(&mut self, hand: HandType) -> &mut Self {
        self.0 |= 1 << hand.value();
        self
    }
    pub fn contains(&self, hand: HandType) -> bool {
        self.0 & (1 << hand.value()) != 0
    }
    pub fn iter(&self) -> impl Iterator<Item = HandType> + '_ {
        HandType::ALL.into_iter().filter(move |h| self.contains(*h))
    }
    pub fn is_empty(&self) -> bool {
        self.0 == 0
    }
    pub fn len(&self) -> usize {
        self.0.count_ones() as usize
    }
}

impl FromIterator<HandType> for HandSet {
    fn from_iter<T: IntoIterator<Item = HandType>>(iter: T) -> Self {
        let mut set = HandSet::new();
        for hand in iter {
            set.insert(hand);
        }
        set
    }
}

impl std::fmt::Display for HandSet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let names: Vec<&str> = self.iter().map(|h| h.label()).collect();
        write!(f, "{{{}}}", names.join(", "))
    }
}

/// level 1 (chips, mult) -- BASE_VALUES.
pub fn base_values(hand: HandType) -> (i32, i32) {
    match hand {
        HandType::HighCard => (5, 1),
        HandType::Pair => (10, 2),
        HandType::TwoPair => (20, 2),
        HandType::ThreeOfAKind => (30, 3),
        HandType::Straight => (30, 4),
        HandType::Flush => (35, 4),
        HandType::FullHouse => (40, 4),
        HandType::FourOfAKind => (60, 7),
        HandType::StraightFlush => (100, 8),
        HandType::FiveOfAKind => (120, 12),
        HandType::FlushHouse => (140, 14),
        HandType::FlushFive => (160, 16),
    }
}

/// The per-level increment (chips, mult) -- LEVEL_GAIN.
pub fn level_gain(hand: HandType) -> (i32, i32) {
    match hand {
        HandType::HighCard => (10, 1),
        HandType::Pair => (15, 1),
        HandType::TwoPair => (20, 1),
        HandType::ThreeOfAKind => (20, 2),
        HandType::Straight => (30, 3),
        HandType::Flush => (15, 2),
        HandType::FullHouse => (25, 2),
        HandType::FourOfAKind => (30, 3),
        HandType::StraightFlush => (40, 4),
        HandType::FiveOfAKind => (35, 3),
        HandType::FlushHouse => (40, 4),
        HandType::FlushFive => (50, 3),
    }
}

/// The three the game starts with `visible = false`. evaluate_play switches a
/// hand visible the first time it is made, and anything picking a poker hand at
/// random -- To Do List, Telescope -- draws from the visible ones only.
pub const SECRET_HANDS: [HandType; 3] = [
    HandType::FiveOfAKind,
    HandType::FlushHouse,
    HandType::FlushFive,
];

pub fn is_secret(hand: HandType) -> bool {
    SECRET_HANDS.contains(&hand)
}

/// G.handlist, strongest first.
///
/// What anything walking the hands in the game's stated order sees -- Telescope
/// reads it with ipairs and a strict >, so a tie on plays goes to the strongest
/// hand rather than the weakest.
pub const HANDLIST: [HandType; 12] = [
    HandType::FlushFive,
    HandType::FlushHouse,
    HandType::FiveOfAKind,
    HandType::StraightFlush,
    HandType::FourOfAKind,
    HandType::FullHouse,
    HandType::Flush,
    HandType::Straight,
    HandType::ThreeOfAKind,
    HandType::TwoPair,
    HandType::Pair,
    HandType::HighCard,
];

/// `pairs(G.GAME.hands)` in the shipped game: the order a walk of the hash
/// table meets the twelve hands.
///
/// Three draws index into a list built by that walk -- To Do List's hand when
/// the card is made (card.lua:313) and at round end (card.lua:2977), and each
/// blind's Orbital Tag hand (UI_definitions.lua:1511) -- so the same draw names
/// a different hand under a different order. The game's `lua51.dll` is LuaJIT
/// 2.0.5, whose string hash is the string's alone, so the table literal in
/// game.lua:2001 lays out the same way in every process; this is that layout,
/// read off the game's own DLL. (lupa's LuaJIT 2.1 seeds its hash per process,
/// which is why the order was once thought unrepeatable.) Walking HANDLIST
/// instead had JOKER189's To Do List name Pair where the game named Three of a
/// Kind, and the policy farmed Pairs for $4 the game never paid, shopping
/// itself to -$36. Holds for a run started fresh; one continued from a save
/// rebuilds the table and may lay it out otherwise.
pub const GAME_PAIRS_ORDER: [HandType; 12] = [
    HandType::FlushHouse,
    HandType::FullHouse,
    HandType::Flush,
    HandType::Pair,
    HandType::HighCard,
    HandType::StraightFlush,
    HandType::Straight,
    HandType::TwoPair,
    HandType::FlushFive,
    HandType::FiveOfAKind,
    HandType::ThreeOfAKind,
    HandType::FourOfAKind,
];

/// Planet card that levels each hand, for shop generation.
pub fn planet_for_hand(hand: HandType) -> &'static str {
    match hand {
        HandType::HighCard => "Pluto",
        HandType::Pair => "Mercury",
        HandType::TwoPair => "Uranus",
        HandType::ThreeOfAKind => "Venus",
        HandType::Straight => "Saturn",
        HandType::Flush => "Jupiter",
        HandType::FullHouse => "Earth",
        HandType::FourOfAKind => "Mars",
        HandType::StraightFlush => "Neptune",
        HandType::FiveOfAKind => "Planet X",
        HandType::FlushHouse => "Ceres",
        HandType::FlushFive => "Eris",
    }
}

/// Per-run level and play-count for every poker hand.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HandLevels {
    pub levels: HashMap<HandType, i32>,
    pub plays: HashMap<HandType, i32>,
}

impl Default for HandLevels {
    fn default() -> Self {
        Self::new()
    }
}

impl HandLevels {
    pub fn new() -> Self {
        HandLevels {
            levels: HandType::ALL.iter().map(|h| (*h, 1)).collect(),
            plays: HandType::ALL.iter().map(|h| (*h, 0)).collect(),
        }
    }

    pub fn level_up(&mut self, hand: HandType, times: i32) {
        *self.levels.entry(hand).or_insert(1) += times;
    }

    pub fn level(&self, hand: HandType) -> i32 {
        *self.levels.get(&hand).unwrap_or(&1)
    }

    pub fn played(&self, hand: HandType) -> i32 {
        *self.plays.get(&hand).unwrap_or(&0)
    }

    /// The chips and mult this hand is worth at its current level.
    pub fn values(&self, hand: HandType) -> (i32, i32) {
        let (base_chips, base_mult) = base_values(hand);
        let (gain_chips, gain_mult) = level_gain(hand);
        let extra = self.level(hand) - 1;
        (
            base_chips + gain_chips * extra,
            base_mult + gain_mult * extra,
        )
    }
}

/// What a played hand turned out to be, and which cards score.
#[derive(Clone, Debug)]
pub struct HandResult {
    pub hand: HandType,
    pub scoring: Vec<CardRef>,
    /// Every hand the played cards *contain*, not just the best one. The game
    /// keeps a table of all of them -- results["Pair"] is set whenever two cards
    /// share a rank, whatever the top hand turns out to be -- and the jokers that
    /// say "if hand contains a Pair" read that table. A Flush with two Kings in
    /// it contains a Pair, so Sly Joker fires on it, and testing the best hand
    /// instead silently misses every one of those.
    pub contains: HandSet,
}

/// `Card:is_suit(suit, nil, true)` -- the question a flush is judged on.
///
/// The game asks its suit question two ways (card.lua:4064), and they part on a
/// debuffed card. The ordinary one refuses it outright. This one -- `flush_calc`
/// -- reads the printed suit anyway, and only a Wild card loses its every-suit
/// to a debuff, falling back to the suit it was printed as:
///
/// ```text
///     if flush_calc then
///         if self.ability.effect == 'Stone Card' then return false end
///         if self.ability.name == "Wild Card" and not self.debuff then
///             return true end
///         if next(find_joker('Smeared Joker')) and
///             (self.base.suit == 'Hearts' or self.base.suit == 'Diamonds')
///             == (suit == 'Hearts' or suit == 'Diamonds') then
///             return true end
///         return self.base.suit == suit
/// ```
///
/// Hand detection used the ordinary question, so a hand of debuffed cards was
/// never a flush. Seed QWERTYUI, Blue Deck, stake 1, on the headless engine at
/// decision 101: The Club debuffs Clubs, Smeared Joker makes Spades Clubs, and
/// A-Q-Q-9-6 of Spades and Clubs was a level-three Flush in the game at
/// 135 x 26 = 3510 and a level-two Pair here at 95 x 21 = 1995 -- exactly the
/// 1515 chips the run parted by. Blackboard asks the same question, so
/// `jokers::counts_for_flush` reads this too.
pub fn flush_suit(card: &CardRef, suit: Suit, smeared: bool) -> bool {
    let c = card.borrow();
    if c.is_stone() {
        return false;
    }
    if c.enhancement == Enhancement::Wild && !c.debuffed {
        return true;
    }
    if smeared {
        return c.suit.is_red() == suit.is_red();
    }
    c.suit == suit
}

/// The order get_flush tries the suits in (misc_functions.lua:525-530).
pub const FLUSH_ORDER: [Suit; 4] = [Suit::Spades, Suit::Hearts, Suit::Clubs, Suit::Diamonds];

/// get_flush (functions/misc_functions.lua:522): the first suit, in the game's
/// order, with enough cards by `flush_suit`.
///
/// Smeared Joker collapses four suits into two, which changes what *is* a flush
/// rather than what one scores -- A 3 5 7 9 in mixed spades and clubs is a flush
/// only because of it.
///
/// The first suit to reach the count is the flush, not the biggest group
/// (532-542). Only Wild cards make two suits reach it at once, and then it
/// decides which cards score: with Four Fingers, four Wild cards and the King of
/// Clubs are a flush of Spades -- the four Wilds -- and the King scores nothing.
/// Taking the biggest group scored all five, 280 against the game's 240 on the
/// headless engine; three Wilds, a Diamond and a Club are the Wilds and the Club,
/// where it took the Diamond (tests/test_hand_evaluation_matches_engine.py).
///
/// More than five cards are no flush at all (531), whatever their suits.
pub fn flush_cards(cards: &[CardRef], needed: usize, smeared: bool) -> Option<Vec<CardRef>> {
    if cards.len() > 5 {
        return None;
    }
    for suit in FLUSH_ORDER {
        let group: Vec<CardRef> = cards
            .iter()
            .filter(|c| flush_suit(c, suit, smeared))
            .cloned()
            .collect();
        if group.len() >= needed {
            return Some(group);
        }
    }
    None
}

/// get_straight (functions/misc_functions.lua:548), line for line.
///
/// Two things the game does that a "longest run of distinct ranks" does not:
///
/// * every card of a rank in the run is part of the straight, not one per rank.
///   With Four Fingers 9 8 7 7 6 is a straight and *both* sevens score. Keeping
///   one card per rank left the second seven scoring nothing -- seed 64PUKM3K at
///   decision 78, 8272 here against the game's 8580, and U1AYP8BC at decision 53,
///   3080 against 3164.
/// * a debuffed card counts. get_id does not look at debuff (card.lua:957), so a
///   debuffed card holds its place in a straight the way it holds its suit in a
///   flush; evaluate_play then skips it when it comes to score.
///
/// Stone cards are out: get_id gives them a negative id, outside 2..14. More than
/// five cards are no straight (551), as they are no flush.
pub fn straight_cards(cards: &[CardRef], needed: usize, shortcut: bool) -> Option<Vec<CardRef>> {
    if cards.len() > 5 {
        return None;
    }
    // Rank value -> the cards of that rank, in played order.
    let mut ids: Vec<(u8, Vec<CardRef>)> = Vec::new();
    for c in cards {
        if is_stone(c) {
            continue;
        }
        let value = rank_of(c).value();
        match ids.iter_mut().find(|(v, _)| *v == value) {
            Some((_, list)) => list.push(c.clone()),
            None => ids.push((value, vec![c.clone()])),
        }
    }
    let has = |value: u8| {
        ids.iter()
            .find(|(v, _)| *v == value)
            .map(|(_, l)| l.clone())
    };

    let mut run: Vec<CardRef> = Vec::new();
    let mut length = 0usize;
    let mut straight = false;
    let mut skipped = false;
    for j in 1..=14u8 {
        let rank = if j == 1 { 14 } else { j };
        match has(rank) {
            Some(list) => {
                length += 1;
                skipped = false;
                run.extend(list);
            }
            None => {
                if shortcut && !skipped && j != 14 {
                    skipped = true;
                } else {
                    length = 0;
                    skipped = false;
                    if straight {
                        break;
                    }
                    run.clear();
                }
            }
        }
        if length >= needed {
            straight = true;
        }
    }
    if straight {
        Some(run)
    } else {
        None
    }
}

/// The four classification flags, which come from the jokers in the row.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct EvalFlags {
    pub four_fingers: bool,
    pub shortcut: bool,
    pub splash: bool,
    pub smeared: bool,
}

/// Ranks in `Counter.most_common()` order: by count descending, ties in
/// first-seen order.
///
/// Python's `Counter.most_common()` is `sorted(items, key=count, reverse=True)`
/// over an insertion-ordered dict, so ties keep the order the cards were
/// played in. `of_rank` walks that order and takes the *first* rank with enough
/// cards, so getting the tie-break wrong picks a different card from the same
/// hand.
fn most_common(counts: &[(Rank, usize)]) -> Vec<(Rank, usize)> {
    let mut out = counts.to_vec();
    out.sort_by(|a, b| b.1.cmp(&a.1));
    out
}

/// Classify a played hand and return the cards that score.
pub fn evaluate(cards: &[CardRef], flags: EvalFlags) -> HandResult {
    assert!(!cards.is_empty(), "cannot evaluate an empty hand");
    let EvalFlags {
        four_fingers,
        shortcut,
        splash,
        smeared,
    } = flags;

    let stones: Vec<CardRef> = cards.iter().filter(|c| is_stone(c)).cloned().collect();
    let ranked: Vec<CardRef> = cards.iter().filter(|c| !is_stone(c)).cloned().collect();

    // First-seen order, which is the played order.
    let mut counts: Vec<(Rank, usize)> = Vec::new();
    for c in &ranked {
        let rank = rank_of(c);
        match counts.iter_mut().find(|(r, _)| *r == rank) {
            Some((_, n)) => *n += 1,
            None => counts.push((rank, 1)),
        }
    }

    let needed = if four_fingers { 4 } else { 5 };
    let flush = flush_cards(cards, needed, smeared);
    let straight = straight_cards(cards, needed, shortcut);

    let of_rank = |n: usize| -> Option<Vec<CardRef>> {
        for (rank, cnt) in most_common(&counts) {
            if cnt >= n {
                return Some(
                    ranked
                        .iter()
                        .filter(|c| rank_of(c) == rank)
                        .take(n)
                        .cloned()
                        .collect(),
                );
            }
        }
        None
    };

    let five = of_rank(5);
    let four = of_rank(4);
    let trips = of_rank(3);
    let pairs: Vec<Rank> = counts
        .iter()
        .filter(|(_, cnt)| *cnt >= 2)
        .map(|(r, _)| *r)
        .collect();

    let mut full_house: Option<Vec<CardRef>> = None;
    if let Some(trips) = &trips {
        let mut rest: Vec<(Rank, usize)> = Vec::new();
        for c in ranked.iter().filter(|c| !trips.contains(c)) {
            let rank = rank_of(c);
            match rest.iter_mut().find(|(r, _)| *r == rank) {
                Some((_, n)) => *n += 1,
                None => rest.push((rank, 1)),
            }
        }

        if let Some((rank, cnt)) = most_common(&rest).first().copied() {
            if cnt >= 2 {
                let mut cards_out = trips.clone();
                cards_out.extend(
                    ranked
                        .iter()
                        .filter(|c| rank_of(c) == rank)
                        .take(2)
                        .cloned(),
                );
                full_house = Some(cards_out);
            }
        }
    }

    // Containment is built from groups of an *exact* size, not "at least".
    // get_X_same(3, hand) matches a rank with three cards and skips one with
    // four or five, and the game then patches a cascade on the end: a Five of a
    // Kind counts as a Four of a Kind, which counts as a Three of a Kind, which
    // counts as a Pair. Nothing in that chain reaches Two Pair.
    //
    // Reading "at least three" instead made a Five of a Kind contain a Full
    // House -- three of the five plus two of the same five -- which the game
    // never does, because both halves would be the same rank. Checked against
    // the engine across twelve hands, that and the Flush Five that followed from
    // it were the only two places the tables disagreed.
    //
    // The top hand is still chosen from the at-least groups below, which is what
    // the game does too: it reads _5 for a Five of a Kind whether or not _3 also
    // matched.
    let mut sizes: Vec<(usize, usize)> = Vec::new();
    for (_, cnt) in &counts {
        match sizes.iter_mut().find(|(s, _)| s == cnt) {
            Some((_, n)) => *n += 1,
            None => sizes.push((*cnt, 1)),
        }
    }
    let size_count = |want: usize| -> usize {
        sizes
            .iter()
            .find(|(s, _)| *s == want)
            .map(|(_, n)| *n)
            .unwrap_or(0)
    };
    let n_five = size_count(5);
    let n_four = size_count(4);
    let n_trips = size_count(3);
    let n_pairs = size_count(2);

    let mut held = HandSet::new();
    held.insert(HandType::HighCard);
    if n_five > 0 {
        held.insert(HandType::FiveOfAKind);
        if flush.is_some() {
            held.insert(HandType::FlushFive);
        }
    }
    if n_trips > 0 && n_pairs > 0 {
        held.insert(HandType::FullHouse);
        if flush.is_some() {
            held.insert(HandType::FlushHouse);
        }
    }
    if n_four > 0 {
        held.insert(HandType::FourOfAKind);
    }
    if flush.is_some() {
        held.insert(HandType::Flush);
    }
    if straight.is_some() {
        held.insert(HandType::Straight);
    }
    if flush.is_some() && straight.is_some() {
        held.insert(HandType::StraightFlush);
    }
    if n_trips > 0 {
        held.insert(HandType::ThreeOfAKind);
    }
    // Two Pair takes two pairs, or a set and a pair -- the one place a Full House
    // reaches down. A Four of a Kind never gets here.
    if n_pairs == 2 || (n_trips == 1 && n_pairs == 1) {
        held.insert(HandType::TwoPair);
    }
    if n_pairs > 0 {
        held.insert(HandType::Pair);
    }
    // The cascade, in the game's own order and stopping where it stops.
    if held.contains(HandType::FiveOfAKind) {
        held.insert(HandType::FourOfAKind);
    }
    if held.contains(HandType::FourOfAKind) {
        held.insert(HandType::ThreeOfAKind);
    }
    if held.contains(HandType::ThreeOfAKind) {
        held.insert(HandType::Pair);
    }

    let result = |hand: HandType, scoring: &[CardRef]| -> HandResult {
        // Stone cards always score, and scoring keeps the played order. Splash
        // widens the set to everything played without changing which hand it is:
        // a High Card with Splash still scores as a High Card, but all five cards
        // contribute their chips.
        if splash {
            return HandResult {
                hand,
                scoring: cards.to_vec(),
                contains: held,
            };
        }
        let mut chosen: Vec<u64> = scoring.iter().map(uid_of).collect();
        chosen.extend(stones.iter().map(uid_of));
        HandResult {
            hand,
            // Re-select by uid from the caller's own handles, as the Python
            // original does: the scored cards are the same objects that will be
            // mutated in place by the jokers later, never copies of them.
            scoring: cards
                .iter()
                .filter(|c| chosen.contains(&uid_of(c)))
                .cloned()
                .collect(),
            contains: held,
        }
    };

    if let (Some(five), Some(_)) = (&five, &flush) {
        return result(HandType::FlushFive, five);
    }
    if let (Some(fh), Some(_)) = (&full_house, &flush) {
        return result(HandType::FlushHouse, fh);
    }
    if let Some(five) = &five {
        return result(HandType::FiveOfAKind, five);
    }
    if let (Some(straight), Some(flush)) = (&straight, &flush) {
        // Every card in either part scores, not only the overlap. With Four
        // Fingers a four-card flush can sit inside a five-card straight, and the
        // game scores all five: A 3 5 7 9 with the 5 off suit is a Straight
        // Flush worth (100 + 35) x 8, not (100 + 30).
        //
        // Nor does the game ask the two parts to overlap at all
        // (misc_functions.lua:428): `if next(parts._flush) and
        // next(parts._straight)`. 2S 3S 4S 5H 9S is a four-card straight and a
        // four-card flush, and so a Straight Flush.
        let mut union = flush.clone();
        union.extend(straight.iter().filter(|c| !flush.contains(c)).cloned());
        return result(HandType::StraightFlush, &union);
    }
    if let Some(four) = &four {
        return result(HandType::FourOfAKind, four);
    }
    if let Some(fh) = &full_house {
        return result(HandType::FullHouse, fh);
    }
    if let Some(flush) = &flush {
        return result(HandType::Flush, flush);
    }
    if let Some(straight) = &straight {
        return result(HandType::Straight, straight);
    }
    if let Some(trips) = &trips {
        return result(HandType::ThreeOfAKind, trips);
    }
    if pairs.len() >= 2 {
        let mut top = pairs.clone();
        top.sort_by(|a, b| b.value().cmp(&a.value()));
        let top: Vec<Rank> = top.into_iter().take(2).collect();
        let chosen: Vec<CardRef> = ranked
            .iter()
            .filter(|c| top.contains(&rank_of(c)))
            .take(4)
            .cloned()
            .collect();
        return result(HandType::TwoPair, &chosen);
    }
    if let Some(first) = pairs.first() {
        let chosen: Vec<CardRef> = ranked
            .iter()
            .filter(|c| rank_of(c) == *first)
            .take(2)
            .cloned()
            .collect();
        return result(HandType::Pair, &chosen);
    }
    if !ranked.is_empty() {
        let high = ranked
            .iter()
            .max_by_key(|c| rank_of(c).value())
            .cloned()
            .unwrap();
        return result(HandType::HighCard, &[high]);
    }
    result(HandType::HighCard, &[])
}
