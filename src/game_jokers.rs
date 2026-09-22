//! Joker-lifecycle, sticker and price methods of `GameState`, split out of
//! `game.rs`.
//!
//! A joker enters the row through `gain_joker`, leaves only through
//! `destroy_joker`, and everything that feeds on it leaving is told from there.
//! The counters a joker moves the moment it joins or leaves the deck live in
//! `_move_joker_counters`, and the stickers a stake puts on a shop joker in
//! `_apply_stickers`.
//!
//! Prices are the game's own `Card:set_cost`: `card_cost` is the one formula and
//! `sell_value`/`consumable_sell_value` are half of it, discount included.

use std::collections::HashSet;
use std::rc::Rc;

use crate::cards::Edition;
use crate::consumables::{ConsumableKind, ConsumableRef};
use crate::game::{BlindSelectEvent, GameState, PackChoice, Phase};
use crate::jokers::{edition_value, JokerInstance, JokerRef, Rarity};

/// Which stickers the stake lets the shop put on a joker.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StickerRules {
    pub eternals: bool,
    pub perishables: bool,
    pub rentals: bool,
}

/// `_EDITION_BY_NAME`, the game's names for the polled edition.
fn edition_by_name(name: &str) -> Edition {
    match name {
        "foil" => Edition::Foil,
        "holo" => Edition::Holographic,
        "polychrome" => Edition::Polychrome,
        "negative" => Edition::Negative,
        _ => Edition::None,
    }
}

impl GameState {
    pub fn add_money(&mut self, amount: i32, source: &str) {
        self.money += amount;
        if amount != 0 && !source.is_empty() {
            let sign = if amount >= 0 { '+' } else { '-' };
            self.log(format!("{}: {}${}", source, sign, amount.abs()));
        }
    }

    /// Remove a joker, unless it is eternal.
    ///
    /// Nothing removes an eternal joker: not selling it, not Hex, not Ankh, not
    /// Madness, not going extinct. The game checks at each call site and the
    /// checks are easy to miss one of, so the gate is here instead -- anything
    /// that wants a joker gone has to come through this.
    pub fn destroy_joker(&mut self, joker: &JokerRef, reason: &str) {
        if joker.borrow().eternal {
            return;
        }
        // Python's `joker in self.jokers` is identity, not value: two handles
        // that print the same joker are still two jokers.
        if let Some(index) = self.jokers.iter().position(|j| Rc::ptr_eq(j, joker)) {
            let removed = self.jokers.remove(index);
            self._move_joker_counters(&removed, false);
            let name = removed.borrow().name();
            if reason.is_empty() {
                self.log(format!("{} destroyed", name));
            } else {
                self.log(format!("{} destroyed ({})", name, reason));
            }
        }
    }

    /// Madness or Ceremonial Dagger taking a joker as the blind is set.
    ///
    /// The game only marks it -- `getting_sliced = true` (card.lua:2512, 2568)
    /// -- and dissolves it in an event, so it keeps its place and its slot,
    /// doing nothing, until the setting_blind pass is over. See _setting_blind.
    /// Outside the pass there is nothing to wait for.
    pub fn slice_joker(&mut self, victim: &JokerRef, reason: &str) {
        if self.blind_select_events.is_none() {
            self.destroy_joker(victim, reason);
            return;
        }
        self.getting_sliced
            .push((victim.borrow().uid, reason.to_string()));
    }

    pub fn is_getting_sliced(&self, joker: &JokerRef) -> bool {
        let uid = joker.borrow().uid;
        self.getting_sliced.iter().any(|(victim, _)| *victim == uid)
    }

