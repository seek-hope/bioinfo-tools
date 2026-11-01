//! Semiglobal adapter alignment, a faithful port of cutadapt 4.6's `Aligner`
//! (see `src/cutadapt/_align.pyx` upstream).
//!
//! The algorithm fills a dynamic-programming matrix with unit costs (edit
//! distance) while simultaneously tracking an alignment score
//! (match = +1, mismatch = -1, indel = -2). Among all alignments whose error
//! rate does not exceed the configured maximum, the one with the highest score
//! is returned; ties are resolved towards earlier (leftmost) occurrences.
//!
//! Only a single column of the DP matrix is kept in memory, so memory use is
//! O(len(adapter)) per thread.

/// A prefix of the reference (adapter) may be skipped at no cost.
pub const REFERENCE_START: u32 = 1;
/// A prefix of the query (read) may be skipped at no cost.
pub const QUERY_START: u32 = 2;
/// A suffix of the reference (adapter) may be skipped at no cost.
pub const REFERENCE_END: u32 = 4;
/// A suffix of the query (read) may be skipped at no cost.
pub const QUERY_STOP: u32 = 8;

/// Flags used by cutadapt for a regular 3' adapter (`Where.BACK`).
pub const BACK: u32 = QUERY_START | QUERY_STOP | REFERENCE_END;

const MATCH_SCORE: i32 = 1;
const MISMATCH_SCORE: i32 = -1;
const INSERTION_SCORE: i32 = -2;
const DELETION_SCORE: i32 = -2;

#[derive(Clone, Copy, Debug, Default)]
struct Entry {
    cost: i32,
    score: i32,
    /// Where the alignment originated: negative for positions within the
    /// reference, positive for positions within the query.
    origin: i32,
}

/// Result of a successful semiglobal alignment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Alignment {
    pub ref_start: usize,
    pub ref_stop: usize,
    pub query_start: usize,
    pub query_stop: usize,
    pub score: i32,
    pub errors: i32,
}

#[derive(Clone, Copy, Debug)]
struct Best {
    origin: i32,
    cost: i32,
    score: i32,
    ref_stop: usize,
    query_stop: usize,
}

/// A rejection-only edit-distance screen. All scoring and tie decisions stay
/// in the scalar aligner. Bit i encodes D(i+1,j)-D(i,j), with D(0,j)=0
/// (a free read prefix). Full-adapter endpoints and final-column prefixes are
/// the only possible BACK endpoints, so absence of both proves no match.
#[derive(Clone, Debug)]
struct BackFilter {
    equality: [u64; 256],
    allowed: Vec<i32>,
    min_overlap: usize,
}

impl BackFilter {
    fn new(reference: &[u8], rate: f64, min_overlap: usize) -> Option<Self> {
        if !(1..=64).contains(&reference.len())
            || min_overlap == 0
            || !reference
                .iter()
                .all(|b| matches!(b, b'A' | b'C' | b'G' | b'T'))
        {
            return None;
        }
        let mut equality = [0; 256];
        for (i, &base) in reference.iter().enumerate() {
            equality[base as usize] |= 1 << i;
            equality[base.to_ascii_lowercase() as usize] |= 1 << i;
        }
        Some(Self {
            equality,
            // Costs are nonnegative integers. Flooring the same binary64
            // product is exactly equivalent to the original <= predicate.
            allowed: (1..=reference.len())
                .map(|i| (i as f64 * rate) as i32)
                .collect(),
            min_overlap,
        })
    }

    fn may_match(&self, query: &[u8]) -> bool {
        let m = self.allowed.len();
        if self.min_overlap > m {
            return false;
        }
        let high = 1u64 << (m - 1);
        let mut positive = !0u64;
        let mut negative = 0u64;
        let mut distance = m as i32;
        for &base in query {
            let equal = self.equality[base as usize];
            let vertical = equal | negative;
            let horizontal = ((equal & positive).wrapping_add(positive) ^ positive) | equal;
            let plus = negative | !(horizontal | positive);
            let minus = positive & horizontal;
            distance += i32::from(plus & high != 0) - i32::from(minus & high != 0);
            if distance <= self.allowed[m - 1] {
                return true;
            }
            // No low-bit injection: the free-prefix boundary remains zero.
            positive = (minus << 1) | !(vertical | (plus << 1));
            negative = (plus << 1) & vertical;
        }
        let mut prefix_distance = 0;
        for (i, &allowed) in self.allowed.iter().enumerate() {
            prefix_distance += ((positive >> i) & 1) as i32 - ((negative >> i) & 1) as i32;
            if i + 1 >= self.min_overlap && prefix_distance <= allowed {
                return true;
            }
        }
        false
    }
}

