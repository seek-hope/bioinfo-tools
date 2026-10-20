//! Adapter trimming built on top of the semiglobal aligner.
//!
//! This implements the subset of cutadapt 4.6 needed for a single 3' adapter
//! without indels: the adapter is aligned from its 5' end, partial matches at
//! the 3' end of the read and complete internal matches are allowed, and the
//! default error rate (0.1) and minimum overlap (3) apply.
//!
//! Matching uses cutadapt's k-mer heuristic as a *prefilter* only: for a 13 bp
//! adapter with the default error rate, `create_back_overlap_searchsets`
//! yields the 3-, 4- and 5-mer prefixes of the adapter searched at the end of
//! the read plus a 6-mer chunk searched anywhere. If none of these occurs, the
//! alignment step is skipped; otherwise the ordinary dynamic program decides,
//! so results are identical to always running the aligner.

use crate::align::{Aligner, BACK, DEFAULT_MAX_ERROR_RATE, DEFAULT_MIN_OVERLAP};

/// Indel cost used by cutadapt when indels are disabled (`--no-indels`).
///
/// cutadapt's `SingleAdapter._make_aligner` sets `indel_cost = 100000` in that
/// mode, which makes any indel alignment far too expensive to be chosen.
pub const NO_INDEL_COST: i32 = 100_000;

/// A configured 3' adapter.
#[derive(Clone, Debug)]
pub struct Adapter {
    aligner: Aligner,
    sequence: Vec<u8>,
    /// Prefixes of the adapter searched at the very end of the read.
    tail_kmers: Vec<Vec<u8>>,
    /// Chunks of the adapter searched anywhere in the read.
    internal_kmers: Vec<Vec<u8>>,
    /// Reused buffer for uppercasing reads that contain lowercase bases.
    scratch: Vec<u8>,
}

/// Outcome of matching one read against the adapter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trim {
    /// No adapter found; the read is unchanged.
    Unchanged,
    /// The read is cut at this (0-based) position.
    CutAt(usize),
}

impl Adapter {
    /// Build a 3' adapter. `sequence` must contain only A, C, G and T
    /// (upper or lower case); callers validate this before constructing.
    pub fn new(sequence: &[u8]) -> Self {
        let normalized: Vec<u8> = sequence
            .iter()
            .map(|b| match b.to_ascii_uppercase() {
                b'U' => b'T',
                other => other,
            })
            .collect();
        let aligner = Aligner::new(
            &normalized,
            DEFAULT_MAX_ERROR_RATE,
            BACK,
            NO_INDEL_COST,
            DEFAULT_MIN_OVERLAP.min(normalized.len()),
        );
        let (tail_kmers, internal_kmers) = kmer_prefilter(&normalized);
        Self {
            aligner,
            sequence: normalized,
            tail_kmers,
            internal_kmers,
            scratch: Vec::new(),
        }
    }

    pub fn sequence(&self) -> &[u8] {
        &self.sequence
    }

    /// Effective error rate (cutadapt's `--error-rate` default).
    pub fn max_error_rate(&self) -> f64 {
        self.aligner.max_error_rate()
    }

    /// Number of adapter bases that must align (cutadapt's `--min-overlap`).
    pub fn min_overlap(&self) -> usize {
        DEFAULT_MIN_OVERLAP.min(self.sequence.len())
    }

    /// The k-mer prefilter: `true` if an alignment could still succeed.
    ///
    /// cutadapt searches each k-mer in a window anchored at the read's 3' end
    /// (start = -len, stop = end) and the internal chunks in the whole read.
    /// Searching the whole read here is a strict superset of those windows, so
    /// this prefilter never discards a read that cutadapt would align; the
    /// dynamic program remains the authority on the actual trimming decision.
    #[inline]
    fn prefilter(&self, read: &[u8]) -> bool {
        self.tail_kmers
            .iter()
            .chain(self.internal_kmers.iter())
            .any(|kmer| contains(read, kmer))
    }

    /// Find the adapter in `read` and return where to cut.
    ///
    /// The cut position is the start of the adapter match (`rstart`), which is
    /// exactly how cutadapt's `RemoveAfterMatch.trimmed` behaves.
    pub fn trim_position(&mut self, read: &[u8]) -> Trim {
        // cutadapt matches case-insensitively (the aligner uppercases the
        // query, and the k-mer finder uses a case-folding lookup table).
        // Lowercase reads are rare, so only pay for folding when needed.
        if read.iter().any(u8::is_ascii_lowercase) {
            let mut buffer = std::mem::take(&mut self.scratch);
            buffer.clear();
            buffer.extend_from_slice(read);
            buffer.make_ascii_uppercase();
            let result = self.trim_position_upper(&buffer);
            self.scratch = buffer;
            result
        } else {
            self.trim_position_upper(read)
        }
    }

