//! CPython-compatible pseudo random numbers.
//!
//! `umi_tools.dedup` seeds the Python `random` module when `--random-seed` is
//! given and uses `random.random()` to break MAPQ ties. Reproducing upstream
//! record-for-record therefore needs CPython's Mersenne Twister seeding and
//! `random()` construction, not merely the same seed value.

use crate::core::rng::Rng;
use std::fmt;

const N: usize = 624;
const M: usize = 397;
const MATRIX_A: u32 = 0x9908_b0df;
const UPPER_MASK: u32 = 0x8000_0000;
const LOWER_MASK: u32 = 0x7fff_ffff;

/// Mersenne Twister with CPython's `init_by_array` seeding.
#[derive(Clone)]
pub struct PyRandom {
    state: [u32; N],
    index: usize,
}

impl PyRandom {
    /// Equivalent to CPython `random.Random(seed)` for an integer seed.
    pub fn from_python_seed(seed: i64) -> PyRandom {
        let magnitude = seed.unsigned_abs();
        let mut key = Vec::new();
        if magnitude == 0 {
            key.push(0u32);
        } else {
            let mut value = magnitude;
            while value > 0 {
                key.push((value & 0xffff_ffff) as u32);
                value >>= 32;
            }
        }
        PyRandom::from_key(&key)
    }

    fn from_key(key: &[u32]) -> PyRandom {
        let mut rng = PyRandom {
            state: [0u32; N],
            index: N,
        };
        rng.init_genrand(19650218);
        let mut i = 1usize;
        let mut j = 0usize;
        let mut remaining = if key.len() > N { key.len() } else { N };
        while remaining > 0 {
            rng.state[i] = (rng.state[i]
                ^ (rng.state[i - 1] ^ (rng.state[i - 1] >> 30)).wrapping_mul(1_664_525))
            .wrapping_add(key[j])
            .wrapping_add(j as u32);
            i += 1;
            j += 1;
            if i >= N {
                rng.state[0] = rng.state[N - 1];
                i = 1;
            }
            if j >= key.len() {
                j = 0;
            }
            remaining -= 1;
        }
        let mut remaining = N - 1;
        while remaining > 0 {
            rng.state[i] = (rng.state[i]
                ^ (rng.state[i - 1] ^ (rng.state[i - 1] >> 30)).wrapping_mul(1_566_083_941))
            .wrapping_sub(i as u32);
            i += 1;
            if i >= N {
                rng.state[0] = rng.state[N - 1];
                i = 1;
            }
            remaining -= 1;
        }
        rng.state[0] = 0x8000_0000;
        rng.index = N;
        rng
    }

    fn init_genrand(&mut self, seed: u32) {
        self.state[0] = seed;
        for i in 1..N {
            self.state[i] = 1_812_433_253u32
                .wrapping_mul(self.state[i - 1] ^ (self.state[i - 1] >> 30))
                .wrapping_add(i as u32);
        }
        self.index = N;
    }

    fn twist(&mut self) {
        for i in 0..N {
            let x = (self.state[i] & UPPER_MASK) | (self.state[(i + 1) % N] & LOWER_MASK);
            let mut shifted = x >> 1;
            if x & 1 != 0 {
                shifted ^= MATRIX_A;
            }
            self.state[i] = self.state[(i + M) % N] ^ shifted;
        }
        self.index = 0;
    }

    /// CPython `genrand_uint32`.
    pub fn next_u32(&mut self) -> u32 {
        if self.index >= N {
            self.twist();
        }
        let mut y = self.state[self.index];
        self.index += 1;
        y ^= y >> 11;
        y ^= (y << 7) & 0x9d2c_5680;
        y ^= (y << 15) & 0xefc6_0000;
        y ^= y >> 18;
        y
    }

    /// CPython `random()`: two draws combined into a 53-bit mantissa.
    pub fn next_f64(&mut self) -> f64 {
        let a = f64::from(self.next_u32() >> 5);
        let b = f64::from(self.next_u32() >> 6);
        (a * 67_108_864.0 + b) * (1.0 / 9_007_199_254_740_992.0)
    }
}

impl Rng for PyRandom {
    fn next_f64(&mut self) -> f64 {
        PyRandom::next_f64(self)
    }
    fn label(&self) -> &'static str {
        "upstream"
    }
}

/// SplitMix64-based generator: reproducible across runs and thread counts
/// without depending on Python internals.
#[derive(Clone)]
pub struct StableRandom(u64);

impl fmt::Debug for StableRandom {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("StableRandom")
    }
}
impl fmt::Debug for PyRandom {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("PyRandom")
    }
}

impl StableRandom {
    pub fn new(seed: u64) -> StableRandom {
        StableRandom(seed)
    }
}

impl Rng for StableRandom {
    fn next_f64(&mut self) -> f64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        (z >> 11) as f64 * (1.0 / 9_007_199_254_740_992.0)
    }
    fn label(&self) -> &'static str {
        "stable"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Values produced by CPython 3.12: `random.Random(seed).random()`.
    const PYTHON_REFERENCE: &[(i64, [f64; 3])] = &[
        (
            1,
            [0.13436424411240122, 0.8474337369372327, 0.763774618976614],
        ),
        (
            0,
            [0.8444218515250481, 0.7579544029403025, 0.420571580830845],
        ),
        (
            42,
            [
                0.6394267984578837,
                0.025010755222666936,
                0.27502931836911926,
            ],
        ),
        (
            12345,
            [
                0.41661987254534116,
                0.010169169457068361,
                0.8252065092537432,
            ],
        ),
    ];

    #[test]
    fn matches_cpython_random_sequences() {
        for (seed, expected) in PYTHON_REFERENCE {
            let mut rng = PyRandom::from_python_seed(*seed);
            for (call, want) in expected.iter().enumerate() {
                let got = rng.next_f64();
                assert!(
                    (got - want).abs() < f64::EPSILON,
                    "seed {seed} call {call}: got {got}, want {want}"
                );
            }
        }
    }

    #[test]
    fn negative_seeds_use_the_absolute_value() {
        let mut a = PyRandom::from_python_seed(-1);
        let mut b = PyRandom::from_python_seed(1);
        assert_eq!(a.next_u32(), b.next_u32());
    }

    #[test]
    fn stable_rng_is_reproducible_and_in_range() {
        let mut a = StableRandom::new(0x5EED);
        let mut b = StableRandom::new(0x5EED);
        for _ in 0..64 {
            let value = a.next_f64();
            assert!((0.0..1.0).contains(&value));
            assert_eq!(value, b.next_f64());
        }
    }
}
