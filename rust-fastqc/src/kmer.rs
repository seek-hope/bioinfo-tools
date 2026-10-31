//! Corrected native implementation of FastQC's positional Kmer Content module.
use std::collections::BTreeMap;

use crate::grouping::base_groups;
use crate::modules::{ModuleResult, Status};
use crate::stats::java_float;

const SAMPLE_EVERY: u64 = 50;
const MAX_PREFIX: usize = 500;

#[derive(Clone, Debug)]
struct Word {
    count: u64,
    positions: Vec<u64>,
}

/// Sampled positional k-mer counts.
#[derive(Clone, Debug)]
pub struct KmerStats {
    k: usize,
    eligible: u64,
    longest: usize,
    opportunities: Vec<u64>,
    words: BTreeMap<Vec<u8>, Word>,
}

impl KmerStats {
    /// Create an accumulator for one fixed k-mer length.
    ///
    /// # Panics
    /// Panics unless k is in 1..=7. The CLI uses the pinned default of seven.
    pub fn new(k: usize) -> Self {
        assert!((1..=7).contains(&k), "k must be between one and seven");
        Self {
            k,
            eligible: 0,
            longest: 0,
            opportunities: Vec::new(),
            words: BTreeMap::new(),
        }
    }

    /// Add one length-filtered read. Every fiftieth eligible read is sampled.
    pub fn add_read(&mut self, sequence: &[u8]) {
        self.eligible += 1;
        if !self.eligible.is_multiple_of(SAMPLE_EVERY) {
            return;
        }
        let sequence = sequence[..sequence.len().min(MAX_PREFIX)].to_ascii_uppercase();
        self.longest = self.longest.max(sequence.len());
        if sequence.len() < self.k {
            return;
        }
        let positions = sequence.len() - self.k + 1;
        self.opportunities
            .resize(self.opportunities.len().max(positions), 0);
        for start in 0..positions {
            let window = &sequence[start..start + self.k];
            if window.contains(&b'N') {
                continue;
            }
            self.opportunities[start] += 1;
            let word = self.words.entry(window.to_vec()).or_insert_with(|| Word {
                count: 0,
                positions: Vec::new(),
            });
            word.count += 1;
            if word.positions.len() <= start {
                word.positions.resize(start + 1, 0);
            }
            word.positions[start] += 1;
        }
    }

    /// Render the independently corrected report.
    pub fn report(&self, nogroup: bool, warn: f64, error: f64) -> ModuleResult {
        if self.k == 0 || self.longest < self.k || self.opportunities.is_empty() {
            return ModuleResult {
                name: "Kmer Content",
                status: Status::Pass,
                lines: Vec::new(),
            };
        }
        let domain = self.longest - self.k + 1;
        let groups = base_groups(domain, nogroup);
        let total: u64 = self.opportunities.iter().take(domain).sum();
        if total == 0 {
            return ModuleResult {
                name: "Kmer Content",
                status: Status::Pass,
                lines: Vec::new(),
            };
        }
        let log_correction = self.k as f64 * 4f64.ln();
        let mut candidates = Vec::new();
        for (sequence, word) in &self.words {
            let proportion = word.count as f64 / total as f64;
            let mut max_ratio = (0u128, 1u128);
            let mut max_position = 0;
            let mut smallest_log_p = 0.0f64;
            let mut enriched = false;
            for (group_index, group) in groups.iter().enumerate() {
                let mut opportunities = 0u64;
                let mut hits = 0u64;
                for position in group.start - 1..group.end {
                    opportunities += self.opportunities.get(position).copied().unwrap_or(0);
                    hits += word.positions.get(position).copied().unwrap_or(0);
                }
                if opportunities == 0 {
                    continue;
                }
                let ratio = (
                    u128::from(hits) * u128::from(total),
                    u128::from(word.count) * u128::from(opportunities),
                );
                if compare_ratios(ratio, max_ratio).is_gt() {
                    max_ratio = ratio;
                    max_position = group_index;
                }
                if compare_ratios(ratio, (5, 1)).is_gt() {
                    let log_p = (binomial_log_upper_tail(opportunities, hits, proportion)
                        + log_correction)
                        .min(0.0);
                    if log_p < 0.01f64.ln() {
                        enriched = true;
                        smallest_log_p = smallest_log_p.min(log_p);
                    }
                }
            }
            if enriched {
                candidates.push(Candidate {
                    sequence: sequence.clone(),
                    count: word.count,
                    log_p_value: smallest_log_p,
                    max_ratio,
                    max_position,
                });
            }
        }
        candidates.sort_by(|left, right| {
            compare_ratios(right.max_ratio, left.max_ratio)
                .then_with(|| left.sequence.cmp(&right.sequence))
        });
        // limits.txt defines verdicts using the most significant k-mer. Compute
        // that before display truncation; row ordering must not change a verdict.
        let significance = candidates
            .iter()
            .map(|candidate| -candidate.log_p_value / std::f64::consts::LN_10)
            .max_by(f64::total_cmp);
        candidates.truncate(20);
        let mut lines = Vec::new();
        if !candidates.is_empty() {
            lines.push("#Sequence\tCount\tPValue\tObs/Exp Max\tMax Obs/Exp Position".into());
            for candidate in &candidates {
                lines.push(format!(
                    "{}\t{}\t{}\t{}\t{}",
                    String::from_utf8_lossy(&candidate.sequence),
                    candidate.count.saturating_mul(SAMPLE_EVERY),
                    java_float(candidate.log_p_value.exp() as f32),
                    java_float(
                        (candidate.max_ratio.0 as f64 / candidate.max_ratio.1 as f64) as f32
                    ),
                    groups[candidate.max_position].label(),
                ));
            }
        }
        let status = if significance.is_some_and(|s| s > error) {
            Status::Fail
        } else if significance.is_some_and(|s| s > warn) {
            Status::Warn
        } else {
            Status::Pass
        };
        ModuleResult {
            name: "Kmer Content",
            status,
            lines,
        }
    }
}