    /// Run what a setting_blind effect leaves to an event, once the pass ends.
    ///
    /// Riff-raff's jokers (card.lua:2532-2543): no joker later in the row sees
    /// them. Outside the pass, at once.
    ///
    /// Python queues *closures*; Rust's queue holds a `BlindSelectEvent` and the
    /// pass performs it (game.py:1993-1997). Outside the pass there is nothing
    /// to wait for, so the event is performed here. The `joker_buffer = 0` that
    /// a Dagger's callback also carries (jokers.py:1138) has no variant and is
    /// the pass's business, not this queue's.
    pub fn after_setting_blind(&mut self, event: BlindSelectEvent) {
        if self.blind_select_events.is_none() {
            match event {
                BlindSelectEvent::CreateJokers { count, .. } => {
                    // Riff-Raff emplaces without asking (card.lua:2536): the
                    // count was the check, and a Dagger's victim may still be
                    // sitting in the row.
                    for _ in 0..count {
                        self.add_random_joker(
                            "Riff-Raff",
                            Some(Rarity::Common),
                            false,
                            "rif",
                            true,
                        );
                    }
                    self.joker_buffer = 0;
                }
                BlindSelectEvent::DestroyJoker { victim_uid, reason } => {
                    let victim = self
                        .jokers
                        .iter()
                        .find(|j| j.borrow().uid == victim_uid)
                        .cloned();
                    if let Some(victim) = victim {
                        self.destroy_joker(&victim, &reason);
                    }
                }
            }
        } else if let Some(events) = self.blind_select_events.as_mut() {
            events.push(event);
        }
    }

    /// A joker from the game's own pool, not from a list of every joker.
    ///
    /// `append` is the key_append the thing creating it uses -- "jud" for
    /// Judgement, "sou" for The Soul, "wra" for Wraith -- and it names the
    /// stream, so getting it wrong draws the right joker from the wrong place.
    /// A forced rarity skips the rarity roll entirely, which is how Wraith is
    /// always rare and never legendary.
    ///
    /// The joker polls an edition too, under the same append: create_card ends
    /// every joker with `poll_edition('edi'..(key_append or '')..ante)`
    /// (common_events.lua:2149), whatever area it was made for. Nothing here
    /// polled one, so a Polychrome Red Card out of a Riff-Raff arrived plain,
    /// and a Judgement, a Soul, a Wraith or a Top-up Tag never made a Foil, a
    /// Holographic or a Negative. The append is kept for a legendary here:
    /// get_current_pool drops it from the *pool* key, create_card does not drop
    /// it from the edition key.
    pub fn add_random_joker(
        &mut self,
        source: &str,
        rarity: Option<Rarity>,
        legendary: bool,
        append: &str,
        room_checked: bool,
    ) {
        // `room_checked` is for a creator that counted the room itself and
        // emplaces whatever the row holds by then: Riff-raff, whose count was
        // fixed mid-pass while a Dagger's victim still sat in its slot.
        if !room_checked && (self.jokers.len() as i32) >= self.joker_slots() {
            return;
        }
        // create_card polls the edition under the same append whatever area the
        // card was made for (common_events.lua:2149).
        let owned: Vec<String> = self
            .full_deck
            .iter()
            .map(|c| format!("m_{}", c.borrow().enhancement.as_str()))
            .collect();
        let seen = self.seen_centers();
        let seen_vec: Vec<String> = seen.into_iter().collect();
        let flags: Vec<String> = self.pool_flags.iter().cloned().collect();
        let showman = self
            .active_jokers()
            .iter()
            .any(|j| j.borrow().spec.allows_duplicates);
        // A forced rarity skips the rarity roll entirely, which is how Wraith is
        // always rare and never legendary; `legendary` names the pool key
        // "Joker4" rather than a rarity flag.
        let rarity_index = if legendary {
            Some(4)
        } else {
            rarity.map(|r| r as u8)
        };
        let key = crate::shop_pool::draw_joker(
            &mut self.rng,
            self.ante,
            &owned,
            &seen_vec,
            showman,
            rarity_index,
            &flags,
            append,
        );
        let name = crate::shop_pool::name_by_joker_key(&key)
            .unwrap_or_else(|| panic!("no joker key {:?} in the registry", key));
        let spec = crate::jokers::spec_or_panic(name);
        let edition_rate = self.edition_rate();
        let edition_key = format!("edi{}{}", append, self.ante);
        let edition_name = crate::shop_pool::poll_edition(
            &mut self.rng,
            &edition_key,
            1.0,
            false,
            edition_rate,
            false,
        );
        let joker = crate::jokers::make_ref(JokerInstance::new(spec));
        joker.borrow_mut().edition = edition_by_name(edition_name);
        // A joker has just been built: what Card:set_ability does with it. For a
        // To Do List that is rolling its hand (card.lua:311-322) -- from every
        // visible hand, on the same 'to_do' stream as the round-end roll, and for
        // every card built whether or not anyone buys it: a shop builds its whole
        // shelf, a Buffoon pack its whole spread. Left to the round-end roll, a
        // To Do List bought mid-ante named nothing and paid nothing for its first
        // round, and each one built without its draw put every later roll in the
        // run a draw out of step with the game's.
        let joker = self._made_joker(joker);
        self.gain_joker(&joker);
        self.log(format!("{}: gained {}", source, spec.name));
    }

