//! Which joker the shop offers, reproduced from the game.
//!
//! Getting every joker's arithmetic right is only half of a faithful simulator.
//! The other half is which jokers a run is ever *shown*: a policy learns the odds
//! it is trained against, so a shop with the wrong distribution teaches a game
//! that does not exist, and does it invisibly, because no rule is broken.
//!
//! The game's algorithm, from get_current_pool and create_card:
//!
//! ```text
//!     rarity = pseudorandom("rarity" .. ante)
//!     rarity = 3 if rarity > 0.95 else 2 if rarity > 0.7 else 1
//!
//!     pool = the rarity's pool, in order, with every entry that fails the
//!            filter replaced by "UNAVAILABLE" rather than dropped
//!
//!     key    = "Joker" .. rarity .. ante
//!     center = pool[random index]
//!     while center == "UNAVAILABLE":
//!         center = pool[random index from key .. "_resample" .. n]
//! ```
//!
//! Two details there are easy to miss and both change the distribution. Filtered
//! entries are *replaced*, not removed, so the pool keeps its length and the draw
//! is uniform over all entries rather than over the available ones -- an
//! unavailable joker costs a resample instead of being skipped. And the resample
//! uses a different pool each time, so it is not a retry of the same draw.
//!
//! The Python functions here are pure over `RunRng` and the game's data tables --
//! they build keys and pools and hand them back, and the caller (game.rs) turns a
//! key into a spec and a card. Nothing here reads or mutates a `GameState`, so no
//! `GameState` methods are needed by this module.

use std::collections::{HashMap, HashSet};

use crate::consumable_data::{CONSUMABLE_DATA, EXCLUDED_FROM_POOLS};
use crate::joker_data::{pool_for_rarity, JOKER_DATA};
use crate::pack_data::{PackRow, PACK_DATA};
use crate::rng::RunRng;
use crate::tag_data::TAG_DATA;
use crate::voucher_data::VOUCHER_DATA;

pub const UNAVAILABLE: &str = "UNAVAILABLE";

/// key -> the enhancement a run must already own before the shop offers it.
///
/// Taken from the game's own enhancement_gate field, exactly as Python builds
/// `GATES` from `JOKER_DATA`; see `build_pool`.
fn gate_for(key: &str) -> &'static str {
    JOKER_DATA
        .iter()
        .find(|row| row.key == key)
        .map(|row| row.enhancement_gate)
        .unwrap_or("")
}

/// When every entry is blanked the game does not hand back a pool of nothing: it
/// throws the pool away and offers one card. A run that has seen every Tarot is
/// offered Strength, over and over. Without this the resample loop -- which the
/// game writes with no bound, because it cannot fail -- never ends.
pub fn empty_pool_fallback(kind: &str) -> &'static str {
    match kind {
        "Tarot" | "Tarot_Planet" => "c_strength",
        "Planet" => "c_pluto",
        "Spectral" => "c_incantation",
        "Joker" => "j_joker",
        "Voucher" => "v_blank",
        "Tag" => "tag_handy",
        _ => "j_joker",
    }
}

/// `_or_fallback`: a blanked pool becomes the single fallback card instead.
pub fn or_fallback(pool: &[String], kind: &str) -> Vec<String> {
    if pool.iter().any(|entry| entry != UNAVAILABLE) {
        pool.to_vec()
    } else {
        vec![empty_pool_fallback(kind).to_string()]
    }
}

fn set_of<S: AsRef<str>>(items: &[S]) -> HashSet<&str> {
    items.iter().map(|s| s.as_ref()).collect()
}

/// One pool draw, resampling past blanks under a different pool name.
pub fn draw(rng: &mut RunRng, pool: &[String], key: &str) -> String {
    let mut center = rng.choice(key, pool);
    let mut attempt = 1;
    while center == UNAVAILABLE {
        attempt += 1;
        if attempt > 100 {
            // cannot happen: see `or_fallback`
            panic!("pool {} never yielded a card", key);
        }
        center = rng.choice(&format!("{}_resample{}", key, attempt), pool);
    }
    center
}

/// 1 common, 2 uncommon, 3 rare. Legendary never comes from a shop.
///
/// `append` is the game's key_append, and it is part of the pool name rather than
/// a label: a card rolled for the shop uses "sho", so it draws from a different
/// stream than one created by a joker or a pack.
pub fn roll_rarity(rng: &mut RunRng, ante: i32, append: &str) -> u8 {
    let roll = rng.pseudorandom(&format!("rarity{}{}", ante, append), None, None);
    if roll > 0.95 {
        3
    } else if roll > 0.7 {
        2
    } else {
        1
    }
}