#[derive(Clone, Debug)]
struct Candidate {
    sequence: Vec<u8>,
    count: u64,
    log_p_value: f64,
    max_ratio: (u128, u128),
    max_position: usize,
}

// Euclidean continued-fraction comparison avoids overflowing cross products.
// Counts times opportunities fit u128, but comparing two such ratios by another
// multiplication need not fit. Equal rational values remain genuine ties.
fn compare_ratios(mut left: (u128, u128), mut right: (u128, u128)) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let mut inverted = false;
    loop {
        let order = (left.0 / left.1).cmp(&(right.0 / right.1));
        let remainders = (left.0 % left.1, right.0 % right.1);
        let order = if order != Ordering::Equal {
            order
        } else {
            match remainders {
                (0, 0) => return Ordering::Equal,
                (0, _) => Ordering::Less,
                (_, 0) => Ordering::Greater,
                (l, r) => {
                    left = (left.1, l);
                    right = (right.1, r);
                    inverted = !inverted;
                    continue;
                }
            }
        };
        return if inverted { order.reverse() } else { order };
    }
}

/// Log of the inclusive `Pr[X >= successes]` for a binomial null model.
///
/// The recurrence starts at the observed tail and stops after terms are below
/// machine precision past the mode. This is deliberately inclusive: the
/// observed outcome belongs to its own upper tail.
fn binomial_log_upper_tail(trials: u64, successes: u64, probability: f64) -> f64 {
    if successes == 0 {
        return 0.0;
    }
    if successes > trials || probability <= 0.0 {
        return f64::NEG_INFINITY;
    }
    if probability >= 1.0 {
        return 0.0;
    }
    let n = trials as f64;
    let k = successes as f64;
    if k <= n * probability {
        let complement = binomial_log_upper_tail(trials, trials - successes + 1, 1.0 - probability);
        return (-complement.exp()).ln_1p();
    }
    let log_pmf =
        log_binomial(trials, successes) + k * probability.ln() + (n - k) * (-probability).ln_1p();
    // Relative PMFs avoid underflow before multiple-testing correction.
    let mut term = 1.0;
    let mut sum = term;
    let mut index = successes;
    while index < trials {
        let next = index + 1;
        term *= (trials - index) as f64 / next as f64 * probability / (1.0 - probability);
        sum += term;
        index = next;
        if term <= sum * f64::EPSILON * 4.0 {
            break;
        }
    }
    (log_pmf + sum.ln()).min(0.0)
}

