//! The game's own randomness, reproduced exactly.
//!
//! Balatro does not draw from one stream. Every decision names a pool -- "Joker1",
//! "shop_pack", "erratic", "cry_e" -- and each pool keeps its own state, so how
//! often the shop rolls cannot shift what a card's edition rolls. That is what
//! makes a seed shareable, and it is also why a simulator that draws from a
//! generic PRNG plays a *different game* on the same seed: same rules, wrong
//! world.
//!
//! Two layers, both exact here:
//!
//! ```text
//!     pseudohash / pseudoseed   float arithmetic over the pool state
//!     TW223                     LuaJIT's math.random, a Tausworthe generator
//!                               (L'Ecuyer 1991, period 2^223)
//! ```
//!
//! `pseudorandom` seeds the second from the first, which means reproducing the
//! game needs LuaJIT's generator bit-for-bit, not merely a good PRNG.

use std::collections::HashMap;

pub const M64: u64 = u64::MAX;

/// i, k, q, s -- L'Ecuyer table 3, first entry: L=64, J=4, k=223, N1=49.
const TW223_PARAMS: [(usize, u32, u32, u32); 4] = [
    (0, 63, 31, 18),
    (1, 58, 19, 28),
    (2, 55, 24, 7),
    (3, 47, 21, 8),
];

/// Python's `%` on floats, which the game's arithmetic relies on.
///
/// Python's modulo returns a result with the sign of the *divisor*, where
/// Rust's `%` (fmod) keeps the sign of the dividend. The hash happens to stay
/// positive, but reproducing the rule rather than the coincidence means a
/// future caller cannot silently diverge.
#[inline]
pub fn py_mod(a: f64, b: f64) -> f64 {
    let r = a % b;
    if r != 0.0 && (r < 0.0) != (b < 0.0) {
        r + b
    } else {
        r
    }
}

/// Pools the game draws on that the Python simulator never did.
///
/// The Python-recorded fixtures (`flow_*`, `fuzz_*`, `fuzz_sweep`, `replay_*`,
/// `stake_deal`) compare the whole pool map after every step, and a pool the
/// Python never rolled appears in it here the first time the rule fires. They
/// leave these out; `tests/facedown_fixture.rs`, recorded off the game's own
/// Lua, is what pins them instead. Every other pool is still compared to the
/// bit.
///
/// * `wheel` -- The Wheel's 1-in-7 face-down roll, one draw per card dealt
///   (blind.lua:608). Moving no other pool, it changes nothing the Python
///   fixtures record but the map itself.
pub const LUA_ONLY_POOLS: &[&str] = &["wheel"];
/// The game's string hash: a reverse fold over the bytes.
///
/// The Python original encodes with `latin-1, replace`, so a character outside
/// latin-1 becomes `?`. Reproducing that keeps the two engines agreeing on
/// non-ASCII seeds rather than panicking or hashing the UTF-8 bytes.
pub fn pseudohash(text: &str) -> f64 {
    let data = latin1_replace(text);
    let mut num = 1.0f64;
    for i in (1..=data.len()).rev() {
        num = py_mod(
            (1.1239285023 / num) * (data[i - 1] as f64) * std::f64::consts::PI
                + std::f64::consts::PI * (i as f64),
            1.0,
        );
    }
    num
}

fn latin1_replace(text: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len());
    for ch in text.chars() {
        let cp = ch as u32;
        if cp <= 0xFF {
            out.push(cp as u8);
        } else {
            // Python's errors="replace" emits a single '?' per *character*.
            out.push(b'?');
        }
    }
    out
}

/// Lua's `string.format("%.13f")`, which the game uses to clamp drift.
///
/// Rust's `{:.13}` and Python's `%.13f` are both correctly rounded with
/// ties-to-even, so this is the same double, not merely the same digits.
pub fn round13(value: f64) -> f64 {
    format!("{:.13}", value).parse::<f64>().unwrap_or(value)
}

/// LuaJIT's math.random. Seeded with a double, as math.randomseed does.
#[derive(Clone, Debug)]
pub struct TW223 {
    gen: [u64; 4],
}

impl TW223 {
    pub fn new(seed: f64) -> Self {
        let mut gen = [0u64; 4];
        let mut r: u64 = 0x11090601; // 64-k[i], four 8-bit constants
        let mut seed = seed;
        for slot in gen.iter_mut() {
            let m = 1u64 << (r & 255);
            r >>= 8;
            seed = seed * 3.14159265358979323846 + 2.7182818284590452354;
            let mut u = seed.to_bits();
            if u < m {
                u += m; // keep the top k[i] bits non-zero
            }
            *slot = u & M64;
        }
        let mut out = TW223 { gen };
        for _ in 0..10 {
            out.step();
        }
        out
    }

