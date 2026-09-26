//! Blind schedule, ante scaling and boss blind effects.

use crate::cards::Suit;

/// The chip requirement per ante, one row per scaling. get_blind_amount keeps
/// three tables and picks by G.GAME.modifiers.scaling, which the stake sets:
/// 1 up to Green, 2 from Green, 3 from Purple. The higher rows are steeper
/// everywhere, not just at the top -- ante 3 is 2000, 2600 or 3200.
const ANTE_BASE: [&[i64; 8]; 3] = [
    &[300, 800, 2000, 5000, 11000, 20000, 35000, 50000],
    &[300, 900, 2600, 8000, 20000, 36000, 60000, 100000],
    &[300, 1000, 3200, 9000, 25000, 60000, 110000, 200000],
];

/// The chips an ante asks for, exactly as get_blind_amount computes them.
///
/// Past ante eight the game leaves the table behind for a formula, and then
/// rounds the result down to two significant figures -- `amount - amount %
/// 10^floor(log10(amount)-1)`. Multiplying by 1.6 in a loop, which is what
/// this did, drifts from it immediately.
///
/// The Python original returns a Python int of unbounded size. This returns
/// the saturating `i64` view: the float-to-int cast clamps at `i64::MAX`,
/// which the value passes at ante 16 and up, so the two agree only to ante 15.
/// `tools/gen_blinds_fixture.py` clamps the same way and compares like for
/// like, but that is this fixture's own choice -- `state_dict`'s `blinds` row
/// is the Python int, so the *observation* diverges past ante 15 in endless
/// play. Closing that needs arbitrary-precision integers throughout (a
/// `chips_scored` of i64 has the same ceiling), so the divergence is recorded
/// rather than papered over; the fuzz fixture is capped below it.
pub fn ante_base_chips(ante: i32, scaling: i32) -> i64 {
    let amounts: &[i64; 8] = match scaling {
        2 => ANTE_BASE[1],
        3 => ANTE_BASE[2],
        _ => ANTE_BASE[0],
    };
    if ante < 1 {
        return 100;
    }
    if ante <= 8 {
        return amounts[(ante - 1) as usize];
    }
    let a = amounts[7] as f64;
    let b = 1.6;
    let c = ante - 8;
    let k = 0.75;
    let d = 1.0 + 0.2 * (ante - 8) as f64;
    let amount = (a * (b + (k * c as f64).powf(d)).powf(c as f64)).floor();
    let modulus = 10f64.powi((amount.log10() - 1.0).floor() as i32);
    let amount = amount as i64;
    amount - amount % (modulus as i64)
}

/// The three blind tiers, in the order the game offers them.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BlindKind {
    Small,
    Big,
    Boss,
}

impl BlindKind {
    /// The game's own lowercase name.
    pub fn as_str(self) -> &'static str {
        match self {
            BlindKind::Small => "small",
            BlindKind::Big => "big",
            BlindKind::Boss => "boss",
        }
    }

    /// The chip multiplier of the blind's tier, before deck scaling.
    pub fn mult(self) -> f64 {
        match self {
            BlindKind::Small => 1.0,
            BlindKind::Big => 1.5,
            BlindKind::Boss => 2.0,
        }
    }

    /// What beating this tier pays -- before the finisher rule in `reward_for`.
    pub fn reward(self) -> i32 {
        match self {
            BlindKind::Small => 3,
            BlindKind::Big => 4,
            BlindKind::Boss => 5,
        }
    }
}

/// Except the five finishers, which pay eight. Straight off P_BLINDS in
/// game.lua, where every one of the twenty-three ordinary bosses carries
/// `dollars = 5` and each of
///
/// ```text
///     bl_final_acorn  bl_final_bell  bl_final_heart
///     bl_final_leaf   bl_final_vessel
/// ```
///
/// carries `dollars = 8`. Paying every boss five made an ante-8 win three
/// dollars short every time, and recording 12 is where that showed: the
/// Crimson Heart cash-out paid $15 there and $12 here. `showdown` in
/// BOSS_DATA is the same flag the game reads, so the two cannot drift apart
/// without the generator noticing.
pub const FINISHER_REWARD: i32 = 8;

