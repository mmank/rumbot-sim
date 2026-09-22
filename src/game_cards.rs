//! Card-area and tag methods of `GameState`, split out of `game.rs`.
//!
//! The card piles are shared handles: one `CardRef` sits in `full_deck`,
//! `draw_pile`, `hand` and `discard_pile` at once, exactly as Python's `Card`
//! object is referenced from several lists. `add_card`/`remove_card` are the two
//! places a card enters and leaves the run, and everything that feeds on a card
//! being destroyed is told from `note_cards_destroyed`.
//!
//! Each of these is an inherent method on `GameState`; Rust allows the impl
//! blocks of one type to live in several modules, which is why the state
//! machine could be cut apart without moving the struct.

use crate::cards::{CardRef, Enhancement};
use crate::consumables::{
    ConsumableInstance, ConsumableKind, ConsumableRef, ConsumableSpec,
};
use crate::game::{tag_by_key, GameState, Tag, IMMEDIATE_TAGS};
use crate::hands::HandType;
use crate::jokers::{JokerRef, Rarity};
use std::rc::Rc;

impl GameState {
    /// Take a card out of the run, and tell what feeds on that.
    ///
    /// Every destruction comes through here -- a glass card shattering, a
    /// Hanged Man, an Immolate, a Familiar clearing space -- so this is the one
    /// place Canio and Glass Joker need to be told, the same way destroy_joker
    /// is the one place a joker can leave.
    ///
    /// `shattered` is not "was it a glass card". See note_cards_destroyed.
    pub fn remove_card(&mut self, card: &CardRef, shattered: bool) {
        let uid = crate::cards::uid_of(card);
        let mut gone = false;
        // The same handle is in several piles at once; walk each and take the
        // first occurrence out, as Python's `pile.remove(card)` does.
        for pile in [
            &mut self.full_deck,
            &mut self.draw_pile,
            &mut self.hand,
            &mut self.discard_pile,
        ] {
            if let Some(index) = pile.iter().position(|c| crate::cards::uid_of(c) == uid) {
                pile.remove(index);
                gone = true;
            }
        }
        if gone {
            let destroyed = std::slice::from_ref(card);
            let shattered_cards: &[CardRef] = if shattered { destroyed } else { &[] };
            self.note_cards_destroyed(destroyed, shattered_cards);
        }
    }

    /// A card joins the deck.
    ///
    /// Note what this does *not* do: it does not stamp `card.uid`. A card gets
    /// its id where it is built, because that is where the game gets one --
    /// `Card:init` is the only place `G.sort_id` is incremented, and it runs at
    /// construction:
    ///
    /// ```text
    ///     G.sort_id = (G.sort_id or 0) + 1
    ///     self.sort_id = G.sort_id
    /// ```
    ///
    /// A booster builds every one of its cards in a single loop the moment it
    /// opens (card.lua:1740-1780), so slot one is older than slot four however
    /// the player picks them. Stamping the id here made it an acquisition order
    /// instead and reversed exactly that pair -- and because `pseudoshuffle`
    /// sorts by id before it shuffles, a reversed pair does not merely mislabel
    /// two cards, it deals a different deck. Recording 10 stopped on that.
    pub fn add_card(&mut self, card: &CardRef) {
        // CardArea:emplace puts a card at the *front* of a deck, which is its
        // bottom -- drawing takes from the back. A card added mid-round is
        // therefore the last one you will see, not the next.
        self.full_deck.push(card.clone());
        self.draw_pile.insert(0, card.clone());
        self.note_card_created(card);
    }

    /// A card made straight into the hand -- Cryptid, Certificate, Grim.
    ///
    /// It joins the deck as well, so it comes round again in later rounds, and
    /// it counts as a card added: playing_card_joker_effects fires for every
    /// playing card the run builds, wherever it lands.
    pub fn add_card_to_hand(&mut self, card: &CardRef) {
        self.full_deck.push(card.clone());
        self.hand.push(card.clone());
        self.note_card_created(card);
    }