    /// One draw, as a double in [1.0, 2.0).
    pub fn step(&mut self) -> f64 {
        let mut r: u64 = 0;
        for &(i, k, q, s) in TW223_PARAMS.iter() {
            let mut z = self.gen[i];
            z = ((((z << q) & M64) ^ z) >> (k - s))
                ^ (((z & ((M64 << (64 - k)) & M64)) << s) & M64);
            r ^= z;
            self.gen[i] = z;
        }
        let bits = (r & 0x000FFFFFFFFFFFFF) | 0x3FF0000000000000;
        f64::from_bits(bits)
    }

    /// `math.random` with optional Lua-style bounds.
    ///
    /// `None` is the bare draw, `Some(low)` is `math.random(low)` (1..low),
    /// and `Some(low), Some(high)` is `math.random(low, high)`, both inclusive.
    pub fn random(&mut self, low: Option<f64>, high: Option<f64>) -> f64 {
        let d = self.step() - 1.0;
        match (low, high) {
            (None, _) => d,
            (Some(low), None) => (d * low).floor() + 1.0,
            (Some(low), Some(high)) => (d * (high - low + 1.0)).floor() + low,
        }
    }
}

/// The pools for one run, keyed by the run's seed string.
///
/// `Clone` is the value copy Python's `copy.deepcopy` makes of it, which is
/// what `GameState::deep_clone` needs for a fork.
#[derive(Clone, Debug)]
pub struct RunRng {
    pub seed: String,
    pub hashed_seed: f64,
    pub pools: HashMap<String, f64>,
    /// Lua has one math.random stream and pseudorandom reseeds it on every
    /// call, so a bare math.random continues from wherever the last named draw
    /// left it.
    pub live: Option<TW223>,
    /// The throwaway generator inside a pessimistic preview stands this up.
    ///
    /// Python's `PessimisticRng` overrides `pseudorandom` and nothing else, so
    /// this flag changes `pseudorandom` only: every named 1-in-N and every
    /// ranged `randint` comes up at its bottom, while `seeded`/`random_element`
    /// and a bare `math.random` draw normally, exactly as the subclass does.
    /// The pools are not advanced and `live` is not touched on those draws.
    pub pessimistic: bool,
}

impl RunRng {
    pub fn new<S: ToString>(seed: S) -> Self {
        let seed = seed.to_string();
        let hashed_seed = pseudohash(&seed);
        RunRng {
            seed,
            hashed_seed,
            pools: HashMap::new(),
            live: None,
            pessimistic: false,
        }
    }

    /// Advance `key`'s pool and return a seed for math.random.
    pub fn pseudoseed(&mut self, key: &str) -> f64 {
        let state = match self.pools.get(key) {
            Some(&state) => state,
            None => pseudohash(&format!("{}{}", key, self.seed)),
        };
        let state = round13(py_mod(2.134453429141 + state * 1.72431234, 1.0)).abs();
        self.pools.insert(key.to_string(), state);
        (state + self.hashed_seed) / 2.0
    }

    /// `math.randomseed(pseudoseed(key))`, keeping the stream for later.
    ///
    /// The returned generator is a *copy*; `live` is the one the draws the
    /// caller makes actually advance (see `pseudorandom`, `random_element_index`
    /// and `shuffle`). Python's `seeded` returns `self.live` itself, so a bare
    /// `math.random` afterwards continues from *after* the named draw. Storing a
    /// clone taken before that draw left `live` one draw behind, which is why
    /// the first shop's Buffoon pack came out as `_2` where the game rolls `_1`
    /// -- `draw_pack` calls `math_random(1, 2)` for it. See
    /// `tests/rng_fixture.rs`, mode `live`; and `tests/flow_fixture.rs`.
    pub fn seeded(&mut self, key: &str) -> TW223 {
        let gen = TW223::new(self.pseudoseed(key));
        self.live = Some(gen.clone());
        gen
    }

    /// `math.random` with no reseed, continuing the live stream.
    pub fn math_random(&mut self, low: Option<f64>, high: Option<f64>) -> f64 {
        self.live
            .as_mut()
            .expect("math.random before anything seeded it")
            .random(low, high)
    }

    pub fn pseudorandom(&mut self, key: &str, low: Option<f64>, high: Option<f64>) -> f64 {
        if self.pessimistic {
            // Python's `PessimisticRng.pseudorandom`: no reseed, no pool
            // advance, no `live` move -- just the bottom of the range.
            return PessimisticRng::pseudorandom(low, high);
        }
        self.seeded(key);
        self.live
            .as_mut()
            .expect("seeded just set the live stream")
            .random(low, high)
    }

    /// The game's pseudorandom_element: draw an index into an ordered sequence.
    ///
    /// Order is the caller's job, exactly as in Lua, where the table is sorted
    /// by sort_id (or by key) first -- an unsorted pool draws reproducibly from
    /// the wrong place.
    pub fn random_element_index(&mut self, len: usize, key: &str) -> usize {
        self.seeded(key);
        let index = self
            .live
            .as_mut()
            .expect("seeded just set the live stream")
            .random(Some(len as f64), None) as usize;
        index - 1
    }

