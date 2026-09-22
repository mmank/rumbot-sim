//! Benchmark the Rust simulator on the workload an RL rollout actually runs.
//!
//! The three measurements mirror `tools/bench_python.py` exactly -- the same
//! seeds, the same policy, the same boundaries -- so the two languages do the
//! same work rather than merely comparable work.
//!
//!     rollout   a whole run driven by a fixed policy, one step at a time
//!     preview   `preview_score` over every legal play from one position, the
//!               per-step cost that dominates (~400 candidates a step)
//!     legal     `legal_actions()` alone
//!
//! Run it release, or the numbers mean nothing:
//!
//!     cargo run --release --bin sim_bench -- 1200

use std::time::Instant;

use jimbot_sim::game::{Action, ActionType, GameState, Phase};

const STEPS_PER_RUN: usize = 400;
/// One-chip blinds with endless play drive the ante to where Python's
/// `ante_base_chips` overflows to infinity and raises, while the Rust port
/// saturates its `i64` cast. Both benches stop at the same ante so they do the
/// same work; the overflow itself is the separate, documented past-ante-15
/// observation divergence.
const ANTE_CAP: i32 = 12;
const DECK: &str = "Red Deck";
const STAKE: i32 = 1;

fn seeds() -> Vec<String> {
    (1..=8).map(|i| format!("BENCH{:03}", i)).collect()
}

/// The policy `tests/flow.rs` mirrors and `bench_python.py` reimplements.
fn choose(game: &mut GameState) -> Action {
    let acts = game.legal_actions();
    let plays: Vec<Action> = acts
        .iter()
        .filter(|a| a.r#type == ActionType::Play)
        .cloned()
        .collect();
    if !plays.is_empty() {
        let mut best = plays[0].clone();
        let mut best_score = game.preview_score(&best.cards, "roll");
        for action in &plays[1..] {
            let score = game.preview_score(&action.cards, "roll");
            if score > best_score {
                best_score = score;
                best = action.clone();
            }
        }
        return best;
    }
    if game.phase == Phase::BlindSelect {
        if let Some(skip) = acts.iter().find(|a| a.r#type == ActionType::SkipBlind) {
            return skip.clone();
        }
    }
    acts[0].clone()
}

/// One chip a blind, so the run keeps moving and the shops, the packs and the
/// later antes are all reached.
fn force_one_chip(game: &mut GameState) {
    if let Some(blind) = game.blind.as_mut() {
        blind.target = 1;
    }
}

fn bench_rollout(limit: usize) -> (usize, usize, f64) {
    let mut steps = 0usize;
    let mut runs = 0usize;
    let t0 = Instant::now();
    for seed in seeds() {
        let mut game = GameState::new(seed, DECK, STAKE);
        game.endless = true;
        for _ in 0..STEPS_PER_RUN {
            if game.is_over() || game.ante > ANTE_CAP {
                break;
            }
            force_one_chip(&mut game);
            let action = choose(&mut game);
            game.step(&action);
            steps += 1;
            if steps >= limit {
                break;
            }
        }
        runs += 1;
        if steps >= limit {
            break;
        }
    }
    (steps, runs, t0.elapsed().as_secs_f64())
}

fn to_playing() -> GameState {
    let mut game = GameState::new(&seeds()[0], DECK, STAKE);
    game.endless = true;
    while game.phase != Phase::Playing && !game.is_over() {
        force_one_chip(&mut game);
        let action = choose(&mut game);
        game.step(&action);
    }
    game
}

fn bench_preview(limit: usize) -> (usize, f64) {
    let mut game = to_playing();
    let mut calls = 0usize;
    let t0 = Instant::now();
    while calls < limit && !game.is_over() && game.ante <= ANTE_CAP {
        let acts = game.legal_actions();
        for action in &acts {
            if action.r#type == ActionType::Play {
                game.preview_score(&action.cards, "roll");
                calls += 1;
            }
        }
        force_one_chip(&mut game);
        let action = choose(&mut game);
        game.step(&action);
    }
    (calls, t0.elapsed().as_secs_f64())
}

/// `legal_actions()` alone, from a mid-hand position. Deliberately does not
/// step: the question is what enumerating every legal play costs, and stepping
/// ends the run.
fn bench_legal(limit: usize) -> (usize, f64) {
    let game = to_playing();
    let t0 = Instant::now();
    for _ in 0..limit {
        game.legal_actions();
    }
    (limit, t0.elapsed().as_secs_f64())
}

/// The same 40-step trace `bench_python.py`'s sibling prints, for diffing the
/// two engines' progression step by step.
fn trace(steps: usize) {
    let mut game = GameState::new(&seeds()[0], DECK, STAKE);
    game.endless = true;
    for i in 0..steps {
        if game.is_over() {
            println!("{:3} over", i);
            break;
        }
        force_one_chip(&mut game);
        let action = choose(&mut game);
        println!(
            "{:3} {:<12} ante={} {}",
            i,
            game.phase.as_str(),
            game.ante,
            action
        );
        game.step(&action);
    }
}

fn main() {
    if std::env::args().nth(1).as_deref() == Some("trace") {
        trace(40);
        return;
    }
    let limit: usize = std::env::args()
        .nth(1)
        .and_then(|a| a.parse().ok())
        .unwrap_or(1200);

    // The Python script warms PyPy up first; a first-touch measurement here
    // would flatter Rust for the wrong reason.
    bench_rollout(40);
    bench_preview(200);

    let (steps, runs, dt) = bench_rollout(limit);
    println!(
        "rollout  {:7} steps / {} runs in {:6.2}s  = {:9.1} steps/s",
        steps,
        runs,
        dt,
        steps as f64 / dt
    );
    let (calls, dt) = bench_preview(6000);
    println!(
        "preview  {:7} calls in {:6.2}s  = {:9.1} previews/s",
        calls,
        dt,
        calls as f64 / dt
    );
    let (calls, dt) = bench_legal(4000);
    println!(
        "legal    {:7} calls in {:6.2}s  = {:9.1} calls/s",
        calls,
        dt,
        calls as f64 / dt
    );
}