    /// Tell the jokers that count sales that one has happened.
    ///
    /// Campfire gains X0.25 for every card sold, of any kind, and resets to
    /// X1 when a Boss Blind is beaten. The counter it reads was sitting at
    /// its starting value for whole runs.
    pub fn note_card_sold(&mut self) {
        self.cards_sold += 1;
        for joker in self.calculating_jokers() {
            let name = joker.borrow().name();
            if name == "Campfire" {
                joker.borrow_mut().counter += 0.25;
            }
        }
    }

    /// Put a joker in the row, stamped with when it arrived.
    ///
    /// The game records hands_played_at_create on every card it builds, and
    /// the jokers that count hands measure from there rather than from the
    /// start of the run -- Loyalty Card's X4 lands on the sixth hand since it
    /// was bought, not the sixth of the run. Nothing was stamping it, so
    /// every joker behaved as though it had been there from the beginning and
    /// Loyalty Card fired on the wrong hand for the whole game.
    pub fn gain_joker(&mut self, joker: &JokerRef) {
        joker.borrow_mut().hands_at_create = self.hands_played;
        // No age is stamped here: the joker has had one since it was built
        // (JokerInstance.uid). Restamping on arrival made the age a purchase
        // order, so Madness, the Wheel of Fortune, Ectoplasm and Hex drew the
        // wrong joker whenever a shop was bought out of slot order.
        self.jokers.push(joker.clone());
        // Chaos the Clown hands over its free reroll the moment it joins the
        // row -- Card:add_to_deck does it -- so buying one in a shop you are
        // standing in gives you a reroll in that shop. Topping up only when
        // the shop opens misses exactly that, which is when anyone would buy
        // it.
        self._move_joker_counters(joker, true);
    }

