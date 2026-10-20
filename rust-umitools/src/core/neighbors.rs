//! Exact neighbour enumeration.
//!
//! Every strategy returns the same edge set: unordered pairs whose Hamming
//! distance is at most `threshold`. The cheapest strategy is chosen from the
//! bundle shape; correctness never depends on the strategy.

use crate::core::distance::hamming;
use crate::core::umi::{Umi, SYMBOL_ALPHABET};
use rustc_hash::{FxHashMap, FxHashSet};

/// Above this many distinct UMIs UMI-tools switches from all-pairs comparison to
/// a substring prefilter. We keep the same cutoff so both implementations
/// exercise comparable code paths.
/// Calibrated cutoffs (see docs/BENCHMARK.md "neighbour strategy"): pairwise
/// wins for small bundles, while substitute (k=1) and partition (k>=2) win as
/// the bundle grows. The scalar paths are portable; no ISA-specific code is
/// enabled because enumeration is a small share of total runtime.
const PAIRWISE_LIMIT_K1: usize = 100;
const PAIRWISE_LIMIT_K2: usize = 3000;
const PAIRWISE_LIMIT_DEFAULT: usize = 500;
pub const PAIRWISE_LIMIT: usize = PAIRWISE_LIMIT_K1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Strategy {
    Pairwise,
    Substitute,
    Partition,
}

/// Unordered pairs `(i, j)` with `i < j` and `hamming(i, j) <= threshold`.
pub fn candidate_pairs(umis: &[Umi], threshold: u32) -> Vec<(usize, usize)> {
    pairs_with_strategy(umis, threshold).0
}

pub fn pairs_with_strategy(umis: &[Umi], threshold: u32) -> (Vec<(usize, usize)>, Strategy) {
    let started = std::time::Instant::now();
    let strategy = choose_strategy(umis, threshold);
    let mut pairs = match strategy {
        Strategy::Pairwise => pairwise(umis, threshold),
        Strategy::Substitute => substitute(umis),
        Strategy::Partition => partition(umis, threshold),
    };
    // Edge order never reaches the output (components and group membership are
    // set based); sorting keeps runs bit-for-bit reproducible.
    pairs.sort_unstable();
    if timing_enabled() {
        eprintln!(
            "[timing] neighbors strategy={strategy:?} umis={} threshold={} pairs={} elapsed={:.6}s",
            umis.len(),
            threshold,
            pairs.len(),
            started.elapsed().as_secs_f64()
        );
    }
    (pairs, strategy)
}

fn timing_enabled() -> bool {
    static TIMING: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *TIMING.get_or_init(|| std::env::var_os("RUST_UMITOOLS_TIMING").is_some())
}

fn strategy_override() -> Option<Strategy> {
    static STRATEGY: std::sync::OnceLock<Option<Strategy>> = std::sync::OnceLock::new();
    *STRATEGY.get_or_init(|| {
        match std::env::var("RUST_UMITOOLS_NEIGHBOR_STRATEGY")
            .ok()
            .as_deref()
        {
            Some("pairwise") => Some(Strategy::Pairwise),
            Some("substitute") => Some(Strategy::Substitute),
            Some("partition") => Some(Strategy::Partition),
            _ => None,
        }
    })
}

fn choose_strategy(umis: &[Umi], threshold: u32) -> Strategy {
    if let Some(forced) = strategy_override() {
        return forced;
    }
    let limit = match threshold {
        0 | 1 => PAIRWISE_LIMIT_K1,
        2 => PAIRWISE_LIMIT_K2,
        _ => PAIRWISE_LIMIT_DEFAULT,
    };
    if umis.len() <= limit {
        return Strategy::Pairwise;
    }
    if threshold == 1 && umis.iter().all(Umi::is_packed) {
        return Strategy::Substitute;
    }
    Strategy::Partition
}

