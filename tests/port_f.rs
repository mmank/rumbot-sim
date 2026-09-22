//! The second wave of Python tests, ported from the suite.
//!
//! Each `#[test]` mirrors one `def test_x()` under
//! `external/jimbot-sim/tests/`, grouped here by the file it came from. The doc
//! comments carry the reasoning the Python test recorded -- these were written
//! over months against real divergences, and the comment is often the only
//! statement of *why* the assertion has the shape it does.
//!
//! These come from files that import `lupa` to drive the game's own Lua, so
//! every test here is one that never touches the engine. Where the Python test
//! builds a position through `SimRun` -- the simulator backend in
//! `jimbot_sim.run`, which is not part of the port -- a small local harness
//! (`SimRun`) reimplements the selection bookkeeping so the assertions survive.
//! Engine-bound tests (headless/Lua, bridge/socket, `native`) are named in the
//! port report, not ported.
//!
//! `test_engine_agreement.py` is the highest-value group: its docstring says the
//! divergences it pins were found by an engine-backed parity walk but are stated
//! "without an engine, from the game's own Lua", and the port rebuilds the same
//! positions directly.

use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use jimbot_sim::blinds::{boss_by_name, make_blind, Blind, BlindKind};
use jimbot_sim::cards::{
    make_card, rank_of, suit_of, uid_of, CardRef, Edition, Enhancement, Rank, Suit,
};
use jimbot_sim::consumables::{self, spec_or_panic, ConsumableSpec};
use jimbot_sim::game::{Action, ActionType, GameState, PackChoice, Phase, Tag};
use jimbot_sim::hands::{evaluate, EvalFlags, HandType};
use jimbot_sim::jokers::{self, JokerRef};
use jimbot_sim::rng::RunRng;
use jimbot_sim::scoring::score_hand;
use jimbot_sim::shop::{pack_from_key, voucher_by_key, Shop, ShopSlot};
use jimbot_sim::shop_pool;
use jimbot_sim::state::{state_dict, StateValue};

const S: Suit = Suit::Spades;
const H: Suit = Suit::Hearts;
const D: Suit = Suit::Diamonds;
const C: Suit = Suit::Clubs;

fn card(rank: Rank, suit: Suit) -> CardRef {
    make_card(rank, suit)
}

fn joker(name: &str) -> JokerRef {
    jokers::make(name)
}

/// The pack option's display name, so a pack can be compared as a list.
fn choice_name(choice: &PackChoice) -> String {
    match choice {
        PackChoice::Joker(j) => j.borrow().name().to_string(),
        PackChoice::Consumable(spec) => spec.name.to_string(),
        PackChoice::Card(card) => card.borrow().to_string(),
    }
}

/// Actions after which the game has taken the highlight off -- `SimRun.step`
/// drops its selection. A buy is on the list for being in a shop, where there
/// is no hand to hold one.
const CLEARS_SELECTION: &[ActionType] = &[
    ActionType::Play,
    ActionType::Discard,
    ActionType::SelectBlind,
    ActionType::SkipPack,
    ActionType::Buy,
    ActionType::BuyAndUse,
    ActionType::BuyVoucher,
    ActionType::BuyPack,
];

/// Consumables whose use takes the highlight off: the Tarots that convert the
/// cards picked (mod_conv, suit_conv; card.lua:1150) and the four seal
/// Spectrals (card.lua:1190). Anything else leaves it where it was.
const UNHIGHLIGHTS: &[&str] = &[
    "c_magician", "c_empress", "c_heirophant", "c_lovers", "c_chariot", "c_justice", "c_devil",
    "c_tower", "c_death", "c_strength", "c_star", "c_moon", "c_sun", "c_world", "c_talisman",
    "c_deja_vu", "c_trance", "c_medium",
];

/// The game allows this many cards highlighted (G.hand.config.highlighted_limit).
const HIGHLIGHT_LIMIT: usize = 5;

/// A miniature of `jimbot_sim.run.SimRun`: just the highlight, the toggles and
/// the joker-swap budget held where the engine shows it, and the same
/// selection-clearing rules -- enough for the pure tests that click cards.
///
/// `run.py` is not part of the port (it is the shared driver, not the rules),
/// so this reimplements only the bookkeeping the ported assertions read.
struct SimRun {
    game: GameState,
    /// Highlighted cards, held by card so it follows a sort or a swap.
    selected: Vec<CardRef>,
    toggles: i32,
    sorted_rank: bool,
    sorted_suit: bool,
    hand_key: Option<Vec<u64>>,
    swaps: i32,
    swap_key: Option<(i32, Vec<String>)>,
}

impl SimRun {
    fn new(game: GameState) -> Self {
        let mut run = SimRun {
            game,
            selected: Vec::new(),
            toggles: 0,
            sorted_rank: false,
            sorted_suit: false,
            hand_key: None,
            swaps: 0,
            swap_key: None,
        };
        run.adopt(Vec::new());
        run
    }

    fn start(seed: &str, deck: &str, stake: i32) -> Self {
        SimRun::new(GameState::new(seed, deck, stake))
    }

    /// Carry on from a `GameState`, with `selected` as hand positions counted
    /// as that many toggles.
    fn adopt(&mut self, selected: Vec<usize>) {
        self.hand_key = None;
        self._held();
        self.selected = selected
            .iter()
            .filter(|i| **i < self.game.hand.len())
            .map(|i| self.game.hand[*i].clone())
            .take(HIGHLIGHT_LIMIT)
            .collect();
        self.toggles = self.selected.len() as i32;
        self.swaps = 0;
        self.swap_key = None;
    }

    /// bot_api's `sort_state`: the toggles and the sort presses start over with
    /// different cards, keyed by the cards held rather than by the action taken.
    fn _held(&mut self) {
        let mut key: Vec<u64> = self.game.hand.iter().map(uid_of).collect();
        key.sort();
        if Some(&key) != self.hand_key.as_ref() {
            self.hand_key = Some(key);
            self.toggles = 0;
            self.sorted_rank = false;
            self.sorted_suit = false;
        }
    }

    fn _swap_budget(&mut self) -> i32 {
        let mut names: Vec<String> = self
            .game
            .jokers
            .iter()
            .map(|j| j.borrow().name().to_string())
            .collect();
        names.sort();
        let key = (self.game.round_number, names);
        if Some(&key) != self.swap_key.as_ref() {
            self.swap_key = Some(key);
            self.swaps = 0;
        }
        self.swaps
    }


    /// The highlighted cards: those clicked, and Cerulean Bell's. The boss
    /// highlights its card from the deal and no click or clear takes it off
    /// (cardarea.lua:188, 201-208), so it counts towards the five.
    fn _chosen(&self) -> Vec<CardRef> {
        match self.game.held_forced_card() {
            Some(forced) if !self.selected.iter().any(|c| uid_of(c) == uid_of(&forced)) => {
                let mut out = self.selected.clone();
                out.push(forced);
                out
            }
            _ => self.selected.clone(),
        }
    }

    fn selection(&self) -> Vec<usize> {
        let chosen = self._chosen();
        self.game
            .hand
            .iter()
            .enumerate()
            .filter(|(_, card)| chosen.iter().any(|c| uid_of(c) == uid_of(card)))
            .map(|(i, _)| i)
            .collect()
    }

    fn toggle(&mut self, index: usize) {
        if index >= self.game.hand.len() {
            return;
        }
        self._held();
        // Only what is still held counts towards the five: The Hanged Man
        // destroys the cards it was aimed at, highlight and all.
        let hand = self.game.hand.clone();
        self.selected
            .retain(|c| hand.iter().any(|h| uid_of(h) == uid_of(c)));
        let card = self.game.hand[index].clone();
        if Some(uid_of(&card)) == self.game.held_forced_card().map(|c| uid_of(&c)) {
            // The game will not let it go.
        } else if self.selected.iter().any(|c| uid_of(c) == uid_of(&card)) {
            self.selected.retain(|c| uid_of(c) != uid_of(&card));
        } else if self._chosen().len() < HIGHLIGHT_LIMIT {
            self.selected.push(card);
        }
        self.toggles += 1;
    }

    fn clear(&mut self) {
        self.selected.clear();
    }

    fn swap_card_left(&mut self, index: usize) {
        self.game.swap_card_left(index);
    }

    fn sort_hand(&mut self, by: &str) {
        self._held();
        self.game.sort_hand(by);
        if by == "suit" {
            self.sorted_suit = true;
        } else {
            self.sorted_rank = true;
        }
    }