/// Declarative boss modifiers; the engine reads these fields directly.
///
/// The four face-down bosses change no rule: a face-down card scores as
/// itself. What they change is what a player can see, so the engine marks the
/// cards (`Card::face_down`) the way `Blind:stay_flipped` (blind.lua:605)
/// decides it, and a policy that plays fair reads the mark. The Wheel's roll
/// is a real draw on the `wheel` pool, one per card dealt into the hand.
#[derive(Clone, Debug, PartialEq)]
pub struct BossEffect {
    pub name: &'static str,
    pub text: &'static str,
    pub chip_mult: f64,
    pub debuff_suit: Option<Suit>,
    pub debuff_face: bool,
    pub hand_size_delta: i32,
    pub hands_delta: i32,
    pub discards_delta: i32,
    pub min_cards_played: i32,
    pub money_per_card_played: i32,
    pub zero_money_on_most_played: bool,
    pub discard_random_on_play: i32,
    pub level_down_played_hand: bool,
    pub no_repeat_hand: bool,
    pub lock_first_hand_type: bool,
    pub debuff_previously_played: bool,
    pub halve_base: bool,
    // The face-down draws, one field a boss (blind.lua:605-620).
    /// The House: dealt face down while no hand has been played and no
    /// discard used this round.
    pub face_down_first_hand: bool,
    /// The Wheel: each card dealt face down on `normal / odds`.
    pub face_down_odds: i32,
    /// The Mark: face cards, `is_face(true)` -- every card beside Pareidolia.
    pub face_down_faces: bool,
    /// The Fish: the draw that follows a played hand, off `Blind.prepped`.
    pub face_down_after_play: bool,
    // The finishers, and one ordinary boss, that do something to the run
    // rather than to a card. These were all left blank on the grounds that
    // face-down cards mean nothing to an engine with full information, which
    // is true of the rules of four of them and not of these five.
    pub always_draw_three: bool,
    /// Amber Acorn: turns the row face down (`JokerInstance::face_down`) and
    /// shuffles it three times on `aajk`.
    pub shuffles_jokers: bool,
    pub debuff_until_sale: bool,
    pub debuff_a_joker: bool,
    pub forces_a_card: bool,
    pub is_finisher: bool,
}

impl BossEffect {
    /// A boss with the Python dataclass defaults: chip_mult 2.0, every flag
    /// false, every delta zero. Fields are overridden with struct update
    /// syntax: `BossEffect { debuff_face: true, ..BossEffect::new(..) }`.
    pub const fn new(name: &'static str, text: &'static str) -> BossEffect {
        BossEffect {
            name,
            text,
            chip_mult: 2.0,
            debuff_suit: None,
            debuff_face: false,
            hand_size_delta: 0,
            hands_delta: 0,
            discards_delta: 0,
            min_cards_played: 0,
            money_per_card_played: 0,
            zero_money_on_most_played: false,
            discard_random_on_play: 0,
            level_down_played_hand: false,
            no_repeat_hand: false,
            lock_first_hand_type: false,
            debuff_previously_played: false,
            halve_base: false,
            face_down_first_hand: false,
            face_down_odds: 0,
            face_down_faces: false,
            face_down_after_play: false,
            always_draw_three: false,
            shuffles_jokers: false,
            debuff_until_sale: false,
            debuff_a_joker: false,
            forces_a_card: false,
            is_finisher: false,
        }
    }
}

impl Default for BossEffect {
    fn default() -> Self {
        BossEffect::new("", "")
    }
}

