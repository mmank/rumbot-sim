//! Balatro as a Rust library.
//!
//! A translation of `jimbot_sim`, the pure-Python simulator in the
//! `jimbot-sim` submodule. Deterministic given a seed, no I/O, and the run is
//! a phase-based state machine:
//!
//! ```text
//!     let mut game = GameState::new("SEED0000", "Red Deck", 1);
//!     while !game.is_over() {
//!         game.step(&game.legal_actions()[0]);
//!     }
//! ```

pub mod blinds;
pub mod boss_data;
pub mod cards;
pub mod consumable_data;
pub mod consumable_specs;
pub mod consumables;
pub mod deck_data;
pub mod effects;
pub mod game;
pub mod game_cards;
pub mod game_clone;
pub mod game_jokers;
pub mod hands;
pub mod joker_data;
pub mod joker_specs;
pub mod jokers;
pub mod pack_data;
pub mod rng;
pub mod scoring;
pub mod shop;
pub mod shop_pool;
pub mod state;
pub mod tag_data;
pub mod vocabulary;
pub mod voucher_data;