    fn _consumable_key(&self, action: &Action) -> Option<String> {
        let held: Vec<&'static ConsumableSpec> = match action.r#type {
            ActionType::UseConsumable => self
                .game
                .consumables
                .iter()
                .map(|c| c.borrow().spec)
                .collect(),
            ActionType::PickPack => self
                .game
                .pack_options
                .iter()
                .filter_map(|o| match o {
                    PackChoice::Consumable(spec) => Some(*spec),
                    _ => None,
                })
                .collect(),
            _ => return None,
        };
        if action.index < 0 {
            return None;
        }
        let index = action.index as usize;
        if index >= held.len() {
            return None;
        }
        shop_pool::key_by_consumable_name(held[index].name).map(|s| s.to_string())
    }

    fn step(&mut self, action: &Action) {
        if action.r#type == ActionType::SwapJokerLeft {
            // Spent from the budget as it stands before the swap.
            self._swap_budget();
            self.swaps += 1;
        }
        let used = self._consumable_key(action);
        self.game.step(action);
        if CLEARS_SELECTION.contains(&action.r#type)
            || used.as_deref().is_some_and(|k| UNHIGHLIGHTS.contains(&k))
        {
            self.selected.clear();
        }
    }

    fn advance(&mut self) -> bool {
        if self.game.phase == Phase::RoundEval {
            self.step(&Action::new(ActionType::CashOut));
            return true;
        }
        false
    }

    /// `SimRun.state()`: `state_dict` plus the sort buttons held here.
    fn state(&mut self) -> StateValue {
        self._held();
        let swaps = self._swap_budget();
        let mut state = state_dict(&self.game, &self.selection(), self.toggles, swaps);
        if let StateValue::Map(entries) = &mut state {
            entries.insert(
                "sorted_rank".to_string(),
                (if self.sorted_rank { 1 } else { 0 }).into(),
            );
            entries.insert(
                "sorted_suit".to_string(),
                (if self.sorted_suit { 1 } else { 0 }).into(),
            );
        }
        state
    }

    /// `selected_hand`, the row the game shows while cards are highlighted.
    fn selected_hand(&self) -> StateValue {
        let chosen = self.selection();
        let mut made: BTreeMap<String, StateValue> = [
            ("name", StateValue::Str(String::new())),
            ("level", 0.into()),
            ("chips", 0.into()),
            ("mult", 0.into()),
            ("cards", 0.into()),
            ("estimate", 0.into()),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect();
        if !chosen.is_empty() {
            let picked: Vec<CardRef> = chosen.iter().map(|i| self.game.hand[*i].clone()).collect();
            let result = self.game.evaluate_selection(&picked);
            let (chips, mult) = self.game.hand_levels.values(result.hand);
            let card_chips: i32 = result.scoring.iter().map(|c| rank_of(c).chips()).sum();
            made.insert("name".to_string(), result.hand.label().into());
            made.insert("level".to_string(), self.game.hand_levels.level(result.hand).into());
            made.insert("chips".to_string(), (chips + card_chips).into());
            made.insert("mult".to_string(), mult.into());
            made.insert("cards".to_string(), (result.scoring.len() as i32).into());
        }
        StateValue::Map(made)
    }

    /// `SimRun.patched`: `previous` brought up to date after clicks alone.
    /// Only what the highlight decides is redone -- which cards read as
    /// highlighted, how many are picked and the toggles spent, and the hand
    /// they make.
    fn patched(&mut self, previous: &StateValue) -> StateValue {
        self._held();
        let chosen = self.selection();
        let mut state = previous.clone();
        if let StateValue::Map(entries) = &mut state {
            if let Some(StateValue::List(rows)) = entries.get_mut("hand") {
                for (i, row) in rows.iter_mut().enumerate() {
                    if let StateValue::Map(fields) = row {
                        fields.insert(
                            "highlighted".to_string(),
                            (if chosen.contains(&i) { 1 } else { 0 }).into(),
                        );
                    }
                }
            }
            entries.insert("selection_size".to_string(), (chosen.len() as i32).into());
            entries.insert("toggles_used".to_string(), self.toggles.into());
            entries.insert("selected_hand".to_string(), self.selected_hand());
        }
        state
    }
}
/// A generic structural difference over two `state_dict` values, naming each
/// differing leaf by its path. `jimbot_sim.compare.differences` is the live
/// driver's field-by-field comparison, out of scope for the port; the ported
/// assertions only read the first word of each line, which is the field.
fn differences(a: &StateValue, b: &StateValue) -> Vec<String> {
    fn walk(a: &StateValue, b: &StateValue, path: &str, out: &mut Vec<String>) {
        match (a, b) {
            (StateValue::Map(ma), StateValue::Map(mb)) => {
                let keys: BTreeSet<&String> = ma.keys().chain(mb.keys()).collect();
                for key in keys {
                    let child = if path.is_empty() {
                        key.clone()
                    } else {
                        format!("{path}.{key}")
                    };
                    match (ma.get(key), mb.get(key)) {
                        (Some(x), Some(y)) => walk(x, y, &child, out),
                        _ => out.push(child),
                    }
                }
            }
            (StateValue::List(la), StateValue::List(lb)) => {
                for (i, (x, y)) in la.iter().zip(lb.iter()).enumerate() {
                    walk(x, y, &format!("{path}[{i}]"), out);
                }
                if la.len() != lb.len() {
                    out.push(format!("{path}.len"));
                }
            }
            (x, y) => {
                if x != y {
                    out.push(path.to_string());
                }
            }
        }
    }
    let mut out = Vec::new();
    walk(a, b, "", &mut out);
    out
}

/// Read a top-level integer field from a state dict.
fn state_int(state: &StateValue, key: &str) -> i64 {
    match state {
        StateValue::Map(entries) => match entries.get(key) {
            Some(StateValue::Int(v)) => *v,
            Some(StateValue::Float(v)) => *v as i64,
            other => panic!("state[{key}] is {other:?}"),
        },
        _ => panic!("not a state map"),
    }
}

/// Read a top-level string field from a state dict.
fn state_str(state: &StateValue, key: &str) -> String {
    match state {
        StateValue::Map(entries) => match entries.get(key) {
            Some(StateValue::Str(v)) => v.clone(),
            other => panic!("state[{key}] is {other:?}"),
        },
        _ => panic!("not a state map"),
    }
}

/// The length of a top-level list field from a state dict.
fn state_len(state: &StateValue, key: &str) -> usize {
    match state {
        StateValue::Map(entries) => match entries.get(key) {
            Some(StateValue::List(items)) => items.len(),
            other => panic!("state[{key}] is {other:?}"),
        },
        _ => panic!("not a state map"),
    }
}

/// `_in_a_shop()` from test_engine_agreement.py: select a blind, cut the target
/// to one chip, play, cash out.
fn in_a_shop(seed: &str) -> GameState {
    let mut game = GameState::new(seed, "Red Deck", 1);
    game.step(&Action::new(ActionType::SelectBlind));
    game.blind.as_mut().unwrap().target = 1;
    game.step(&Action::with_cards(ActionType::Play, vec![0]));
    game.step(&Action::new(ActionType::CashOut));
    assert_eq!(game.phase, Phase::Shop);
    game
}

/// `_boss(name)`: the boss in force for the run's ante.
fn boss_blind(game: &GameState, name: &str) -> Blind {
    make_blind(BlindKind::Boss, game.ante, boss_by_name(name), 1.0, 1, false)
}

/// `_on_deck(name)`: the boss offered on the blind select screen, not yet set.
fn on_deck(game: &GameState, name: &str) -> Blind {
    let mut blind = boss_blind(game, name);
    blind.on_deck = true;
    blind
}

fn use_consumable(game: &mut GameState, name: &str) {
    game.use_consumable(spec_or_panic(name), &[], false);
}