    /// The run-level counters Card:add_to_deck and remove_from_deck move.
    ///
    /// Both are immediate rather than next-round, which is the whole point: a
    /// Chaos the Clown bought in a shop hands over its reroll in that shop,
    /// and a Merry Andy hands over its three discards the moment it is
    /// bought. The round allowance counts these jokers as well, so leaving
    /// them out here did not lose the discards -- it delayed them by a round,
    /// which is worse, because it looks right everywhere except the shop you
    /// bought it in.
    ///
    /// ```text
    ///     if self.ability.d_size > 0 then
    ///         G.GAME.round_resets.discards = ... + self.ability.d_size
    ///         ease_discard(self.ability.d_size)
    ///     end
    /// ```
    ///
    /// Recording 8 stopped on exactly that at step 206 of 443: five discards
    /// recorded against two simulated, one action after a Merry Andy was
    /// bought.
    pub fn _move_joker_counters(&mut self, joker: &JokerRef, arriving: bool) {
        // Both halves open on added_to_deck (card.lua:566, 646), and a debuff
        // has already run remove_from_deck(true): selling a joker Crimson Heart
        // holds gives nothing back a second time. set_joker_debuff clears the
        // flag before it puts a joker back.
        if joker.borrow().debuffed {
            return;
        }
        let rerolls = joker.borrow().spec.free_rerolls;
        if rerolls != 0 {
            if let Some(shop) = self.shop.as_mut() {
                shop.free_rerolls = if arriving {
                    shop.free_rerolls + rerolls
                } else {
                    (shop.free_rerolls - rerolls).max(0)
                };
            } else {
                self.free_rerolls_carried = if arriving {
                    self.free_rerolls_carried + rerolls
                } else {
                    (self.free_rerolls_carried - rerolls).max(0)
                };
            }
        }
        // Strictly `> 0`, as the game has it: a negative d_size takes nothing
        // away on arrival. Clamped on the way down because ease_discard is
        // `mod = math.max(-G.GAME.current_round.discards_left, mod)`.
        let discards = joker.borrow().spec.extra_discards;
        if discards > 0 {
            self.discards_left = if arriving {
                self.discards_left + discards
            } else {
                (self.discards_left - discards).max(0)
            };
        }
        // change_size for the joker's hand size: h_size, Turtle Bean,
        // Troubadour and Stuntman on the way in (card.lua:587, 606, 624, 628)
        // and the reverse on the way out (card.lua:649, 663, 681, 685).
        // Nothing for a debuffed joker -- its debuff already ran
        // remove_from_deck(true), and both are guarded by added_to_deck --
        // which is also why hand_size reads active_jokers.
        let size = {
            let j = joker.borrow();
            if j.spec.hand_size_from_counter {
                j.counter as i32
            } else {
                j.spec.hand_size
            }
        };
        self._hand_size_changed(if arriving { size } else { -size });
        // And both end by resetting the blind (card.lua add_to_deck and
        // remove_from_deck: set_blind(nil, true)), which asks every playing
        // card its debuff again: a Smeared Joker sold under The Window lets
        // go of the Hearts it had caught, a Pareidolia bought under The Plant
        // catches every card. Not the jokers -- a reset skips them.
        //
        // The row is cloned (cheap: these are handles) so the loop can hold the
        // deck while asking the boss, which needs the run mutably.
        for card in self.full_deck.clone() {
            self.debuff_card(&card);
        }
    }

    /// Deal into a hand size that has just grown, as change_size does.
    ///
    /// hand_size is derived from the row, so the limit itself has already
    /// moved; what the game also does is deal. CardArea:change_size
    /// (cardarea.lua:94-111):
    ///
    /// ```text
    ///     if delta > 0 and self.config.real_card_limit > 1 and self == G.hand
    ///        and self.cards[1] and (G.STATE == G.STATES.DRAW_TO_HAND
    ///                               or G.STATE == G.STATES.SELECTING_HAND)
    ///     then for i=1, math.abs(delta) do draw_card(G.deck, G.hand, ...)
    ///              ... self:sort() ... end end
    /// ```
    ///
    /// |delta| cards off the top of the deck, not a top-up to the limit: a
    /// hand already over it still gets them. A decrease only lowers the limit
    /// and discards nothing. U2EBFAQ2 stopped on this at decision 186: the
    /// policy sold Stuntman while selecting a hand, the game held 10 cards
    /// and the simulator 8.
    ///
    /// Not during a consumable, whose G.STATE is PLAY_TAROT until after the
    /// change_size event has run (see playing_tarot): on the engine a Hex
    /// that destroys a Stuntman and a Judgement that makes a Juggler both
    /// raise the limit and deal nothing.
    pub fn _hand_size_changed(&mut self, delta: i32) {
        if delta <= 0 || self.playing_tarot {
            return;
        }
        if self.phase != Phase::Playing || self.hand.is_empty() {
            return;
        }
        // real_card_limit is unfloored, but after an increase it is above one
        // exactly when the floored hand_size is.
        if self.hand_size() <= 1 {
            return;
        }
        self._draw_cards(delta);
    }

