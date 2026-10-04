//! Card areas, enhancements, joker lifecycle and the affordability rules.
//!
//! Every number the tests assert was produced by the Python original. The
//! `FIXTURE` block below is the generator's own output; the replay is here so
//! the Rust rules are checked against the simulator rather than against a
//! hand-written guess. Regenerated with:
//!
//! ```text
//!     cd /home/marcin/balatro_bot && .venv/bin/python - <<'PY'
//!     import sys; sys.path.insert(0, 'external/jimbot-sim/src')
//!     from jimbot_sim.game import GameState
//!     from jimbot_sim.jokers import JokerInstance, REGISTRY
//!     g = GameState(0, 'Red Deck', 1); g.money = 10
//!     g.add_money(5, 'X'); g.add_money(-3, 'Y'); g.add_money(0, 'Z')
//!     print('money', g.money); print('logs', '|'.join(g.logs))
//!     print('affords', ','.join('1' if g.affords(c) else '0'
//!           for c in (-1, 0, 1, 11, 12, 13)))
//!     print('bankrupt_plain', g.bankrupt_at)
//!     g.jokers.append(JokerInstance(REGISTRY['Credit Card']))
//!     print('credit_card', g.bankrupt_at, g.spendable,
//!           ','.join('1' if g.affords(c) else '0'
//!                    for c in (0, 1, 35, 36, 37)))
//!     PY
//! ```

use std::rc::Rc;

use rumbot_sim::cards::{
    enhancement_of, extra_chips_of, make_card, played_this_ante_of, set_played_this_ante, Edition,
    Enhancement, Rank, Suit,
};
use rumbot_sim::game::GameState;
use rumbot_sim::jokers::{JokerInstance, JokerSpec};

const FIXTURE: &str = "\
money\t10\t5\t-3\t0\t12\n\
logs\tX: +$5|Y: -$3\n\
affords\t1,1,1,1,1,0\n\
bankrupt_plain\t0\n\
spendable_plain\t12\n\
credit_card\t-20\t32\t1,1,0,0,0\n";

/// One tab-split field of a fixture record.
fn field(record: &str, index: usize) -> &str {
    record.split('\t').nth(index).unwrap()
}

/// A fixture line by its first column.
fn record(key: &str) -> &'static str {
    FIXTURE
        .lines()
        .find(|line| line.split('\t').next() == Some(key))
        .unwrap()
}

/// The flags a fixture record encodes as `1`/`0`, comma separated.
fn flags(record: &str, index: usize) -> Vec<bool> {
    field(record, index).split(',').map(|f| f == "1").collect()
}

fn game() -> GameState {
    GameState::new("SEED0000", "Red Deck", 1)
}

#[test]
fn a_card_added_lands_in_both_piles_and_is_one_object() {
    let mut game = game();
    let card = make_card(Rank::King, Suit::Hearts);
    let deck_before = game.full_deck.len();
    let pile_before = game.draw_pile.len();

    game.add_card(&card);

    assert_eq!(game.full_deck.len(), deck_before + 1);
    assert_eq!(game.draw_pile.len(), pile_before + 1);
    // CardArea:emplace puts it at the *front* of the draw pile -- the deck's
    // bottom -- not the top.
    assert!(Rc::ptr_eq(game.draw_pile.first().unwrap(), &card));
    assert!(Rc::ptr_eq(game.full_deck.last().unwrap(), &card));

    // Mutate through one handle: the other sees it, because there is one
    // object, exactly as Python's reference does.
    game.draw_pile[0].borrow_mut().extra_chips = 9;
    assert_eq!(extra_chips_of(game.full_deck.last().unwrap()), 9);
}

#[test]
fn remove_card_takes_it_out_of_every_pile_it_is_in() {
    let mut game = game();
    let card = make_card(Rank::King, Suit::Hearts);
    let deck = game.full_deck.len();
    let pile = game.draw_pile.len();

    game.add_card(&card);
    game.add_card_to_hand(&card);
    game.discard_pile.push(card.clone());
    // The same handle now sits in all four piles (twice in the deck).
    assert_eq!(game.full_deck.len(), deck + 2);
    assert_eq!(game.draw_pile.len(), pile + 1);
    assert_eq!(game.hand.len(), 1);
    assert_eq!(game.discard_pile.len(), 1);

    game.remove_card(&card, false);

    // One occurrence is removed from each pile, as `list.remove` does: the
    // deck keeps the second copy.
    assert_eq!(game.full_deck.len(), deck + 1);
    assert_eq!(game.draw_pile.len(), pile);
    assert!(game.hand.is_empty());
    assert!(game.discard_pile.is_empty());
}

#[test]
fn set_enhancement_keeps_extra_chips_and_clears_played_this_ante() {
    let mut game = game();
    let card = make_card(Rank::Ace, Suit::Spades);
    card.borrow_mut().extra_chips = 5;
    set_played_this_ante(&card, true);

    game.set_enhancement(&card, Enhancement::Glass);

    assert_eq!(enhancement_of(&card), Enhancement::Glass);
    // perma_bonus survives the rebuild; played_this_ante is what The Pillar
    // debuffs and does not.
    assert_eq!(extra_chips_of(&card), 5);
    assert!(!played_this_ante_of(&card));
}

