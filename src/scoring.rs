//! The scoring pipeline.
//!
//! Balatro resolves a played hand in a fixed order, and the order matters because
//! XMult is not commutative with +Mult:
//!
//! ```text
//!     1. base chips/mult for the poker hand at its current level
//!     2. each scoring card, left to right: chips, enhancement, edition, seal,
//!        then every joker's "on scored card" hook
//!     3. each card held in hand: enhancement (Steel), then joker "held" hooks
//!     4. every joker's independent effect, left to right, plus its own edition
//!     5. score = floor(chips * mult)
//! ```
//!
//! The one deliberate shape change from the Python is that the run is passed to
//! [`score_hand`] (and to the hooks that need it) as `&mut GameState` rather than
//! living inside `ScoreContext.game`; see `PORTING.md`.

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use crate::cards::{CardRef, Edition, Enhancement, Seal};
use crate::effects::ScoreContext;
use crate::game::GameState;
use crate::hands::{planet_for_hand, HandResult, HandSet, HandType};
use crate::jokers::{Copier, JokerRef, JokerSpec};

const GLASS_SHATTER_CHANCE: (i32, i32) = (1, 4);
const LUCKY_MULT_CHANCE: (i32, i32) = (1, 5);
const LUCKY_MONEY_CHANCE: (i32, i32) = (1, 15);

/// A listed probability, scaled by any Oops! All 6s in play.
///
/// The game writes every one of these as
/// `pseudorandom(key) < G.GAME.probabilities.normal / odds`, and Oops! All 6s
/// doubles that numerator, so a one in four becomes a one in two. The jokers went
/// through a helper that knew this; the card effects here did not, so a glass
/// card with an Oops! in the row shattered half as often as it should -- and a
/// shattered card leaves the deck, which changes every hand dealt afterwards.
fn listed(game: &mut GameState, key: &str, numerator: i32, denominator: i32) -> bool {
    let scale = game.probability_scale();
    game.rng
        .chance(key, numerator as f64 * scale, denominator as f64)
}

