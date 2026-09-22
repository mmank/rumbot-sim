//! Tarot, Planet and Spectral cards.
//!
//! Like jokers, consumables are a registry of specs. `targets` is how many cards
//! from the current hand the effect needs; the engine only offers a consumable as
//! a legal action when that many cards are selected.

use std::cell::RefCell;
use std::rc::Rc;

use crate::cards::CardRef;
use crate::cards::Edition;
use crate::game::GameState;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum ConsumableKind {
    Tarot,
    Planet,
    Spectral,
}

impl ConsumableKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ConsumableKind::Tarot => "tarot",
            ConsumableKind::Planet => "planet",
            ConsumableKind::Spectral => "spectral",
        }
    }

    /// The game's own set name, which is how the data table is keyed.
    pub fn set_name(self) -> &'static str {
        match self {
            ConsumableKind::Tarot => "Tarot",
            ConsumableKind::Planet => "Planet",
            ConsumableKind::Spectral => "Spectral",
        }
    }
}

/// What using the card does. Python's `apply(game, cards)`.
pub type ApplyHook = fn(&mut GameState, &[CardRef]);

#[derive(Clone, Copy, Debug)]
pub struct ConsumableSpec {
    pub name: &'static str,
    pub kind: ConsumableKind,
    pub text: &'static str,
    pub targets: i32,
    pub max_targets: Option<i32>,
    pub apply: Option<ApplyHook>,
    pub cost: i32,
}

impl ConsumableSpec {
    pub const DEFAULT: ConsumableSpec = ConsumableSpec {
        name: "",
        kind: ConsumableKind::Tarot,
        text: "",
        targets: 0,
        max_targets: None,
        apply: None,
        cost: 3,
    };

    /// How many cards this one will accept.
    pub fn accepts(&self, n_selected: i32) -> bool {
        let low = self.targets;
        let high = self.max_targets.unwrap_or(self.targets);
        low <= n_selected && n_selected <= high
    }
}

pub type ConsumableRef = Rc<RefCell<ConsumableInstance>>;

pub fn make_ref(instance: ConsumableInstance) -> ConsumableRef {
    Rc::new(RefCell::new(instance))
}

/// One consumable actually held, rather than the kind of thing it is.
///
/// The registry entry is a centre -- what a Death is. This is a card: what
/// *this* Death is, which for a consumable means its edition and the sell value
/// it has picked up. Perkeo is why the difference has to exist. Its copy is
/// Negative, and Card:add_to_deck raises G.consumeables' card limit for a
/// negative consumable exactly as it raises the joker limit for a negative joker,
/// so the copy costs no slot. Holding the row as shared registry singletons left
/// nowhere to record that, and no way to tell which of two Fools was the free one
/// when a Fool was later used.
#[derive(Debug)]
pub struct ConsumableInstance {
    pub spec: &'static ConsumableSpec,
    pub edition: Edition,
    /// ability.extra_value, which Gift Card raises on every consumable held as
    /// well as every joker and Card:set_cost adds to the sell price. A card's,
    /// not a centre's: two Fools held for different numbers of rounds sell for
    /// different money.
    pub extra_sell_value: i32,
    /// Age, for Perkeo's draw, which sorts by it. A card bought from the shop
    /// carries the age it was stocked with.
    pub uid: u64,
}

impl ConsumableInstance {
    pub fn new(spec: &'static ConsumableSpec, edition: Edition) -> Self {
        ConsumableInstance {
            spec,
            edition,
            extra_sell_value: 0,
            uid: crate::cards::next_sort_id(),
        }
    }
}

impl std::fmt::Display for ConsumableInstance {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.edition == Edition::None {
            write!(f, "<{}>", self.spec.name)
        } else {
            write!(f, "<{} {}>", self.edition.as_str(), self.spec.name)
        }
    }
}

// --------------------------------------------------------------------------
// the registry
// --------------------------------------------------------------------------
//
// Python fills a dict with `register(ConsumableSpec(...))` calls at import time.
// Rust keeps the specs in a `const` array in `consumable_specs.rs` -- the
// translation of those same calls -- and indexes it lazily, so `spec(name)` is
// O(1).

/// Every registered consumable, in the order the Python file registers them.
pub fn all_specs() -> &'static [ConsumableSpec] {
    crate::consumable_specs::SPECS
}

fn index() -> &'static std::collections::HashMap<&'static str, &'static ConsumableSpec> {
    static INDEX: std::sync::OnceLock<
        std::collections::HashMap<&'static str, &'static ConsumableSpec>,
    > = std::sync::OnceLock::new();
    INDEX.get_or_init(|| {
        let mut map = std::collections::HashMap::with_capacity(all_specs().len());
        for spec in all_specs() {
            if map.insert(spec.name, spec).is_some() {
                panic!("consumable {:?} is registered twice", spec.name);
            }
        }
        map
    })
}

pub fn spec(name: &str) -> Option<&'static ConsumableSpec> {
    index().get(name).copied()
}

/// The spec of this name; panics with the name if it is not registered.
pub fn spec_or_panic(name: &str) -> &'static ConsumableSpec {
    spec(name).unwrap_or_else(|| panic!("no consumable named {:?} is registered", name))
}

/// A new instance of a registered consumable.
pub fn make(name: &str) -> ConsumableRef {
    make_ref(ConsumableInstance::new(spec_or_panic(name), Edition::None))
}

/// Cards of that kind, in the game's pool order (Python `by_kind`).
pub fn by_kind(kind: ConsumableKind) -> Vec<&'static ConsumableSpec> {
    let mut out: Vec<&'static ConsumableSpec> =
        all_specs().iter().filter(|s| s.kind == kind).collect();
    out.sort_by_key(|s| spec_order(s.name));
    out
}

/// The game's own pool position, from the generated `consumable_data` table.
pub fn spec_order(name: &str) -> i32 {
    crate::consumable_data::consumable_row(name)
        .map(|row| row.order)
        .unwrap_or(i32::MAX)
}

/// The pool a card of this kind may be drawn from, excluding what the run cannot
/// use. Python's `EXCLUDED_FROM_POOLS` plus the softlock rule.
pub fn is_excluded(name: &str) -> bool {
    crate::consumable_data::EXCLUDED_FROM_POOLS.contains(&name)
}