/// Semiglobal aligner with cutadapt-compatible scoring and tie-breaking.
#[derive(Clone, Debug)]
pub struct Aligner {
    reference: Vec<u8>,
    m: usize,
    max_error_rate: f64,
    start_in_reference: bool,
    start_in_query: bool,
    stop_in_reference: bool,
    stop_in_query: bool,
    insertion_cost: i32,
    /// True when IUPAC wildcards in the READ also match (`--match-read-wildcards`).
    match_read_wildcards: bool,
    /// True when wildcards in the ADAPTER are literal (`-N`).
    adapter_wildcards_literal: bool,
    deletion_cost: i32,
    min_overlap: usize,
    column: Vec<Entry>,
    back_filter: Option<Box<BackFilter>>,
}

/// Effective maximum error rate used by cutadapt (`--error-rate` default).
pub const DEFAULT_MAX_ERROR_RATE: f64 = 0.1;
/// Effective minimum overlap used by cutadapt (`--min-overlap` default).
pub const DEFAULT_MIN_OVERLAP: usize = 3;

impl Aligner {
    /// Create an aligner for `reference` (the adapter sequence, ASCII, already
    /// validated to contain only A/C/G/T).
    ///
    /// `indel_cost` is 1 in cutadapt's default (indel-allowing) mode. A large
    /// value makes insertions/deletions prohibitively expensive, which is how
    /// cutadapt implements `--no-indels`; this crate only implements that mode.
    pub fn new(
        reference: &[u8],
        max_error_rate: f64,
        flags: u32,
        indel_cost: i32,
        min_overlap: usize,
    ) -> Self {
        assert!(indel_cost >= 1);
        assert!((0.0..=1.0).contains(&max_error_rate));
        let m = reference.len();
        let reference = reference.to_ascii_uppercase();
        let back_filter = if flags == BACK && indel_cost == 1 {
            BackFilter::new(&reference, max_error_rate, min_overlap).map(Box::new)
        } else {
            None
        };
        Self {
            reference,
            m,
            max_error_rate,
            start_in_reference: flags & REFERENCE_START != 0,
            start_in_query: flags & QUERY_START != 0,
            stop_in_reference: flags & REFERENCE_END != 0,
            stop_in_query: flags & QUERY_STOP != 0,
            insertion_cost: indel_cost,
            deletion_cost: indel_cost,
            min_overlap,
            match_read_wildcards: false,
            adapter_wildcards_literal: false,
            column: vec![Entry::default(); m + 1],
            back_filter,
        }
    }

    /// Enable or disable IUPAC wildcards in the read (`--match-read-wildcards`).
    pub fn with_read_wildcards(mut self, enabled: bool) -> Self {
        self.match_read_wildcards = enabled;
        self
    }

    /// Treat wildcards in the adapter literally (`-N`).
    pub fn with_literal_adapter(mut self, literal: bool) -> Self {
        self.adapter_wildcards_literal = literal;
        self
    }

    pub fn reference(&self) -> &[u8] {
        &self.reference
    }

    pub fn max_error_rate(&self) -> f64 {
        self.max_error_rate
    }

    /// Maximum number of errors allowed for an alignment of the full adapter.
    pub fn max_errors_for_full_length(&self) -> i32 {
        (self.max_error_rate * self.m as f64) as i32
    }