/// The rarity's pool with unavailable entries blanked, not removed.
///
/// Length is preserved deliberately: the game draws an index into the whole pool,
/// so removing entries would change every draw after the first gap.
pub fn build_pool<O: AsRef<str>, S: AsRef<str>, P: AsRef<str>>(
    rarity: u8,
    owned_enhancements: &[O],
    seen_jokers: &[S],
    showman: bool,
    pool_flags: &[P],
) -> Vec<String> {
    let owned = set_of(owned_enhancements);
    let seen = set_of(seen_jokers);
    let flags = set_of(pool_flags);
    let mut pool = Vec::new();
    for row in pool_for_rarity(rarity) {
        let key = row.key;
        let gate = gate_for(key);
        let no_flag = row.no_flag;
        let yes_flag = row.yes_flag;
        if !gate.is_empty() && !owned.contains(gate) {
            pool.push(UNAVAILABLE.to_string()); // no Lucky Cat without a Lucky card
        } else if !no_flag.is_empty() && flags.contains(no_flag) {
            pool.push(UNAVAILABLE.to_string()); // Gros Michel, once extinct
        } else if !yes_flag.is_empty() && !flags.contains(yes_flag) {
            pool.push(UNAVAILABLE.to_string()); // Cavendish, until then
        } else if seen.contains(key) && !showman {
            pool.push(UNAVAILABLE.to_string()); // each joker appears once a run
        } else {
            pool.push(key.to_string());
        }
    }
    pool
}


/// One shop joker, as the game would roll it.
pub fn draw_joker<O: AsRef<str>, S: AsRef<str>, P: AsRef<str>>(
    rng: &mut RunRng,
    ante: i32,
    owned_enhancements: &[O],
    seen_jokers: &[S],
    showman: bool,
    rarity: Option<u8>,
    pool_flags: &[P],
    append: &str,
) -> String {
    let rarity = match rarity {
        Some(rarity) => rarity,
        None => roll_rarity(rng, ante, append),
    };
    let pool = build_pool(
        rarity,
        owned_enhancements,
        seen_jokers,
        showman,
        pool_flags,
    );
    // A legendary draw names a different stream, and the game is explicit about it
    // in get_current_pool:
    //
    //   _pool_key = 'Joker'..rarity..((not _legendary and _append) or '')
    //   return _pool, _pool_key..(not _legendary and ante or '')
    //
    // Both `and` clauses fail when _legendary is true, so the append and the ante
    // are dropped: The Soul draws from "Joker4", never "Joker4sou8". The simulator
    // was appending both, which is a real stream with a real sequence in it -- so
    // it drew a legendary every time, plausibly, and drew the wrong one. Recording
    // 8 stopped on it at step 190 of 443: Chicot recorded, Triboulet simulated.
    //
    // Keyed on rarity four rather than on a flag because in the game the two are
    // the same thing. A forced _rarity is a probability there, not an index, and
    // it is compared against 0.95 and 0.7 -- so passing four makes a *rare* joker,
    // and four is reachable only through _legendary.
    let key = if rarity == 4 {
        "Joker4".to_string()
    } else {
        format!("Joker{}{}{}", rarity, append, ante)
    };
    draw(rng, &or_fallback(&pool, "Joker"), &key)
}

/// A Tarot, Planet or Spectral pool, blanked the same way as jokers.
///
/// The rule worth knowing is the Planet softlock: Planet X, Ceres and Eris are
/// only in the pool once the hand they level has been played. A run that has never
/// made a Five of a Kind is never offered the card for it, so a simulator that
/// ignores this hands out deck-defining cards for hands the player cannot yet
/// make.
pub fn build_consumable_pool<H: AsRef<str>, S: AsRef<str>>(
    card_set: &str,
    played_hands: &[H],
    seen: &[S],
    showman: bool,
) -> Vec<String> {
    let played = set_of(played_hands);
    let already = set_of(seen);
    let mut pool = Vec::new();
    for row in CONSUMABLE_DATA.iter().filter(|row| row.set == card_set) {
        if EXCLUDED_FROM_POOLS.contains(&row.name) {
            pool.push(UNAVAILABLE.to_string()); // The Soul, Black Hole
        } else if row.softlock && !played.contains(row.hand_type) {
            pool.push(UNAVAILABLE.to_string()); // Planet X before a Five of a Kind
        } else if already.contains(row.key) && !showman {
            pool.push(UNAVAILABLE.to_string());
        } else {
            pool.push(row.key.to_string());
        }
    }
    pool
}