/// An action that both names a slot and carries the highlighted cards.
fn with_index_cards(r#type: ActionType, index: i32, cards: Vec<usize>) -> Action {
    let mut action = Action::at(r#type, index);
    action.cards = cards;
    action
}

/// `GameState.reroll_cost_carried`: `current_round.reroll_cost` outside a shop.
fn reroll_cost_carried(game: &GameState) -> i32 {
    if game.free_rerolls_carried > 0 {
        0
    } else {
        game.reroll_price_carried
    }
}


// ==========================================================================
// tests/test_engine_agreement.py -- "Where the simulator parted from the
// engine, walked side by side."
// ==========================================================================

#[test]
fn test_a_standard_pack_holds_playing_cards_not_jokers() {
    // SET_IDS in bot_api.lua: Default 7, Enhanced 8. This said Joker (1).
    let mut game = in_a_shop("AGREE001");
    game._open_pack(pack_from_key("p_standard_normal_1"), false);
    assert!(!game.pack_options.is_empty());
    assert!(game
        .pack_options
        .iter()
        .all(|option| matches!(option, PackChoice::Card(_))));
}

#[test]
fn test_chaos_the_clown_moves_the_reroll_price_between_shops() {
    // calculate_reroll_cost: free while current_round.free_rerolls > 0, and
    // add_to_deck / remove_from_deck move that count in or out of a shop.
    let mut game = in_a_shop("AGREE001");
    game.step(&Action::new(ActionType::LeaveShop));
    assert_eq!(reroll_cost_carried(&game), 5);
    let chaos = joker("Chaos the Clown");
    game.gain_joker(&chaos);
    assert_eq!(reroll_cost_carried(&game), 0);
    let index = game
        .jokers
        .iter()
        .position(|j| Rc::ptr_eq(j, &chaos))
        .unwrap();
    game.step(&Action::at(ActionType::SellJoker, index as i32));
    assert_eq!(reroll_cost_carried(&game), 5);
}

#[test]
fn test_the_d6_tag_prices_the_next_round_too() {
    // round_resets.temp_reroll_cost stands until end_round
    // (state_events.lua:271), so the round after the shop reads 0.
    let mut game = GameState::new("AGREE002", "Red Deck", 1);
    game.temp_reroll_cost = true;
    game.step(&Action::new(ActionType::SelectBlind));
    assert_eq!(reroll_cost_carried(&game), 0);
    game.blind.as_mut().unwrap().target = 1;
    game.step(&Action::with_cards(ActionType::Play, vec![0]));
    assert!(!game.temp_reroll_cost);
    assert_eq!(reroll_cost_carried(&game), 5);
}

#[test]
fn test_two_pack_tags_open_one_pack_each_in_turn() {
    // One new_blind_choice firing opens one pack (tag.lua breaks the loop);
    // the second opens when the first closes (button_callbacks.lua:2618).
    let mut game = GameState::new("AGREE003", "Red Deck", 1);
    game.tags = vec![Tag::Ethereal, Tag::Ethereal];
    game._apply_blind_select_tags();
    assert_eq!(game.phase, Phase::Pack);
    assert_eq!(game.tags, vec![Tag::Ethereal]);
    let first: Vec<String> = game.pack_options.iter().map(choice_name).collect();
    game.step(&Action::new(ActionType::SkipPack));
    assert_eq!(game.phase, Phase::Pack);
    assert!(game.tags.is_empty());
    let second: Vec<String> = game.pack_options.iter().map(choice_name).collect();
    assert_ne!(second, first);
}

#[test]
fn test_any_voucher_bought_takes_the_antes_off_the_shelves() {
    // Card:redeem clears current_round.voucher whichever voucher it was.
    let mut game = in_a_shop("AGREE004");
    game.money = 100;
    if game.shop.as_ref().unwrap().vouchers.is_empty() {
        return;
    }
    game.step(&Action::at(ActionType::BuyVoucher, 0));
    assert_eq!(game.round_voucher, "");
}

#[test]
fn test_the_cerulean_bells_card_is_selected_and_stays_so() {
    let mut run = SimRun::start("AGREE005", "Red Deck", 1);
    run.step(&Action::new(ActionType::SelectBlind));
    run.game.blind = Some(boss_blind(&run.game, "Cerulean Bell"));
    run.game.phase = Phase::Playing;
    run.game.forced_card = Some(run.game.hand[3].clone());
    assert_eq!(run.selection(), vec![3]);
    run.toggle(3); // a click cannot free it
    assert_eq!(run.selection(), vec![3]);
    run.clear();
    assert_eq!(run.selection(), vec![3]);
    for i in [0, 1, 2, 4, 5] {
        run.toggle(i);
    }
    assert_eq!(run.selection().len(), 5); // it counts towards five
}

#[test]
fn test_a_planet_leaves_the_highlight_a_tarot_that_converts_takes_it() {
    // card.lua:1150 and 1190 unhighlight; nothing else does.
    let mut run = SimRun::start("AGREE006", "Red Deck", 1);
    run.step(&Action::new(ActionType::SelectBlind));
    run.game.consumables.clear();
    run.game.add_consumables(
        &[spec_or_panic("Pluto"), spec_or_panic("The Lovers")],
        Edition::None,
    );
    run.toggle(0);
    let chosen = run.selection();
    run.step(&with_index_cards(ActionType::UseConsumable, 0, chosen));
    assert_eq!(run.selection(), vec![0]);
    assert_eq!(run.toggles, 1); // the same cards held
    let chosen = run.selection();
    run.step(&with_index_cards(ActionType::UseConsumable, 0, chosen));
    assert!(run.selection().is_empty());
}


#[test]
fn test_a_skipped_orbital_tag_levels_its_hand() {
    // Built with the blind it was offered on, as the select screen builds it;
    // without, its hand was a placeholder and applying it raised. The headless
    // test sets the Small blind's tag and skips; the port sets the tag on the
    // blind on deck and skips it the same way.
    let mut game = GameState::new("ORBIT001", "Red Deck", 1);
    game.ante_tag_keys[0] = "tag_orbital".to_string();
    let before: i32 = game.hand_levels.levels.values().sum();
    game.step(&Action::new(ActionType::SkipBlind));
    let after: i32 = game.hand_levels.levels.values().sum();
    assert_eq!(after, before + 3);
}

#[test]
fn test_checkered_rolls_its_held_cards_before_it_converts() {
    // start_run rolls The Idol's card and Castle's suit synchronously; the
    // Checkered Deck's conversion is an event after it (back.lua:239).
    for seed in ["CHECK001", "CHECK002", "CHECK003", "CHECK004"] {
        let red = GameState::new(seed, "Red Deck", 1);
        let checkered = GameState::new(seed, "Checkered Deck", 1);
        assert_eq!(checkered.castle_suit, red.castle_suit);
        assert_eq!(checkered.idol_suit, red.idol_suit);
    }
}

#[test]
fn test_a_draw_of_nothing_leaves_the_hand_as_dragged() {
    let mut game = GameState::new("AGREE010", "Red Deck", 1);
    game.step(&Action::new(ActionType::SelectBlind));
    game.swap_card_left(4);
    let order: Vec<u64> = game.hand.iter().map(uid_of).collect();
    game._draw_to_hand_size(); // the hand is already full
    let after: Vec<u64> = game.hand.iter().map(uid_of).collect();
    assert_eq!(after, order);
}

#[test]
fn test_the_d6_tags_price_leaves_the_vouchers_out() {
    // (temp_reroll_cost or round_resets.reroll_cost) + increase; a reroll
    // voucher bought there cuts the current price until the next reroll.
    let mut shop = Shop {
        free_reroll_cost: true,
        rerolls: 1,
        ..Shop::default()
    };
    assert_eq!(shop.reroll_price(2), 1);
    shop.cut_until_reroll = 2;
    assert_eq!(shop.reroll_price(2), 0);
}

#[test]
fn test_every_antes_orbital_hands_are_rolled_at_its_blind_select() {
    // create_UIBox_blind_choice rolls Small, Big and Boss in turn.
    let game = GameState::new("AGREE011", "Red Deck", 1);
    let rolled: BTreeSet<(i32, i32)> = game.orbital_choices.keys().cloned().collect();
    assert!(BTreeSet::from([(1, 0), (1, 1), (1, 2)]).is_subset(&rolled));
}

#[test]
fn test_the_boss_on_deck_is_not_in_force_until_the_round_sets_it() {
    // On the blind select screen G.GAME.blind is the empty blind the last
    // round left (blind.lua:336); the boss on offer acts from set_blind.
    let mut game = in_a_shop("AGREE012");
    game.step(&Action::new(ActionType::LeaveShop));
    assert_eq!(game.phase, Phase::BlindSelect);
    assert!(game.blind.as_ref().unwrap().on_deck);
    let blind = on_deck(&game, "The Manacle");
    game.blind = Some(blind);
    let size = game.hand_size();
    assert!(game.boss().is_none());
    game.step(&Action::new(ActionType::SelectBlind));
    assert!(!game.blind.as_ref().unwrap().on_deck);
    assert!(game.boss().is_some());
    assert_eq!(game.hand_size(), size - 1);
}

#[test]
fn test_a_boss_built_off_the_blind_select_is_in_force() {
    // What a policy's fork does to price the boss to come from a shop.
    let mut game = in_a_shop("AGREE018");
    let blind = boss_blind(&game, "The Manacle");
    game.blind = Some(blind);
    assert!(game.boss().is_some());
}

#[test]
fn test_a_pack_at_the_blind_select_does_not_spend_the_bells_draw() {
    let mut game = GameState::new("AGREE013", "Red Deck", 1);
    let blind = on_deck(&game, "Cerulean Bell");
    game.blind = Some(blind);
    game._open_pack(pack_from_key("p_arcana_normal_1"), false);
    assert!(!game.hand.is_empty());
    assert!(game.forced_card.is_none());
    assert!(!game.rng.pools.keys().any(|k| k.starts_with("cerulean_bell")));
}

#[test]
fn test_a_sale_before_the_verdant_leaf_leaves_it_standing() {
    let mut game = GameState::new("AGREE014", "Red Deck", 1);
    game.gain_joker(&joker("Joker"));
    let blind = on_deck(&game, "Verdant Leaf");
    game.blind = Some(blind);
    game.step(&Action::at(ActionType::SellJoker, 0));
    assert!(!game.blind.as_ref().unwrap().disabled);
}

#[test]
fn test_a_coupon_tag_taken_mid_shop_waits_for_the_next_shop() {
    let mut game = in_a_shop("AGREE015");
    game.money = 100;
    game.tags.push(Tag::Coupon);
    game.step(&Action::new(ActionType::Reroll));
    assert!(game.tags.contains(&Tag::Coupon));
    assert!(game
        .shop
        .as_ref()
        .unwrap()
        .slots
        .iter()
        .all(|slot| !slot.couponed));
}


#[test]
fn test_ankh_draws_its_joker_oldest_first() {
    // pseudorandom_element sorts by sort_id (card.lua:1434); a drag must not
    // change which joker is copied.
    let mut picks: Vec<Vec<String>> = Vec::new();
    for order in [[0usize, 1usize], [1, 0]] {
        let mut game = GameState::new("AGREE016", "Red Deck", 1);
        let made = [joker("Joker"), joker("Jolly Joker")];
        for i in order {
            game.gain_joker(&made[i]);
        }
        use_consumable(&mut game, "Ankh");
        let mut names: Vec<String> = game
            .jokers
            .iter()
            .map(|j| j.borrow().name().to_string())
            .collect();
        names.sort();
        picks.push(names);
    }
    assert_eq!(picks[0], picks[1]);
}

#[test]
fn test_a_joker_changing_hands_asks_every_card_its_debuff_again() {
    // add_to_deck and remove_from_deck end with set_blind(nil, true).
    let mut game = GameState::new("AGREE017", "Red Deck", 1);
    let blind = boss_blind(&game, "The Window"); // Diamonds
    game.blind = Some(blind);
    game.step(&Action::new(ActionType::SelectBlind));
    let blind = boss_blind(&game, "The Window");
    game.blind = Some(blind);
    game.phase = Phase::Playing;
    let smeared = joker("Smeared Joker");
    game.gain_joker(&smeared);
    let hearts: Vec<CardRef> = game
        .full_deck
        .iter()
        .filter(|c| suit_of(c) == Suit::Hearts)
        .cloned()
        .collect();
    assert!(hearts.iter().all(|c| c.borrow().debuffed));
    let index = game
        .jokers
        .iter()
        .position(|j| Rc::ptr_eq(j, &smeared))
        .unwrap();
    game.step(&Action::at(ActionType::SellJoker, index as i32));
    assert!(hearts.iter().all(|c| !c.borrow().debuffed));
}

#[test]
fn test_marbles_stone_card_is_made_after_the_verdant_leaf_is_set() {
    // set_blind asks every card its debuff before new_round runs the
    // setting_blind context (state_events.lua:333-337), and Card() builds
    // the Stone card `initial`, never asking (card.lua:43-44).
    let mut game = GameState::new("AGREE019", "Red Deck", 1);
    game.gain_joker(&joker("Marble Joker"));
    let blind = on_deck(&game, "Verdant Leaf");
    game.blind = Some(blind);
    game.step(&Action::new(ActionType::SelectBlind));
    let stones: Vec<CardRef> = game
        .full_deck
        .iter()
        .filter(|c| c.borrow().enhancement == Enhancement::Stone)
        .cloned()
        .collect();
    assert_eq!(stones.len(), 1);
    assert!(!stones[0].borrow().debuffed);
    assert!(game
        .full_deck
        .iter()
        .filter(|c| uid_of(c) != uid_of(&stones[0]))
        .all(|c| c.borrow().debuffed));
}

#[test]
fn test_perkeo_draws_its_consumable_oldest_first() {
    // pseudorandom_element sorts by sort_id (misc_functions.lua:260).
    let mut picks: Vec<String> = Vec::new();
    for order in [[0usize, 1usize], [1, 0]] {
        let mut game = GameState::new("AGREE020", "Red Deck", 1);
        let made = [
            game.hold_consumable(consumables::spec_or_panic("The Fool"), Edition::None),
            game.hold_consumable(consumables::spec_or_panic("Strength"), Edition::None),
        ];
        game.consumables = order.iter().map(|i| made[*i].clone()).collect();
        let perkeo = joker("Perkeo");
        let hook = perkeo.borrow().spec.on_shop_end.unwrap();
        hook(&perkeo, &mut game);
        picks.push(game.consumables.last().unwrap().borrow().spec.name.to_string());
    }
    assert_eq!(picks[0], picks[1]);
}

#[test]
fn test_a_consumable_bought_is_as_old_as_the_shop_that_stocked_it() {
    let mut game = in_a_shop("AGREE021");
    game.money = 100;
    let mut slot = ShopSlot::new("consumable", 3);
    slot.consumable = Some(consumables::spec_or_panic("The Fool"));
    game.shop.as_mut().unwrap().slots.insert(0, slot);
    let later = game.hold_consumable(consumables::spec_or_panic("Strength"), Edition::None);
    game.consumables.push(later.clone());
    game.step(&Action::at(ActionType::Buy, 0));
    let fool = game
        .consumables
        .iter()
        .find(|c| c.borrow().spec.name == "The Fool")
        .unwrap()
        .clone();
    assert!(fool.borrow().uid < later.borrow().uid);
}


// ==========================================================================
// tests/test_scoring_fixtures.py -- "Pinned scores for hands the recordings
// never play."
// ==========================================================================

/// `scene.play([...])`: score these positions out of this hand, with an
/// optional hand level, through the run's own pipeline.
fn score_play(cards: &[CardRef], played: &[usize], jokers: &[&str], level: Option<HandType>) -> i64 {
    let played_cards: Vec<CardRef> = played.iter().map(|i| cards[*i].clone()).collect();
    let held: Vec<CardRef> = cards
        .iter()
        .enumerate()
        .filter(|(i, _)| !played.contains(i))
        .map(|(_, c)| c.clone())
        .collect();
    let mut game = GameState::new(0, "Red Deck", 1);
    game.jokers = jokers.iter().map(|n| jokers::make(n)).collect();
    if let Some(hand) = level {
        game.hand_levels.level_up(hand, 1);
    }
    game.hand = cards.to_vec();
    let result = evaluate(&played_cards, EvalFlags::default());
    let ctx = score_hand(&mut game, &result, &played_cards, &held);
    ctx.score()
}

/// The hand from `PAIR_OF_ACES`, in dealt order.
fn pair_of_aces() -> Vec<CardRef> {
    vec![
        card(Rank::Ace, H),
        card(Rank::Ace, D),
        card(Rank::Two, S),
        card(Rank::Three, C),
        card(Rank::Four, H),
        card(Rank::Five, C),
        card(Rank::Seven, D),
        card(Rank::Nine, S),
    ]
}

#[test]
fn test_straight_flush() {
    // (100 base + 11+10+10+10+10 card chips) * 8 mult
    let cards = vec![
        card(Rank::Ace, S),
        card(Rank::King, S),
        card(Rank::Queen, S),
        card(Rank::Jack, S),
        card(Rank::Ten, S),
        card(Rank::Two, H),
        card(Rank::Three, D),
        card(Rank::Four, C),
    ];
    assert_eq!(score_play(&cards, &[0, 1, 2, 3, 4], &[], None), (100 + 51) * 8);
}

#[test]
fn test_pair() {
    // (10 base + 11+11) * 2 mult
    assert_eq!(score_play(&pair_of_aces(), &[0, 1], &[], None), (10 + 22) * 2);
}

#[test]
fn test_flush() {
    // (35 base + 2+4+6+8+10) * 4 mult
    let cards = vec![
        card(Rank::Two, H),
        card(Rank::Four, H),
        card(Rank::Six, H),
        card(Rank::Eight, H),
        card(Rank::Ten, H),
        card(Rank::Ace, S),
        card(Rank::Three, D),
        card(Rank::Five, C),
    ];
    assert_eq!(score_play(&cards, &[0, 1, 2, 3, 4], &[], None), (35 + 30) * 4);
}

#[test]
fn test_hand_level_raises_pair() {
    // Pair gains +15 chips and +1 mult a level: level 2 is 25 chips, 3 mult.
    assert_eq!(
        score_play(&pair_of_aces(), &[0, 1], &[], Some(HandType::Pair)),
        (25 + 22) * 3
    );
}

#[test]
fn test_enhancement_on_scored_card() {
    // (10 + 22 + 30) * 2 for m_bonus, (10 + 22) * (2 + 4) for m_mult,
    // (10 + 22) * (2 * 2) for m_glass.
    for (enhancement, expected) in [
        (Enhancement::Bonus, (10 + 22 + 30) * 2),
        (Enhancement::Mult, (10 + 22) * (2 + 4)),
        (Enhancement::Glass, (10 + 22) * (2 * 2)),
    ] {
        let cards = pair_of_aces();
        cards[0].borrow_mut().enhancement = enhancement;
        assert_eq!(score_play(&cards, &[0, 1], &[], None), expected);
    }
}

#[test]
fn test_edition_on_scored_card() {
    // (10 + 22 + 50) * 2 for foil, (10 + 22) * (2 + 10) for holo,
    // int((10 + 22) * 2 * 1.5) for polychrome.
    for (edition, expected) in [
        (Edition::Foil, (10 + 22 + 50) * 2),
        (Edition::Holographic, (10 + 22) * (2 + 10)),
        (Edition::Polychrome, ((10.0 + 22.0) * 2.0 * 1.5) as i64),
    ] {
        let cards = pair_of_aces();
        cards[0].borrow_mut().edition = edition;
        assert_eq!(score_play(&cards, &[0, 1], &[], None), expected);
    }
}

#[test]
fn test_steel_card_held_in_hand() {
    // Steel triggers while held, not played: (10 + 22) * 2 * 1.5
    let cards = pair_of_aces();
    cards[7].borrow_mut().enhancement = Enhancement::Steel;
    assert_eq!(
        score_play(&cards, &[0, 1], &[], None),
        ((10.0 + 22.0) * 2.0 * 1.5) as i64
    );
}

#[test]
fn test_greedy_joker_pays_per_diamond() {
    // Greedy Joker is +3 mult for each Diamond scored: five of them.
    let cards = vec![
        card(Rank::Two, D),
        card(Rank::Four, D),
        card(Rank::Six, D),
        card(Rank::Eight, D),
        card(Rank::Ten, D),
        card(Rank::Ace, S),
        card(Rank::Three, H),
        card(Rank::Five, C),
    ];
    assert_eq!(
        score_play(&cards, &[0, 1, 2, 3, 4], &["Greedy Joker"], None),
        (35 + 30) * (4 + 3 * 5)
    );
}

#[test]
fn test_joker_order_matters_for_multiplication() {
    // +mult before xmult is not the same as after. Bull (+chips) is order
    // independent, so use two that are not: Joker (+4 mult) and a polychrome
    // card (x1.5) resolve card-first, then jokers left to right.
    assert_eq!(
        score_play(&pair_of_aces(), &[0, 1], &["Joker"], None),
        (10 + 22) * (2 + 4)
    );
}


// ==========================================================================
// tests/test_getting_sliced.py -- "A joker Madness or Ceremonial Dagger takes
// is gone only once the blind is set."
// ==========================================================================

/// `_select`: gain the jokers by key, perch the named ones, select the blind.
fn select_row(seed: &str, keys: &[&str], perished: &[usize]) -> GameState {
    let mut game = GameState::new(seed, "Red Deck", 1);
    for (i, key) in keys.iter().enumerate() {
        let name = shop_pool::name_by_joker_key(key)
            .unwrap_or_else(|| panic!("no joker key {key:?}"));
        game.gain_joker(&joker(name));
        if perished.contains(&i) {
            let j = game.jokers[i].clone();
            j.borrow_mut().perishable = true;
            j.borrow_mut().perish_tally = 0;
            game.set_joker_debuff(&j, true);
        }
    }
    game.step(&Action::new(ActionType::SelectBlind));
    game
}

/// `_row`: the joker keys in the row, in order.
fn row_keys(game: &GameState) -> Vec<&'static str> {
    game.jokers
        .iter()
        .map(|j| {
            shop_pool::key_by_joker_name(j.borrow().name())
                .unwrap_or_else(|| panic!("no joker key for {:?}", j.borrow().name()))
        })
        .collect()
}