#[test]
fn destroy_joker_removes_exactly_that_joker_and_leaves_the_others() {
    // A synthetic spec: this test is about the lifecycle, and the real joker
    // table is another module's.
    static JOKER: JokerSpec = JokerSpec {
        name: "Joker",
        ..JokerSpec::DEFAULT
    };
    let mut game = game();
    let first = rumbot_sim::jokers::make_ref(JokerInstance::new(&JOKER));
    let second = rumbot_sim::jokers::make_ref(JokerInstance::new(&JOKER));
    game.jokers.push(first.clone());
    game.jokers.push(second.clone());

    game.destroy_joker(&first, "test");

    assert_eq!(game.jokers.len(), 1);
    assert!(Rc::ptr_eq(&game.jokers[0], &second));

    // An eternal joker is never removed, whatever is asked.
    let eternal = rumbot_sim::jokers::make_ref(JokerInstance::new(&JOKER));
    eternal.borrow_mut().eternal = true;
    game.jokers.push(eternal.clone());
    game.destroy_joker(&eternal, "test");
    assert_eq!(game.jokers.len(), 2);
    assert!(Rc::ptr_eq(game.jokers.last().unwrap(), &eternal));
}

#[test]
fn money_spendable_and_affords_agree_with_the_python_rules() {
    let money = record("money");
    let logs = record("logs");
    let affords = record("affords");

    let mut game = game();
    game.money = field(money, 1).parse().unwrap();
    game.add_money(field(money, 2).parse().unwrap(), "X");
    game.add_money(field(money, 3).parse().unwrap(), "Y");
    game.add_money(field(money, 4).parse().unwrap(), "Z");

    assert_eq!(game.money, field(money, 5).parse::<i32>().unwrap());
    assert_eq!(game.logs.join("|"), field(logs, 1));

    assert_eq!(
        game.bankrupt_at(),
        field(record("bankrupt_plain"), 1).parse::<i32>().unwrap()
    );
    assert_eq!(
        game.spendable(),
        field(record("spendable_plain"), 1).parse::<i32>().unwrap()
    );
    let costs = [-1, 0, 1, 11, 12, 13];
    for (cost, want) in costs.iter().zip(flags(affords, 1)) {
        assert_eq!(game.affords(*cost), want, "affords({})", cost);
    }
}

#[test]
fn credit_card_moves_the_floor_and_the_free_escape_stays() {
    let rec = record("credit_card");
    let want_bankrupt: i32 = field(rec, 1).parse().unwrap();
    let want_spendable: i32 = field(rec, 2).parse().unwrap();

    // A synthetic Credit Card: its whole effect is the twenty dollars of debt
    // limit, so only `debt_limit` matters here.
    static CREDIT: JokerSpec = JokerSpec {
        name: "Credit Card",
        cost: 4,
        debt_limit: 20,
        ..JokerSpec::DEFAULT
    };
    let mut game = game();
    game.money = 12;
    game.jokers
        .push(rumbot_sim::jokers::make_ref(JokerInstance::new(&CREDIT)));

    assert_eq!(game.bankrupt_at(), want_bankrupt);
    assert_eq!(game.spendable(), want_spendable);
    let costs = [0, 1, 35, 36, 37];
    for (cost, want) in costs.iter().zip(flags(rec, 3)) {
        assert_eq!(game.affords(*cost), want, "affords({})", cost);
    }
    // A free item is always takeable, even past the floor.
    let mut broke = GameState::new("SEED0000", "Red Deck", 1);
    broke.money = -100;
    assert!(broke.affords(0));
    assert!(!broke.affords(1));
}

#[test]
fn card_cost_is_the_games_set_cost_formula() {
    let mut game = game();
    assert_eq!(game.card_cost(4, Edition::None), 4);
    // A $10 voucher under Clearance Sale is floor(10.5 * 0.75) = $7, not $8.
    game.vouchers.push(rumbot_sim::shop::Voucher {
        discount_percent: 25,
        ..Default::default()
    });
    assert_eq!(game.card_cost(10, Edition::None), 7);
    // The game floors at one, so a heavily discounted cheap card is not free.
    game.vouchers[0].discount_percent = 60;
    assert_eq!(game.card_cost(1, Edition::None), 1);
    // Edition value is added before the discount and the half.
    game.vouchers.clear();
    assert_eq!(game.card_cost(4, Edition::Polychrome), 9);
    assert_eq!(game.card_cost(4, Edition::None), 4);
}

#[test]
fn seen_centers_is_the_centres_that_currently_exist() {
    let mut game = game();
    assert!(game.seen_centers().is_empty());
    // A held joker names its centre; the key is the game's own.
    static JOKER: JokerSpec = JokerSpec {
        name: "Joker",
        ..JokerSpec::DEFAULT
    };
    game.jokers
        .push(rumbot_sim::jokers::make_ref(JokerInstance::new(&JOKER)));
    assert!(game.seen_centers().contains("j_joker"));
}