    /// Card:set_debuff on a joker (card.lua:526-538).
    ///
    /// A perished joker stays debuffed whatever is asked. Otherwise a change
    /// takes the joker out of the deck or puts it back -- remove_from_deck(true)
    /// or add_to_deck(true) -- so its hand size, discards and rerolls go and
    /// come with it, and a hand size given back is dealt into
    /// (_hand_size_changed). Crimson Heart's pick and the release when the
    /// blind is disabled or beaten both come through here.
    pub fn set_joker_debuff(&mut self, joker: &JokerRef, debuff: bool) {
        if joker.borrow().perishable && joker.borrow().perish_tally <= 0 {
            joker.borrow_mut().debuffed = true;
            return;
        }
        if joker.borrow().debuffed == debuff {
            return;
        }
        if debuff {
            // Out while the flag is still clear: _move_joker_counters leaves
            // a joker that is already debuffed alone.
            self._move_joker_counters(joker, false);
            joker.borrow_mut().debuffed = true;
        } else {
            joker.borrow_mut().debuffed = false;
            self._move_joker_counters(joker, true);
        }
    }

    /// A copy of a joker already held, if the row has room for it.
    pub fn add_joker_copy(&mut self, joker: &JokerRef, source: &str) {
        if (self.jokers.len() as i32) >= self.joker_slots() {
            return;
        }
        self.copy_joker(joker, source);
    }

    /// copy_card, add_to_deck and emplace (common_events.lua:2156-2181).
    ///
    /// Everything in the ability table comes across -- counters, stickers,
    /// and hands_played_at_create too: set_ability stamps the new card's own
    /// (card.lua:337) and the loop over other.ability writes the original's
    /// over it, so a copied Loyalty Card keeps the original's cycle.
    ///
    /// Except a Negative. Both callers pass strip_edition for one (Ankh at
    /// card.lua:1445, Invisible Joker at 2384), which skips set_edition, so
    /// the copy has no edition at all rather than a free slot of its own.
    ///
    /// It is a new card all the same: Card:init gives it the next sort_id
    /// (a deepcopy would keep the original's age), and set_ability runs
    /// before the ability table is copied over, so a To Do List copy spends
    /// its creation draw and then keeps the original's hand.
    pub fn copy_joker(&mut self, joker: &JokerRef, source: &str) -> JokerRef {
        // Python `copy.deepcopy`; every field comes across, then Card:init's
        // fresh age is stamped over the copied uid.
        let clone = {
            let j = joker.borrow();
            JokerInstance {
                spec: j.spec,
                uid: crate::cards::next_sort_id(),
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
            }
        };
        let clone = crate::jokers::make_ref(clone);
        let clone = self._made_joker(clone);
        // ...and *then* keeps the original's hand. It is a new card all the same:
        // Card:init gives it the next sort_id (a deepcopy would keep the
        // original's age), and set_ability runs before the ability table is
        // copied over, so a To Do List copy spends its creation draw and then
        // keeps the original's hand.
        clone.borrow_mut().named_hand = joker.borrow().named_hand;
        if joker.borrow().edition == Edition::Negative {
            clone.borrow_mut().edition = Edition::None;
        }
        self.gain_joker(&clone);
        clone.borrow_mut().hands_at_create = joker.borrow().hands_at_create;
        let name = joker.borrow().name();
        self.log(format!("{}: copied {}", source, name));
        clone
    }