/// `_counters`: hands and discards left.
fn counters(game: &GameState) -> (i32, i32) {
    (game.hands_left, game.discards_left)
}

fn full_row() -> Vec<&'static str> {
    vec!["j_joker", "j_greedy_joker", "j_lusty_joker"]
}

#[test]
fn test_a_joker_madness_takes_does_not_fire() {
    let game = select_row("SLICE1", &["j_madness", "j_burglar"], &[]);
    assert_eq!(row_keys(&game), vec!["j_madness"]);
    assert_eq!(
        counters(&game),
        counters(&select_row("SLICE1", &[], &[])),
        "the eaten Burglar still gave hands"
    );
}

#[test]
fn test_a_joker_madness_takes_keeps_its_slot_for_the_pass() {
    let keys = vec!["j_madness", "j_riff_raff"]
        .into_iter()
        .chain(full_row())
        .collect::<Vec<_>>();
    let game = select_row("SLICE1", &keys, &[]);
    assert_eq!(
        game.jokers.len(),
        4,
        "Riff-raff filled the slot of a joker that was still in the row"
    );
}

#[test]
fn test_the_dagger_hands_its_slot_back_through_the_buffer() {
    let game = select_row(
        "SLICE1",
        &[
            "j_ceremonial",
            "j_joker",
            "j_riff_raff",
            "j_greedy_joker",
            "j_lusty_joker",
        ],
        &[],
    );
    assert_eq!(
        row_keys(&game)[..4],
        ["j_ceremonial", "j_riff_raff", "j_greedy_joker", "j_lusty_joker"]
    );
    assert_eq!(game.jokers.len(), 5);
}