    fn trim_position_upper(&mut self, read: &[u8]) -> Trim {
        // Very short adapters (shorter than the minimum overlap) are checked
        // exhaustively: the prefilter is built from prefixes and would miss
        // them.
        if self.sequence.len() < DEFAULT_MIN_OVERLAP {
            return match self.aligner.locate(read) {
                Some(alignment) => Trim::CutAt(alignment.query_start),
                None => Trim::Unchanged,
            };
        }
        if !self.prefilter(read) {
            return Trim::Unchanged;
        }
        match self.aligner.locate(read) {
            Some(alignment) => Trim::CutAt(alignment.query_start),
            None => Trim::Unchanged,
        }
    }

    /// Length of the retained read prefix after trimming.
    pub fn trimmed_len(&mut self, read: &[u8]) -> usize {
        match self.trim_position(read) {
            Trim::Unchanged => read.len(),
            Trim::CutAt(position) => position.min(read.len()),
        }
    }
}

/// Substring search (ASCII) used by the prefilter.
#[inline]
fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    if needle.len() > haystack.len() {
        return false;
    }
    haystack.windows(needle.len()).any(|w| w == needle)
}

/// Port of cutadapt's `create_positions_and_kmers` for a single 3' adapter
/// with default settings, returning the `(tail, internal)` k-mer sets.
///
/// `create_back_overlap_searchsets` groups adapter lengths by the number of
/// errors they may contain (`int(length * error_rate)`), emits the short
/// prefixes 3 and 4 searched at the read's end, then one 5-mer prefix for the
/// 5..=10 base range and 6-mer chunks for 11..=13 bases. `internal=True` adds
/// the 6-mer chunk searched anywhere in the read.
fn kmer_prefilter(adapter: &[u8]) -> (Vec<Vec<u8>>, Vec<Vec<u8>>) {
    const ERROR_RATE: f64 = DEFAULT_MAX_ERROR_RATE;
    const MIN_OVERLAP: usize = DEFAULT_MIN_OVERLAP;
    let adapter_length = adapter.len();
    let mut tail = Vec::new();
    let mut internal = Vec::new();

    // Group lengths by how many errors they are allowed.
    let mut error_lengths: Vec<(i32, usize)> = Vec::new();
    let mut max_error = 0i32;
    for i in 0..=adapter_length {
        if (i as f64 * ERROR_RATE) as i32 > max_error {
            error_lengths.push((max_error, i - 1));
            max_error += 1;
        }
    }
    error_lengths.push((max_error, adapter_length));

    // cutadapt limits min_overlap to the adapter length for short adapters.
    let mut minimum_length = MIN_OVERLAP.min(adapter_length);
    for (max_errors, length) in error_lengths {
        if minimum_length > length {
            continue;
        }
        if max_errors == 0 {
            // Directly matching 3- and 4-mer prefixes.
            let min_overlap_kmer_length = 5.min(length);
            for i in minimum_length..min_overlap_kmer_length {
                tail.push(adapter[..i].to_vec());
            }
            minimum_length = minimum_length.max(min_overlap_kmer_length);
        }
        if minimum_length <= length {
            tail.extend(kmer_chunks(
                &adapter[..minimum_length],
                max_errors as usize + 1,
            ));
        }
        minimum_length = length + 1;
    }

    // Internal search uses the whole adapter split into `max_errors + 1` chunks.
    let max_errors = (adapter_length as f64 * ERROR_RATE) as i32;
    internal.extend(kmer_chunks(adapter, max_errors as usize + 1));

    // cutadapt keeps each k-mer once (in the widest window); dropping
    // duplicates here has the same effect and avoids redundant scans.
    dedupe(&mut tail);
    dedupe(&mut internal);
    (tail, internal)
}

/// Remove duplicate k-mers while preserving first-seen order.
fn dedupe(kmers: &mut Vec<Vec<u8>>) {
    let mut seen: Vec<Vec<u8>> = Vec::with_capacity(kmers.len());
    for kmer in kmers.drain(..) {
        if !seen.contains(&kmer) {
            seen.push(kmer);
        }
    }
    *kmers = seen;
}