/// The twenty-three ordinary bosses, in the game's order.
pub const BOSSES: &[BossEffect] = &[
    BossEffect {
        discard_random_on_play: 2,
        ..BossEffect::new("The Hook", "Discards 2 random cards per hand played")
    },
    BossEffect {
        zero_money_on_most_played: true,
        ..BossEffect::new("The Ox", "Playing your most played hand sets money to $0")
    },
    BossEffect {
        face_down_first_hand: true,
        ..BossEffect::new("The House", "First hand is drawn face down")
    },
    BossEffect {
        chip_mult: 4.0,
        ..BossEffect::new("The Wall", "Extra large blind")
    },
    BossEffect {
        face_down_odds: 7,
        ..BossEffect::new("The Wheel", "1 in 7 cards get drawn face down")
    },
    BossEffect {
        level_down_played_hand: true,
        ..BossEffect::new("The Arm", "Decrease level of played poker hand")
    },
    BossEffect {
        debuff_suit: Some(Suit::Clubs),
        ..BossEffect::new("The Club", "All Club cards are debuffed")
    },
    BossEffect {
        face_down_after_play: true,
        ..BossEffect::new("The Fish", "Cards drawn face down after each hand played")
    },
    BossEffect {
        min_cards_played: 5,
        ..BossEffect::new("The Psychic", "Must play 5 cards")
    },
    BossEffect {
        debuff_suit: Some(Suit::Spades),
        ..BossEffect::new("The Goad", "All Spade cards are debuffed")
    },
    BossEffect {
        discards_delta: -99,
        ..BossEffect::new("The Water", "Start with 0 discards")
    },
    BossEffect {
        debuff_suit: Some(Suit::Diamonds),
        ..BossEffect::new("The Window", "All Diamond cards are debuffed")
    },
    BossEffect {
        hand_size_delta: -1,
        ..BossEffect::new("The Manacle", "-1 hand size")
    },
    BossEffect {
        no_repeat_hand: true,
        ..BossEffect::new("The Eye", "No repeat hand types this round")
    },
    BossEffect {
        lock_first_hand_type: true,
        ..BossEffect::new("The Mouth", "Play only one hand type this round")
    },
    BossEffect {
        debuff_face: true,
        ..BossEffect::new("The Plant", "All face cards are debuffed")
    },
    BossEffect {
        always_draw_three: true,
        ..BossEffect::new("The Serpent", "After play or discard, always draw 3 cards")
    },
    BossEffect {
        debuff_previously_played: true,
        ..BossEffect::new("The Pillar", "Cards played earlier this ante are debuffed")
    },
    // One hand, and the *small* blind's requirement for it: bl_needle is
    // `mult = 1` in game.lua:285, alone among the ordinary bosses. The
    // default here said two, so the simulator asked for twice what the
    // game asks -- see the check below, which is why it cannot happen
    // again.
    BossEffect {
        hands_delta: -99,
        chip_mult: 1.0,
        ..BossEffect::new("The Needle", "Play only 1 hand")
    },
    BossEffect {
        debuff_suit: Some(Suit::Hearts),
        ..BossEffect::new("The Head", "All Heart cards are debuffed")
    },
    BossEffect {
        money_per_card_played: -1,
        ..BossEffect::new("The Tooth", "Lose $1 per card played")
    },
    BossEffect {
        halve_base: true,
        ..BossEffect::new("The Flint", "Base Chips and Mult are halved")
    },
    BossEffect {
        face_down_faces: true,
        ..BossEffect::new("The Mark", "All face cards are drawn face down")
    },
];

/// The five finisher bosses, in the game's order.
pub const FINISHER_BOSSES: &[BossEffect] = &[
    BossEffect {
        is_finisher: true,
        shuffles_jokers: true,
        ..BossEffect::new("Amber Acorn", "Flips and shuffles all Jokers")
    },
    BossEffect {
        is_finisher: true,
        debuff_until_sale: true,
        ..BossEffect::new("Verdant Leaf", "All cards debuffed until a Joker is sold")
    },
    BossEffect {
        chip_mult: 6.0,
        is_finisher: true,
        ..BossEffect::new("Violet Vessel", "Very large blind")
    },
    BossEffect {
        is_finisher: true,
        debuff_a_joker: true,
        ..BossEffect::new("Crimson Heart", "One random Joker disabled each hand")
    },
    BossEffect {
        is_finisher: true,
        forces_a_card: true,
        ..BossEffect::new("Cerulean Bell", "Forces one card to always be selected")
    },
];

/// The boss with this display name, ordinary or finisher.
pub fn boss_by_name(name: &str) -> Option<&'static BossEffect> {
    all_bosses().find(|boss| boss.name == name)
}

/// Every boss, ordinary then finisher.
pub fn all_bosses() -> impl Iterator<Item = &'static BossEffect> {
    BOSSES.iter().chain(FINISHER_BOSSES.iter())
}