#[test]
fn test_a_dagger_eats_nothing_behind_a_neighbour_madness_took() {
    let game = select_row(
        "SLICE2",
        &["j_madness", "j_ceremonial", "j_joker", "j_greedy_joker"],
        &[],
    );
    assert_eq!(
        row_keys(&game),
        vec!["j_madness", "j_ceremonial", "j_greedy_joker"]
    );
    assert_eq!(game.jokers[1].borrow().counter, 0.0);
}

#[test]
fn test_riff_raffs_jokers_are_not_in_the_row_during_the_pass() {
    let game = select_row("SLICE1", &["j_riff_raff", "j_ceremonial"], &[]);
    assert_eq!(row_keys(&game)[..2], ["j_riff_raff", "j_ceremonial"]);
    assert_eq!(game.jokers.len(), 4);
    assert_eq!(
        game.jokers[1].borrow().counter,
        0.0,
        "the Dagger ate a joker made after it"
    );
}

#[test]
fn test_blueprint_copies_a_blind_select_effect() {
    let game = select_row("SLICE1", &["j_blueprint", "j_burglar"], &[]);
    let allowance = counters(&select_row("SLICE1", &[], &[]));
    assert_eq!(counters(&game), (allowance.0 + 6, 0));
}

#[test]
fn test_a_copy_of_a_joker_getting_sliced_does_not_fire() {
    let game = select_row("SLICE2", &["j_madness", "j_blueprint", "j_burglar"], &[]);
    assert_eq!(row_keys(&game), vec!["j_madness", "j_blueprint"]);
    assert_eq!(counters(&game), counters(&select_row("SLICE2", &[], &[])));
}

#[test]
fn test_a_copier_getting_sliced_copies_nothing() {
    let game = select_row("SLICE1", &["j_madness", "j_blueprint", "j_burglar"], &[]);
    assert_eq!(row_keys(&game), vec!["j_madness", "j_burglar"]);
    let allowance = counters(&select_row("SLICE1", &[], &[]));
    assert_eq!(counters(&game), (allowance.0 + 3, 0));
}

#[test]
fn test_a_perished_joker_does_nothing_when_the_blind_is_selected() {
    let game = select_row("SLICE1", &["j_burglar"], &[0]);
    assert_eq!(counters(&game), counters(&select_row("SLICE1", &[], &[])));
}


// ==========================================================================
// tests/test_shop_pool.py -- "Which joker the shop offers, draw for draw
// against the game."
// ==========================================================================

fn build(rarity: u8, owned: &[&str], seen: &[&str], showman: bool, flags: &[&str]) -> Vec<String> {
    shop_pool::build_pool(rarity, owned, seen, showman, flags)
}

fn joker_key(name: &str) -> &'static str {
    jimbot_sim::joker_data::joker_row(name)
        .unwrap_or_else(|| panic!("no joker named {name:?}"))
        .key
}

#[test]
fn test_the_rarity_split_is_the_games() {
    // Roughly 70/25/5, from thresholds at 0.7 and 0.95. Checked loosely: the
    // point is that the thresholds are the right way round and applied to the
    // right roll, not that a sample of 400 lands on the mean.
    let mut rng = RunRng::new("TESTSEED");
    let mut counts = [0i32; 4];
    for _ in 0..400 {
        counts[shop_pool::roll_rarity(&mut rng, 1, "") as usize] += 1;
    }
    assert!(counts[1] > counts[2] && counts[2] > counts[3]);
    assert!(0.55 < counts[1] as f64 / 400.0 && counts[1] as f64 / 400.0 < 0.85);
    assert!(counts[3] as f64 / 400.0 < 0.15);
}

#[test]
fn test_a_gated_joker_is_blanked_not_dropped() {
    // Filtering must preserve the pool's length, or every later draw shifts.
    let full = build(2, &[], &[], false, &[]);
    let gated = build(2, &["m_lucky"], &[], false, &[]);
    assert_eq!(full.len(), gated.len());
    assert_eq!(full.len(), jimbot_sim::joker_data::pool_for_rarity(2).len());
    let lucky_cat = joker_key("Lucky Cat");
    assert!(!full.contains(&lucky_cat.to_string()), "Lucky Cat offered with no Lucky card owned");
    assert!(gated.contains(&lucky_cat.to_string()), "Lucky Cat still withheld once one is owned");
}

#[test]
fn test_every_gate_names_a_real_enhancement() {
    let gates: BTreeSet<&str> = jimbot_sim::joker_data::JOKER_DATA
        .iter()
        .map(|row| row.enhancement_gate)
        .filter(|g| !g.is_empty())
        .collect();
    let real: BTreeSet<&str> = ["m_glass", "m_gold", "m_lucky", "m_steel", "m_stone"]
        .into_iter()
        .collect();
    assert!(gates.is_subset(&real));
    let gated = jimbot_sim::joker_data::JOKER_DATA
        .iter()
        .filter(|row| !row.enhancement_gate.is_empty())
        .count();
    assert_eq!(gated, 5);
}

#[test]
fn test_a_joker_already_seen_is_withheld_unless_showman() {
    let banner = joker_key("Banner");
    assert!(!build(1, &[], &[banner], false, &[]).contains(&banner.to_string()));
    assert!(build(1, &[], &[banner], true, &[]).contains(&banner.to_string()));
}

#[test]
fn test_the_pools_are_the_games_sizes() {
    let sizes: Vec<usize> = [1, 2, 3, 4]
        .iter()
        .map(|r| jimbot_sim::joker_data::pool_for_rarity(*r).len())
        .collect();
    assert_eq!(sizes, vec![61, 64, 20, 5]);
    assert_eq!(sizes.iter().sum::<usize>(), 150);
}

#[test]
fn test_gros_michel_and_cavendish_swap_places() {
    // The pair the pool flag exists for, and it is not in either joker's text.
    // Before the flag is set the pool can offer Gros Michel and not Cavendish;
    // after it, the other way round.
    let gros = joker_key("Gros Michel");
    let cavendish = joker_key("Cavendish");

    let fresh = build(1, &[], &[], false, &[]);
    assert!(fresh.contains(&gros.to_string()));
    assert!(!fresh.contains(&cavendish.to_string()));

    let extinct = build(1, &[], &[], false, &["gros_michel_extinct"]);
    assert!(!extinct.contains(&gros.to_string()));
    assert!(extinct.contains(&cavendish.to_string()));
}


#[test]
fn test_a_planet_is_locked_until_its_hand_is_played() {
    // Planet X, Ceres and Eris are absent until the hand has been made. This
    // is a real constraint on what a run can be offered, not a nicety.
    let common = [
        "High Card",
        "Pair",
        "Two Pair",
        "Three of a Kind",
        "Straight",
        "Flush",
        "Full House",
        "Four of a Kind",
        "Straight Flush",
    ];
    let without = shop_pool::build_consumable_pool("Planet", &common, &[] as &[&str], false);
    assert!(!without.contains(&"c_planet_x".to_string()));
    assert!(!without.contains(&"c_ceres".to_string()));
    assert!(!without.contains(&"c_eris".to_string()));

    let all: Vec<&str> = common
        .iter()
        .copied()
        .chain(["Five of a Kind", "Flush House", "Flush Five"])
        .collect();
    let with_all = shop_pool::build_consumable_pool("Planet", &all, &[] as &[&str], false);
    assert!(with_all.contains(&"c_planet_x".to_string()));
    assert!(with_all.contains(&"c_ceres".to_string()));
    assert!(with_all.contains(&"c_eris".to_string()));
    assert_eq!(without.len(), 12);
    assert_eq!(with_all.len(), 12);
}