    /// The centres that currently exist, which is what blanks a pool.
    ///
    /// This was a set that only ever grew -- every card the run had ever
    /// drawn -- and that is not what the game keeps. G.GAME.used_jokers is
    /// set when a card is built and *cleared* when the last card of that name
    /// is removed, so it is closer to an inventory than a history: the shop's
    /// current cards count, the shop before the reroll does not, and a Tarot
    /// the player used is available again immediately.
    ///
    /// The difference is not small. Twenty-eight actions into a real run the
    /// engine had three entries where the simulator had twenty, so the
    /// simulator was drawing from a pool with seventeen cards wrongly blanked
    /// -- which is how a purple seal handed over Strength where the run was
    /// given the Wheel of Fortune.
    pub fn seen_centers(&self) -> HashSet<String> {
        let mut keys = HashSet::new();
        for joker in &self.jokers {
            if let Some(row) = crate::joker_data::joker_row(joker.borrow().name()) {
                keys.insert(row.key.to_string());
            }
        }
        for held in &self.consumables {
            if let Some(row) = crate::consumable_data::consumable_row(held.borrow().spec.name) {
                keys.insert(row.key.to_string());
            }
        }
        if let Some(shop) = &self.shop {
            for slot in &shop.slots {
                if let Some(joker) = &slot.joker {
                    if let Some(row) = crate::joker_data::joker_row(joker.borrow().name()) {
                        keys.insert(row.key.to_string());
                    }
                } else if let Some(spec) = slot.consumable {
                    if let Some(row) = crate::consumable_data::consumable_row(spec.name) {
                        keys.insert(row.key.to_string());
                    }
                }
            }
        }
        for option in &self.pack_options {
            // Python reads a bare `.name`; a playing card has none, so only a
            // joker or a consumable names a centre here.
            let name = match option {
                PackChoice::Joker(joker) => Some(joker.borrow().name()),
                PackChoice::Consumable(spec) => Some(spec.name),
                PackChoice::Card(_) => None,
            };
            if let Some(name) = name {
                if let Some(row) = crate::joker_data::joker_row(name) {
                    keys.insert(row.key.to_string());
                } else if let Some(row) = crate::consumable_data::consumable_row(name) {
                    keys.insert(row.key.to_string());
                }
            }
        }
        // The consumable in use is in no area but still exists until its
        // effect has run -- its used_jokers entry is cleared by Card:remove
        // (card.lua:4741-4749), which comes after. Without this The Emperor
        // drew from a pool with itself back in it.
        if !self.using_key.is_empty() {
            keys.insert(self.using_key.clone());
        }
        keys
    }

    /// How far into debt the run may go. Credit Card lowers the floor.
    pub fn bankrupt_at(&self) -> i32 {
        -self
            .active_jokers()
            .iter()
            .map(|j| j.borrow().spec.debt_limit)
            .sum::<i32>()
    }

    /// What the run may lay out, which is not what it holds.
    ///
    /// Every affordability test in the game is `cost > dollars -
    /// bankrupt_at`, and Credit Card exists only to move bankrupt_at. Testing
    /// against money alone -- which is what every check here did -- made
    /// Credit Card an entirely inert purchase, and it is a joker whose whole
    /// text is the twenty dollars of credit. They stack, too: measured on the
    /// engine at $0, one allows a $20 buy and refuses $21, two allow $40 and
    /// refuse $41.
    pub fn spendable(&self) -> i32 {
        self.money - self.bankrupt_at()
    }

    /// The game's test, including its free-item escape.
    ///
    /// `(cost > dollars - bankrupt_at) and (cost > 0)` -- so something free
    /// is always takeable, even by a run already past its floor.
    pub fn affords(&self, cost: i32) -> bool {
        cost <= 0 || cost <= self.spendable()
    }

    /// Which stickers the stake lets the shop put on a joker.
    pub fn sticker_rules(&self) -> StickerRules {
        if self.all_stickers {
            StickerRules {
                eternals: true,
                perishables: true,
                rentals: true,
            }
        } else {
            StickerRules {
                eternals: self.stake >= 4,
                perishables: self.stake >= 7,
                rentals: self.stake >= 8,
            }
        }
    }