/// One consumable, as the game would roll it.
pub fn draw_consumable<H: AsRef<str>, S: AsRef<str>>(
    rng: &mut RunRng,
    card_set: &str,
    ante: i32,
    played_hands: &[H],
    seen: &[S],
    showman: bool,
    append: &str,
) -> String {
    let pool = build_consumable_pool(card_set, played_hands, seen, showman);
    draw(
        rng,
        &or_fallback(&pool, card_set),
        &format!("{}{}{}", card_set, append, ante),
    )
}

/// The run's starting rates, from G.GAME. Vouchers and decks move them -- Tarot
/// Merchant raises tarot_rate, the Ghost Deck sets spectral_rate to 2 -- so a run's
/// shop mix is not fixed even though these defaults are.
pub const BASE_RATES: [(&str, f64); 5] = [
    ("Joker", 20.0),
    ("Tarot", 4.0),
    ("Planet", 4.0),
    ("Base", 0.0),
    ("Spectral", 0.0),
];

/// The order matters: the roll walks these bands in sequence, so reordering them
/// changes which type a given roll lands on even with the same weights.
pub const RATE_ORDER: [&str; 5] = ["Joker", "Tarot", "Planet", "Base", "Spectral"];

pub const SHOP_APPEND: &str = "sho";

/// `dict(BASE_RATES)`, for a caller building the run's own rates.
pub fn base_rates() -> HashMap<String, f64> {
    BASE_RATES
        .iter()
        .map(|(kind, rate)| (kind.to_string(), *rate))
        .collect()
}

/// Which kind of card fills one shop slot.
///
/// The game rolls once against the summed rates and walks the bands in a fixed
/// order, so this is not a dictionary lookup by weight -- the sequence is part of
/// the answer.
pub fn roll_slot_type(rng: &mut RunRng, ante: i32, rates: Option<&HashMap<String, f64>>) -> String {
    let owned;
    let rates = match rates {
        Some(rates) => rates,
        None => {
            owned = base_rates();
            &owned
        }
    };
    let total: f64 = RATE_ORDER
        .iter()
        .map(|kind| *rates.get(*kind).unwrap_or(&0.0))
        .sum();
    let polled = rng.pseudorandom(&format!("cdt{}", ante), None, None) * total;
    let mut seen = 0.0;
    for kind in RATE_ORDER {
        let value = *rates.get(kind).unwrap_or(&0.0);
        if polled > seen && polled <= seen + value {
            return kind.to_string();
        }
        seen += value;
    }
    RATE_ORDER[0].to_string()
}


/// One shop slot: its type and the card in it.
///
/// Shop cards carry the game's "sho" key_append, which puts them on their own
/// pools -- the same joker rolled for a pack draws from a different stream.
pub fn draw_shop_card<
    O: AsRef<str>,
    S: AsRef<str>,
    P: AsRef<str>,
    H: AsRef<str>,
>(
    rng: &mut RunRng,
    ante: i32,
    rates: Option<&HashMap<String, f64>>,
    owned_enhancements: &[O],
    seen_jokers: &[S],
    showman: bool,
    pool_flags: &[P],
    played_hands: &[H],
) -> (String, String) {
    let kind = roll_slot_type(rng, ante, rates);
    if kind == "Joker" {
        // Jokers care about the deck's enhancements and the run's flags;
        // consumables care about which hands have been played. Passing either set
        // to the wrong side is a signature error, not a filter.
        let key = draw_joker(
            rng,
            ante,
            owned_enhancements,
            seen_jokers,
            showman,
            None,
            pool_flags,
            SHOP_APPEND,
        );
        (kind, key)
    } else if kind == "Tarot" || kind == "Planet" || kind == "Spectral" {
        let key = draw_consumable(
            rng,
            &kind,
            ante,
            played_hands,
            seen_jokers,
            showman,
            SHOP_APPEND,
        );
        (kind, key)
    } else {
        (kind, "playing_card".to_string())
    }
}

// --------------------------------------------------------------------------
// turning a game key back into a simulator spec
// --------------------------------------------------------------------------
//
// The pools speak the game's keys, because that is what can be checked against the
// engine. The simulator's registries are keyed by display name, so the two have to
// be joined somewhere; doing it here keeps the pools honest rather than renaming
// them to suit us. The Python maps are dicts; Rust gives each name an O(1)-ish
// lookup over the two generated tables.

/// `NAME_BY_JOKER_KEY[key]`: the display name of a joker centre.
pub fn name_by_joker_key(key: &str) -> Option<&'static str> {
    JOKER_DATA
        .iter()
        .find(|row| row.key == key)
        .map(|row| row.name)
}