#[test]
fn test_the_special_planets_get_no_probability_boost() {
    // Once unlocked they are drawn like any other Planet, not more often.
    // Worth pinning: the centers carry a `freq` field that looks like a weight,
    // and it is 1 for every planet and never read by the pool code.
    let played = [
        "High Card",
        "Pair",
        "Two Pair",
        "Three of a Kind",
        "Straight",
        "Flush",
        "Full House",
        "Four of a Kind",
        "Straight Flush",
        "Five of a Kind",
        "Flush House",
        "Flush Five",
    ];
    let mut rng = RunRng::new("TESTSEED");
    let mut special = 0;
    for _ in 0..3000 {
        let drawn =
            shop_pool::draw_consumable(&mut rng, "Planet", 1, &played, &[] as &[&str], false, "");
        if ["c_planet_x", "c_ceres", "c_eris"].contains(&drawn.as_str()) {
            special += 1;
        }
    }
    let ratio = special as f64 / 3000.0;
    assert!(0.20 < ratio && ratio < 0.30, "expected about 3 in 12");
}

#[test]
fn test_black_hole_and_the_soul_never_come_from_a_pool() {
    // They have their own path; drawing them normally would be wrong.
    let spectral =
        shop_pool::build_consumable_pool("Spectral", &[] as &[&str], &[] as &[&str], false);
    assert!(!spectral.contains(&"c_black_hole".to_string()));
    assert!(!spectral.contains(&"c_soul".to_string()));
}

#[test]
fn test_a_shop_does_not_offer_the_same_consumable_twice() {
    // A card blanks its own pool entry the moment it is built, so the second
    // slot of a shop is drawn from a pool the first slot has already left.
    for episode in 0..60 {
        let mut game = GameState::new(format!("DUPES{:03}", episode), "Red Deck", 1);
        game._open_shop();
        let shop = game.shop.as_ref().unwrap();
        let names: Vec<String> = shop
            .slots
            .iter()
            .filter_map(|s| s.consumable.map(|c| c.name.to_string()))
            .collect();
        assert_eq!(
            names.len(),
            names.iter().collect::<BTreeSet<_>>().len(),
            "{episode}"
        );
        let jokers: Vec<String> = shop
            .slots
            .iter()
            .filter_map(|s| s.joker.as_ref().map(|j| j.borrow().name().to_string()))
            .collect();
        assert_eq!(
            jokers.len(),
            jokers.iter().collect::<BTreeSet<_>>().len(),
            "{episode}"
        );
    }
}

/// How many shops offer `name` while one is already held.
fn shops_offering(name: &str, showman: bool, episodes: usize) -> i32 {
    let mut found = 0;
    for episode in 0..episodes {
        let mut game = GameState::new(format!("SHOWMAN{:03}", episode), "Red Deck", 1);
        if showman {
            game.gain_joker(&joker("Showman"));
        }
        let held = game.hold_consumable(spec_or_panic(name), Edition::None);
        game.consumables.push(held);
        game._open_shop();
        found += game
            .shop
            .as_ref()
            .unwrap()
            .slots
            .iter()
            .any(|s| s.consumable.is_some_and(|c| c.name == name)) as i32;
    }
    found
}

#[test]
fn test_a_shop_withholds_what_the_run_already_holds() {
    // Holding The Star takes it out of the shop's pool: the same `used_jokers`
    // rule as the duplicate test, from the other direction.
    assert_eq!(shops_offering("The Star", false, 80), 0);
}

#[test]
fn test_a_showman_puts_it_back() {
    // Which is what the joker is for, and the shop draw never asked.
    assert!(shops_offering("The Star", true, 80) > 0);
}


// ==========================================================================
// tests/test_pack_contents.py -- "What a booster pack contains, checked card
// for card against the game."
//
// The three pure tests here read the simulator's own pack construction; the
// seven that drive the game's `create_card` through `lupa` are listed as
// engine-bound in the report.
// ==========================================================================

/// `pack_contents(rng, "Standard", n, ante, ...)` with every other pool empty.
fn pack_standard(rng: &mut RunRng, ante: i32, cards: i32, rate: f64) -> Vec<shop_pool::PackCard> {
    shop_pool::pack_contents(
        rng,
        "Standard",
        cards,
        ante,
        &[] as &[&str],
        &[] as &[&str],
        false,
        &[] as &[&str],
        &[] as &[&str],
        &[] as &[&str],
        false,
        false,
        false,
        false,
        None,
        None,
        rate,
    )
}

#[test]
fn test_the_enhancement_pool_is_the_games_order() {
    // Alphabetising it would hand out different enhancements from a seed.
    assert_eq!(
        &shop_pool::ENHANCEMENTS[..4],
        ["m_bonus", "m_mult", "m_wild", "m_glass"]
    );
    let mut sorted = shop_pool::ENHANCEMENTS;
    sorted.sort_unstable();
    assert_ne!(sorted, shop_pool::ENHANCEMENTS);
}

#[test]
fn test_pack_data_carries_the_prices() {
    // The cost is the pack's, not a function of its size class.
    let by_key = |key: &str| {
        jimbot_sim::pack_data::PACK_DATA
            .iter()
            .find(|row| row.key == key)
            .unwrap_or_else(|| panic!("no pack {key:?}"))
    };
    let buffoon = by_key("p_buffoon_normal_1");
    assert_eq!((buffoon.cards, buffoon.cost), (2, 4));
    let mega = by_key("p_arcana_mega_1");
    assert_eq!((mega.choose, mega.cards, mega.cost), (2, 5, 8));
}

#[test]
fn test_a_standard_pack_is_not_all_plain_cards() {
    // Guard against the comparison passing on a degenerate sequence.
    let mut rng = RunRng::new("TESTSEED");
    let mut cards = Vec::new();
    for _ in 0..3 {
        cards.extend(pack_standard(&mut rng, 1, 5, 1.0));
    }
    assert!(
        cards.iter().any(|c| c.enhancement.is_some()),
        "thirty standard cards and not one enhanced"
    );
    let fronts: BTreeSet<(String, String)> = cards
        .iter()
        .map(|c| {
            (
                c.rank.clone().unwrap_or_default(),
                c.suit.clone().unwrap_or_default(),
            )
        })
        .collect();
    assert!(fronts.len() > 8);
}

#[test]
fn test_a_standard_packs_cards_are_polled_at_the_runs_edition_rate() {
    // card.lua:1761 passes the pack's own doubling as poll_edition's `_mod`.
    // The non-guaranteed branch then multiplies it by `G.GAME.edition_rate`,
    // which Hone sets to 2 and Glow Up to 4 (common_events.lua:2071-2076). The
    // simulator passed the doubling and dropped the run's rate, which moves
    // exactly one boundary -- holographic against foil.
    let editions = |rate: f64| -> Vec<&'static str> {
        (1..60)
            .map(|ante| {
                let mut rng = RunRng::new(format!("EDITION{}", ante));
                pack_standard(&mut rng, ante, 1, rate)[0]
                    .edition
                    .unwrap_or("none")
            })
            .collect()
    };
    let plain = editions(1.0);
    let honed = editions(2.0);
    assert!(
        honed.iter().filter(|e| **e != "none").count()
            > plain.iter().filter(|e| **e != "none").count()
    );
    // Hone never takes an edition away, it only widens the bands.
    for (a, b) in plain.iter().zip(honed.iter()) {
        assert!(*a == "none" || *b != "none");
    }
}

#[test]
fn test_the_run_hands_its_edition_rate_to_the_pack() {
    // The wiring, so the rate cannot be right and unused.
    let mut game = GameState::new("TESTSEED", "Red Deck", 1);
    assert_eq!(game.edition_rate(), 1.0);
    let voucher = voucher_by_key("v_hone").expect("v_hone");
    game.vouchers.push(voucher);
    game._redeem_voucher(voucher);
    assert_eq!(game.edition_rate(), 2.0);
}


// ==========================================================================
// tests/test_headless.py -- "Fidelity tests against the real game's own Lua."
//
// Every test in the Python file drives the headless engine, so each ported one
// here rebuilds the *same position* in the simulator and asserts the same
// observable a player can check by hand. The ones that test the harness itself
// -- booting, the centre table, unlock profiles, the action audit, event-count
// and UIBox leaks -- are listed as engine-bound in the report.
// ==========================================================================

/// `start(jokers=(), seed=SEED)`: boot a Red Deck run, add the jokers, select
/// the blind.
fn start_run(names: &[&str]) -> GameState {
    let mut game = GameState::new("ABCDEFGH", "Red Deck", 1);
    for name in names {
        game.gain_joker(&joker(name));
    }
    game.step(&Action::new(ActionType::SelectBlind));
    game
}

/// The score of playing these hand positions, through the run's own pipeline.
fn hand_score(game: &mut GameState, indices: &[usize]) -> i64 {
    game.preview_score(indices, "roll")
}

#[test]
fn test_run_starts_with_expected_state() {
    let game = GameState::new("ABCDEFGH", "Red Deck", 1);
    assert_eq!(game.ante, 1);
    assert_eq!(game.money, 4);
    assert_eq!(game.full_deck.len(), 52);
}

