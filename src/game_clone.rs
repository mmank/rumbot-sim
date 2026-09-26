//! The Python `copy.deepcopy` of a run, which the policy forks through.
//!
//! `src/handcrafted/position.py`'s `fork` is built on `copy.deepcopy(game)`, and
//! the whole policy is priced by lookahead: put a joker in the row and score a
//! hand, throw these cards and score the draw, become a Pair build and price the
//! payoff. Rust's derived `Clone` cannot stand in for it. A `GameState` holds
//! `Rc<RefCell<Card>>` shared across `full_deck`, `draw_pile`, `hand` and
//! `discard_pile`, so `clone()` copies the *pointer*: buying a joker or playing a
//! card on the fork then mutates the run it was forked from. Every rule would
//! still look right and every number would be wrong, which is the worst kind of
//! bug to have in a policy.
//!
//! So this is the deep copy, with Python's identity semantics kept: one new
//! `Card` per original uid, and every pile that held a given card holds the same
//! new one. The memo is keyed by the `Rc`'s address, exactly as `deepcopy`'s is
//! keyed by `id(obj)`, so a card that appears in three lists is copied once.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::cards::{Card, CardRef};
use crate::consumables::{ConsumableInstance, ConsumableRef};
use crate::game::{GameState, PackChoice};
use crate::jokers::{JokerInstance, JokerRef};
use crate::shop::{Shop, ShopSlot};

/// The memo `deepcopy` would keep: old `Rc` address -> the one copy made.
#[derive(Default)]
struct Memo {
    cards: HashMap<usize, CardRef>,
    jokers: HashMap<usize, JokerRef>,
    consumables: HashMap<usize, ConsumableRef>,
}

impl Memo {
    fn card(&mut self, old: &CardRef) -> CardRef {
        let key = Rc::as_ptr(old) as *const () as usize;
        if let Some(new) = self.cards.get(&key) {
            return new.clone();
        }
        let c = old.borrow();
        let new = Rc::new(RefCell::new(Card {
            rank: c.rank,
            suit: c.suit,
            enhancement: c.enhancement,
            edition: c.edition,
            seal: c.seal,
            extra_chips: c.extra_chips,
            original_suit: c.original_suit,
            played_this_ante: c.played_this_ante,
            uid: c.uid,
            debuffed: c.debuffed,
        }));
        drop(c);
        self.cards.insert(key, new.clone());
        new
    }

    fn card_opt(&mut self, old: &Option<CardRef>) -> Option<CardRef> {
        old.as_ref().map(|c| self.card(c))
    }

    fn joker(&mut self, old: &JokerRef) -> JokerRef {
        let key = Rc::as_ptr(old) as *const () as usize;
        if let Some(new) = self.jokers.get(&key) {
            return new.clone();
        }
        let j = old.borrow();
        let new = Rc::new(RefCell::new(JokerInstance {
            spec: j.spec,
            uid: j.uid,
            edition: j.edition,
            counter: j.counter,
            eternal: j.eternal,
            perishable: j.perishable,
            perish_tally: j.perish_tally,
            rental: j.rental,
            debuffed: j.debuffed,
            hands_at_create: j.hands_at_create,
            secondary: j.secondary,
            extra_sell_value: j.extra_sell_value,
            named_hand: j.named_hand,
        }));
        drop(j);
        self.jokers.insert(key, new.clone());
        new
    }

    fn consumable(&mut self, old: &ConsumableRef) -> ConsumableRef {
        let key = Rc::as_ptr(old) as *const () as usize;
        if let Some(new) = self.consumables.get(&key) {
            return new.clone();
        }
        let c = old.borrow();
        let new = Rc::new(RefCell::new(ConsumableInstance {
            spec: c.spec,
            edition: c.edition,
            extra_sell_value: c.extra_sell_value,
            uid: c.uid,
        }));
        drop(c);
        self.consumables.insert(key, new.clone());
        new
    }

    fn cards(&mut self, old: &[CardRef]) -> Vec<CardRef> {
        old.iter().map(|c| self.card(c)).collect()
    }

    fn jokers(&mut self, old: &[JokerRef]) -> Vec<JokerRef> {
        old.iter().map(|j| self.joker(j)).collect()
    }

    fn consumables(&mut self, old: &[ConsumableRef]) -> Vec<ConsumableRef> {
        old.iter().map(|c| self.consumable(c)).collect()
    }

    fn slot(&mut self, old: &ShopSlot) -> ShopSlot {
        ShopSlot {
            kind: old.kind,
            base_cost: old.base_cost,
            couponed: old.couponed,
            joker: old.joker.as_ref().map(|j| self.joker(j)),
            consumable: old.consumable,
            card: old.card.as_ref().map(|c| self.card(c)),
            sort_id: old.sort_id,
        }
    }