/// `NAME_BY_CONSUMABLE_KEY[key]`: the display name of a consumable centre.
pub fn name_by_consumable_key(key: &str) -> Option<&'static str> {
    CONSUMABLE_DATA
        .iter()
        .find(|row| row.key == key)
        .map(|row| row.name)
}

/// The reverse of `name_by_joker_key` -- `KEY_BY_JOKER_NAME`.
pub fn key_by_joker_name(name: &str) -> Option<&'static str> {
    JOKER_DATA
        .iter()
        .find(|row| row.name == name)
        .map(|row| row.key)
}

/// The reverse of `name_by_consumable_key` -- `KEY_BY_CONSUMABLE_NAME`.
pub fn key_by_consumable_name(name: &str) -> Option<&'static str> {
    CONSUMABLE_DATA
        .iter()
        .find(|row| row.name == name)
        .map(|row| row.key)
}

/// The game's poll_edition: one roll, compared against stacked bands.
///
/// Written as descending thresholds off 1.0 rather than as weights, because that
/// is how the game writes it and the two are not the same when a modifier scales
/// them: `mod_` widens every band from the top, so the boundaries move relative to
/// each other rather than in proportion.
///
/// Returns "none", "foil", "holo", "polychrome" or "negative".
pub fn poll_edition(
    rng: &mut RunRng,
    key: &str,
    mod_: f64,
    no_negative: bool,
    edition_rate: f64,
    guaranteed: bool,
) -> &'static str {
    let poll = rng.pseudorandom(key, None, None);
    if guaranteed {
        // The Wheel of Fortune's form: the bands are twenty-five times as wide and
        // cover the whole range, so something always comes out. `mod` and the
        // run's edition rate are ignored here, as in the game.
        if poll > 1.0 - 0.003 * 25.0 && !no_negative {
            return "negative";
        }
        if poll > 1.0 - 0.006 * 25.0 {
            return "polychrome";
        }
        if poll > 1.0 - 0.02 * 25.0 {
            return "holo";
        }
        if poll > 1.0 - 0.04 * 25.0 {
            return "foil";
        }
        return "none";
    }
    if poll > 1.0 - 0.003 * mod_ && !no_negative {
        return "negative";
    }
    if poll > 1.0 - 0.006 * edition_rate * mod_ {
        return "polychrome";
    }
    if poll > 1.0 - 0.02 * edition_rate * mod_ {
        return "holo";
    }
    if poll > 1.0 - 0.04 * edition_rate * mod_ {
        return "foil";
    }
    "none"
}


// --------------------------------------------------------------------------
// booster packs
// --------------------------------------------------------------------------

/// One booster pack, as get_pack rolls it.
///
/// Two things here are not obvious from playing. The first shop of a run always
/// offers a Buffoon pack -- the game short-circuits before any roll, so a simulator
/// that rolls normally there gives the player a different opening than the game
/// ever does. And the weights are not uniform across sizes: a mega pack is a
/// quarter as likely as a normal one of the same kind, so treating a pack type as
/// one choice and its size as another gives the right types at the wrong sizes.
///
/// Returns the pool entry: (key, kind, weight, choose, cards, cost).
pub fn draw_pack(rng: &mut RunRng, ante: i32, first_shop: bool, key: &str) -> &'static PackRow {
    if first_shop {
        // p_buffoon_normal_1 or _2, chosen with a bare math.random(1, 2). There is
        // no pool name here, so it continues whatever stream the last seeded draw
        // left behind -- see RunRng.math_random.
        let index = rng.math_random(Some(1.0), Some(2.0)) as i64;
        let wanted = format!("p_buffoon_normal_{}", index);
        for entry in PACK_DATA {
            if entry.key == wanted {
                return entry;
            }
        }
    }

    let total: f64 = PACK_DATA.iter().map(|entry| entry.weight).sum();
    let poll = rng.pseudorandom(&format!("{}{}", key, ante), None, None) * total;
    let mut seen = 0.0;
    for entry in PACK_DATA {
        let weight = entry.weight;
        seen += weight;
        if seen >= poll && seen - weight <= poll {
            return entry;
        }
    }
    &PACK_DATA[PACK_DATA.len() - 1]
}

// --------------------------------------------------------------------------
// skip tags
// --------------------------------------------------------------------------

