//! Shop contents: joker/consumable slots, vouchers and booster packs.

use crate::cards::{CardRef, next_sort_id};
use crate::consumables::ConsumableSpec;
use crate::jokers::JokerRef;
use crate::pack_data::PACK_DATA;
use crate::voucher_data::VOUCHER_DATA;

/// One voucher, with the run state its redemption moves.
///
/// Every field here is something Card:apply_to_run sets. The ones it does not set
/// -- Telescope, Observatory, Omen Globe, Director's Cut -- are read where they
/// act rather than applied on redemption, so they carry no numbers and are
/// recognised by key.
#[derive(Clone, Copy, Debug, Default)]
pub struct Voucher {
    pub key: &'static str,
    pub name: &'static str,
    pub cost: i32,
    pub requires: &'static str,
    pub shop_slots: i32,
    pub consumable_slots: i32,
    pub extra_hands: i32,
    pub extra_discards: i32,
    pub reroll_discount: i32,
    pub discount_percent: i32,
    pub interest_cap: i32,
    pub joker_slots: i32,
    pub hand_size: i32,
    pub ante_shift: i32,
    pub tarot_rate: f64,
    pub planet_rate: f64,
    pub edition_rate: f64,
    pub playing_card_rate: f64,
}

impl Voucher {
    pub fn price_multiplier(&self) -> f64 {
        1.0 - self.discount_percent as f64 / 100.0
    }
}

/// What each voucher does, keyed by the game's name, as a function of the
/// `extra` the game stores alongside it.
///
/// Taken from Card:apply_to_run -- an upgrade is usually its base with a bigger
/// `extra`, which is why the effects are written once for both.
pub fn voucher_effect(name: &str, extra: f64) -> Voucher {
    let mut v = Voucher::default();
    match name {
        "Overstock" | "Overstock Plus" => v.shop_slots = 1,
        "Tarot Merchant" | "Tarot Tycoon" => v.tarot_rate = 4.0 * extra,
        "Planet Merchant" | "Planet Tycoon" => v.planet_rate = 4.0 * extra,
        "Hone" | "Glow Up" => v.edition_rate = extra,
        "Magic Trick" | "Illusion" => v.playing_card_rate = extra,
        "Crystal Ball" => v.consumable_slots = 1,
        "Clearance Sale" | "Liquidation" => v.discount_percent = extra as i32,
        "Reroll Surplus" | "Reroll Glut" => v.reroll_discount = extra as i32,
        // The game stores the cap in dollars held and pays one interest per five
        // of them: min(floor(dollars/5), interest_cap/5). The simulator counts
        // the payments, so fifty dollars held is a cap of ten.
        "Seed Money" | "Money Tree" => v.interest_cap = extra as i32 / 5,
        "Grabber" | "Nacho Tong" => v.extra_hands = extra as i32,
        "Wasteful" | "Recyclomancy" => v.extra_discards = extra as i32,
        "Paint Brush" | "Palette" => v.hand_size = 1,
        "Antimatter" => v.joker_slots = 1,
        // Hieroglyph and Petroglyph each take an ante away and pay for it with a
        // hand or a discard. Fewer antes is the whole point of them.
        "Hieroglyph" => {
            v.ante_shift = -(extra as i32);
            v.extra_hands = -(extra as i32);
        }
        "Petroglyph" => {
            v.ante_shift = -(extra as i32);
            v.extra_discards = -(extra as i32);
        }
        // Blank, Telescope, Omen Globe, Retcon...
        _ => {}
    }
    v
}

/// Every voucher, built from the game's own table.
pub fn all_vouchers() -> Vec<Voucher> {
    VOUCHER_DATA
        .iter()
        .map(|row| {
            let mut v = voucher_effect(row.name, row.extra);
            v.key = row.key;
            v.name = row.name;
            v.cost = row.cost;
            v.requires = row.requires;
            v
        })
        .collect()
}

