//! Shared scoring context passed to every joker and card effect.
//!
//! The Python original holds a live `GameState` reference (`ctx.game`) and reads
//! money and probabilities straight off the run. Rust cannot lend the run out
//! mutably while the context is being passed around by the same pipeline, so the
//! two values those reads need are *injected* instead: `base_money` is the run's
//! dollars when the hand began, and `probability_scale` is Oops! All 6s's
//! numerator. Both are constants for the duration of a hand -- the simulator
//! banks `money_gained` only after scoring, so `game.money` does not move while
//! any of this runs -- which makes the substitution exact rather than
//! approximate.
//!
//! One deliberate difference: the log strings use Rust's float formatting where
//! Python wrote `%g`. The logs are diagnostics that no test compares, and
//! reproducing `%g`'s six-significant-digit rule would add a formatter to every
//! scoring path for text nobody reads back.

use crate::cards::CardRef;
use crate::hands::{HandSet, HandType};

/// Mutable chip/mult accumulator for one played hand.
#[derive(Clone, Debug)]
pub struct ScoreContext {
    pub hand: HandType,
    pub scoring: Vec<CardRef>,
    pub played: Vec<CardRef>,
    pub held: Vec<CardRef>,
    /// The run's dollars when the hand started; see the module comment.
    pub base_money: i32,
    /// Oops! All 6s: 2.0 when it is in the row, else 1.0.
    pub probability_scale: f64,
    /// Every hand the played cards contain, which is what the "if hand
    /// contains a Pair" jokers actually ask about.
    pub contains: HandSet,
    pub chips: f64,
    pub mult: f64,
    pub money_gained: i32,
    /// Lucky card triggers this hand, hit or miss. See `preview_outcome`.
    pub lucky_rolls: i32,
    /// The money a play earns in expectation rather than as rolled: what each
    /// chance payout paid (`chance_paid`) and what it is worth at its odds
    /// (`chance_expected`), so the certain money plus the expected is
    /// `money_gained - chance_paid + chance_expected`. Business Card's $2 on a
    /// coin read $2 or nothing on one roll, and four fixed rolls read a King's
    /// coin as $2 every time. See `preview_money`.
    pub chance_paid: i32,
    pub chance_expected: f64,
    /// Every line carries the running totals as well as the change. Without
    /// them the log says what happened and not what it added up to, and a hand
    /// that scores wrong is a question about the total at each step -- which
    /// joker took it away from what the real game reached.
    pub log: Vec<String>,
}

impl ScoreContext {
    pub fn new(
        hand: HandType,
        scoring: Vec<CardRef>,
        played: Vec<CardRef>,
        held: Vec<CardRef>,
        contains: HandSet,
        base_money: i32,
        probability_scale: f64,
    ) -> Self {
        ScoreContext {
            hand,
            scoring,
            played,
            held,
            base_money,
            probability_scale,
            contains,
            chips: 0.0,
            mult: 0.0,
            money_gained: 0,
            lucky_rolls: 0,
            chance_paid: 0,
            chance_expected: 0.0,
            log: Vec::new(),
        }
    }

    /// Count a chance payout at its odds, scaled as the roll is by any Oops! All
    /// 6s, before the roll decides what it actually pays.
    pub fn expect(&mut self, dollars: f64, numerator: i32, denominator: i32) {
        let chance = numerator as f64 * self.probability_scale / denominator as f64;
        self.chance_expected += dollars * chance.min(1.0);
    }

    /// The dollars a joker scoring *now* would see.
    ///
    /// `money_gained` is banked once, after the hand. The game does not wait:
    /// `ease_dollars` runs as each card pays, so a joker further right in the row
    /// reads the larger number.
    ///
    /// Recording 10 stopped on the difference. A Flush of five Diamonds with
    /// Rough Gem left of Bull: Rough Gem pays $1 a Diamond and Hack retriggers
    /// the Five, so six dollars land during the hand. Bull is "+2 Chips per
    /// dollar held" and read $27 instead of $33 -- 54 chips against 66, and 5427
    /// against the game's 5913 on a hand that decided the blind.
    pub fn money(&self) -> i32 {
        (self.base_money + self.money_gained).max(0)
    }

    pub fn add_chips(&mut self, amount: f64, source: &str) {
        if amount != 0.0 {
            self.chips += amount;
            self.log.push(format!(
                "{}: +{} chips -> {} x {}",
                source, amount, self.chips, self.mult
            ));
        }
    }

    pub fn add_mult(&mut self, amount: f64, source: &str) {
        if amount != 0.0 {
            self.mult += amount;
            self.log.push(format!(
                "{}: +{} mult -> {} x {}",
                source, amount, self.chips, self.mult
            ));
        }
    }

    pub fn times_mult(&mut self, factor: f64, source: &str) {
        if factor != 1.0 {
            self.mult *= factor;
            self.log.push(format!(
                "{}: x{} mult -> {} x {}",
                source, factor, self.chips, self.mult
            ));
        }
    }

    /// Balatro truncates the product of chips and mult.
    pub fn score(&self) -> i64 {
        (self.chips * self.mult) as i64
    }
}