/// Tags on offer at this ante, blanked rather than dropped as ever.
///
/// Five tags name a centre that must have been *discovered* -- Rare Tag wants
/// Blueprint seen, the edition tags want their edition seen. Discovery belongs to
/// the profile rather than the run, and a profile that has played at all has them,
/// which is what the engine reports. So the default is that they are known, and a
/// caller who wants to model a fresh profile passes the set it has. Gating on an
/// empty set instead blanked four tags the engine was offering, and the draw landed
/// elsewhere from there on.
pub fn build_tag_pool<S: AsRef<str>>(ante: i32, discovered: Option<&[S]>) -> Vec<String> {
    let known: Option<HashSet<&str>> = discovered.map(set_of);
    let mut pool = Vec::new();
    for row in TAG_DATA {
        if row.min_ante != 0 && row.min_ante > ante {
            pool.push(UNAVAILABLE.to_string());
        } else if !row.requires.is_empty()
            && known
                .as_ref()
                .map_or(false, |known| !known.contains(row.requires))
        {
            pool.push(UNAVAILABLE.to_string());
        } else {
            pool.push(row.key.to_string());
        }
    }
    pool
}

/// The reward for skipping a blind, as get_next_tag_key rolls it.
///
/// Same machinery as every other pool: an index into a list that keeps its length,
/// and a resample from a differently-named pool when the entry is blank. The
/// simulator used to roll from eight tags of its own with its own key, which handed
/// a run a tag it was never offered -- and an Uncommon or Rare tag hands over a
/// joker with it.
pub fn draw_tag<S: AsRef<str>>(
    rng: &mut RunRng,
    ante: i32,
    discovered: Option<&[S]>,
    append: &str,
) -> String {
    let pool = or_fallback(&build_tag_pool(ante, discovered), "Tag");
    draw(rng, &pool, &format!("Tag{}{}", append, ante))
}


// --------------------------------------------------------------------------
// what is inside a pack
// --------------------------------------------------------------------------

/// The Enhanced pool in the game's own order. pseudorandom_element sorts an
/// array-shaped pool by its integer keys, so this order is the draw order and
/// alphabetising it would hand out different enhancements from the same seed.
pub const ENHANCEMENTS: [&str; 8] = [
    "m_bonus", "m_mult", "m_wild", "m_glass", "m_steel", "m_stone", "m_gold", "m_lucky",
];

// G.P_CARDS is keyed by strings, so pseudorandom_element sorts it by key: clubs,
// diamonds, hearts, spades, and within a suit 2-9 then A J K Q T.
pub const SUITS: [&str; 4] = ["C", "D", "H", "S"];
pub const RANKS: [&str; 13] = ["2", "3", "4", "5", "6", "7", "8", "9", "A", "J", "K", "Q", "T"];

/// `SUITS` x `RANKS`, in the game's key order -- the Standard pack's front draw.
pub const FRONTS: [&str; 52] = [
    "C_2", "C_3", "C_4", "C_5", "C_6", "C_7", "C_8", "C_9", "C_A", "C_J", "C_K", "C_Q", "C_T",
    "D_2", "D_3", "D_4", "D_5", "D_6", "D_7", "D_8", "D_9", "D_A", "D_J", "D_K", "D_Q", "D_T",
    "H_2", "H_3", "H_4", "H_5", "H_6", "H_7", "H_8", "H_9", "H_A", "H_J", "H_K", "H_Q", "H_T",
    "S_2", "S_3", "S_4", "S_5", "S_6", "S_7", "S_8", "S_9", "S_A", "S_J", "S_K", "S_Q", "S_T",
];

/// The appends each kind of pack creates its cards under. They are pool names, not
/// labels: a Tarot from an Arcana pack draws from "Tarotar11", a Tarot from the
/// shop from "Tarotsho1", and the two streams run independently.
pub fn pack_append(kind: &str) -> &'static str {
    match kind {
        "Arcana" => "ar1",
        "Celestial" => "pl1",
        "Spectral" => "spe",
        "Standard" => "sta",
        "Buffoon" => "buf",
        _ => panic!("unknown pack kind {:?}", kind),
    }
}

/// One card a pack offers.
///
/// Python hands back a dict whose keys depend on the kind; this is the same shape,
/// with the fields a kind does not use left None/default.
#[derive(Clone, Debug, Default)]
pub struct PackCard {
    pub set: &'static str,
    pub key: Option<String>,
    pub rank: Option<String>,
    pub suit: Option<String>,
    pub enhancement: Option<String>,
    pub edition: Option<&'static str>,
    pub seal: Option<String>,
    pub eternal: bool,
    pub perishable: bool,
    pub rental: bool,
}

fn soul_key(card_set: &str, ante: i32) -> String {
    format!("soul_{}{}", card_set, ante)
}