    fn shop(&mut self, old: &Shop) -> Shop {
        Shop {
            slots: old.slots.iter().map(|s| self.slot(s)).collect(),
            packs: old.packs.clone(),
            vouchers: old.vouchers.clone(),
            rerolls: old.rerolls,
            free_rerolls: old.free_rerolls,
            free_reroll_cost: old.free_reroll_cost,
            cut_until_reroll: old.cut_until_reroll,
        }
    }

    fn pack_choice(&mut self, old: &PackChoice) -> PackChoice {
        match old {
            PackChoice::Joker(j) => PackChoice::Joker(self.joker(j)),
            PackChoice::Card(c) => PackChoice::Card(self.card(c)),
            PackChoice::Consumable(spec) => PackChoice::Consumable(spec),
        }
    }
}

impl GameState {
    /// Python's `copy.deepcopy(self)`, identity included.
    pub fn deep_clone(&self) -> GameState {
        let mut memo = Memo::default();
        GameState {
            seed: self.seed.clone(),
            deck: self.deck.clone(),
            rng: self.rng.clone(),

            ante: self.ante,
            blind_index: self.blind_index,
            round_number: self.round_number,
            money: self.money,
            money_at_play: self.money_at_play,

            full_deck: memo.cards(&self.full_deck),
            starting_deck_size: self.starting_deck_size,
            draw_pile: memo.cards(&self.draw_pile),
            hand: memo.cards(&self.hand),
            discard_pile: memo.cards(&self.discard_pile),

            jokers: memo.jokers(&self.jokers),
            consumables: memo.consumables(&self.consumables),
            vouchers: self.vouchers.clone(),
            tags: self.tags.clone(),
            ante_tags: self.ante_tags.clone(),
            using_from_pack: self.using_from_pack,
            using_key: self.using_key.clone(),
            playing_tarot: self.playing_tarot,
            blind_select_events: self.blind_select_events.clone(),
            getting_sliced: self.getting_sliced.clone(),
            joker_buffer: self.joker_buffer,
            dollar_rows: self.dollar_rows.as_ref().map(|r| memo.jokers(r)),
            ante_tag_keys: self.ante_tag_keys.clone(),

            hand_levels: self.hand_levels.clone(),

            base_hand_size: self.base_hand_size,
            base_joker_slots: self.base_joker_slots,
            extra_consumable_slots: self.extra_consumable_slots,

            blind: self.blind.clone(),
            beaten_blind: self.beaten_blind.clone(),
            pending_payout: self.pending_payout,
            beaten_was_boss: self.beaten_was_boss,
            hand_sort: self.hand_sort.clone(),
            last_hand: self.last_hand.clone(),
            hands_played: self.hands_played,
            idol_rank: self.idol_rank,
            idol_suit: self.idol_suit,
            ancient_suit: self.ancient_suit,
            mail_rank: self.mail_rank,
            castle_suit: self.castle_suit,

            discards_used: self.discards_used,
            bosses_used: self.bosses_used.clone(),
            tarots_used: self.tarots_used,
            planets_used: self.planets_used,
            unique_planets: self.unique_planets.clone(),
            pool_flags: self.pool_flags.clone(),
            rerolls: self.rerolls,
            blinds_skipped: self.blinds_skipped,
            unused_discards: self.unused_discards,
            orbital_choices: self.orbital_choices.clone(),
            temp_hand_size: self.temp_hand_size,
            most_played_hand: self.most_played_hand,
            best_hand: self.best_hand,
            free_rerolls_carried: self.free_rerolls_carried,
            reroll_price_carried: self.reroll_price_carried,
            temp_reroll_cost: self.temp_reroll_cost,
            skipped_this_ante: self.skipped_this_ante.clone(),
            cards_sold: self.cards_sold,
            glass_destroyed: self.glass_destroyed,
            cards_created: self.cards_created,
            lucky_triggers: self.lucky_triggers,
            chips_scored: self.chips_scored,
            hands_left: self.hands_left,
            discards_left: self.discards_left,
            hands_played_this_round: self.hands_played_this_round.clone(),
            mouth_only_hand: self.mouth_only_hand,

            phase: self.phase,
            shop: self.shop.as_ref().map(|s| memo.shop(s)),
            pack: self.pack,
            first_shop_buffoon: self.first_shop_buffoon,
            round_voucher: self.round_voucher.clone(),
            pack_dealt_hand: self.pack_dealt_hand,
            shop_free: self.shop_free,
            last_tarot_planet: self.last_tarot_planet.clone(),
            ecto_minus: self.ecto_minus,
            ante_boss: self.ante_boss.clone(),
            forced_card: memo.card_opt(&self.forced_card),
            boss_rerolled: self.boss_rerolled,
            stake: self.stake,
            all_stickers: self.all_stickers,
            endless: self.endless,
            pack_options: self
                .pack_options
                .iter()
                .map(|o| memo.pack_choice(o))
                .collect(),
            pack_picks_left: self.pack_picks_left,

            preview_expected: self.preview_expected,

            logs: self.logs.clone(),
            verbose: self.verbose,
        }
    }
}