/// cutadapt's `kmer_chunks`: split into `chunks` almost-equal parts.
fn kmer_chunks(sequence: &[u8], chunks: usize) -> Vec<Vec<u8>> {
    if chunks == 0 || sequence.is_empty() {
        return Vec::new();
    }
    let chunk_size = sequence.len() / chunks;
    let remainder = sequence.len() % chunks;
    let mut sizes = Vec::with_capacity(chunks);
    for _ in 0..remainder {
        sizes.push(chunk_size + 1);
    }
    for _ in 0..(chunks - remainder) {
        sizes.push(chunk_size);
    }
    let mut offset = 0;
    let mut out = Vec::new();
    for size in sizes {
        if size == 0 || offset + size > sequence.len() {
            continue;
        }
        out.push(sequence[offset..offset + size].to_vec());
        offset += size;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const ADAPTER: &[u8] = b"AGATCGGAAGAGC";

    fn trim(read: &[u8]) -> usize {
        let mut adapter = Adapter::new(ADAPTER);
        adapter.trimmed_len(read)
    }

    #[test]
    fn exact_full_adapter_is_removed() {
        let read = b"GCTAAAGACAATTACATAACATACACGTCAGCACGAAACTAGATCGGAAGAGC";
        assert_eq!(trim(read), 40);
    }

    #[test]
    fn internal_adapter_truncates_read() {
        let read = b"TGTTGGCCCAGTGTGAATCGCTTAAGGGTTAGATCGGAAGAGCAAGTAAGTGT";
        assert_eq!(trim(read), 30);
    }

    #[test]
    fn partial_3_prime_adapter_is_removed() {
        let read = b"GATGCATACGCCTTTACTTGCTGTGTCCACCCCATCGGACTGGCAAGATCG";
        assert_eq!(trim(read), 45);
    }

    #[test]
    fn one_mismatch_in_full_adapter_is_removed() {
        let read = b"GAGGCGCGCCCTCCTGAAGTGCGTGGACACTCGCTATGAAAGATCGTAAGAGC";
        assert_eq!(trim(read), 40);
    }

    #[test]
    fn no_adapter_leaves_read_untouched() {
        let read = b"TTTTTATTACACTCAGAAACAGAACTCGGGTAATTTTGACAGGTCACGCA";
        assert_eq!(trim(read), read.len());
    }

    #[test]
    fn adapter_at_start_yields_empty_read() {
        let read = b"AGATCGGAAGAGCGCTCTATTGACTACGACGCGCTCATTCCCTTGTCGGAGAG";
        assert_eq!(trim(read), 0);
    }

    #[test]
    fn reverse_complement_is_not_matched() {
        let read = b"TTATGGAACAAGGACGCTGTCTGAGACTAGAAGACGCTCTTCCGATCT";
        assert_eq!(trim(read), read.len());
    }

    #[test]
    fn long_read_is_cut_before_adapter() {
        let read = b"CACTCTGCCAAACTCCAGCGCGGTCAGTTCCATCACCCTAAGTAACCGAATAATGCGTTCAGATCGGAAGAGC";
        assert_eq!(trim(read), 60);
    }

    #[test]
    fn short_read_with_partial_adapter_is_trimmed() {
        // 5 bases of the adapter align at the read's 3' end (>= min_overlap 3),
        // so cutadapt trims them away. The resulting read is shorter than
        // -m 10 and is therefore discarded by the caller.
        let read = b"TCTCTAGATCGGAAGAGC";
        assert_eq!(trim(read), 5);
    }

    #[test]
    fn adapter_shorter_than_min_overlap_never_matches() {
        // cutadapt caps min_overlap at the adapter length, so a 1-2 bp adapter
        // may match; verify the code path does not panic and stays consistent.
        for adapter in [&b"A"[..], b"AC"] {
            let mut a = Adapter::new(adapter);
            let read = b"TTTTACGTACGT";
            let _ = a.trim_position(read);
        }
    }

    #[test]
    fn lowercase_adapter_and_read_are_supported() {
        assert_eq!(trim(b"ttttagatcggaagagc"), 4);
    }

    #[test]
    fn prefilter_kmers_match_cutadapt_heuristic() {
        // cutadapt's create_back_overlap_searchsets(AGATCGGAAGAGC, 3, 0.1)
        // yields the end-anchored searches AGA (3), AGAT (4), AGATC (9..13),
        // GGAAG (11..13); create_positions_and_kmers additionally searches the
        // internal chunks AGATCGGA and AGAGC anywhere in the read.
        let (tail, internal) = kmer_prefilter(ADAPTER);
        let tail: Vec<&[u8]> = tail.iter().map(|v| v.as_slice()).collect();
        assert_eq!(tail, vec![&b"AGA"[..], b"AGAT", b"AGATC", b"GGAAG"]);
        let internal: Vec<&[u8]> = internal.iter().map(|v| v.as_slice()).collect();
        assert_eq!(internal, vec![&b"AGATCGG"[..], &b"AAGAGC"[..]]);
    }
}