/// The 1-in-333 that turns a pack card into The Soul or Black Hole.
///
/// This runs *before* the pool draw and it runs whether or not it fires, so a
/// simulator that skips it draws every later card of that type from a pool one step
/// behind. Spectral polls twice -- once for each -- against the same pool name,
/// which advances it twice.
fn soulable(
    rng: &mut RunRng,
    card_set: &str,
    ante: i32,
    soul_used: bool,
    black_hole_used: bool,
    showman: bool,
) -> Option<&'static str> {
    if (card_set == "Tarot" || card_set == "Spectral") && !(soul_used && !showman) {
        if rng.pseudorandom(&soul_key(card_set, ante), None, None) > 0.997 {
            return Some("c_soul");
        }
    }
    if (card_set == "Planet" || card_set == "Spectral") && !(black_hole_used && !showman) {
        if rng.pseudorandom(&soul_key(card_set, ante), None, None) > 0.997 {
            return Some("c_black_hole");
        }
    }
    None
}

/// One consumable from a pack, The Soul/Black Hole check included.
fn pack_consumable(
    rng: &mut RunRng,
    card_set: &'static str,
    ante: i32,
    append: &str,
    played_hands: &[&str],
    seen: &HashSet<String>,
    showman: bool,
) -> PackCard {
    let forced = soulable(
        rng,
        card_set,
        ante,
        seen.contains("c_soul"),
        seen.contains("c_black_hole"),
        showman,
    );
    if let Some(forced) = forced {
        return PackCard {
            set: "Spectral",
            key: Some(forced.to_string()),
            ..PackCard::default()
        };
    }
    let seen_vec: Vec<&str> = seen.iter().map(|s| s.as_str()).collect();
    let key = draw_consumable(rng, card_set, ante, played_hands, &seen_vec, showman, append);
    PackCard {
        set: card_set,
        key: Some(key),
        ..PackCard::default()
    }
}


/// One card from a Standard pack: face, enhancement, edition, seal.
///
/// The order matters as much as the rolls. The game decides enhanced-or-not first,
/// then draws the enhancement, then the face, then the edition, then whether there
/// is a seal and only then which seal -- five pools, each advanced whether or not
/// anything comes of it.
///
/// `edition_rate` is the run's, which Hone and Glow Up raise; card.lua:1761 passes
/// its own doubling as poll_edition's `_mod` and the game multiplies the two.
/// Leaving the run's out moved exactly one boundary -- holographic against foil --
/// which is what a live run showed: `card-holo` from the game where the shadow had
/// `card-foil`.
fn standard_card(rng: &mut RunRng, ante: i32, edition_rate: f64) -> PackCard {
    let enhanced = rng.pseudorandom(&format!("stdset{}", ante), None, None) > 0.6;
    let enhancement = if enhanced {
        Some(rng.choice(&format!("Enhancedsta{}", ante), &ENHANCEMENTS).to_string())
    } else {
        None
    };
    let front = rng.choice(&format!("frontsta{}", ante), &FRONTS);
    let (suit, rank) = front.split_once('_').expect("front is suit_rank");
    let edition = poll_edition(
        rng,
        &format!("standard_edition{}", ante),
        2.0,
        true,
        edition_rate,
        false,
    );
    let mut seal = None;
    if rng.pseudorandom(&format!("stdseal{}", ante), None, None) > 0.8 {
        // 1 - 0.02*10
        let roll = rng.pseudorandom(&format!("stdsealtype{}", ante), None, None);
        seal = Some(
            (if roll > 0.75 {
                "Red"
            } else if roll > 0.5 {
                "Blue"
            } else if roll > 0.25 {
                "Gold"
            } else {
                "Purple"
            })
            .to_string(),
        );
    }
    PackCard {
        set: "Playing",
        rank: Some(rank.to_string()),
        suit: Some(suit.to_string()),
        enhancement,
        edition: Some(edition),
        seal,
        ..PackCard::default()
    }
}


/// Everything a pack offers, in the order the game creates it.
///
/// The simulator drew pack contents uniformly from whole card sets, which is wrong
/// twice over: it ignores the pool -- so it offers a Tarot the run has already
/// seen, or Planet X for a hand nobody has played -- and it ignores the stream, so
/// every draw afterwards is off by however many rolls the pack should have taken.
///
/// The `stickers`/`edition_rate` pair is the run's stake and its vouchers, exactly
/// as the shop passes them.
#[allow(clippy::too_many_arguments)]
pub fn pack_contents<
    H: AsRef<str>,
    S: AsRef<str>,
    O: AsRef<str>,
    J: AsRef<str>,
    P: AsRef<str>,