#[test]
fn test_full_content_is_loaded() {
    // The whole point of running the real Lua: nothing is a subset. The
    // port's registries and data tables are the same content, so the counts
    // are asserted against them.
    assert_eq!(jimbot_sim::jokers::all_specs().len(), 150);
    assert_eq!(
        consumables::by_kind(consumables::ConsumableKind::Tarot).len(),
        22
    );
    assert_eq!(
        consumables::by_kind(consumables::ConsumableKind::Planet).len(),
        12
    );
    assert_eq!(
        consumables::by_kind(consumables::ConsumableKind::Spectral).len(),
        18
    );
    assert_eq!(jimbot_sim::shop::all_vouchers().len(), 32);
    // G.P_BLINDS: the Small and Big blinds plus every boss, ordinary then
    // finisher.
    assert_eq!(jimbot_sim::blinds::all_bosses().count() + 2, 30);
    assert_eq!(jimbot_sim::tag_data::TAG_DATA.len(), 24);
}

#[test]
fn test_seed_is_deterministic() {
    let hand_of = |seed: &str| -> Vec<(Rank, Suit)> {
        let game = start_run_with(seed, &[]);
        game.hand.iter().map(|c| (rank_of(c), suit_of(c))).collect()
    };
    assert_eq!(hand_of("ZZZ11111"), hand_of("ZZZ11111"));
    assert_ne!(hand_of("ZZZ11111"), hand_of("YYY22222"));
}

/// `start(jokers, seed)`.
fn start_run_with(seed: &str, names: &[&str]) -> GameState {
    let mut game = GameState::new(seed, "Red Deck", 1);
    for name in names {
        game.gain_joker(&joker(name));
    }
    game.step(&Action::new(ActionType::SelectBlind));
    game
}

#[test]
fn test_blind_select_deals_a_hand() {
    let game = start_run(&[]);
    assert_eq!(game.hand.len(), 8);
    assert_eq!(game.blind.as_ref().unwrap().target, 300);
    assert_eq!(game.hands_left, 4);
    // Red Deck's +1 discard, straight from the game's own deck definition.
    assert_eq!(game.discards_left, 4);
}

#[test]
fn test_discard_draws_replacements() {
    let mut game = start_run(&[]);
    let before = game.discards_left;
    game.step(&Action::with_cards(ActionType::Discard, vec![0, 1]));
    assert_eq!(game.discards_left, before - 1);
    assert_eq!(game.hand.len(), 8);
}

#[test]
fn test_two_pair_scores_by_hand() {
    // seed ABCDEFGH opens 8H 8D 7S 7H at positions 2-5 (0-indexed 1-4).
    // Two Pair is 20 chips x 2 mult; cards add 8+8+7+7 = 30. (20+30) * 2 = 100.
    let mut game = start_run(&[]);
    assert_eq!(hand_score(&mut game, &[1, 2, 3, 4]), 100);
}

#[test]
fn test_joker_scoring_matches_hand_calculation() {
    for (names, expected, why) in [
        (vec![], 100, "50 chips x 2 mult"),
        (vec!["Joker"], 300, "+4 mult: 50 x 6"),
        (vec!["The Duo"], 200, "X2 mult: 50 x 4"),
        (vec!["Joker", "The Duo"], 600, "+4 then X2: 50 x ((2+4)*2)"),
        (vec!["The Duo", "Joker"], 400, "X2 then +4: 50 x ((2*2)+4)"),
        (vec!["Blueprint", "The Duo"], 400, "Blueprint copies The Duo: 50 x (2*2*2)"),
    ] {
        let mut game = start_run(&names);
        assert_eq!(hand_score(&mut game, &[1, 2, 3, 4]), expected, "{why}");
    }
}

#[test]
fn test_joker_slot_order_changes_the_score() {
    // XMult does not commute with +Mult -- the classic Balatro gotcha.
    let mut forward = start_run(&["Joker", "The Duo"]);
    let mut reverse = start_run(&["The Duo", "Joker"]);
    assert_ne!(
        hand_score(&mut forward, &[1, 2, 3, 4]),
        hand_score(&mut reverse, &[1, 2, 3, 4])
    );
}

#[test]
fn test_beating_a_blind_advances_the_round() {
    let mut game = start_run(&["Baseball Card", "The Duo", "The Trio"]);
    for _ in 0..4 {
        if game.phase != Phase::Playing {
            break;
        }
        let count = game.hand.len().min(5);
        game.step(&Action::with_cards(ActionType::Play, (0..count).collect()));
    }
    assert!(!game.is_over());
    assert_ne!(game.phase, Phase::Playing);
}

#[test]
fn test_a_round_pays_out_and_opens_the_shop() {
    // Beating a blind must cash out and stock a shop, not park on the screen.
    let mut game = start_run(&["Baseball Card", "The Duo", "The Trio"]);
    for _ in 0..4 {
        if game.phase != Phase::Playing {
            break;
        }
        let count = game.hand.len().min(5);
        game.step(&Action::with_cards(ActionType::Play, (0..count).collect()));
    }
    assert_eq!(game.phase, Phase::RoundEval);
    let paid = game.pending_payout;
    game.step(&Action::new(ActionType::CashOut));
    assert!(paid > 0);
    assert_eq!(game.phase, Phase::Shop);
    let shop = game.shop.as_ref().unwrap();
    assert!(!shop.slots.is_empty() || !shop.packs.is_empty());
}

#[test]
fn test_shop_purchase_costs_money_and_grants_the_card() {
    let mut game = start_run(&["The Duo"]);
    for _ in 0..4 {
        if game.phase != Phase::Playing {
            break;
        }
        let count = game.hand.len().min(5);
        game.step(&Action::with_cards(ActionType::Play, (0..count).collect()));
    }
    game.step(&Action::new(ActionType::CashOut));
    game.money = 100;
    let shop = game.shop.as_ref().unwrap();
    let index = match shop.slots.iter().position(|s| s.joker.is_some()) {
        Some(index) => index,
        None => return, // this seed's first shop has no joker
    };
    let cost = game.slot_price(&game.shop.as_ref().unwrap().slots[index]);
    let before_money = game.money;
    let before_jokers = game.jokers.len();
    game.step(&Action::at(ActionType::Buy, index as i32));
    assert_eq!(game.money, before_money - cost);
    assert_eq!(game.jokers.len(), before_jokers + 1);
}


#[test]
fn test_leaving_the_shop_returns_to_blind_select() {
    let mut game = start_run(&["The Duo"]);
    for _ in 0..4 {
        if game.phase != Phase::Playing {
            break;
        }
        let count = game.hand.len().min(5);
        game.step(&Action::with_cards(ActionType::Play, (0..count).collect()));
    }
    game.step(&Action::new(ActionType::CashOut));
    game.step(&Action::new(ActionType::LeaveShop));
    assert_eq!(game.phase, Phase::BlindSelect);
    assert_eq!(game.blind.as_ref().unwrap().kind, BlindKind::Big);
}

#[test]
fn test_joker_reordering_changes_the_score() {
    // Order is strategy, not cosmetics: XMult after +Mult differs.
    let mut forward = start_run(&["Joker", "The Duo"]);
    let mut reverse = start_run(&["The Duo", "Joker"]);
    assert_eq!(hand_score(&mut forward, &[1, 2, 3, 4]), 600);
    assert_eq!(hand_score(&mut reverse, &[1, 2, 3, 4]), 400);

    // Reordering at runtime must reproduce the other ordering exactly.
    let mut moved = start_run(&["The Duo", "Joker"]);
    moved.step(&Action::at(ActionType::SwapJokerLeft, 1));
    assert_eq!(hand_score(&mut moved, &[1, 2, 3, 4]), 600);
}

#[test]
fn test_eternal_jokers_are_not_sellable() {
    let game = start_run(&["Joker", "The Duo"]);
    game.jokers[1].borrow_mut().eternal = true;
    // The engine's `api.sell` raises "cannot sell"; the simulator gates the
    // same refusal through `is_legal`/`legal_actions` rather than inside
    // `step`, so that is where the not-sellable is asserted.
    assert!(game.is_legal(&Action::at(ActionType::SellJoker, 0)));
    assert!(!game.is_legal(&Action::at(ActionType::SellJoker, 1)));
    assert!(!game
        .legal_actions()
        .contains(&Action::at(ActionType::SellJoker, 1)));
}

#[test]
fn test_booster_packs_open_with_contents() {
    // Regression: Card:open gates emplacing its cards on the pack area having
    // animated into view, so headless the pack stayed permanently empty.
    let mut game = start_run(&["Baseball Card", "The Duo", "The Trio"]);
    for _ in 0..6 {
        if game.phase != Phase::Playing {
            break;
        }
        let count = game.hand.len().min(5);
        game.step(&Action::with_cards(ActionType::Play, (0..count).collect()));
    }
    game.step(&Action::new(ActionType::CashOut));
    game.money = 100;
    if game.shop.as_ref().unwrap().packs.is_empty() {
        return; // no booster pack in this shop
    }
    game.step(&Action::at(ActionType::BuyPack, 0));
    assert_eq!(game.phase, Phase::Pack, "buying a pack did not open it");
    assert!(!game.pack_options.is_empty(), "pack opened empty");
}

