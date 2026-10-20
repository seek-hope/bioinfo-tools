//! Deterministic sampling for `sample`.
//!
//! # What is guaranteed, and what is not
//!
//! **Guaranteed:** for a fixed `--rand-seed`, the same input produces the same
//! output on every run, on every platform, for every thread count. That is the
//! property the plan's acceptance check names ("`sample` reproducible for a fixed
//! seed"), and it is verified in-repo by a test that runs the sampler twice.
//!
//! **NOT guaranteed:** that the selection is *the same set of records* SeqKit
//! would pick for the same seed. SeqKit is written in Go and uses Go's
//! `math/rand`; reproducing its exact stream would mean reimplementing Go's
//! lagged-Fibonacci generator and seeding routine, which **cannot be verified
//! here** because the SeqKit binary is unavailable. Claiming parity would be
//! inventing evidence, so it is not claimed — see `docs/EVIDENCE-W1-3.md` DR-4.
//! A consequence a reviewer must know: for `sample`, byte-identity with SeqKit is
//! *not* expected, and the differential harness compares this subcommand on the
//! reproducibility property instead of on bytes.
//!
//! # The generator
//!
//! A 64-bit xorshift with a splitmix-style seed expander. Both are small,
//! fully specified, and depend only on integer arithmetic, so the stream is
//! identical on every target. It is *not* cryptographic and is not used for
//! anything security-sensitive.

/// A deterministic 64-bit pseudo-random generator.
#[derive(Clone, Debug)]
pub struct Rng {
    state: u64,
}

impl Rng {
    /// Seed the generator.
    ///
    /// The seed is expanded first so that small seeds (`1`, `11`, `42`) do not
    /// produce correlated early streams, which a bare xorshift would.
    pub fn new(seed: u64) -> Rng {
        // splitmix64 finaliser, used only to spread the seed.
        let mut z = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        Rng {
            // xorshift64 requires a non-zero state.
            state: if z == 0 { 0x9E37_79B9_7F4A_7C15 } else { z },
        }
    }

    /// The next 64-bit value.
    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.state = x;
        x
    }

    /// A uniform float in `[0, 1)`.
    ///
    /// Uses the top 53 bits, which is the standard way to get a full-precision
    /// double without bias.
    pub fn next_f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 * (1.0 / 9_007_199_254_740_992.0)
    }
}

/// How to choose records.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Strategy {
    /// Keep each record with probability `proportion` (`-p`).
    Proportion(f64),
    /// Keep about `count` records (`-n`).
    ///
    /// SeqKit documents that `-n` "may return fewer than requested", because it
    /// samples by probability rather than by exact count. This implementation
    /// reproduces that: it samples each record with probability `count / total`,
    /// so the result is binomially distributed around `count`.
    Number(u64),
}

/// A streaming Bernoulli sampler.
///
/// Streaming rather than reservoir-based so a whole file is never held in memory,
/// matching the plan's bounded-memory rule.
#[derive(Clone, Debug)]
pub struct Sampler {
    rng: Rng,
    probability: f64,
}

impl Sampler {
    /// A sampler with an explicit per-record keep probability.
    pub fn with_probability(seed: u64, probability: f64) -> Sampler {
        Sampler {
            rng: Rng::new(seed),
            probability: probability.clamp(0.0, 1.0),
        }
    }

    /// A sampler for `strategy`, where `total` is the number of records in the
    /// input (needed to turn a count into a probability).
    ///
    /// `total` of zero yields a probability of zero, so an empty input produces an
    /// empty output rather than a division by zero.
    pub fn for_strategy(seed: u64, strategy: Strategy, total: u64) -> Sampler {
        let probability = match strategy {
            Strategy::Proportion(p) => p.clamp(0.0, 1.0),
            Strategy::Number(count) => {
                if total == 0 {
                    0.0
                } else {
                    (count as f64 / total as f64).clamp(0.0, 1.0)
                }
            }
        };
        Sampler::with_probability(seed, probability)
    }