    /// Poll a shop joker's stickers onto it.
    ///
    /// The first poll happens whether or not any sticker is enabled, so it
    /// is made on every stake -- see shop_pool.poll_stickers. What a rental
    /// then costs is `slot_price`'s business: set_cost puts it at a dollar
    /// after the discount, however expensive the joker is, which is six
    /// dollars a recording said the run still had.
    pub fn _apply_stickers(&mut self, joker: &JokerRef, in_pack: bool) {
        // The centre gets a veto, and it is not a preference: set_eternal and
        // set_perishable simply drop the sticker when the joker refuses it
        // (card.lua:506, 513). Ride the Bus is `perishable_compat = false`, and
        // handing it one anyway debuffed it five rounds into a run the game had
        // left alone -- which is how the live differential found that these flags
        // were not modelled at all. poll_stickers applies it, so a Buffoon pack's
        // jokers get it too.
        let rules = self.sticker_rules();
        let name = joker.borrow().name();
        let stickers = crate::shop_pool::poll_stickers(
            &mut self.rng,
            self.ante,
            in_pack,
            rules.eternals,
            rules.perishables,
            rules.rentals,
            Some(name),
        );
        let mut j = joker.borrow_mut();
        j.eternal = stickers.eternal;
        j.perishable = stickers.perishable;
        j.rental = stickers.rental;
        if j.perishable {
            j.perish_tally = crate::game::PERISHABLE_ROUNDS;
        }
    }

    /// What a card costs, exactly as Card:set_cost works it out.
    ///
    /// The half is the game's, not a rounding choice here:
    ///
    /// ```text
    ///     cost = max(1, floor((base + extra + 0.5) * (100 - discount)/100))
    /// ```
    ///
    /// so a four dollar joker under Liquidation costs two rather than the
    /// two-and-a-bit that rounding would give.
    pub fn card_cost(&self, base: i32, edition: Edition) -> i32 {
        let extra = edition_value(edition);
        // G.GAME.discount_percent -- Clearance Sale and Liquidation.
        let discount = self
            .vouchers
            .iter()
            .map(|v| v.discount_percent)
            .max()
            .unwrap_or(0);
        let scaled = (base + extra) as f64 + 0.5;
        let scaled = scaled * (100 - discount) as f64 / 100.0;
        // Python `max(1, int(scaled))`: the double truncates, and even a
        // Liquidation-sale one-dollar card never costs nothing.
        (scaled as i32).max(1)
    }

    /// Half what the card costs -- and the cost includes the discount.
    ///
    /// A voucher that makes the shop cheaper makes selling worth less too,
    /// which is easy to miss because it reads like a pure gain. Reading the
    /// joker's list price instead paid a dollar too much for every joker a
    /// run with Liquidation sold, and Temperance pays out the sell value of
    /// every joker held, so it compounds.
    pub fn sell_value(&self, joker: &JokerRef) -> i32 {
        let j = joker.borrow();
        let cost = if j.rental {
            1
        } else {
            self.card_cost(j.spec.cost, j.edition)
        };
        (cost / 2).max(1) + j.extra_sell_value as i32
    }

    /// What selling a held consumable pays -- Card:set_cost, as for a joker.
    ///
    /// ```text
    ///     self.cost = max(1, floor((base_cost + extra_cost + 0.5)
    ///                              * (100 - discount_percent) / 100))
    ///     -- a Planet while Astronomer is held (find_joker, not debuffed):
    ///     self.cost = 0                                     (card.lua:380)
    ///     self.sell_cost = max(1, floor(self.cost/2)) + ability.extra_value
    /// ```
    ///
    /// extra_cost carries the edition (card.lua:372-373), so Perkeo's
    /// Negative copy of a Tarot sells for $4, and extra_value is what Gift
    /// Card adds every round (3000-3005). set_cost is rerun on every card
    /// when the discount or Astronomer changes (1917-1923, 619, 676), so this
    /// is worked out now rather than kept. It sold every consumable for
    /// `max(1, cost // 2)`: no edition, no discount, and no Gift Card money.
    pub fn consumable_sell_value(&self, held: &ConsumableRef) -> i32 {
        let h = held.borrow();
        let astronomer = self
            .active_jokers()
            .iter()
            .any(|j| j.borrow().spec.free_planets);
        let cost = if h.spec.kind == ConsumableKind::Planet && astronomer {
            0
        } else {
            self.card_cost(h.spec.cost, h.edition)
        };
        (cost / 2).max(1) + h.extra_sell_value
    }
}