>(
    rng: &mut RunRng,
    kind: &str,
    cards: i32,
    ante: i32,
    played_hands: &[H],
    seen: &[S],
    showman: bool,
    owned_enhancements: &[O],
    seen_jokers: &[J],
    pool_flags: &[P],
    soul_used: bool,
    black_hole_used: bool,
    telescope: bool,
    omen_globe: bool,
    most_played_planet: Option<&str>,
    stickers: Option<&StickerOptions>,
    edition_rate: f64,
) -> Vec<PackCard> {
    let append = pack_append(kind);
    let played: Vec<&str> = played_hands.iter().map(|s| s.as_ref()).collect();
    let owned: Vec<&str> = owned_enhancements.iter().map(|s| s.as_ref()).collect();
    let flags: Vec<&str> = pool_flags.iter().map(|s| s.as_ref()).collect();
    let stickers = stickers.copied().unwrap_or_default();

    // A card marks its own centre used the moment it is constructed -- see
    // Card:set_ability -- not when the player takes it. So a pack blanks each card
    // it has just made from the pool the next one draws from, and cannot offer the
    // same Tarot twice. G.GAME.used_jokers is one table for jokers and consumables
    // alike, which is why one set covers both here.
    let mut made: HashSet<String> = seen.iter().map(|s| s.as_ref().to_string()).collect();
    made.extend(seen_jokers.iter().map(|s| s.as_ref().to_string()));
    if soul_used {
        made.insert("c_soul".to_string());
    }
    if black_hole_used {
        made.insert("c_black_hole".to_string());
    }

    let mut out = Vec::new();
    for i in 1..=cards {
        let card = if kind == "Arcana" {
            if omen_globe && rng.pseudorandom("omen_globe", None, None) > 0.8 {
                pack_consumable(rng, "Spectral", ante, "ar2", &played, &made, showman)
            } else {
                pack_consumable(rng, "Tarot", ante, append, &played, &made, showman)
            }
        } else if kind == "Celestial" {
            // The Telescope voucher forces the first card to the planet for the
            // hand the run has played most, with no roll at all.
            if telescope && i == 1 && most_played_planet.is_some() {
                PackCard {
                    set: "Planet",
                    key: most_played_planet.map(|s| s.to_string()),
                    ..PackCard::default()
                }
            } else {
                pack_consumable(rng, "Planet", ante, append, &played, &made, showman)
            }
        } else if kind == "Spectral" {
            pack_consumable(rng, "Spectral", ante, append, &played, &made, showman)
        } else if kind == "Buffoon" {
            let made_vec: Vec<&str> = made.iter().map(|s| s.as_str()).collect();
            let key = draw_joker(rng, ante, &owned, &made_vec, showman, None, &flags, append);
            // A pack joker takes the same sticker polls a shop joker does, under
            // the pack's own pool names, and its centre refuses what it will not
            // take (card.lua:506-518).
            let marks = poll_stickers(
                rng,
                ante,
                true,
                stickers.eternals,
                stickers.perishables,
                stickers.rentals,
                name_by_joker_key(&key),
            );
            // poll_edition reads the global G.GAME.edition_rate, so Hone and Glow
            // Up widen a pack joker's bands as they do a shop joker's
            // (common_events.lua:2071-2076, 2149).
            let edition = poll_edition(
                rng,
                &format!("edi{}{}", append, ante),
                1.0,
                false,
                edition_rate,
                false,
            );
            PackCard {
                set: "Joker",
                key: Some(key),
                edition: Some(edition),
                eternal: marks.eternal,
                perishable: marks.perishable,
                rental: marks.rental,
                ..PackCard::default()
            }
        } else if kind == "Standard" {
            standard_card(rng, ante, edition_rate)
        } else {
            panic!("unknown pack kind {:?}", kind);
        };
        if !showman {
            if let Some(key) = &card.key {
                made.insert(key.clone());
            }
        }
        out.push(card);
    }
    out
}


// --------------------------------------------------------------------------
// stickers
// --------------------------------------------------------------------------

/// Which stickers the stake allows a caller to ask about, as Python reads them
/// out of the `**stickers` dict.
#[derive(Clone, Copy, Debug, Default)]
pub struct StickerOptions {
    pub eternals: bool,
    pub perishables: bool,
    pub rentals: bool,
}

/// The sticker poll's answer.
#[derive(Clone, Copy, Debug, Default)]
pub struct StickerPoll {
    pub eternal: bool,
    pub perishable: bool,
    pub rental: bool,
}