    /// Change a card's enhancement, and lose what that costs.
    ///
    /// Card:set_ability rebuilds the whole ability table from the new centre and
    /// carries exactly two things across: perma_bonus, so a Hiker's chips
    /// survive, and forced_selection, so Cerulean Bell keeps its grip.
    /// Everything else is rebuilt -- including played_this_ante, which is what
    /// The Pillar debuffs.
    ///
    /// So changing an *enhancement* launders a card that has already been played
    /// this ante, and changing its suit, rank, edition or seal does not: those
    /// write self.base, self.edition and self.seal and never touch ability at
    /// all. It is a real difference and an easy one to have backwards.
    ///
    /// And it ends by asking the boss about the card again -- `if not initial
    /// then G.GAME.blind:debuff_card(self) end` (card.lua:365) -- so The Lovers
    /// under The Window debuffs the Wild card it has just made, on the spot
    /// rather than at the next hand. 5LPYZ3QU discarded one straight away into
    /// a Clubs Castle: +6 in the game, +9 here.
    pub fn set_enhancement(&mut self, card: &CardRef, enhancement: Enhancement) {
        // `extra_chips` is perma_bonus and survives because nothing rebuilds it;
        // `forced_selection` is Cerulean Bell's grip and lives on the run
        // (`game.forced_card`), not on the card.
        crate::cards::set_enhancement(card, enhancement);
        // `if not initial then G.GAME.blind:debuff_card(self) end` (card.lua:365)
        // -- the boss's half of Card:set_ability, so The Lovers under The Window
        // debuffs the Wild card it has just made, on the spot rather than at the
        // next hand.
        self.debuff_card(card);
    }

    /// Tell the jokers that feed on cards leaving the deck.
    ///
    /// Canio counts the face cards among `cards`, whatever destroyed them.
    /// Glass Joker counts `shattered`, which is narrower than "the glass cards
    /// among them" in a way that is worth spelling out, because it decides real
    /// money and it is not what the card text suggests.
    ///
    /// The game marks a destroyed Glass Card `shattered` and everything else
    /// `destroyed`, then hands the jokers the list. But the two are not written
    /// at the same moment. Scoring and discarding set the flag inline and notify
    /// straight after, so the joker sees it. The tarots that destroy -- Hanged
    /// Man, Familiar, Grim, Incantation, Immolate -- queue the shatter as an
    /// animation event and notify *first*, so the flag is still unset when Glass
    /// Joker looks, and it is paid nothing.
    ///
    /// That is an accident of animation order rather than a rule, but it is the
    /// behaviour, and it is why The Hanged Man has a second, separate handler of
    /// its own (see _hanged_man) while Familiar and the rest have none. Verified
    /// against the engine: a Familiar eating a glass card moves Glass Joker by
    /// X0.00 and Canio, if it was a face, by X1.00.
    pub fn note_cards_destroyed(&mut self, cards: &[CardRef], shattered: &[CardRef]) {
        if cards.is_empty() {
            return;
        }
        // Resolve the row through calculating_hooks so a Blueprint or a
        // Brainstorm answers the copied joker's own hook, exactly as Python does.
        for (joker, hook) in self.calculating_card_hooks("on_cards_destroyed") {
            hook(&joker, cards, self);
        }
        if !shattered.is_empty() {
            for (joker, hook) in self.calculating_card_hooks("on_glass_shattered") {
                hook(&joker, shattered, self);
            }
        }
    }