fn pairwise(umis: &[Umi], threshold: u32) -> Vec<(usize, usize)> {
    let mut pairs = Vec::new();
    for i in 0..umis.len() {
        for j in (i + 1)..umis.len() {
            if matches!(hamming(&umis[i], &umis[j]), Some(d) if d <= threshold) {
                pairs.push((i, j));
            }
        }
    }
    pairs
}

/// k = 1 with packed UMIs: replace every symbol by every other symbol and look
/// the result up. Cost is O(U * L * 4) lookups instead of O(U^2 * L).
fn substitute(umis: &[Umi]) -> Vec<(usize, usize)> {
    // Packed UMIs allow the whole neighbour enumeration to stay in integer
    // registers: the three-bit code is the hash key and a substitution is just
    // a masked XOR. The byte fallback below keeps arbitrary UMIs correct.
    if umis.iter().all(Umi::is_packed) {
        let codes: Vec<u64> = umis
            .iter()
            .map(|umi| umi.packed_code().expect("checked packed"))
            .collect();
        let mut index: FxHashMap<u64, usize> = FxHashMap::default();
        index.reserve(codes.len());
        for (position, &code) in codes.iter().enumerate() {
            index.insert(code, position);
        }
        let mut pairs = Vec::new();
        for (i, umi) in umis.iter().enumerate() {
            let len = umi.len();
            for position in 0..len {
                let shift = 3 * (len - 1 - position);
                let mask = 0b111u64 << shift;
                let base = codes[i] & !mask;
                let current = (codes[i] >> shift) & 0b111;
                for value in 0..SYMBOL_ALPHABET.len() as u64 {
                    if value == current {
                        continue;
                    }
                    let candidate = base | (value << shift);
                    if let Some(&j) = index.get(&candidate) {
                        if j > i {
                            pairs.push((i, j));
                        }
                    }
                }
            }
        }
        pairs.sort_unstable();
        return pairs;
    }

    let mut index: FxHashMap<&Umi, usize> = FxHashMap::default();
    index.reserve(umis.len());
    for (i, umi) in umis.iter().enumerate() {
        index.insert(umi, i);
    }
    let mut pairs = Vec::new();
    for (i, umi) in umis.iter().enumerate() {
        for position in 0..umi.len() {
            let current = match umi.symbol(position) {
                Some(v) => v,
                None => continue,
            };
            for value in 0..SYMBOL_ALPHABET.len() as u8 {
                if value == current {
                    continue;
                }
                if let Some(candidate) = umi.with_symbol(position, value) {
                    if let Some(&j) = index.get(&candidate) {
                        if j > i {
                            pairs.push((i, j));
                        }
                    }
                }
            }
        }
    }
    pairs.sort_unstable();
    pairs
}

/// General threshold: split each UMI into `threshold + 1` blocks. Two UMIs
/// within the threshold must share at least one block (pigeonhole), so a shared
/// block is a superset of the true neighbourhood; the distance is then verified
/// exactly. No edge can be missed.
fn partition(umis: &[Umi], threshold: u32) -> Vec<(usize, usize)> {
    let first_len = umis[0].len();
    if first_len <= threshold as usize {
        return pairwise(umis, threshold);
    }
    let blocks = threshold as usize + 1;
    let base = first_len / blocks;
    let extra = first_len % blocks;
    let mut sizes = Vec::with_capacity(blocks);
    for i in 0..blocks {
        sizes.push(if i < extra { base + 1 } else { base });
    }

    let mut pairs: Vec<(usize, usize)> = Vec::new();
    let mut seen: FxHashSet<usize> = FxHashSet::default();
    let mut maps: Vec<FxHashMap<u64, Vec<usize>>> = Vec::with_capacity(blocks);

    let mut offset = 0usize;
    for size in &sizes {
        let slice = offset..offset + size;
        offset += size;
        let mut map: FxHashMap<u64, Vec<usize>> = FxHashMap::default();
        for (i, umi) in umis.iter().enumerate() {
            let key = hash_block(umi, slice.clone());
            map.entry(key).or_default().push(i);
        }
        maps.push(map);
    }

    for i in 0..umis.len() {
        seen.clear();
        let mut offset = 0usize;
        for (block, size) in sizes.iter().enumerate() {
            let slice = offset..offset + size;
            offset += size;
            let key = hash_block(&umis[i], slice);
            if let Some(bucket) = maps[block].get(&key) {
                for &j in bucket {
                    if j <= i || !seen.insert(j) {
                        continue;
                    }
                    if matches!(hamming(&umis[i], &umis[j]), Some(d) if d <= threshold) {
                        pairs.push((i, j));
                    }
                }
            }
        }
    }
    pairs.sort_unstable();
    pairs
}