    /// A "1 in N" roll, exactly as the game words it.
    ///
    /// The game writes these as
    ///
    /// ```text
    ///     pseudorandom(key) < G.GAME.probabilities.normal / odds
    /// ```
    ///
    /// which is a float draw compared against a ratio, not an integer draw from
    /// 1..N. The two agree on how often they fire and disagree on *which* draws
    /// fire, so a simulator using the wrong form matches the odds and still
    /// diverges hand by hand from the same seed.
    pub fn chance(&mut self, key: &str, numerator: f64, denominator: f64) -> bool {
        self.pseudorandom(key, None, None) < numerator / denominator
    }

    /// The game's pseudoshuffle, in place.
    ///
    /// Note the bounds: Lua walks #list down to 2 and swaps with math.random(i),
    /// which is 1-based and inclusive. The caller is responsible for sorting by
    /// sort_id first, as CardArea does.
    pub fn shuffle<T>(&mut self, items: &mut [T], key: &str) {
        self.seeded(key);
        let mut i = items.len();
        while i > 1 {
            let j = self
                .live
                .as_mut()
                .expect("seeded just set the live stream")
                .random(Some(i as f64), None) as usize;
            items.swap(i - 1, j - 1);
            i -= 1;
        }
    }

    pub fn state(&self) -> HashMap<String, f64> {
        self.pools.clone()
    }

    // -- bridges for the simulator's call sites -------------------------------
    //
    // These name a pool first, matching the game, so a caller that rolls more
    // often cannot shift another subsystem's draws.

    pub fn choice<T: Clone>(&mut self, key: &str, items: &[T]) -> T {
        let i = self.random_element_index(items.len(), key);
        items[i].clone()
    }

    pub fn randint(&mut self, key: &str, low: f64, high: f64) -> i64 {
        self.pseudorandom(key, Some(low), Some(high)) as i64
    }

    /// Draw `count` distinct indices, one at a time from a shrinking pool.
    ///
    /// This is the shape The Hook uses -- repeated pseudorandom_element against
    /// the same pool name, removing each pick.
    pub fn sample_index(&mut self, key: &str, len: usize, count: usize) -> Vec<usize> {
        let mut pool: Vec<usize> = (0..len).collect();
        let mut out = Vec::new();
        for _ in 0..count.min(pool.len()) {
            let i = self.random_element_index(pool.len(), key);
            out.push(pool.remove(i));
        }
        out
    }
}

/// The throwaway generator for a pessimistic preview: nothing lucky.
///
/// Every "1 in N" misses and every ranged draw comes up at its bottom, so
/// Misprint adds nothing, a Lucky card neither pays nor multiplies, Bloodstone
/// and Space Joker never fire. A policy that asks "is this safe?" of a preview
/// wants this number: previews rolled off one fixed stream are about 10%
/// optimistic across the plays a policy picks, because it picks the best of
/// ~200 previews and the best is where the fixed dice landed well.
///
/// A flag rather than a subclass: it overrides the draw only, so the pool
/// arithmetic is never touched.
pub struct PessimisticRng;

impl PessimisticRng {
    #[inline]
    pub fn pseudorandom(low: Option<f64>, high: Option<f64>) -> f64 {
        match low {
            None => 1.0 - 1e-9,
            Some(low) => match high {
                Some(high) if low > high => high,
                _ => low,
            },
        }
    }
}

/// How the game builds a starting seed: 1-9, A-N, P-Z.
pub fn random_string(length: usize, rng: &mut TW223) -> String {
    let mut out = String::with_capacity(length);
    for _ in 0..length {
        if rng.random(None, None) > 0.7 {
            let c = rng.random(Some('1' as u32 as f64), Some('9' as u32 as f64)) as u32;
            out.push(char::from_u32(c).unwrap());
        } else if rng.random(None, None) > 0.45 {
            let c = rng.random(Some('A' as u32 as f64), Some('N' as u32 as f64)) as u32;
            out.push(char::from_u32(c).unwrap());
        } else {
            let c = rng.random(Some('P' as u32 as f64), Some('Z' as u32 as f64)) as u32;
            out.push(char::from_u32(c).unwrap());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pseudohash_known_values() {
        assert_eq!(
            pseudohash("SEED0000").to_bits(),
            0.9364595550447348f64.to_bits()
        );
        assert_eq!(pseudohash("").to_bits(), 1.0f64.to_bits());
    }

    #[test]
    fn py_mod_keeps_the_divisors_sign() {
        assert_eq!(py_mod(-0.25, 1.0), 0.75);
        assert_eq!(py_mod(1.25, 1.0), 0.25);
    }

    #[test]
    fn pessimistic_draws_at_the_bottom() {
        assert_eq!(PessimisticRng::pseudorandom(None, None), 1.0 - 1e-9);
        assert_eq!(PessimisticRng::pseudorandom(Some(3.0), Some(9.0)), 3.0);
    }
}