    /// `(joker, hook)` for each joker that answers a card hook, copies included.
    ///
    /// Python `GameState.calculating_hooks` (game.py:1463) is generic over every
    /// hook a joker can answer. The consumables only need the two playing-card
    /// hooks -- `on_cards_destroyed` and `on_glass_shattered` -- so this is that
    /// method specialised to them, and it is what `the_hanged_man` uses to pay
    /// Glass Joker directly.
    ///
    /// The resolution mirrors Python's: walk the row against
    /// `scoring::effective_specs`, skip a debuffed owner, and yield the *source*
    /// joker -- the one whose state the hook reads, which for a copy is the
    /// copied joker and not the copier. A copier whose copy names the hook in
    /// `_NOT_COPIED` yields nothing; for these two that is Canio and Glass Joker
    /// themselves, so a Blueprint on one of them does not answer twice.
    pub fn calculating_card_hooks(
        &self,
        hook: &str,
    ) -> Vec<(JokerRef, crate::jokers::CardsHook)> {
        let shut: Option<&[&str]> = match hook {
            "on_cards_destroyed" => Some(&["Canio"]),
            "on_glass_shattered" => Some(&["Glass Joker"]),
            _ => None,
        };
        let row = self.jokers.clone();
        let effective = crate::scoring::effective_specs(&row);
        let mut out = Vec::new();
        for (owner, (spec, source)) in row.iter().zip(effective) {
            if owner.borrow().debuffed {
                continue;
            }
            let answer = match hook {
                "on_cards_destroyed" => spec.on_cards_destroyed,
                "on_glass_shattered" => spec.on_glass_shattered,
                _ => None,
            };
            let answer = match answer {
                Some(answer) => answer,
                None => continue,
            };
            if !Rc::ptr_eq(&source, owner) {
                if let Some(names) = shut {
                    if names.contains(&spec.name) {
                        continue;
                    }
                }
            }
            out.push((source, answer));
        }
        out
    }

    /// `Blind:debuff_card` for one playing card (blind.lua:624-653).
    ///
    /// The whole deck is asked when the blind is set (blind.lua:207-210), but
    /// the game also asks it of a single card whenever that card changes:
    /// `Card:set_ability` (card.lua:365), `Card:set_base` (card.lua:143) and
    /// `Card:change_suit` (card.lua:561). A card that no rule claims falls
    /// through to `set_debuff(false)` (blind.lua:653), so a change can release a
    /// card as well as catch it.
    pub fn debuff_card(&mut self, card: &CardRef) {
        card.borrow_mut().debuffed = false;
        let boss = match self.boss() {
            Some(boss) => boss,
            None => return,
        };
        if boss.debuff_until_sale {
            // Verdant Leaf debuffs every card -- not the jokers -- until a joker
            // is sold, which disables the blind.
            card.borrow_mut().debuffed = true;
            return;
        }
        // Blind:debuff_card asks `card:is_suit(suit, true)`, which is not the
        // card's printed suit: a Wild Card is every suit, a Stone Card none, and
        // a Smeared Joker pairs hearts with diamonds and spades with clubs.
        if let Some(suit) = boss.debuff_suit {
            if crate::jokers::suit_matches_for(card, suit, self) {
                card.borrow_mut().debuffed = true;
            }
        }
        // `card:is_face(true)` in the same function (blind.lua:630): not the
        // printed rank, so a Stone King is spared, and Pareidolia makes every
        // card a face card -- The Plant debuffs the whole deck, Stone included,
        // while one is held.
        if boss.debuff_face && crate::jokers::is_face_for(card, self, true) {
            card.borrow_mut().debuffed = true;
        }
        if boss.debuff_previously_played && card.borrow().played_this_ante {
            card.borrow_mut().debuffed = true;
        }
    }

    /// Tell the jokers that count cards added that one has been.
    ///
    /// Three effects built cards straight into the deck and the hand without
    /// going through here, so Hologram -- X0.25 for every playing card added --
    /// undercounted by however many Cryptid copies and Certificate cards a run
    /// made. The deck size stayed right, which is what made it hard to see:
    /// Hologram counts *additions*, not cards.
    ///
    /// The run's own total is kept here too, so a policy can price a joker that
    /// grows on additions by the rate this run actually manages rather than by
    /// hoping.
    pub fn note_card_created(&mut self, _card: &CardRef) {
        self.cards_created += 1;
        for joker in self.calculating_jokers() {
            let name = joker.borrow().name();
            if name == "Hologram" {
                joker.borrow_mut().counter += 0.25;
            }
        }
    }