fn hash_block(umi: &Umi, slice: std::ops::Range<usize>) -> u64 {
    // Equal blocks always hash equally, so this cannot hide a neighbour;
    // a collision only adds a candidate that is verified by distance.
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for position in slice {
        let value = umi.symbol(position).map(u64::from).unwrap_or(255);
        hash ^= value.wrapping_add(1);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    fn umis(raws: &[&str]) -> Vec<Umi> {
        raws.iter().map(|r| Umi::from_bytes(r.as_bytes())).collect()
    }

    fn brute(umis: &[Umi], threshold: u32) -> Vec<(usize, usize)> {
        let mut out = Vec::new();
        for i in 0..umis.len() {
            for j in (i + 1)..umis.len() {
                if matches!(hamming(&umis[i], &umis[j]), Some(d) if d <= threshold) {
                    out.push((i, j));
                }
            }
        }
        out.sort_unstable();
        out
    }

    #[test]
    fn all_strategies_agree_on_small_inputs() {
        let set = umis(&[
            "AAAA", "AAAC", "AAAG", "AACC", "ACCC", "CCCC", "CCCA", "GAAA",
        ]);
        for threshold in 0..=2u32 {
            let expected = brute(&set, threshold);
            assert_eq!(
                pairwise(&set, threshold),
                expected,
                "pairwise k={threshold}"
            );
            assert_eq!(
                partition(&set, threshold),
                expected,
                "partition k={threshold}"
            );
        }
        assert_eq!(substitute(&set), brute(&set, 1), "substitute k=1");
    }

    #[test]
    fn substitute_handles_all_positions_and_n() {
        let set = umis(&["AAAAAAAAAA", "CAAAAAAAAA", "AAAAAAAAAN", "TTTTTTTTTT"]);
        assert_eq!(substitute(&set), brute(&set, 1));
        // The first base must not be special-cased away (regression B01).
        assert!(substitute(&set).contains(&(0, 1)));
        assert!(substitute(&set).contains(&(0, 2)));
    }

    #[test]
    fn partition_is_exact_for_larger_sets() {
        let mut raw = Vec::new();
        let alphabet = *b"ACGT";
        for i in 0..140u32 {
            let mut s = [b'A'; 8];
            for (pos, slot) in s.iter_mut().enumerate() {
                let shift = (pos as u32) * 2;
                *slot = alphabet[((i >> shift) & 3) as usize];
            }
            raw.push(String::from_utf8(s.to_vec()).unwrap());
        }
        let set: Vec<Umi> = raw.iter().map(|r| Umi::from_bytes(r.as_bytes())).collect();
        for threshold in 0..=3u32 {
            assert_eq!(
                partition(&set, threshold),
                brute(&set, threshold),
                "k={threshold}"
            );
        }
    }

    #[test]
    fn mixed_lengths_never_pair() {
        let set = umis(&["A", "AA", "AAA"]);
        assert!(candidate_pairs(&set, 2).is_empty());
    }

    #[test]
    fn threshold_larger_than_length_pairs_everything() {
        let set = umis(&["AC", "GT", "TT"]);
        assert_eq!(candidate_pairs(&set, 5).len(), 3);
    }
}