    /// Find the query within the reference. Returns `None` if no alignment
    /// within the allowed error rate exists.
    pub fn locate(&mut self, query: &[u8]) -> Option<Alignment> {
        if !self.match_read_wildcards
            && self
                .back_filter
                .as_ref()
                .is_some_and(|filter| !filter.may_match(query))
        {
            return None;
        }
        let m = self.m;
        let n = query.len();
        // cutadapt compares uppercased query characters against the reference.
        let max_error_rate = self.max_error_rate;
        let stop_in_query = self.stop_in_query;

        // Maximum number of errors allowed for an alignment of the full adapter.
        let k = (max_error_rate * m as f64) as i32;

        // Determine the largest and smallest column we need to compute.
        let max_n = if self.start_in_query {
            n
        } else {
            n.min((m as i64 + k as i64).max(0) as usize)
        };
        let min_n = if self.stop_in_query {
            0
        } else {
            (n as i64 - m as i64 - k as i64).max(0) as usize
        };

        let column = &mut self.column;

        // Fill column `min_n`.
        if !self.start_in_reference && !self.start_in_query {
            for (i, entry) in column.iter_mut().enumerate() {
                entry.score = 0;
                entry.cost = i.max(min_n) as i32 * self.insertion_cost;
                entry.origin = 0;
            }
        } else if self.start_in_reference && !self.start_in_query {
            for (i, entry) in column.iter_mut().enumerate() {
                entry.score = 0;
                entry.cost = min_n as i32 * self.insertion_cost;
                entry.origin = (min_n as i32 - i as i32).min(0);
            }
        } else if !self.start_in_reference && self.start_in_query {
            for (i, entry) in column.iter_mut().enumerate() {
                entry.score = 0;
                entry.cost = i as i32 * self.insertion_cost;
                entry.origin = (min_n as i32 - i as i32).max(0);
            }
        } else {
            for (i, entry) in column.iter_mut().enumerate() {
                entry.score = 0;
                entry.cost = i.min(min_n) as i32 * self.insertion_cost;
                entry.origin = min_n as i32 - i as i32;
            }
        }

        let mut best = Best {
            ref_stop: m,
            query_stop: n,
            cost: (m + n + 1) as i32,
            origin: 0,
            score: 0,
        };

        // Ukkonen's trick: index of the last cell that is at most k errors away.
        let mut last: i64 = if self.start_in_reference {
            m as i64
        } else {
            (m as i64).min(k as i64 + 1)
        };

        // May be -1 (no cell within `k` errors in this column), exactly like
        // cutadapt's `last_filled_i`.
        let mut last_filled_i: i64 = 0;
        let mut diag_entry;

        for j in (min_n + 1)..=max_n {
            diag_entry = column[0];
            if self.start_in_query {
                column[0].origin = j as i32;
            } else {
                column[0].cost = j as i32 * self.insertion_cost;
            }

            let mut i: usize = 1;
            while last >= 0 && (i as i64) <= last {
                // An IUPAC wildcard in the ADAPTER matches any read base
                // (cutadapt enables adapter wildcards by default; -N disables
                // them and is handled by rejecting wildcards at parse time).
                let reference_base = self.reference[i - 1];
                let query_base = query[j - 1].to_ascii_uppercase();
                let characters_equal = reference_base == query_base
                    || (self.match_read_wildcards
                        && matches!(
                            query_base,
                            b'N' | b'R'
                                | b'Y'
                                | b'S'
                                | b'W'
                                | b'K'
                                | b'M'
                                | b'B'
                                | b'D'
                                | b'H'
                                | b'V'
                        ))
                    || (!self.adapter_wildcards_literal
                        && matches!(
                            reference_base,
                            b'N' | b'R'
                                | b'Y'
                                | b'S'
                                | b'W'
                                | b'K'
                                | b'M'
                                | b'B'
                                | b'D'
                                | b'H'
                                | b'V'
                        ));
                let (cost, origin, score);
                if characters_equal {
                    cost = diag_entry.cost;
                    origin = diag_entry.origin;
                    score = diag_entry.score + MATCH_SCORE;
                } else {
                    let cost_diag = diag_entry.cost + 1;
                    let cost_deletion = column[i].cost + self.deletion_cost;
                    let cost_insertion = column[i - 1].cost + self.insertion_cost;
                    if cost_diag.min(cost_deletion).min(cost_insertion) == cost_diag
                        && cost_diag <= cost_deletion
                        && cost_diag <= cost_insertion
                    {
                        // MISMATCH
                        cost = cost_diag;
                        origin = diag_entry.origin;
                        score = diag_entry.score + MISMATCH_SCORE;
                    } else if cost_insertion <= cost_deletion {
                        // INSERTION
                        cost = cost_insertion;
                        origin = column[i - 1].origin;
                        score = column[i - 1].score + INSERTION_SCORE;
                    } else {
                        // DELETION
                        cost = cost_deletion;
                        origin = column[i].origin;
                        score = column[i].score + DELETION_SCORE;
                    }
                }
                diag_entry = column[i];
                column[i].cost = cost;
                column[i].origin = origin;
                column[i].score = score;
                i += 1;
            }
            last_filled_i = last;

            while last >= 0 && column[last as usize].cost > k {
                last -= 1;
            }
            if last < m as i64 {
                last += 1;
            } else if stop_in_query {
                // Found a match. Find the best match in the last row.
                let cost = column[m].cost;
                let score = column[m].score;
                let origin = column[m].origin;
                let length = (m as i32 + origin.min(0)).max(0) as usize;
                let cur_effective_length = length;
                let is_acceptable = length >= self.min_overlap
                    && (cost as f64) <= (cur_effective_length as f64) * max_error_rate;
                let best_length = (m as i32 + best.origin.min(0)).max(0) as usize;
                if is_acceptable
                    && (best.cost == (m + n + 1) as i32
                        || (origin <= best.origin + (m / 2) as i32 && score > best.score)
                        || (length > best_length && score > best.score))
                {
                    best.score = score;
                    best.cost = cost;
                    best.origin = origin;
                    best.ref_stop = m;
                    best.query_stop = j;
                    if cost == 0 && origin >= 0 {
                        // Exact match; stop early.
                        break;
                    }
                }
            }
        }

        if max_n == n {
            let first_i = if self.stop_in_reference { 0 } else { m };
            let mut cursor = last_filled_i;
            loop {
                if cursor < first_i as i64 {
                    break;
                }
                let i = cursor as usize;
                let length = (i as i32 + column[i].origin.min(0)).max(0) as usize;
                let cost = column[i].cost;
                let score = column[i].score;
                let cur_effective_length = length;
                let is_acceptable = length >= self.min_overlap
                    && (cost as f64) <= (cur_effective_length as f64) * max_error_rate;
                let best_length = (best.ref_stop as i32 + best.origin.min(0)).max(0) as usize;
                let origin = column[i].origin;
                if is_acceptable
                    && (best.cost == (m + n + 1) as i32
                        || (origin <= best.origin + (m / 2) as i32 && score > best.score)
                        || (length > best_length && score > best.score))
                {
                    best.score = score;
                    best.cost = cost;
                    best.origin = origin;
                    best.ref_stop = i;
                    best.query_stop = n;
                }
                if cursor == 0 {
                    break;
                }
                cursor -= 1;
            }
        }

        if best.cost == (m + n + 1) as i32 {
            return None;
        }

        let (ref_start, query_start) = if best.origin >= 0 {
            (0, best.origin as usize)
        } else {
            ((-best.origin) as usize, 0)
        };

        Some(Alignment {
            ref_start,
            ref_stop: best.ref_stop,
            query_start,
            query_stop: best.query_stop,
            score: best.score,
            errors: best.cost,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejection_filter_preserves_exhaustive_short_alignments() {
        fn word(length: usize, bits: usize) -> Vec<u8> {
            (0..length)
                .map(|i| if bits & (1 << i) == 0 { b'A' } else { b'C' })
                .collect()
        }
        for m in 1..=6 {
            for bits in 0..(1 << m) {
                let reference = word(m, bits);
                for rate in [0.0, 0.1, 0.3333333333333333, 0.5, 1.0] {
                    for overlap in [1, 3, m, m + 1] {
                        let mut screened = Aligner::new(&reference, rate, BACK, 1, overlap);
                        let mut scalar = screened.clone();
                        scalar.back_filter = None;
                        for n in 0..=8 {
                            for query_bits in 0..(1 << n) {
                                let query = word(n, query_bits);
                                assert_eq!(screened.locate(&query), scalar.locate(&query),
                                    "reference={reference:?} query={query:?} rate={rate} overlap={overlap}");
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn rejection_filter_preserves_long_words_and_fallbacks() {
        let mut state = 20261101u64;
        for m in [1, 8, 31, 32, 33, 63, 64, 65] {
            let reference: Vec<_> = (0..m).map(|i| b"ACGT"[(i * 7 + i / 3) % 4]).collect();
            for rate in [0.0, 0.099999999999, 0.1, 0.2, 0.5, 1.0] {
                for flags in [BACK, QUERY_START | QUERY_STOP | REFERENCE_START] {
                    let mut screened = Aligner::new(&reference, rate, flags, 1, 3);
                    let mut scalar = screened.clone();
                    scalar.back_filter = None;
                    for iteration in 0..1000 {
                        let n = iteration % 160;
                        let mut query: Vec<_> = (0..n)
                            .map(|_| {
                                state ^= state << 13;
                                state ^= state >> 7;
                                state ^= state << 17;
                                b"ACGTNacgtnU?"[state as usize % 12]
                            })
                            .collect();
                        if iteration % 3 == 0 {
                            query.extend_from_slice(&reference[..(iteration % m + 1)]);
                        }
                        if iteration % 5 == 0 {
                            query.extend_from_slice(&reference);
                            query.extend_from_slice(b"NNAC");
                        }
                        assert_eq!(screened.locate(&query), scalar.locate(&query));
                    }
                }
            }
        }
        // Wildcard behavior remains the scalar implementation, including -N.
        for reference in [b"ACNGT".as_slice(), b"ACGT".as_slice()] {
            for literal in [true, false] {
                let mut screened = Aligner::new(reference, 0.1, BACK, 1, 3)
                    .with_read_wildcards(true)
                    .with_literal_adapter(literal);
                let mut scalar = screened.clone();
                scalar.back_filter = None;
                for query in [b"ACNGT".as_slice(), b"NNNN".as_slice(), b"ACGT".as_slice()] {
                    assert_eq!(screened.locate(query), scalar.locate(query));
                }
            }
        }
    }

    fn locate(reference: &str, query: &str, rate: f64, flags: u32) -> Option<Alignment> {
        Aligner::new(reference.as_bytes(), rate, flags, 1, 1).locate(query.as_bytes())
    }

    fn locate_no_indels(
        reference: &str,
        query: &str,
        rate: f64,
        min_overlap: usize,
    ) -> Option<Alignment> {
        Aligner::new(reference.as_bytes(), rate, BACK, 1000, min_overlap).locate(query.as_bytes())
    }

    // Test vectors from cutadapt 4.6 tests/test_align.py.
    #[test]
    fn illumina_adapter_partial_match() {
        let a = locate(
            "GCCGAACTTCTTAGACTGCCTTAAGGACGT",
            "CAAATCACCAGAAGGCGCCTAACTTCTTAGACTGCC",
            0.1,
            BACK,
        )
        .unwrap();
        assert_eq!(a.ref_start, 0);
        assert_eq!(a.ref_stop, 20);
        assert_eq!(a.query_start, 16);
        assert_eq!(a.query_stop, 36);
        assert_eq!(a.score, 18);
        assert_eq!(a.errors, 1);
    }

    #[test]
    fn poly_t() {
        let a = locate("TTTT", "CCTTTT", 0.25, BACK).unwrap();
        assert_eq!(
            (a.ref_start, a.ref_stop, a.query_start, a.query_stop),
            (0, 4, 2, 6)
        );
        assert_eq!(a.errors, 0);
    }

    #[test]
    fn poly_t_partial() {
        let a = locate("TTTTTT", "CCTTTT", 0.25, BACK).unwrap();
        assert_eq!(
            (a.ref_start, a.ref_stop, a.query_start, a.query_stop),
            (0, 4, 2, 6)
        );
        assert_eq!(a.errors, 0);
    }

    #[test]
    fn poly_a() {
        let s = "AAAAAAAAAAAAAAAAA";
        let t = "ACAGAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
        let a = locate(s, t, 0.0, BACK).unwrap();
        assert_eq!(
            (a.ref_start, a.ref_stop, a.query_start, a.query_stop),
            (0, s.len(), 4, 4 + s.len())
        );
        assert_eq!(a.errors, 0);
    }

    #[test]
    fn no_match() {
        assert!(locate("CTGATCTGGCCG", "AAAAGGG", 0.1, BACK).is_none());
    }

    #[test]
    fn indels_penalized() {
        // cutadapt 4.6: indels are penalized in the score, not preferred.
        let a = locate("CCAGTCCTCT", "CCAGTCCTTTCCTGAGAGT", 0.3, QUERY_STOP).unwrap();
        assert_eq!(
            (a.ref_start, a.ref_stop, a.query_start, a.query_stop),
            (0, 10, 0, 10)
        );
        assert_eq!(a.score, 8);
        assert_eq!(a.errors, 1);

        let a = locate("TCGATC", "TCGATGC", 1.5 / 6.0, QUERY_STOP).unwrap();
        assert_eq!(
            (a.ref_start, a.ref_stop, a.query_start, a.query_stop),
            (0, 6, 0, 6)
        );
        assert_eq!(a.score, 4);
        assert_eq!(a.errors, 1);
    }

    #[test]
    fn empty_in_empty() {
        let mut aligner = Aligner::new(b"", 0.0, 0, 1, 0);
        let a = aligner.locate(b"").unwrap();
        assert_eq!(
            (
                a.ref_start,
                a.ref_stop,
                a.query_start,
                a.query_stop,
                a.score,
                a.errors
            ),
            (0, 0, 0, 0, 0, 0)
        );
    }

    #[test]
    fn lowercase_query_is_matched() {
        // cutadapt uppercases the query before comparison.
        let a = locate("AGATCGGAAGAGC", "ttttagatcggaagagc", 0.1, BACK).unwrap();
        assert_eq!(a.query_start, 4);
        assert_eq!(a.errors, 0);
    }

    // No-indel mode: mismatch-only alignments.
    #[test]
    fn no_indels_exact_partial() {
        let a = locate_no_indels("AGATCGGAAGAGC", "TTTTAGATCG", 0.1, 3).unwrap();
        assert_eq!(a.query_start, 4);
        assert_eq!(a.ref_stop, 6);
        assert_eq!(a.errors, 0);
    }

    #[test]
    fn no_indels_single_mismatch_full_length() {
        let a = locate_no_indels("AGATCGGAAGAGC", "TTTTAGATCGTAAGAGC", 0.1, 3).unwrap();
        assert_eq!(a.query_start, 4);
        assert_eq!(a.ref_stop, 13);
        assert_eq!(a.errors, 1);
    }

    #[test]
    fn no_indels_rejects_two_mismatches_in_13() {
        // 2 errors in 13 bp exceed 13 * 0.1 = 1.3, so no alignment of
        // length 13 (or anything longer) is acceptable.
        assert!(locate_no_indels("AGATCGGAAGAGC", "TTTTAGATCATAAGAGC", 0.1, 3).is_none());
    }

    #[test]
    fn no_indels_min_overlap_boundary() {
        // 2 bp matches are below the minimum overlap of 3.
        assert!(locate_no_indels("AGATCGGAAGAGC", "TTTTTTAG", 0.1, 3).is_none());
        // 3 bp matches are accepted.
        let a = locate_no_indels("AGATCGGAAGAGC", "TTTTTTAGA", 0.1, 3).unwrap();
        assert_eq!(a.ref_stop, 3);
    }

    #[test]
    fn no_indels_shorter_adapter_prefix_still_matches() {
        // With a 10 bp adapter and a 10 bp read suffix the alignment is exact.
        let a = locate_no_indels("AGATCGGAAGAGC", "AGATCGGAAG", 0.1, 3).unwrap();
        assert_eq!(a.ref_stop, 10);
        assert_eq!(a.errors, 0);
    }
}