#[test]
fn test_a_voucher_can_only_be_bought_once() {
    // Regression: buying a voucher must take it off the shelf.
    // Card:redeem() does not remove itself from the shop -- G.FUNCS.use_card
    // does that first. Calling redeem directly let the same voucher be bought
    // repeatedly, and Hieroglyph (-1 ante) drove a run's ante to -99.
    let mut game = start_run(&["Baseball Card", "The Duo", "The Trio"]);
    for _ in 0..6 {
        if game.phase != Phase::Playing {
            break;
        }
        let count = game.hand.len().min(5);
        game.step(&Action::with_cards(ActionType::Play, (0..count).collect()));
    }
    game.step(&Action::new(ActionType::CashOut));
    game.money = 500;

    let voucher = game
        .shop
        .as_ref()
        .unwrap()
        .vouchers
        .first()
        .map(|v| v.key);
    let voucher = match voucher {
        Some(key) => key,
        None => return, // no voucher in this shop
    };
    let before_ante = game.ante;
    game.step(&Action::at(ActionType::BuyVoucher, 0));
    assert!(
        !game
            .shop
            .as_ref()
            .unwrap()
            .vouchers
            .iter()
            .any(|v| v.key == voucher),
        "voucher still on sale after being bought"
    );

    // And the ante must not run away even if a policy keeps trying to buy.
    for _ in 0..5 {
        if game.shop.as_ref().unwrap().vouchers.is_empty() {
            break;
        }
        game.step(&Action::at(ActionType::BuyVoucher, 0));
    }
    assert!(game.ante >= before_ante - 2);
}


// ==========================================================================
// tests/test_run.py -- "One interface for every backend."
//
// `jimbot_sim.run`'s `SimRun` is the simulator backend, not part of the port,
// so these use the local `SimRun` harness; `ready_if_sold_out` is
// reimplemented below. Everything that goes through the bridge client
// (`EngineRun`, `Mirrored`, `_Driver`, `BalatroBridge`) is engine-bound.
// ==========================================================================

/// `_in_a_round()`: a Red Deck run, the blind selected.
fn in_a_round() -> SimRun {
    let mut run = SimRun::start("ABCDEFGH", "Red Deck", 1);
    run.step(&Action::new(ActionType::SelectBlind));
    run
}

/// `ready_if_sold_out`: a shop with nothing left counts as stocked.
fn ready_if_sold_out(state: &StateValue, game: &GameState) -> StateValue {
    let name = match state {
        StateValue::Map(entries) => entries
            .get("state_name")
            .map(|v| match v {
                StateValue::Str(s) => s.clone(),
                _ => String::new(),
            })
            .unwrap_or_default(),
        _ => String::new(),
    };
    let shop_ready = match state {
        StateValue::Map(entries) => match entries.get("shop_ready") {
            Some(StateValue::Int(v)) => *v,
            _ => 0,
        },
        _ => 0,
    };
    let sold_out = game.shop.as_ref().is_some_and(|shop| {
        shop.slots.is_empty() && shop.vouchers.is_empty() && shop.packs.is_empty()
    });
    if name == "SHOP" && shop_ready == 0 && game.phase == Phase::Shop && sold_out {
        let mut out = state.clone();
        if let StateValue::Map(entries) = &mut out {
            entries.insert("shop_ready".to_string(), 1.into());
        }
        return out;
    }
    state.clone()
}

#[test]
fn test_a_sim_run_speaks_the_engines_shape() {
    let mut run = SimRun::start("ABCDEFGH", "Red Deck", 1);
    assert_eq!(state_str(&run.state(), "state_name"), "BLIND_SELECT");
    assert!(!run.game.is_over());
    run.step(&Action::new(ActionType::SelectBlind));
    assert_eq!(state_str(&run.state(), "state_name"), "SELECTING_HAND");
    assert_eq!(state_len(&run.state(), "hand"), 8);
}

#[test]
fn test_a_state_does_not_differ_from_itself() {
    let mut run = SimRun::start("ABCDEFGH", "Red Deck", 1);
    let state = run.state();
    assert!(differences(&state, &state).is_empty());
    let mut richer = state.clone();
    if let StateValue::Map(entries) = &mut richer {
        if let Some(StateValue::Int(dollars)) = entries.get("dollars").cloned() {
            entries.insert("dollars".to_string(), (dollars + 2).into());
        }
    }
    let found = differences(&richer, &state);
    let firsts: Vec<String> = found
        .iter()
        .map(|line| line.split_whitespace().next().unwrap().to_string())
        .collect();
    assert_eq!(firsts, vec!["dollars"]);
}


#[test]
fn test_a_highlight_is_held_by_card_as_the_game_holds_it() {
    let mut run = in_a_round();
    run.toggle(6);
    run.toggle(2);
    let picked: BTreeSet<u64> = run
        .selection()
        .iter()
        .map(|i| uid_of(&run.game.hand[*i]))
        .collect();
    assert_eq!(run.selection(), vec![2, 6]);
    run.swap_card_left(2);
    assert_eq!(run.selection(), vec![1, 6]);
    run.sort_hand("suit");
    let after: BTreeSet<u64> = run
        .selection()
        .iter()
        .map(|i| uid_of(&run.game.hand[*i]))
        .collect();
    assert_eq!(after, picked);
    assert_eq!(state_int(&run.state(), "toggles_used"), 2);
    run.toggle(run.selection()[0]);
    assert_eq!(run.selection().len(), 1);
    run.clear();
    assert!(run.selection().is_empty());
    assert_eq!(state_int(&run.state(), "toggles_used"), 3); // a clear is not a toggle
}

#[test]
fn test_a_sixth_card_is_not_highlighted() {
    let mut run = in_a_round();
    for i in 0..6 {
        run.toggle(i);
    }
    assert_eq!(run.selection(), vec![0, 1, 2, 3, 4]);
    let state = run.state();
    assert_eq!(state_int(&state, "selection_size"), 5);
    assert_eq!(state_int(&state, "toggles_used"), 6); // the refused one counts
}

#[test]
fn test_a_play_starts_the_highlight_over() {
    let mut run = in_a_round();
    run.toggle(0);
    let chosen = run.selection();
    run.step(&Action::with_cards(ActionType::Play, chosen));
    assert!(run.selection().is_empty());
    assert_eq!(state_int(&run.state(), "toggles_used"), 0);
}

#[test]
fn test_each_sort_button_is_one_press_a_hand() {
    let mut run = in_a_round();
    assert_eq!(
        (
            state_int(&run.state(), "sorted_rank"),
            state_int(&run.state(), "sorted_suit")
        ),
        (0, 0)
    );
    run.sort_hand("suit");
    assert_eq!(
        (
            state_int(&run.state(), "sorted_rank"),
            state_int(&run.state(), "sorted_suit")
        ),
        (0, 1)
    );
    run.toggle(0);
    let chosen = run.selection();
    run.step(&Action::with_cards(ActionType::Discard, chosen));
    assert_eq!(
        (
            state_int(&run.state(), "sorted_rank"),
            state_int(&run.state(), "sorted_suit")
        ),
        (0, 0)
    );
}

#[test]
fn test_the_joker_swap_budget_follows_the_round_and_the_row() {
    let mut run = in_a_round();
    for name in ["Joker", "Blueprint"] {
        run.game.gain_joker(&joker(name));
    }
    run.step(&Action::at(ActionType::SwapJokerLeft, 1));
    run.step(&Action::at(ActionType::SwapJokerLeft, 1));
    assert_eq!(state_int(&run.state(), "joker_swaps_used"), 2);
    run.game.gain_joker(&joker("Jolly Joker"));
    assert_eq!(state_int(&run.state(), "joker_swaps_used"), 0); // a new set of jokers
}

#[test]
fn test_a_round_end_is_cashed_out_by_advance() {
    let mut run = in_a_round();
    run.game.blind.as_mut().unwrap().target = 1;
    run.toggle(0);
    let chosen = run.selection();
    run.step(&Action::with_cards(ActionType::Play, chosen));
    assert_eq!(run.game.phase, Phase::RoundEval);
    assert!(run.advance());
    assert_eq!(state_str(&run.state(), "state_name"), "SHOP");
    assert!(!run.advance());
}

#[test]
fn test_a_patch_after_clicks_is_the_state_rebuilt() {
    let mut run = in_a_round();
    let mut state = run.state();
    for i in [3, 1, 4, 1, 5] {
        run.toggle(i);
        state = run.patched(&state);
        assert_eq!(state, run.state());
    }
    run.clear();
    assert_eq!(run.patched(&state), run.state());
}

#[test]
fn test_an_adopted_position_keeps_its_highlight() {
    let game = in_a_round().game;
    let mut run = SimRun::new(game);
    run.adopt(vec![2, 0, 9]); // 9 is past the hand
    assert_eq!(run.selection(), vec![0, 2]);
    assert_eq!(state_int(&run.state(), "toggles_used"), 2);
}

#[test]
fn test_a_bought_out_shop_is_not_waited_on() {
    // `shop_ready` is raised while any shelf holds a card, so a shop bought
    // out of everything never raised it and `advance` waited thirty seconds.
    let mut game = GameState::new("AWEFRTUZ", "Blue Deck", 1);
    game._open_shop();
    assert_eq!(game.phase, Phase::Shop);
    let stocking = StateValue::Map(
        [
            ("state_name".to_string(), StateValue::Str("SHOP".to_string())),
            ("shop_ready".to_string(), 0.into()),
        ]
        .into_iter()
        .collect(),
    );
    // A shop still dealing is waited on: the shadow's shelves are full.
    assert_eq!(state_int(&ready_if_sold_out(&stocking, &game), "shop_ready"), 0);
    game.shop.as_mut().unwrap().slots.clear();
    game.shop.as_mut().unwrap().packs.clear();
    game.shop.as_mut().unwrap().vouchers.clear();
    assert_eq!(state_int(&ready_if_sold_out(&stocking, &game), "shop_ready"), 1);
    // And nothing else about the state is touched.
    let other = StateValue::Map(
        [(
            "state_name".to_string(),
            StateValue::Str("SELECTING_HAND".to_string()),
        )]
        .into_iter()
        .collect(),
    );
    assert_eq!(ready_if_sold_out(&other, &game), other);
}