    /// Decide whether to keep the next record.
    ///
    /// Advances the generator exactly once per call, so the decision stream
    /// depends only on the seed and the record index — never on wall-clock time,
    /// thread scheduling, or buffer size.
    pub fn keep_next(&mut self) -> bool {
        self.rng.next_f64() < self.probability
    }

    /// The probability this sampler keeps a record.
    pub fn probability(&self) -> f64 {
        self.probability
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn the_same_seed_gives_the_same_stream() {
        let a: Vec<u64> = {
            let mut rng = Rng::new(11);
            (0..64).map(|_| rng.next_u64()).collect()
        };
        let b: Vec<u64> = {
            let mut rng = Rng::new(11);
            (0..64).map(|_| rng.next_u64()).collect()
        };
        assert_eq!(a, b);
    }

    #[test]
    fn different_seeds_give_different_streams() {
        let mut a = Rng::new(1);
        let mut b = Rng::new(2);
        let stream_a: Vec<u64> = (0..32).map(|_| a.next_u64()).collect();
        let stream_b: Vec<u64> = (0..32).map(|_| b.next_u64()).collect();
        assert_ne!(stream_a, stream_b);
    }

    #[test]
    fn a_zero_seed_still_produces_a_valid_stream() {
        // xorshift64 diverges on a zero state, so the seed must be remapped.
        let mut rng = Rng::new(0);
        let values: Vec<u64> = (0..16).map(|_| rng.next_u64()).collect();
        assert!(
            values.iter().any(|&v| v != 0),
            "a zero seed must not lock the generator"
        );
        assert!(
            values.windows(2).any(|w| w[0] != w[1]),
            "the stream must not be constant"
        );
    }

    #[test]
    fn floats_are_in_the_unit_interval() {
        let mut rng = Rng::new(7);
        for _ in 0..10_000 {
            let value = rng.next_f64();
            assert!((0.0..1.0).contains(&value), "out of range: {value}");
        }
    }

    #[test]
    fn floats_are_reasonably_uniform() {
        let mut rng = Rng::new(99);
        let mut buckets = [0u32; 10];
        let draws = 100_000;
        for _ in 0..draws {
            let index = (rng.next_f64() * 10.0) as usize;
            buckets[index.min(9)] += 1;
        }
        // Each bucket should hold ~10%. A very loose bound avoids flakiness while
        // still catching a broken generator.
        for (index, count) in buckets.iter().enumerate() {
            let share = *count as f64 / draws as f64;
            assert!(
                (0.08..0.12).contains(&share),
                "bucket {index} share {share} outside 8-12%"
            );
        }
    }

    #[test]
    fn a_probability_of_one_keeps_everything() {
        let mut sampler = Sampler::with_probability(5, 1.0);
        assert!((0..1000).all(|_| sampler.keep_next()));
    }

    #[test]
    fn a_probability_of_zero_keeps_nothing() {
        let mut sampler = Sampler::with_probability(5, 0.0);
        assert!((0..1000).all(|_| !sampler.keep_next()));
    }

    #[test]
    fn a_half_probability_keeps_about_half() {
        let mut sampler = Sampler::with_probability(3, 0.5);
        let kept = (0..100_000).filter(|_| sampler.keep_next()).count();
        let share = kept as f64 / 100_000.0;
        assert!((0.48..0.52).contains(&share), "kept {share}");
    }

    #[test]
    fn sampling_is_reproducible_across_instances() {
        let run = |seed: u64| -> Vec<bool> {
            let mut sampler = Sampler::with_probability(seed, 0.3);
            (0..1000).map(|_| sampler.keep_next()).collect()
        };
        assert_eq!(
            run(42),
            run(42),
            "the same seed must reproduce the decisions"
        );
        assert_ne!(run(42), run(43), "a different seed must change them");
    }

    // The plan's `sample` criterion: reproducible for a fixed seed. This pins the
    // actual decisions so a refactor of the generator cannot silently change
    // which records a fixed seed selects.
    #[test]
    fn a_fixed_seed_selects_a_pinned_set() {
        let mut sampler = Sampler::with_probability(11, 0.25);
        let kept: Vec<usize> = (0..40).filter(|_| sampler.keep_next()).collect();
        // Recorded from this implementation; a change here is a behaviour change
        // and must be accompanied by an EVIDENCE-W1-3.md update.
        assert_eq!(kept, vec![6, 9, 10, 17, 19, 21, 27, 30, 31, 32, 34, 39]);
    }

    #[test]
    fn proportion_and_number_agree_on_probability() {
        let by_proportion = Sampler::for_strategy(1, Strategy::Proportion(0.25), 100);
        let by_number = Sampler::for_strategy(1, Strategy::Number(25), 100);
        assert_eq!(by_proportion.probability(), by_number.probability());
        // And they therefore select the same records.
        let a: Vec<bool> = {
            let mut s = by_proportion;
            (0..100).map(|_| s.keep_next()).collect()
        };
        let b: Vec<bool> = {
            let mut s = by_number;
            (0..100).map(|_| s.keep_next()).collect()
        };
        assert_eq!(a, b);
    }

    #[test]
    fn number_sampling_may_return_fewer_than_requested() {
        // Documented SeqKit behaviour: `-n` samples by probability, so the count
        // is binomial, not exact.
        let mut sampler = Sampler::for_strategy(11, Strategy::Number(1000), 3000);
        let kept = (0..3000).filter(|_| sampler.keep_next()).count();
        assert!(
            (850..1150).contains(&kept),
            "expected roughly 1000 of 3000, got {kept}"
        );
        assert_ne!(
            kept, 1000,
            "an exact count would contradict the documented 'may return fewer'"
        );
    }

    #[test]
    fn an_empty_input_yields_an_empty_sample_without_dividing_by_zero() {
        let sampler = Sampler::for_strategy(1, Strategy::Number(10), 0);
        assert_eq!(sampler.probability(), 0.0);
    }

    #[test]
    fn out_of_range_probabilities_are_clamped() {
        assert_eq!(Sampler::with_probability(1, 2.0).probability(), 1.0);
        assert_eq!(Sampler::with_probability(1, -1.0).probability(), 0.0);
        assert_eq!(
            Sampler::for_strategy(1, Strategy::Proportion(5.0), 10).probability(),
            1.0
        );
    }

    #[test]
    fn requesting_more_records_than_exist_keeps_everything() {
        let sampler = Sampler::for_strategy(1, Strategy::Number(100), 10);
        assert_eq!(sampler.probability(), 1.0);
    }

    // A sampler must be independent of how the stream is chunked: consuming the
    // decisions in one loop or in three must give the same set.
    #[test]
    fn decisions_do_not_depend_on_chunking() {
        let all: Vec<bool> = {
            let mut sampler = Sampler::with_probability(8, 0.4);
            (0..300).map(|_| sampler.keep_next()).collect()
        };
        let mut chunked = Vec::new();
        let mut sampler = Sampler::with_probability(8, 0.4);
        for _ in 0..3 {
            for _ in 0..100 {
                chunked.push(sampler.keep_next());
            }
        }
        assert_eq!(all, chunked);
    }

    #[test]
    fn selected_positions_are_unique_and_ordered() {
        let mut sampler = Sampler::with_probability(21, 0.5);
        let kept: Vec<usize> = (0..500).filter(|_| sampler.keep_next()).collect();
        let unique: HashSet<_> = kept.iter().copied().collect();
        assert_eq!(
            unique.len(),
            kept.len(),
            "a record must not be selected twice"
        );
        assert!(
            kept.windows(2).all(|w| w[0] < w[1]),
            "selection must be in input order"
        );
    }
}