/// (eternal, perishable) for this joker, by the game's own centre flags.
///
/// Card:set_eternal and Card:set_perishable (card.lua:506, 513) drop the sticker
/// when the centre refuses it -- a joker that destroys itself is never eternal, and
/// one whose whole value is a counter it would lose is never perishable. The poll
/// happens either way; only the sticker is refused, so the stream is unaffected.
pub fn takes_sticker(name: &str) -> (bool, bool) {
    JOKER_DATA
        .iter()
        .find(|row| row.name == name)
        .map(|row| (row.eternal_ok, row.perishable_ok))
        .unwrap_or((true, true))
}

/// Eternal, perishable and rental, as create_card polls them.
///
/// Every joker made for a shop or a Buffoon pack takes this poll, and the first
/// draw happens *whether or not any sticker is enabled* -- the game reads the roll
/// into a local and only then asks whether the stake allows anything. So a
/// White-stake run still spends it, and a simulator that skips it stands one draw
/// behind on that pool for the rest of the run. The rental roll is different: it
/// sits behind an `and`, so it is only spent when rentals are on.
///
/// The names change inside a pack: "packetper" and "packssjr" rather than
/// "etperpoll" and "ssjr".
///
/// `name` is the joker the stickers are for, and its centre gets the veto
/// set_eternal and set_perishable give it (card.lua:506-518) -- in the shop and in
/// a pack alike, since both go through create_card (common_events.lua:2137-2146).
/// The veto lived in GameState._apply_stickers, which a pack joker never passes
/// through, so 0K02UUCE's Spare Trousers came out of a Buffoon pack perishable and
/// was debuffed five rounds later in a run where the game had left it plain.
pub fn poll_stickers(
    rng: &mut RunRng,
    ante: i32,
    in_pack: bool,
    eternals: bool,
    perishables: bool,
    rentals: bool,
    name: Option<&str>,
) -> StickerPoll {
    let (eternal_ok, perishable_ok) = match name {
        Some(name) => takes_sticker(name),
        None => (true, true),
    };
    let mut out = StickerPoll::default();
    let poll = rng.pseudorandom(
        &format!("{}{}", if in_pack { "packetper" } else { "etperpoll" }, ante),
        None,
        None,
    );
    if eternals && poll > 0.7 {
        out.eternal = eternal_ok;
    } else if perishables && poll > 0.4 && poll <= 0.7 {
        out.perishable = perishable_ok;
    }
    if rentals
        && rng.pseudorandom(
            &format!("{}{}", if in_pack { "packssjr" } else { "ssjr" }, ante),
            None,
            None,
        ) > 0.7
    {
        out.rental = true;
    }
    out
}


// --------------------------------------------------------------------------
// vouchers
// --------------------------------------------------------------------------

/// `NAME_BY_VOUCHER_KEY[key]`: the display name of a voucher.
pub fn name_by_voucher_key(key: &str) -> Option<&'static str> {
    VOUCHER_DATA
        .iter()
        .find(|row| row.key == key)
        .map(|row| row.name)
}

/// The voucher pool, blanked the same way as every other.
///
/// Three things take an entry out. A voucher already redeemed cannot come again --
/// unlike a joker, there is no Showman that brings it back. An upgrade is gated on
/// its base having been redeemed, so half the list is blank at the start of a run.
/// And a voucher already sitting in the shop is withheld, which matters when a
/// Voucher Tag adds a second one.
pub fn build_voucher_pool<R: AsRef<str>, O: AsRef<str>>(
    redeemed: &[R],
    on_offer: &[O],
) -> Vec<String> {
    let owned = set_of(redeemed);
    let offered = set_of(on_offer);
    let mut pool = Vec::new();
    for row in VOUCHER_DATA {
        if owned.contains(row.key) || offered.contains(row.key) {
            pool.push(UNAVAILABLE.to_string());
        } else if !row.requires.is_empty() && !owned.contains(row.requires) {
            pool.push(UNAVAILABLE.to_string());
        } else {
            pool.push(row.key.to_string());
        }
    }
    pool
}

/// The voucher this round's shop offers, as get_next_voucher_key rolls it.
///
/// Rolled when the round starts rather than when the shop opens, which is why it is
/// drawn against the ante and not the shop.
pub fn draw_voucher<R: AsRef<str>, O: AsRef<str>>(
    rng: &mut RunRng,
    ante: i32,
    redeemed: &[R],
    on_offer: &[O],
    from_tag: bool,
) -> String {
    let pool = or_fallback(&build_voucher_pool(redeemed, on_offer), "Voucher");
    let key = if from_tag {
        "Voucher_fromtag".to_string()
    } else {
        format!("Voucher{}", ante)
    };
    draw(rng, &pool, &key)
}