/// A blind at a given ante: its target, its reward and any boss in force.
#[derive(Clone, Debug)]
pub struct Blind {
    pub kind: BlindKind,
    pub ante: i32,
    pub target: i64,
    pub reward: i32,
    pub boss: Option<&'static BossEffect>,
    // Set by Luchador, Chicot and The Fool's Gold. The game keeps this on the
    // blind rather than on the joker, which matters: the blind stays disabled
    // for the rest of the round even after the joker that did it is gone.
    pub disabled: bool,
    // G.GAME.blind.triggered: whether the boss's ability went off on the hand
    // being played, which is the only thing Matador reads. Starts at nil in
    // set_blind (blind.lua:93); every play clears it, and GameState._play and
    // score_hand set it again the way the game does.
    pub triggered: bool,
    // Set by set_blind (blind.lua:94) -- which here is _start_round, not the
    // moment a blind is put on offer -- and by press_play when Crimson Heart
    // has a joker to take (blind.lua:488-493); cleared by drawn_to_hand
    // (blind.lua:602). Crimson Heart reads it in GameState._drawn_to_hand, and
    // The Fish in GameState._stay_flipped: set_blind clears it for the Fish
    // (blind.lua:176) and every press_play sets it (blind.lua:494), so only
    // the draw after a played hand is dealt face down.
    pub prepped: bool,
    // On offer on the blind select screen and not yet set (set_blind is
    // _start_round). The game's G.GAME.blind is then the empty one the last
    // round left (blind.lua:336), so a boss on deck does nothing: see
    // GameState.boss. A blind built any other way -- a scenario, a policy's
    // fork pricing against the boss to come -- is in force.
    pub on_deck: bool,
}

impl Blind {
    /// The display name: the boss's, or the tier title-cased plus " Blind".
    pub fn name(&self) -> String {
        match self.boss {
            Some(boss) => boss.name.to_string(),
            None => format!(
                "{} Blind",
                match self.kind {
                    BlindKind::Small => "Small",
                    BlindKind::Big => "Big",
                    BlindKind::Boss => "Boss",
                }
            ),
        }
    }
}

/// What beating this blind pays, which is not one number per kind.
pub fn reward_for(kind: BlindKind, boss: Option<&BossEffect>) -> i32 {
    if kind == BlindKind::Boss && boss.is_some_and(|boss| boss.is_finisher) {
        return FINISHER_REWARD;
    }
    kind.reward()
}

/// The game: get_blind_amount(ante) * mult * ante_scaling.
///
/// ante_scaling comes from the deck -- the Plasma Deck doubles every target
/// in the run, which is the price it pays for balancing chips and mult.
pub fn make_blind(
    kind: BlindKind,
    ante: i32,
    boss: Option<&'static BossEffect>,
    ante_scaling: f64,
    scaling: i32,
    no_reward: bool,
) -> Blind {
    let mult = match (kind, boss) {
        (BlindKind::Boss, Some(boss)) => boss.chip_mult,
        _ => kind.mult(),
    };
    Blind {
        kind,
        ante,
        target: (ante_base_chips(ante, scaling) as f64 * mult * ante_scaling) as i64,
        reward: if no_reward { 0 } else { reward_for(kind, boss) },
        boss,
        disabled: false,
        triggered: false,
        prepped: false,
        on_deck: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::boss_data::BOSS_DATA;

    /// The multiplier is written twice -- here, and in the table generated from
    /// the game's own P_BLINDS -- so it is checked here rather than trusted. The
    /// Needle is what this is for: `mult = 1` in the game and two by default
    /// here, so the simulator asked four thousand chips for a blind the game
    /// prices at two, and nothing said so until the policy played the engine
    /// with a shadow beside it.
    #[test]
    fn every_boss_multiplier_agrees_with_boss_data() {
        for boss in all_bosses() {
            let row = BOSS_DATA
                .iter()
                .find(|row| row.name == boss.name)
                .unwrap_or_else(|| panic!("{} is not in BOSS_DATA", boss.name));
            assert_eq!(
                boss.chip_mult, row.chip_mult,
                "{} asks x{} here and x{} in the game's own table",
                boss.name, boss.chip_mult, row.chip_mult
            );
        }
    }

    /// The finishers pay eight, the ordinary bosses five.
    #[test]
    fn only_finishers_pay_the_finisher_reward() {
        for boss in all_bosses() {
            let reward = reward_for(BlindKind::Boss, Some(boss));
            if boss.is_finisher {
                assert_eq!(reward, FINISHER_REWARD);
            } else {
                assert_eq!(reward, 5);
            }
        }
    }

    /// The past-ante-eight formula rounds to two significant figures, so every
    /// result past ante eight ends in the right number of zeros.
    #[test]
    fn past_ante_eight_rounds_to_two_significant_figures() {
        for ante in 9..=15 {
            let value = ante_base_chips(ante, 1);
            let modulus = 10i64.pow((value as f64).log10().floor() as u32 - 1);
            assert_eq!(value % modulus, 0, "ante {} -> {}", ante, value);
        }
    }
}