pub fn voucher_by_key(key: &str) -> Option<Voucher> {
    all_vouchers().into_iter().find(|v| v.key == key)
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum PackKind {
    Arcana,
    Celestial,
    Standard,
    Buffoon,
    Spectral,
}

impl PackKind {
    pub fn as_str(self) -> &'static str {
        match self {
            PackKind::Arcana => "arcana",
            PackKind::Celestial => "celestial",
            PackKind::Standard => "standard",
            PackKind::Buffoon => "buffoon",
            PackKind::Spectral => "spectral",
        }
    }

    /// The game's capitalised set name, as `ConsumableKind::set_name` gives.
    pub fn title(self) -> &'static str {
        match self {
            PackKind::Arcana => "Arcana",
            PackKind::Celestial => "Celestial",
            PackKind::Standard => "Standard",
            PackKind::Buffoon => "Buffoon",
            PackKind::Spectral => "Spectral",
        }
    }

    pub fn from_name(name: &str) -> Option<PackKind> {
        Some(match name {
            "arcana" => PackKind::Arcana,
            "celestial" => PackKind::Celestial,
            "standard" => PackKind::Standard,
            "buffoon" => PackKind::Buffoon,
            "spectral" => PackKind::Spectral,
            _ => return None,
        })
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PackSpec {
    pub kind: PackKind,
    /// `"normal" | "jumbo" | "mega"`.
    pub size: &'static str,
    /// cards shown
    pub options: i32,
    /// cards you may take
    pub picks: i32,
    pub cost: i32,
    /// the game's own centre key, e.g. `p_arcana_mega_1`
    pub key: &'static str,
}

impl PackSpec {
    pub fn name(&self) -> String {
        let prefix = match self.size {
            "jumbo" => "Jumbo ",
            "mega" => "Mega ",
            _ => "",
        };
        format!("{}{} Pack", prefix, self.kind.title())
    }
}

/// A PackSpec from one row of the game's Booster pool.
pub fn pack_from_row(row: &crate::pack_data::PackRow) -> PackSpec {
    PackSpec {
        kind: PackKind::from_name(&row.kind.to_lowercase()).unwrap_or(PackKind::Arcana),
        // key is `p_<kind>_<size>_<n>`; Python takes the third field.
        size: row.key.split('_').nth(2).unwrap_or("normal"),
        options: row.cards,
        picks: row.choose,
        cost: row.cost,
        key: row.key,
    }
}

pub fn all_packs() -> Vec<PackSpec> {
    PACK_DATA.iter().map(pack_from_row).collect()
}

pub fn pack_from_key(key: &str) -> PackSpec {
    all_packs()
        .into_iter()
        .find(|p| p.key == key)
        .unwrap_or_else(|| panic!("no booster pack with key {:?}", key))
}

/// One purchasable item in the shop's main row.
#[derive(Clone, Debug)]
pub struct ShopSlot {
    /// `"joker" | "consumable" | "card"`
    pub kind: &'static str,
    /// The game's own `base_cost`, not what it sells for. A price is not fixed
    /// when the shop stocks: `Card:set_cost` runs over every card on screen
    /// whenever anything that touches a price changes, and a Clearance Sale
    /// redeemed from this very shop is exactly that. `GameState::slot_price`
    /// works the sale price out from this, every time it is asked.
    pub base_cost: i32,
    /// An edition tag or the Coupon Tag marks a card couponed, which is the
    /// game's way of saying "this one is free" -- set_cost zeroes it.
    pub couponed: bool,
    pub joker: Option<JokerRef>,
    pub consumable: Option<&'static ConsumableSpec>,
    pub card: Option<CardRef>,
    /// The consumable's age: the game made the card when it stocked the shop,
    /// not when it was bought. A Tarot stocked before a pack was opened is older
    /// than one the pack's joker made, whichever reached the row first.
    pub sort_id: u64,
}

impl ShopSlot {
    pub fn new(kind: &'static str, base_cost: i32) -> Self {
        ShopSlot {
            kind,
            base_cost,
            couponed: false,
            joker: None,
            consumable: None,
            card: None,
            sort_id: next_sort_id(),
        }
    }

    pub fn label(&self) -> String {
        if let Some(joker) = &self.joker {
            return joker.borrow().to_string();
        }
        if let Some(spec) = self.consumable {
            return spec.name.to_string();
        }
        match &self.card {
            Some(card) => card.borrow().to_string(),
            None => String::new(),
        }
    }
}

/// The shop's whole row: main slots, boosters and vouchers.
#[derive(Clone, Debug, Default)]
pub struct Shop {
    pub slots: Vec<ShopSlot>,
    pub packs: Vec<PackSpec>,
    /// G.shop_vouchers, whose card limit is one plus however many Voucher Tags
    /// were held when the shop opened -- each raises it by one and emplaces a
    /// card. There is no ceiling on that: sell a stack of Diet Colas for Double
    /// Tags, take a Voucher Tag, and every Double copies it, so the row can be
    /// arbitrarily long. Buying takes the card out of the row, so this list is
    /// exactly what is still for sale.
    pub vouchers: Vec<Voucher>,
    pub rerolls: i32,
    /// Chaos the Clown's free reroll. current_round.free_rerolls in the game,
    /// topped up as the shop opens.
    pub free_rerolls: i32,
    /// The D6 Tag, which sets round_resets.temp_reroll_cost to zero for this
    /// shop. Not a spare reroll like Chaos: the price starts at nothing and
    /// climbs from there as usual.
    pub free_reroll_cost: bool,
    /// A reroll voucher bought here cuts the current price on the spot --
    /// `current_round.reroll_cost - extra`, floored at 0 (card.lua:1925-1929) --
    /// and the cut stands until the next reroll works the price out again. Only
    /// visible under the D6 Tag, whose price leaves the vouchers out.
    pub cut_until_reroll: i32,
}

impl Shop {
    /// The vouchers still buyable, in the order the shop shows them.
    pub fn vouchers_on_offer(&self) -> &[Voucher] {
        &self.vouchers
    }

    /// What the next reroll costs.
    ///
    /// A free reroll is free *and* does not raise the price of the next one:
    /// calculate_reroll_cost returns before it increments, so Chaos the Clown's
    /// reroll is genuinely a spare rather than a discount on the first of a
    /// series.
    pub fn reroll_cost(&self, discount: i32) -> i32 {
        if self.free_rerolls > 0 {
            return 0;
        }
        self.reroll_price(discount)
    }

    /// The price a reroll has once no free one is left.
    ///
    /// `(temp_reroll_cost or round_resets.reroll_cost) + increase`
    /// (common_events.lua:2268). The reroll vouchers lower
    /// round_resets.reroll_cost, so the D6 Tag's temp price replaces the discount
    /// along with the base: its rerolls climb 0, 1, 2 whatever vouchers the run
    /// holds.
    pub fn reroll_price(&self, discount: i32) -> i32 {
        if self.free_reroll_cost {
            return (self.rerolls - self.cut_until_reroll).max(0);
        }
        (5 + self.rerolls - discount).max(0)
    }
}
