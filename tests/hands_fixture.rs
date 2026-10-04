//! Replay the Python simulator's hand classification over 4,000 hands.
//!
//! Every line is one `evaluate` call: the same cards, the same four flags, and
//! the answer the Python engine gave -- the hand, the positions that score, and
//! every hand the played cards contain. A port that classifies a different top
//! hand, or scores a different card, or misses a containment, fails here rather
//! than three thousand decisions into a run.

use rumbot_sim::cards::{make_card, uid_of, CardRef, Enhancement, Rank, Suit};
use rumbot_sim::hands::{evaluate, EvalFlags, HandSet, HandType};
use std::rc::Rc;

const FIXTURE: &str = include_str!("fixtures/hands.txt");

fn enhancement(name: &str) -> Enhancement {
    match name {
        "none" => Enhancement::None,
        "bonus" => Enhancement::Bonus,
        "mult" => Enhancement::Mult,
        "wild" => Enhancement::Wild,
        "glass" => Enhancement::Glass,
        "steel" => Enhancement::Steel,
        "stone" => Enhancement::Stone,
        "gold" => Enhancement::Gold,
        "lucky" => Enhancement::Lucky,
        other => panic!("unknown enhancement {:?}", other),
    }
}

fn parse_card(text: &str) -> CardRef {
    let f: Vec<&str> = text.split(':').collect();
    let rank = Rank::from_code(f[0]).expect("rank");
    let suit = Suit::from_code(f[1]).expect("suit");
    let card = make_card(rank, suit);
    card.borrow_mut().enhancement = enhancement(f[2]);
    card.borrow_mut().debuffed = f[3] == "1";
    card
}

#[test]
fn the_classification_matches_the_python_simulator() {
    let mut checked = 0usize;
    for (lineno, line) in FIXTURE.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let f: Vec<&str> = line.split('\t').collect();
        assert_eq!(f[0], "eval");
        let bits: Vec<char> = f[1].chars().collect();
        let flags = EvalFlags {
            four_fingers: bits[0] == '1',
            shortcut: bits[1] == '1',
            splash: bits[2] == '1',
            smeared: bits[3] == '1',
        };
        let cards: Vec<CardRef> = f[2].split('|').map(parse_card).collect();
        let want_hand = f[3].parse::<u8>().unwrap();
        let want_places: Vec<usize> = if f[4].is_empty() {
            Vec::new()
        } else {
            f[4].split(',').map(|s| s.parse().unwrap()).collect()
        };
        let want_contains: Vec<u8> = if f[5].is_empty() {
            Vec::new()
        } else {
            f[5].split(',').map(|s| s.parse().unwrap()).collect()
        };

        let got = evaluate(&cards, flags);
        // The scored cards are the caller's own handles, not copies: the jokers
        // mutate them in place afterwards (Hiker's chips, glass shattering), and
        // a copy would take the mutation with it and leave the piles stale.
        for scored in &got.scoring {
            assert!(
                cards.iter().any(|input| Rc::ptr_eq(input, scored)),
                "line {}: scoring holds a copy, not the caller's handle",
                lineno + 1
            );
        }
        assert_eq!(
            got.hand.value(),
            want_hand,
            "line {}: {} on {}",
            lineno + 1,
            got.hand.label(),
            f[2]
        );
        let scored: Vec<u64> = got.scoring.iter().map(uid_of).collect();
        let got_places: Vec<usize> = cards
            .iter()
            .enumerate()
            .filter(|(_, c)| scored.contains(&uid_of(c)))
            .map(|(i, _)| i)
            .collect();
        assert_eq!(
            got_places,
            want_places,
            "line {}: scoring positions differ for {} on {}",
            lineno + 1,
            got.hand.label(),
            f[2]
        );
        let got_contains: Vec<u8> = got.contains.iter().map(|h| h.value()).collect();
        let want_contains = {
            let mut w = want_contains;
            w.sort();
            w
        };
        assert_eq!(
            got_contains,
            want_contains,
            "line {}: containment differs for {}",
            lineno + 1,
            got.hand.label()
        );
        checked += 1;
    }
    assert!(checked >= 3999, "fixture only exercised {} hands", checked);
}

#[test]
fn containment_is_a_set_of_exact_group_sizes() {
    // A five of a kind contains a four, a three and a pair -- but never a full
    // house, which is the cascade the game patches on the end.
    let cards: Vec<CardRef> = (0..5)
        .map(|i| {
            let suits = [Suit::Spades, Suit::Hearts, Suit::Diamonds, Suit::Clubs];
            let c = make_card(Rank::Five, suits[i % 4]);
            c.borrow_mut().uid = i as u64;
            c
        })
        .collect();
    let got = evaluate(&cards, EvalFlags::default());
    assert_eq!(got.hand, HandType::FiveOfAKind);
    assert!(got.contains.contains(HandType::FourOfAKind));
    assert!(got.contains.contains(HandType::ThreeOfAKind));
    assert!(got.contains.contains(HandType::Pair));
    assert!(!got.contains.contains(HandType::FullHouse));
    assert_eq!(got.contains.len(), 5); // High Card, Five, Four, Three, Pair
    let empty = HandSet::new();
    assert!(empty.is_empty());
}