/// What each joker in the row actually runs, and whose state it runs on.
///
/// Both halves matter for a copier. The game does not lift an ability out of the
/// copied joker and run it on Blueprint; it calls the copied joker:
///
/// ```text
///     local eval = other_joker:calculate_joker(context)
/// ```
///
/// so a Blueprint on a Ceremonial Dagger adds the *dagger's* accumulated mult,
/// and a Brainstorm on a Hiker reads the Hiker's counter. Returning the spec alone
/// and then calling it with the copier's instance looked right and silently
/// scored zero for every copied joker that keeps a counter -- which is most of the
/// ones worth copying. It contributed nothing at all rather than contributing
/// wrongly, so it left no trace in the log either.
///
/// Recording 8 stopped on it: a Brainstorm copying a Ceremonial Dagger worth
/// eighteen mult, adding none of it.
///
/// The edition stays with the copier -- a polychrome Blueprint is polychrome
/// whatever it copies -- so the caller keeps the row's own joker for that.
pub fn effective_specs(jokers: &[JokerRef]) -> Vec<(&'static JokerSpec, JokerRef)> {
    let mut out: Vec<(&'static JokerSpec, JokerRef)> = Vec::with_capacity(jokers.len());
    for (i, joker) in jokers.iter().enumerate() {
        let mut spec = joker.borrow().spec;
        let mut source = joker.clone();
        let mut seen: HashSet<usize> = HashSet::new();
        seen.insert(i);
        let mut idx = i;
        while let Some(copier) = spec.copier {
            let target = match copier {
                Copier::Right => idx + 1,
                Copier::Leftmost => 0,
            };
            if target >= jokers.len() || seen.contains(&target) || jokers[target].borrow().debuffed
            {
                // Copies nothing: its own spec has no effect hooks. A debuffed
                // joker is nothing to copy -- other_joker:calculate_joker is nil
                // for it (card.lua:2292) -- and the copier does not reach past
                // it, which is why this wants the whole row, debuffs and all.
                spec = joker.borrow().spec;
                source = joker.clone();
                break;
            }
            seen.insert(target);
            idx = target;
            spec = jokers[target].borrow().spec;
            source = jokers[target].clone();
        }
        out.push((spec, source));
    }
    out
}

/// Each joker in the row that answers at all, with the ability it runs and the
/// joker whose state that ability reads.
///
/// `jokers` is the whole row. A debuffed joker answers no calculate_joker context
/// (card.lua:2291-2292) and is left out, and a copier beside one copies nothing
/// (effective_specs). Resolving the copies over the working jokers alone moved a
/// Blueprint on to the joker past a debuffed neighbour, and a Brainstorm on to the
/// second joker when the first was debuffed.
///
/// The pairs are returned in the same order as the row's non-debuffed jokers, so a
/// caller that needs the owner as well (the pipeline does, for `owner is source`)
/// can zip them back together; see `score_hand`.
pub fn calculating_specs(jokers: &[JokerRef]) -> Vec<(&'static JokerSpec, JokerRef)> {
    let effective = effective_specs(jokers);
    jokers
        .iter()
        .zip(effective)
        .filter(|(owner, _)| !owner.borrow().debuffed)
        .map(|(_, pair)| pair)
        .collect()
}

/// The row's non-debuffed jokers, in order -- the owners that pair up with
/// [`calculating_specs`].
fn calculating_owners(jokers: &[JokerRef]) -> Vec<JokerRef> {
    jokers
        .iter()
        .filter(|j| !j.borrow().debuffed)
        .cloned()
        .collect()
}

/// A joker's foil and holo, which land before its own effect.
///
/// The game evaluates a joker's edition twice and in two places. The chips and the
/// mult -- foil and holo -- go in before the joker does anything, and the X-mult of
/// a polychrome goes in after, once the joker-on-joker effects have run too.
/// Applying the whole edition afterwards, which is what this did, gets polychrome
/// right and holo wrong: a holographic Loyalty Card should give its ten mult and
/// *then* be multiplied by four, not the other way round, which is a third of the
/// hand.
fn edition_before(edition: Edition, ctx: &mut ScoreContext, source: &str) {
    match edition {
        Edition::Foil => ctx.add_chips(50.0, &format!("{} foil", source)),
        Edition::Holographic => ctx.add_mult(10.0, &format!("{} holo", source)),
        _ => {}
    }
}

/// And the polychrome X-mult, which lands last.
fn edition_after(edition: Edition, ctx: &mut ScoreContext, source: &str) {
    if edition == Edition::Polychrome {
        ctx.times_mult(1.5, &format!("{} polychrome", source));
    }
}

/// Both halves at once -- right for a playing card, which the game does evaluate
/// in one pass, in eval_card rather than in the joker loop.
fn apply_edition(edition: Edition, ctx: &mut ScoreContext, source: &str) {
    edition_before(edition, ctx, source);
    edition_after(edition, ctx, source);
}

/// One trigger of a single scoring card's own abilities.
///
/// Returns the card's `lucky_trigger` for this trigger: whether either of a Lucky
/// card's rolls hit (card.lua:988-989, 1076-1077). The jokers answer it next, and
/// the game clears it once they have (state_events.lua:700).
fn score_card_once(card: &CardRef, ctx: &mut ScoreContext, game: &mut GameState) -> bool {
    let mut lucky_trigger = false;
    let (base_chips, enhancement, edition, seal) = {
        let c = card.borrow();
        (c.base_chips(), c.enhancement, c.edition, c.seal)
    };
    ctx.add_chips(base_chips as f64, &format!("{}", card.borrow()));

    if enhancement == Enhancement::Bonus {
        ctx.add_chips(30.0, "bonus card");
    } else if enhancement == Enhancement::Mult {
        ctx.add_mult(4.0, "mult card");
    } else if enhancement == Enhancement::Glass {
        ctx.times_mult(2.0, "glass card");
    } else if enhancement == Enhancement::Lucky {
        // Every trigger rolls, hit or miss, so this count is exact where the
        // rolls themselves are not: how many chances the play gives a Lucky Cat.
        // See GameState.preview_outcome.
        ctx.lucky_rolls += 1;
        // The game's pool is called lucky_mult, and a pool is identified by its
        // name -- a different name is a different stream of numbers.
        if listed(game, "lucky_mult", LUCKY_MULT_CHANCE.0, LUCKY_MULT_CHANCE.1) {
            ctx.add_mult(20.0, "lucky card");
            lucky_trigger = true;
        }
        ctx.expect(20.0, LUCKY_MONEY_CHANCE.0, LUCKY_MONEY_CHANCE.1);
        if listed(
            game,
            "lucky_money",
            LUCKY_MONEY_CHANCE.0,
            LUCKY_MONEY_CHANCE.1,
        ) {
            ctx.money_gained += 20;
            ctx.chance_paid += 20;
            lucky_trigger = true;
        }
    }

    apply_edition(edition, ctx, "card");

    if seal == Seal::Gold {
        ctx.money_gained += 3;
    }
    lucky_trigger
}

/// One pass over a card held in hand; whether anything answered for it.
///
/// The card's own effect (a Steel card's x_mult, common_events.lua:630-633) and
/// then each joker's, in row order. Every one of them moves the score or the money
/// -- Steel, Baron, Shoot the Moon, Raised Fist, a Reserved Parking roll that pays
/// -- so a pass that moved neither had no effect, which is what the game asks
/// before it repeats a held card. A Parking roll that misses has still drawn from
/// the stream, and still counts as nothing.
fn held_card_once(
    card: &CardRef,
    ctx: &mut ScoreContext,
    pairs: &[(&'static JokerSpec, JokerRef)],
    game: &mut GameState,
) -> bool {
    let before = (ctx.log.len(), ctx.money_gained);
    if card.borrow().enhancement == Enhancement::Steel {
        ctx.times_mult(1.5, "steel card");
    }
    for (spec, source) in pairs {
        if let Some(held) = spec.held {
            held(source, card, ctx, game);
        }
    }
    (ctx.log.len(), ctx.money_gained) != before
}

/// `context.after`: the joker pass every played hand gets, scored or not.
///
/// Scaling jokers grow *after* the hand they are part of, which the game does
/// under context.after. Running it first cost Ice Cream five chips on every hand
/// including its first, and would have done the same to Square Joker and Runner
/// the moment a hand met their condition.
///
/// evaluate_play asks it outside `if not G.GAME.blind:debuff_hand(...)`
/// (state_events.lua:614, 1068-1075), so a hand the boss refuses is asked too --
/// GameState._play calls this for one, since score_hand is never reached. Ice
/// Cream (card.lua:3571) and Seltzer (card.lua:3601) are the two jokers that
/// answer; every other growth on a played hand is `context.before`
/// (card.lua:3411-3569) and is skipped with the block.
///
/// Only on its own account, for the reason given in score_hand: both branches are
/// `not context.blueprint`.
pub fn after_hand_pass(
    pairs: &[(&'static JokerSpec, JokerRef)],
    ctx: &mut ScoreContext,
    game: &mut GameState,
) {
    let owners = calculating_owners(&game.jokers);
    after_hand_pass_owned(&owners, pairs, ctx, game);
}

/// The same pass, with the owners the caller already resolved.
fn after_hand_pass_owned(
    owners: &[JokerRef],
    pairs: &[(&'static JokerSpec, JokerRef)],
    ctx: &mut ScoreContext,
    game: &mut GameState,
) {
    for (i, (spec, source)) in pairs.iter().enumerate() {
        let own_account = match owners.get(i) {
            Some(owner) => Rc::ptr_eq(owner, source),
            None => true,
        };
        if !own_account {
            continue;
        }
        if let Some(update) = spec.update {
            if !spec.update_before_scoring {
                update(source, ctx, game);
            }
        }
    }
}

/// Run the full pipeline and return the context (caller reads `.score()`).
pub fn score_hand(
    game: &mut GameState,
    result: &HandResult,
    played: &[CardRef],
    held: &[CardRef],
) -> ScoreContext {
    let mut ctx = ScoreContext::new(
        result.hand,
        result.scoring.clone(),
        played.to_vec(),
        held.to_vec(),
        result.contains,
        game.money,
        game.probability_scale(),
    );
    // A debuffed joker scores nothing at all -- a perishable that has run out its
    // rounds sits in the row contributing neither chips nor mult.
    // (owner, spec, source): the joker in the row, the ability it runs, and the
    // joker whose state that ability reads. They differ only for a Blueprint or a
    // Brainstorm, and only the owner's edition applies.
    let pairs = calculating_specs(&game.jokers);
    let owners = calculating_owners(&game.jokers);

    for (i, (spec, source)) in pairs.iter().enumerate() {
        // context.before (state_events.lua:628-638), copies included. To Do List
        // pays here, not with the jokers' main effects: card.lua:3491-3499 is
        // `ease_dollars` plus `G.GAME.dollar_buffer`, and Bootstraps (card.lua:
        // 4046) and Bull (3936) read `dollars + dollar_buffer` in joker_main --
        // so they count the $4 wherever the list sits in the row. Paying it from
        // the main pass, in row order, left a Bootstraps to its left reading the
        // money from before the hand. 2MIUP34I, Zodiac Deck, stake 8: $4 held, a
        // High Card the list named, the game 46 x 31 = 1426 and this 46 x 29 =
        // 1334. It came and went between runs of the same seed because the hand
        // the list names does (see hands.py).
        if let Some(before) = spec.before {
            before(source, &mut ctx, game);
        }
        // Only on its own account. A copier runs the copied joker's scoring
        // hooks, and the game guards the *scaling* branches against that with
        // `not context.blueprint` -- so a Blueprint standing left of an Ice Cream
        // adds its chips and does not make it melt twice as fast.
        //
        // Marcin stopped a live run on it: seed 12346, decision 114, row Ride the
        // Bus / Photograph / Blueprint / Ice Cream / Jolly. One hand took the
        // game's Ice Cream from 100 to 95 and this from 100 to 90, and the two
        // decks of chips drifted apart from there.
        if !Rc::ptr_eq(&owners[i], source) {
            continue;
        }
        if let Some(update) = spec.update {
            if spec.update_before_scoring {
                update(source, &mut ctx, game);
            }
        }
    }

    // The base is read after the before pass, and it is this reading The Flint
    // halves: evaluate_play reads G.GAME.hands[text] again at state_events.lua:
    // 640-641, straight after the jokers' `before` pass, and passes that to
    // Blind:modify_hand (645-646). Space Joker levels the hand in that pass
    // (card.lua:3420-3426, level_up_hand at 634-635), so the new level is what
    // scores and what gets halved. Reading and halving the base first, with Space
    // Joker adding the level's gain on top, let the gain through whole into The
    // Flint: OCMTUFBK, Blue Deck, stake 3, a Two Pair levelled from 9 to 10 scored
    // 214 x 74 in the game and 224 x 74 here.
    let (mut chips, mut mult) = game.hand_levels.values(result.hand);
    let boss = game.boss();
    if let Some(boss) = boss {
        if boss.halve_base {
            // The Flint rounds rather than halving. Blind:modify_hand is
            //
            //     max(floor(mult*0.5 + 0.5), 1), max(floor(chips*0.5 + 0.5), 0)
            //
            // so a Three of a Kind's three mult becomes two, not one and a half.
            // Dividing by two loses a whole point of mult on every odd number,
            // and it is the base mult, so everything the hand multiplies by
            // magnifies it.
            chips = ((chips as f64 * 0.5 + 0.5) as i32).max(0);
            mult = ((mult as f64 * 0.5 + 0.5) as i32).max(1);
        }
    }
    // G.GAME.blind.triggered, the half of it set while the hand scores:
    // modify_hand for The Flint (blind.lua:512), and any debuffed card in the
    // scoring hand, whatever the boss (state_events.lua:655-656) -- which is how a
    // Flush of Clubs into The Club triggers it. GameState._play has cleared it and
    // set the rest; Matador reads it with the jokers below.
    let debuffed_in_scoring = result.scoring.iter().any(|c| c.borrow().debuffed);
    if game.blind.is_some() && (boss.map_or(false, |b| b.halve_base) || debuffed_in_scoring) {
        if let Some(blind) = game.blind.as_mut() {
            blind.triggered = true;
        }
    }
    ctx.add_chips(chips as f64, result.hand.label());
    ctx.add_mult(mult as f64, result.hand.label());

    for card in &result.scoring {
        if card.borrow().debuffed {
            continue;
        }
        let mut triggers = 1 + if card.borrow().seal == Seal::Red {
            1
        } else {
            0
        };
        for (spec, source) in &pairs {
            if let Some(retrigger) = spec.retrigger_scored {
                triggers += retrigger(source, card, &mut ctx, game);
            }
        }
        for _ in 0..triggers {
            let lucky_trigger = score_card_once(card, &mut ctx, game);
            for (i, (spec, source)) in pairs.iter().enumerate() {
                if let Some(scored) = spec.scored {
                    scored(source, card, &mut ctx, game);
                }
                // The growth the same branch guards with `not
                // context.blueprint`: Wee Joker's +8 per scoring 2
                // (card.lua:3083-3085). A copy adds the chips in joker_main and
                // does not grow them; asking `scored` for copies grew a Wee Joker
                // beside a Blueprint to 48 on three 2s where the engine's reached
                // 24.
                if let Some(growth) = spec.scored_growth {
                    if Rc::ptr_eq(&owners[i], source) {
                        growth(source, card, &mut ctx, game);
                    }
                }
                // Lucky Cat, in the same pass and on its own account only:
                // `not context.blueprint` (card.lua:3076). Once per trigger
                // however many of the card's rolls hit, since the flag is cleared
                // after this pass (state_events.lua:700), not after each roll.
                // AWEFRTUZ, Blue Deck, stake 5, decision 70: a Lucky Queen under
                // Hanging Chad hit both rolls on its third trigger, the game's
                // Lucky Cat went to X1.25 and the simulator's, which had nothing
                // that grew it, stayed at X1.
                if lucky_trigger {
                    if let Some(lucky) = spec.lucky_trigger {
                        if Rc::ptr_eq(&owners[i], source) {
                            lucky(source, card, &mut ctx, game);
                        }
                    }
                }
            }
        }
    }

    // A held card is repeated only when its first pass did something. The game
    // asks for repetitions once, after that pass, and gates the red seal on
    // `next(effects[1]) or #effects > 1` (state_events.lua:812-817) while Mime
    // asks the same of context.card_effects (card.lua:2879-2880); played cards
    // have no such gate (669-683). So a face card whose Reserved Parking roll
    // misses is not rolled again under Mime, and the draw stays on the stream for
    // the next one. Retriggering every held card rolled it twice: OH4OWIIZ, Ghost
    // Deck, stake 8, Mime and Reserved Parking, took one draw too many at decision
    // 12 and paid $2 at decision 21 where the game's Jack drew the leftover and
    // missed -- game $1, simulator $3.
    for card in held {
        if card.borrow().debuffed {
            continue;
        }
        if !held_card_once(card, &mut ctx, &pairs, game) {
            continue;
        }
        let mut repeats = if card.borrow().seal == Seal::Red {
            1
        } else {
            0
        };
        for (spec, source) in &pairs {
            if let Some(retrigger) = spec.retrigger_held {
                repeats += retrigger(source, card, &mut ctx, game);
            }
        }
        for _ in 0..repeats {
            held_card_once(card, &mut ctx, &pairs, game);
        }
    }

    // One joker at a time, and the whole row answers about each one before the
    // next (state_events.lua:877-944): its foil and holo, its own joker_main,
    // then every joker asked under context.other_joker, then its polychrome.
    // Baseball Card lives in that third step (card.lua:3396-3408), so its X1.5
    // lands straight after each Uncommon joker and not at its own position. The
    // game walks every joker in the row there, debuffed or not; a debuffed one has
    // no edition and no effect of its own (card.lua:1016-1017, 2291-2292) but is
    // still a joker for the others to answer about. 8KUQ2KZU stopped on it at
    // decision 19: Mime, Popcorn, Baseball Card, Troubadour scored 77 x 54 here
    // against the game's 77 x 39.
    let mut answering: HashMap<u64, (&'static JokerSpec, JokerRef)> = HashMap::new();
    for (i, pair) in pairs.iter().enumerate() {
        answering.insert(owners[i].borrow().uid, (pair.0, pair.1.clone()));
    }
    let about_others: Vec<(&'static JokerSpec, JokerRef)> = pairs
        .iter()
        .filter(|(spec, _)| spec.other_joker.is_some())
        .map(|(spec, source)| (*spec, source.clone()))
        .collect();
    for joker in game.jokers.clone() {
        let own = {
            let uid = joker.borrow().uid;
            answering.get(&uid).cloned()
        };
        if let Some((spec, source)) = own.as_ref() {
            let (edition, name) = {
                let j = joker.borrow();
                (j.edition, j.name())
            };
            edition_before(edition, &mut ctx, name);
            if let Some(independent) = spec.independent {
                independent(source, &mut ctx, game);
            }
        }
        for (spec, source) in &about_others {
            if let Some(other) = spec.other_joker {
                other(source, &joker, &mut ctx, game);
            }
        }
        if own.is_some() {
            let (edition, name) = {
                let j = joker.borrow();
                (j.edition, j.name())
            };
            edition_after(edition, &mut ctx, name);
        }
    }

    // Observatory: a Planet card sitting in a consumable slot gives X1.5 Mult for
    // its own hand type. It is the one voucher whose effect is a scoring one,
    // which is why it had no field on Voucher to hold it and was doing nothing at
    // all.
    if game.vouchers.iter().any(|v| v.key == "v_observatory") {
        let planet = planet_for_hand(result.hand);
        for consumable in &game.consumables {
            let name = consumable.borrow().spec.name;
            if name == planet {
                ctx.times_mult(1.5, "Observatory");
            }
        }
    }

    after_hand_pass_owned(&owners, &pairs, &mut ctx, game);

    // The Plasma Deck's final scoring step: chips and mult are averaged, both
    // floored, so a hand scores the square of half their sum. It happens after
    // everything else has had its say, which is why it lives at the very end
    // rather than anywhere a joker could reach.
    //
    // Python also asks `game.deck_config.get("balance_chips_mult")` first, but no
    // deck's config sets that key -- it is always None -- so the deck name is the
    // whole condition.
    if game.deck == "Plasma Deck" {
        let total = ctx.chips + ctx.mult;
        ctx.chips = (total / 2.0).floor();
        ctx.mult = (total / 2.0).floor();
    }

    ctx
}

/// Glass cards that break after scoring, to be removed from the deck.
///
/// A debuffed Glass card never breaks, and never rolls to (state_events.lua:961):
///
/// ```text
///     if scoring_hand[i].ability.name == 'Glass Card'
///         and not scoring_hand[i].debuff
///         and pseudorandom('glass') < G.GAME.probabilities.normal/... then
/// ```
///
/// The `and` stops before the draw, so the 'glass' stream does not move either.
/// Unreachable until a debuffed card could score at all -- it can in a flush, since
/// a flush reads a debuffed card's printed suit (see `hands.flush_suit`). Seed
/// QWERTYUI on the headless engine, the decision after that fix: The Club and
/// Smeared Joker debuff a Glass Ace of Spades in a scoring flush, and this broke
/// it -- the deck went to 51 here and stayed at 52 in the game.
pub fn shattered_glass(game: &mut GameState, scoring: &[CardRef]) -> Vec<CardRef> {
    let mut out = Vec::new();
    for card in scoring {
        let (is_glass, debuffed) = {
            let c = card.borrow();
            (c.enhancement == Enhancement::Glass, c.debuffed)
        };
        if is_glass
            && !debuffed
            && listed(
                game,
                "glass",
                GLASS_SHATTER_CHANCE.0,
                GLASS_SHATTER_CHANCE.1,
            )
        {
            out.push(card.clone());
        }
    }
    out
}

/// How many times a card held in hand fires its abilities.
///
/// A red seal retriggers it once and Mime retriggers every held ability, and both
/// apply at the end of the round as well as during scoring -- the game runs the
/// same repetition loop over G.hand in its end-of-round pass. So a blue seal under
/// a Mime makes two Planet cards, and a gold card pays six dollars rather than
/// three.
///
/// Through effective_specs, so a Blueprint or a Brainstorm copying a Mime
/// retriggers too. Reading each joker's own spec missed that, and missed it
/// quietly: the copier simply had no retrigger to offer, so the count came out one
/// short and everything downstream was merely smaller. Recording 8 stopped on it at
/// step 314 -- two gold Kings with red seals, held under a Mime with a Brainstorm
/// copying it, paid $18 here against the game's $24.
pub fn held_triggers(game: &mut GameState, card: &CardRef) -> i32 {
    if card.borrow().debuffed {
        return 0;
    }
    let mut triggers = 1 + if card.borrow().seal == Seal::Red {
        1
    } else {
        0
    };
    // Python passes `None` as the context here; the retrigger hooks never read it.
    let mut ctx = ScoreContext::new(
        HandType::HighCard,
        Vec::new(),
        Vec::new(),
        Vec::new(),
        HandSet::new(),
        game.money,
        game.probability_scale(),
    );
    for (spec, source) in calculating_specs(&game.jokers) {
        if let Some(retrigger) = spec.retrigger_held {
            triggers += retrigger(&source, card, &mut ctx, game);
        }
    }
    triggers
}

#[cfg(test)]
mod tests {
    //! The fixture replay cannot reach the copier resolution, the owner-only
    //! guards or the independent pass while the joker registry is empty, so those
    //! are pinned here against hand-built specs.

    use super::*;
    use crate::cards::{make_card, Rank, Suit};
    use crate::jokers::{make_ref, JokerInstance};

    fn add_mult_hook(joker: &JokerRef, ctx: &mut ScoreContext, _game: &mut GameState) {
        ctx.add_mult(joker.borrow().counter, "adder");
    }

    fn retrigger_hook(
        _joker: &JokerRef,
        _card: &CardRef,
        _ctx: &mut ScoreContext,
        _game: &mut GameState,
    ) -> i32 {
        1
    }

    fn update_hook(joker: &JokerRef, _ctx: &mut ScoreContext, _game: &mut GameState) {
        joker.borrow_mut().counter += 1.0;
    }

    static BLUEPRINT: JokerSpec = JokerSpec {
        name: "Blueprint",
        copier: Some(Copier::Right),
        ..JokerSpec::DEFAULT
    };
    static BRAINSTORM: JokerSpec = JokerSpec {
        name: "Brainstorm",
        copier: Some(Copier::Leftmost),
        ..JokerSpec::DEFAULT
    };
    static ADDER: JokerSpec = JokerSpec {
        name: "Adder",
        independent: Some(add_mult_hook),
        ..JokerSpec::DEFAULT
    };
    static MIME: JokerSpec = JokerSpec {
        name: "Mime",
        retrigger_held: Some(retrigger_hook),
        ..JokerSpec::DEFAULT
    };
    static GROWER: JokerSpec = JokerSpec {
        name: "Grower",
        update: Some(update_hook),
        ..JokerSpec::DEFAULT
    };

    fn inst(spec: &'static JokerSpec, counter: f64, debuffed: bool) -> JokerRef {
        let joker = make_ref(JokerInstance::new(spec));
        {
            let mut b = joker.borrow_mut();
            b.counter = counter;
            b.debuffed = debuffed;
        }
        joker
    }

    fn empty_ctx() -> ScoreContext {
        ScoreContext::new(
            HandType::HighCard,
            Vec::new(),
            Vec::new(),
            Vec::new(),
            HandSet::new(),
            0,
            1.0,
        )
    }

    #[test]
    fn a_copier_runs_the_copied_joker_on_its_own_state() {
        let adder = inst(&ADDER, 5.0, false);
        let blueprint = inst(&BLUEPRINT, 0.0, false);

        // Blueprint looks right: with the Adder to its right it runs the Adder's
        // spec against the Adder's own state.
        let row = vec![blueprint.clone(), adder.clone()];
        let effective = effective_specs(&row);
        assert_eq!(effective[0].0.name, "Adder");
        assert!(Rc::ptr_eq(&effective[0].1, &adder));
        // With nothing to its right it copies nothing and keeps its own spec.
        let row = vec![adder.clone(), blueprint.clone()];
        assert_eq!(effective_specs(&row)[1].0.name, "Blueprint");

        // A chain of copiers reaches the joker at the end of it.
        let second = inst(&BLUEPRINT, 0.0, false);
        let row = vec![blueprint.clone(), second, adder.clone()];
        let effective = effective_specs(&row);
        assert_eq!(effective[0].0.name, "Adder");
        assert!(Rc::ptr_eq(&effective[0].1, &adder));
        assert_eq!(effective[1].0.name, "Adder");
        assert!(Rc::ptr_eq(&effective[1].1, &adder));

        // A debuffed neighbour is nothing to copy, and the copier does not reach
        // past it.
        let dead = inst(&ADDER, 0.0, true);
        let row = vec![blueprint.clone(), dead];
        assert_eq!(effective_specs(&row)[0].0.name, "Blueprint");

        // Brainstorm looks at the leftmost joker instead: with the Adder first it
        // copies it.
        let row = vec![adder.clone(), inst(&BRAINSTORM, 0.0, false)];
        let effective = effective_specs(&row);
        assert_eq!(effective[1].0.name, "Adder");
        assert!(Rc::ptr_eq(&effective[1].1, &adder));
        // A Brainstorm in the leftmost slot only ever targets itself.
        let row = vec![inst(&BRAINSTORM, 0.0, false), adder.clone()];
        assert_eq!(effective_specs(&row)[0].0.name, "Brainstorm");

        // A debuffed owner answers no context at all.
        let row = vec![inst(&ADDER, 0.0, true), inst(&ADDER, 0.0, false)];
        assert_eq!(calculating_specs(&row).len(), 1);
    }

    #[test]
    fn held_triggers_count_the_red_seal_and_a_mime() {
        let mut game = GameState::new("SEED0000", "Red Deck", 1);
        let card = make_card(Rank::Ace, Suit::Spades);
        assert_eq!(held_triggers(&mut game, &card), 1);
        card.borrow_mut().seal = Seal::Red;
        assert_eq!(held_triggers(&mut game, &card), 2);
        game.jokers = vec![inst(&MIME, 0.0, false)];
        assert_eq!(held_triggers(&mut game, &card), 3);
        card.borrow_mut().debuffed = true;
        assert_eq!(held_triggers(&mut game, &card), 0);
    }

    #[test]
    fn the_independent_pass_takes_the_copier_edition_and_the_copied_effect() {
        let mut game = GameState::new("SEED0000", "Red Deck", 1);
        let adder = inst(&ADDER, 7.0, false);
        adder.borrow_mut().edition = Edition::Polychrome;
        game.jokers = vec![adder];
        let played = vec![
            make_card(Rank::Ace, Suit::Spades),
            make_card(Rank::Ace, Suit::Hearts),
        ];
        let result = game.evaluate_selection(&played);
        let ctx = score_hand(&mut game, &result, &played, &[]);
        // Pair: 10 chips, 2 mult; two Aces add 22 chips; Adder adds 7 mult; its
        // polychrome X1.5 lands last.
        assert_eq!(ctx.chips, 32.0);
        assert_eq!(ctx.mult, 13.5);
        assert_eq!(ctx.score(), 432);

        // A Blueprint left of the Adder adds the Adder's mult a second time,
        // because the independent pass runs the copied effect too.
        let mut game = GameState::new("SEED0000", "Red Deck", 1);
        game.jokers = vec![inst(&BLUEPRINT, 0.0, false), inst(&ADDER, 5.0, false)];
        let result = game.evaluate_selection(&played);
        let ctx = score_hand(&mut game, &result, &played, &[]);
        assert_eq!(ctx.mult, 12.0);
    }

    #[test]
    fn after_hand_pass_grows_only_the_joker_itself() {
        let mut game = GameState::new("SEED0000", "Red Deck", 1);
        let grower = inst(&GROWER, 0.0, false);
        game.jokers = vec![inst(&BLUEPRINT, 0.0, false), grower.clone()];
        let pairs = calculating_specs(&game.jokers);
        let mut ctx = empty_ctx();
        after_hand_pass(&pairs, &mut ctx, &mut game);
        // The Blueprint's copy must not make the Grower grow twice.
        assert_eq!(grower.borrow().counter, 1.0);
    }
}