    /// Wrap a registry entry as a card the run is actually holding.
    ///
    /// Passing an instance back through is fine and returns it unchanged, so
    /// callers that already have one -- Perkeo copying a held card -- do not
    /// have to care which they were given. Rust says the same thing with two
    /// types: this wraps a `ConsumableSpec`, and a caller holding a
    /// `ConsumableRef` simply keeps using it.
    pub fn hold_consumable(
        &self,
        spec: &'static ConsumableSpec,
        edition: crate::cards::Edition,
    ) -> ConsumableRef {
        crate::consumables::make_ref(ConsumableInstance::new(spec, edition))
    }

    /// Consumables from the game's pool, under the creator's own name.
    ///
    /// This drew uniformly from every card of the kind under a name of its own,
    /// which is wrong the same way the packs were: it ignores what the run has
    /// already seen and what a Planet is gated on, and it draws from a stream
    /// the game does not have. `append` is the key_append of whatever is
    /// creating the card -- "8ba" for a purple seal, "emp" for The Emperor,
    /// "pri" for The High Priestess -- and each is a separate stream.
    pub fn random_consumables(
        &mut self,
        kind: ConsumableKind,
        count: i32,
        append: &str,
    ) -> Vec<&'static ConsumableSpec> {
        let card_set = kind.set_name();
        // Planets are gated on the hand they level having been played.
        let played: Vec<&str> = self
            .hand_levels
            .plays
            .iter()
            .filter(|(_, n)| **n > 0)
            .map(|(hand, _)| hand.label())
            .collect();
        let showman = self
            .active_jokers()
            .iter()
            .any(|j| j.borrow().spec.allows_duplicates);
        // The game tests for a free slot *before* it creates the card, so a
        // full row of consumables costs nothing at all. Drawing and then
        // dropping the card, which is what this did, spends a roll the game
        // never spends and puts every later draw from that pool one place
        // along -- which is how a purple seal handed over Strength where the
        // run was given the Wheel of Fortune.
        let room = self.consumable_slots() - self.consumables.len() as i32;
        // Each card made blanks itself from the pool the next one draws from
        // -- a card marks its centre used the moment it is built. The Emperor
        // makes two Tarots and they cannot be the same Tarot; drawing both
        // against the pool as it stood at the start gave the run two copies
        // of one card.
        let mut made = self.seen_centers();
        let mut out = Vec::new();
        for _ in 0..count.min(room).max(0) {
            let seen_refs: Vec<&str> = made.iter().map(|s| s.as_str()).collect();
            let key = crate::shop_pool::draw_consumable(
                &mut self.rng,
                card_set,
                self.ante,
                &played,
                &seen_refs,
                showman,
                append,
            );
            if !showman {
                made.insert(key.clone());
            }
            let name = crate::shop_pool::name_by_consumable_key(&key)
                .unwrap_or_else(|| panic!("no consumable key {:?} in the registry", key));
            out.push(crate::consumables::spec_or_panic(name));
        }
        out
    }

    /// Bank registry entries as held cards, as far as the row will take.
    ///
    /// `edition` is for the one thing that makes a copy rather than a card:
    /// Perkeo's is Negative, and a Negative one does not use a slot.
    pub fn add_consumables(
        &mut self,
        specs: &[&'static ConsumableSpec],
        edition: crate::cards::Edition,
    ) {
        for spec in specs {
            if (self.consumables.len() as i32) < self.consumable_slots() {
                let held = self.hold_consumable(spec, edition);
                self.consumables.push(held);
            }
        }
    }

    /// Hand the run a tag the game names by key, if it is one we model.
    ///
    /// A Double Tag copies whatever arrives next -- anything but another Double
    /// Tag -- and is spent doing it. That is `tag_add`, the moment a tag joins
    /// the list rather than the moment it fires, so everything has to come
    /// through here.
    pub fn add_tag_by_key(&mut self, key: &str) {
        let tag = match tag_by_key(key) {
            Some(tag) => tag,
            None => return,
        };

        // add_tag walks the whole tag list firing `tag_add` and never breaks, so
        // *every* Double Tag held copies the incoming one -- not just the first.
        // Each sets triggered before its copy is queued, so the copies do not
        // cascade into each other. Consuming one Double at a time was the
        // difference between one spare and a row of them, and a row is
        // reachable: sell a stack of Diet Colas, or play the Anaglyph Deck,
        // which hands over a Double every time a boss falls.
        let doubles = if tag == Tag::Double {
            0
        } else {
            self.tags.iter().filter(|t| **t == Tag::Double).count()
        };
        for _ in 0..doubles {
            if let Some(index) = self.tags.iter().position(|t| *t == Tag::Double) {
                self.tags.remove(index);
            }
        }

        self.tags.push(tag);
        self.log(format!("gained {}", tag.label()));
        for _ in 0..doubles {
            self.tags.push(tag);
            self.log(format!("Double Tag: and another {}", tag.label()));
        }
    }

    /// The five that pay the instant a blind is skipped.
    ///
    /// Three of them read a run total rather than anything about the round just
    /// skipped: Handy counts every hand played this run, Garbage every discard
    /// left unspent at the end of a round, and Speed every blind skipped --
    /// including the one being skipped now, because skip_blind increments the
    /// counter before it hands the tag over.
    pub fn _fire_immediate_tags(&mut self) {
        let snapshot: Vec<Tag> = self.tags.clone();
        for tag in snapshot {
            if !IMMEDIATE_TAGS.contains(&tag) {
                continue;
            }
            if let Some(index) = self.tags.iter().position(|t| *t == tag) {
                self.tags.remove(index);
            }
            match tag {
                Tag::Handy => {
                    let total = self.hands_played;
                    self.add_money(total, "Handy Tag");
                }
                Tag::Garbage => {
                    let total = self.unused_discards;
                    self.add_money(total, "Garbage Tag");
                }
                Tag::Speed => {
                    let total = 5 * self.blinds_skipped;
                    self.add_money(total, "Speed Tag");
                }
                Tag::TopUp => {
                    // Two Common jokers, under append "top". The game passes a
                    // forced rarity poll of 0 rather than rolling one, so no
                    // draw is spent deciding they are Common, and it re-checks
                    // the room before each -- a single free slot yields one
                    // joker, not two.
                    for _ in 0..2 {
                        if (self.jokers.len() as i32) >= self.joker_slots() {
                            break;
                        }
                        self.add_random_joker(
                            "Top-up Tag",
                            Some(Rarity::Common),
                            false,
                            "top",
                            false,
                        );
                    }
                }
                Tag::Orbital => {
                    let hand = self._orbital_hand();
                    self.hand_levels.level_up(hand, 3);
                    self.log(format!("Orbital Tag: {} up three levels", hand.label()));
                }
                _ => {}
            }
        }
    }

    /// The hand an Orbital Tag names.
    ///
    /// Drawn from the *visible* hands, so nine of them until a secret hand has
    /// been made, and remembered per ante and blind -- which is what lets a
    /// Double Tag's copy level the same hand as the original. Same
    /// `pairs(G.GAME.hands)` pool as To Do List, with the same caveat about the
    /// engine's own order not being reproducible; see hands.py.
    pub fn _orbital_hand(&mut self) -> HandType {
        self._roll_orbital_choices();
        self.orbital_choices[&(self.ante, self.blind_index)]
    }

    /// Each blind's Orbital hand for the ante, as the select screen rolls it.
    ///
    /// create_UIBox_blind_choice rolls the Small's, the Big's and the Boss's in
    /// turn as it builds the screen (UI_definitions.lua:1506-1515), once an
    /// ante, whatever tags the blinds carry -- so every ante spends three draws
    /// from 'orbital', and the Big blind's hand is the second of them. Rolling
    /// one only when an Orbital Tag fired read the stream elsewhere.
    pub fn _roll_orbital_choices(&mut self) {
        for index in 0..3 {
            let slot = (self.ante, index);
            if !self.orbital_choices.contains_key(&slot) {
                let hands = self.visible_hands();
                let choice = self.rng.choice("orbital", &hands);
                self.orbital_choices.insert(slot, choice);
            }
        }
    }
}