fn log_binomial(trials: u64, successes: u64) -> f64 {
    let k = successes.min(trials - successes);
    let mut result = 0.0;
    for i in 1..=k {
        result += ((trials - k + i) as f64).ln() - (i as f64).ln();
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn samples_every_fiftieth_read_and_extrapolates_count() {
        let mut stats = KmerStats::new(2);
        for _ in 0..49 {
            stats.add_read(b"TT");
        }
        stats.add_read(b"AA");
        assert_eq!(stats.words.len(), 1);
        assert_eq!(stats.words[b"AA".as_slice()].count, 1);
        assert!(stats.report(false, 2.0, 5.0).lines.is_empty());
    }

    #[test]
    fn n_windows_extend_the_domain_without_becoming_opportunities() {
        let mut stats = KmerStats::new(2);
        for _ in 0..50 {
            stats.add_read(b"AANAAA");
        }
        assert_eq!(stats.longest, 6);
        assert_eq!(stats.opportunities.len(), 5);
        assert_eq!(stats.opportunities[2], 0);
    }

    #[test]
    fn inclusive_tail_handles_all_successes() {
        let p = binomial_log_upper_tail(4, 4, 0.5).exp();
        assert!((p - 0.0625).abs() < 1e-12, "{p}");
    }

    #[test]
    fn exact_ratios_do_not_cross_thresholds_or_split_ties() {
        let mut stats = KmerStats::new(7);
        for _ in 0..900 {
            stats.add_read(b"AAAAAAACCCC");
        }
        for _ in 0..100 {
            stats.add_read(b"TTTTTTTCCCC");
        }
        assert!(stats.report(false, 2.0, 5.0).lines.is_empty());
        assert_eq!(
            compare_ratios((7_000, 70), (10_000, 100)),
            std::cmp::Ordering::Equal
        );
        let maximum = u128::MAX;
        assert!(compare_ratios((maximum, maximum - 1), (maximum - 1, maximum - 2)).is_lt());
        for a in 0..20u128 {
            for b in 1..20u128 {
                for c in 0..20u128 {
                    for d in 1..20u128 {
                        assert_eq!(compare_ratios((a, b), (c, d)), (a * d).cmp(&(c * b)));
                    }
                }
            }
        }
    }

    #[test]
    fn direct_small_binomial_sums_match_log_recurrence() {
        for n in [1u64, 2, 4, 10, 40] {
            for p in [0.01f64, 0.25, 0.5, 0.9] {
                for k in 0..=n {
                    let mut coefficient = 1u64;
                    let mut exact = 0.0;
                    for j in 0..=n {
                        if j >= k {
                            exact += coefficient as f64
                                * p.powi(j as i32)
                                * (1.0 - p).powi((n - j) as i32);
                        }
                        if j < n {
                            coefficient = coefficient * (n - j) / (j + 1);
                        }
                    }
                    let got = binomial_log_upper_tail(n, k, p).exp();
                    assert!(
                        (got - exact).abs() <= 1e-12 * exact,
                        "n={n} k={k} p={p}: {got} != {exact}"
                    );
                }
            }
        }
    }

    #[test]
    fn significance_survives_raw_probability_underflow() {
        let mut stats = KmerStats::new(7);
        let sequence = [vec![b'A'; 7], vec![b'C'; 99]].concat();
        for _ in 0..8150 {
            stats.add_read(&sequence);
        }
        assert_eq!(stats.report(false, 320.0, 323.0).status, Status::Warn);
    }

    #[test]
    fn no_enrichment_passes_even_with_negative_limits() {
        let mut stats = KmerStats::new(7);
        for _ in 0..50 {
            stats.add_read(b"AAAAAAAAAA");
        }
        assert_eq!(stats.report(false, -2.0, -1.0).status, Status::Pass);
    }

    #[test]
    fn report_is_repeatable_and_has_no_table_when_empty() {
        let stats = KmerStats::new(7);
        let first = stats.report(false, 2.0, 5.0);
        assert_eq!(first, stats.report(false, 2.0, 5.0));
        assert!(first.lines.is_empty());
    }
}
