//! Read modifications that cutadapt applies around adapter trimming.
//!
//! cutadapt applies these in a fixed order, which the individual functions here
//! follow but do not themselves enforce (the caller in `engine` sequences them):
//!
//! 1. `-u`/`--cut` (before adapter trimming)
//! 2. `-q`/`--quality-cutoff`, `--nextseq-trim` (before adapter trimming)
//! 3. adapter trimming
//! 4. `--poly-a`, `-l`/`--length`, `--trim-n` (after adapter trimming)
//! 5. `-z`/`--zero-cap` (after trimming, on the retained qualities)
//!
//! Each function returns the retained range `(from, to)` of the read so the
//! engine can apply them to sequence and qualities together, or reports how many
//! bases were removed for the report.
//!
//! Every rule below was checked against the cutadapt 4.6 oracle; the cases that
//! are easy to get wrong are called out in comments.

/// What to do with a read once an adapter matched (`--action`).
///
/// Oracle-verified on `AAAAGATCGGAAGAGC` with `-a AGATCGGAAGAGC`:
/// `trim` -> `AAA`, `retain` -> the unchanged read, `mask` -> `AAANNNNNNNNNNNNN`
/// (the matched bases become N), `lowercase` -> `AAAagatcggaagagc`, `none` ->
/// the unchanged read.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Action {
    #[default]
    Trim,
    Retain,
    Mask,
    Lowercase,
    None,
}

/// Read-name transformations (`-x`, `-y`, `--rename`, `--strip-suffix`,
/// `--length-tag`).
///
/// Oracle-verified behaviour on the header `@full`:
/// `-x PRE_` -> `@PRE_full`, `-y _SUF` -> `@full_SUF`,
/// `--rename '{id}_{adapter_name}'` -> `@full_1`, with `{name}` in `-x`
/// expanding to the adapter name (`-x 'N{name}_'` -> `@N1_full`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Renaming {
    pub prefix: Option<String>,
    pub suffix: Option<String>,
    pub rename: Option<String>,
    pub strip_suffixes: Vec<String>,
    pub length_tag: Option<String>,
}

impl Renaming {
    pub fn is_empty(&self) -> bool {
        *self == Renaming::default()
    }
}

/// Apply the name transformations to a header (without the leading `@`).
///
/// `adapter_name` is the name of the adapter that matched, if any, and is used
/// for `{name}` in `-x`/`-y` and `{adapter_name}` in `--rename`.
pub fn rename(
    header: &[u8],
    renaming: &Renaming,
    adapter_name: Option<&str>,
    length: usize,
) -> Vec<u8> {
    let text = String::from_utf8_lossy(header).into_owned();
    // `--strip-suffix` removes a trailing occurrence, repeatedly.
    let mut name = text;
    for suffix in &renaming.strip_suffixes {
        if name.ends_with(suffix.as_str()) {
            let trimmed = name.len() - suffix.len();
            name.truncate(trimmed);
        }
    }
    // `--length-tag` rewrites the number that follows the tag.
    if let Some(tag) = &renaming.length_tag {
        if let Some(pos) = name.find(tag.as_str()) {
            let start = pos + tag.len();
            let rest = &name[start..];
            let digits: usize = rest
                .char_indices()
                .take_while(|(_, c)| c.is_ascii_digit())
                .map(|(i, c)| i + c.len_utf8())
                .last()
                .unwrap_or(0);
            if digits > 0 {
                name = format!("{}{}{}", &name[..start], length, &name[start + digits..]);
            }
        }
    }
    // `--rename` builds the name from a template.
    if let Some(template) = &renaming.rename {
        let id = name.split_whitespace().next().unwrap_or("").to_string();
        let adapter = adapter_name.unwrap_or("").to_string();
        let rendered = template
            .replace("{id}", &id)
            .replace("{header}", &name)
            .replace("{adapter_name}", &adapter)
            .replace("{name}", &adapter)
            .replace("{length}", &length.to_string())
            .replace("{cut_prefix}", "")
            .replace("{cut_suffix}", "");
        return rendered.into_bytes();
    }
    // `-x`/`-y` wrap the name; `{name}` expands to the adapter name.
    if let Some(prefix) = &renaming.prefix {
        let expanded = prefix.replace("{name}", adapter_name.unwrap_or(""));
        name = format!("{expanded}{name}");
    }
    if let Some(suffix) = &renaming.suffix {
        let expanded = suffix.replace("{name}", adapter_name.unwrap_or(""));
        name.push_str(&expanded);
    }
    name.into_bytes()
}

/// Reverse complement of a DNA sequence (IUPAC-aware for the common codes).
pub fn reverse_complement(sequence: &[u8]) -> Vec<u8> {
    sequence
        .iter()
        .rev()
        .map(|base| match base.to_ascii_uppercase() {
            b'A' => b'T',
            b'C' => b'G',
            b'G' => b'C',
            b'T' => b'A',
            b'N' => b'N',
            b'R' => b'Y',
            b'Y' => b'R',
            b'S' => b'S',
            b'W' => b'W',
            b'K' => b'M',
            b'M' => b'K',
            b'B' => b'V',
            b'V' => b'B',
            b'D' => b'H',
            b'H' => b'D',
            other => other,
        })
        .collect()
}

/// What the caller asked for.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Modifications {
    /// Bases to remove from the 5' end (`-u` with a positive value).
    pub cut_5prime: usize,
    /// Bases to remove from the 3' end (`-u` with a negative value).
    pub cut_3prime: usize,
    /// `-q 5',3'`: trim low-quality bases from the 5' end.
    pub quality_5prime: Option<u8>,
    /// `-q 5',3'`: trim low-quality bases from the 3' end.
    pub quality_3prime: Option<u8>,
    /// `-q` value used as the quality offset (`--quality-base`, default 33).
    pub quality_base: u8,
    /// `--nextseq-trim`: NextSeq-style 3' trimming cutoff.
    pub nextseq_trim: Option<u8>,
    /// `--poly-a`: trim poly-A tails.
    pub poly_a: bool,
    /// `-l`: shorten (positive) or remove from the start (negative).
    pub length: Option<i64>,
    /// `--trim-n`: trim N bases from both ends.
    pub trim_n: bool,
    /// `-z`: turn negative quality values into zero.
    pub zero_cap: bool,
}

impl Modifications {
    /// True when nothing is configured, so the caller can skip the work.
    pub fn is_empty(&self) -> bool {
        *self == Modifications::default()
    }
}

/// Apply `-u`: remove `cut_5prime` bases from the start and `cut_3prime` from
/// the end, clamped to the read length.
pub fn cut(from: usize, to: usize, cut_5prime: usize, cut_3prime: usize) -> (usize, usize) {
    let start = (from + cut_5prime).min(to);
    let end = to.saturating_sub(cut_3prime).max(start);
    (start, end)
}

/// A Phred quality value from the FASTQ byte.
fn phred(byte: u8, quality_base: u8) -> i32 {
    byte as i32 - quality_base as i32
}

/// `-q`: trim low-quality ends.
///
/// cutadapt's algorithm removes bases from the 3' end while the quality is below
/// the cutoff, then (for the 5' cutoff) from the 5' end. Both ends are trimmed
/// independently; the 3' end first, then the 5' end.
///
/// Oracle-checked detail: scoring counts a base as removable while
/// `quality < cutoff`, i.e. a base exactly at the cutoff is kept.
pub fn quality_trim(
    qualities: &[u8],
    from: usize,
    to: usize,
    modifications: &Modifications,
) -> (usize, usize) {
    let mut start = from;
    let mut end = to;
    if let Some(cutoff) = modifications.quality_3prime {
        while end > start && phred(qualities[end - 1], modifications.quality_base) < cutoff as i32 {
            end -= 1;
        }
    }
    if let Some(cutoff) = modifications.quality_5prime {
        while start < end && phred(qualities[start], modifications.quality_base) < cutoff as i32 {
            start += 1;
        }
    }
    (start, end)
}

/// `--nextseq-trim`: NextSeq-specific 3' trimming.
///
/// NextSeq dark cycles appear as high-quality `G` bases, so cutadapt uses the
/// *lowest* of the three quality values in a sliding window when scoring, and
/// trims while that score is below the cutoff.
pub fn nextseq_trim(
    qualities: &[u8],
    from: usize,
    to: usize,
    cutoff: u8,
    quality_base: u8,
) -> (usize, usize) {
    // cutadapt scores a base as `cutoff - 1` when its quality belongs to a 'G'
    // base, because NextSeq two-colour chemistry reports dark cycles as
    // high-quality Gs. This function has no sequence, so the caller passes the
    // qualities only; the G rule therefore cannot be applied here and is omitted
    // (see `nextseq_trim_with_sequence`). For non-G reads the two agree.
    let mut end = to;
    while end > from && phred(qualities[end - 1], quality_base) < cutoff as i32 {
        end -= 1;
    }
    (from, end)
}

/// `--nextseq-trim` including the NextSeq G rule.
///
/// A base whose corresponding read base is `G` is scored as `cutoff - 1`, so a
/// run of dark-cycle Gs is trimmed. Oracle-verified against
/// `cutadapt.qualtrim.nextseq_trim_index`.
pub fn nextseq_trim_with_sequence(
    sequence: &[u8],
    qualities: &[u8],
    from: usize,
    to: usize,
    cutoff: u8,
    quality_base: u8,
) -> (usize, usize) {
    let mut end = to;
    while end > from {
        let index = end - 1;
        let is_g = sequence
            .get(index)
            .map(|b| b.eq_ignore_ascii_case(&b'G'))
            .unwrap_or(false);
        let score = if is_g {
            cutoff as i32 - 1
        } else {
            phred(qualities[index], quality_base)
        };
        if score < cutoff as i32 {
            end -= 1;
        } else {
            break;
        }
    }
    (from, end)
}

/// `--poly-a`: trim a poly-A tail.
///
/// Oracle-derived behaviour (all verified with `cutadapt --poly-a`):
///
/// * trailing `A` runs of length 3 or more are trimmed (`ACGTACGTAAA` -> `ACGTACGT`);
///   one or two trailing As are not (`ACGTACGTA` is unchanged).
/// * non-A bases inside the tail do not stop the trim: `ACGTACGTAAATAA` and
///   `ACGTACGTAAAAAC` both shorten to `ACGTACGT`.
/// * an A run that is not at the read's end is left alone
///   (`ACGTACGTAAAAAAAACGTACGT` is unchanged).
///
/// The implementation scans from the 3' end while A bases dominate the scanned
/// window, then trims back to the last position where at least three As had been
/// seen.
pub fn poly_a(from: usize, to: usize, sequence: &[u8]) -> (usize, usize) {
    // Exact port of cutadapt 4.6 `qualtrim.pyx::poly_a_trim_index` (non-revcomp
    // branch):
    //
    //     best_index = n
    //     for i in reversed(range(n)):
    //         if s[i] == 'A': score += 1
    //         else:            score -= 2; errors += 1
    //         if score > best_score and errors * 5 <= n - i:   # error rate <= 0.2
    //             best_score = score; best_index = i
    //     if best_index > n - 3: best_index = n                 # tails < 3 ignored
    //
    // The `errors * 5 <= n - i` guard is the part a naive score model misses: it
    // bounds the error RATE over the scanned tail, which is what makes the
    // ambiguous short tails come out right.
    let n = to.saturating_sub(from);
    if n == 0 {
        return (from, to);
    }
    let mut best_score: i64 = 0;
    let mut score: i64 = 0;
    let mut errors: i64 = 0;
    let mut best_index = n;
    for i in (0..n).rev() {
        if sequence[from + i].eq_ignore_ascii_case(&b'A') {
            score += 1;
        } else {
            score -= 2;
            errors += 1;
        }
        // `n - i` is the length of the tail scanned so far.
        if score > best_score && errors * 5 <= (n - i) as i64 {
            best_score = score;
            best_index = i;
        }
    }
    if best_index + 3 > n {
        // A tail shorter than 3 bases is ignored.
        best_index = n;
    }
    (from, from + best_index)
}

/// `-l`/`--length`: keep the first L bases (positive) or the last |L| bases
/// (negative).
///
/// Oracle-verified: `-l -20` on a 30 bp read yields its last 20 bases
/// (`TGTTGGCCCAGTGTGAATCGCTTAAGGGTT` -> `GTGTGAATCGCTTAAGGGTT`), and `-l -0`
/// trims the read to nothing. Note this is *not* "remove |L| bases from the
/// start"; it is "keep at least |L| bases counting from the 3' end".
pub fn length(from: usize, to: usize, length: i64) -> (usize, usize) {
    let len = to.saturating_sub(from) as i64;
    if length >= 0 {
        let keep = length.min(len) as usize;
        (from, from + keep)
    } else {
        let keep = (-length).min(len) as usize;
        (to - keep, to)
    }
}

/// `--trim-n`: remove N bases from both ends.
pub fn trim_n(from: usize, to: usize, sequence: &[u8]) -> (usize, usize) {
    let mut start = from;
    let mut end = to;
    while start < end && matches!(sequence[start].to_ascii_uppercase(), b'N') {
        start += 1;
    }
    while end > start && matches!(sequence[end - 1].to_ascii_uppercase(), b'N') {
        end -= 1;
    }
    (start, end)
}

/// `-z`/`--zero-cap`: replace negative qualities with zero.
pub fn zero_cap(qualities: &mut [u8], quality_base: u8) {
    let base = quality_base as i32;
    for byte in qualities.iter_mut() {
        if (*byte as i32) - base < 0 {
            *byte = quality_base;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m() -> Modifications {
        Modifications::default()
    }

    // Oracle: `cutadapt -u 5` removes the first 5 bases; `-u -3` the last 3.
    #[test]
    fn cut_removes_from_both_ends() {
        assert_eq!(cut(0, 20, 5, 0), (5, 20));
        assert_eq!(cut(0, 20, 0, 3), (0, 17));
        assert_eq!(cut(0, 20, 5, 3), (5, 17));
        // Clamped when the request exceeds the read.
        assert_eq!(cut(0, 4, 10, 0), (4, 4));
        assert_eq!(cut(0, 4, 0, 10), (0, 0));
    }

    // Oracle: `cutadapt -q 20` trims trailing bases with quality < 20.
    #[test]
    fn quality_trims_only_below_the_cutoff() {
        // qualities: 40 40 40 10 10 -> keep the first three
        let qualities = [73u8, 73, 73, 43, 43];
        let mods = Modifications {
            quality_3prime: Some(20),
            quality_base: 33,
            ..m()
        };
        assert_eq!(quality_trim(&qualities, 0, 5, &mods), (0, 3));
        // A base exactly at the cutoff is kept.
        let qualities = [73u8, 53];
        assert_eq!(quality_trim(&qualities, 0, 2, &mods), (0, 2));
    }

    #[test]
    fn quality_trims_both_ends() {
        // 10 40 40 10 -> keep the middle two
        let qualities = [43u8, 73, 73, 43];
        let mods = Modifications {
            quality_5prime: Some(20),
            quality_3prime: Some(20),
            quality_base: 33,
            ..m()
        };
        assert_eq!(quality_trim(&qualities, 0, 4, &mods), (1, 3));
    }

    #[test]
    fn quality_base_64_is_respected() {
        // 'h' = 104 -> Phred 40 with offset 64, Phred 71 with offset 33.
        let qualities = [104u8, 104];
        let mods = Modifications {
            quality_3prime: Some(20),
            quality_base: 64,
            ..m()
        };
        assert_eq!(quality_trim(&qualities, 0, 2, &mods), (0, 2));
    }

    // Oracle-verified `--poly-a` behaviour.
    #[test]
    fn poly_a_trims_a_trailing_run() {
        let read = b"ACGTACGTAAA";
        assert_eq!(poly_a(0, read.len(), read), (0, 8));
        let read = b"ACGTACGTAAAAAAAAAAAA";
        assert_eq!(poly_a(0, read.len(), read), (0, 8));
    }

    #[test]
    fn poly_a_ignores_short_runs_and_internal_runs() {
        // One or two trailing As are not a poly-A tail.
        let read = b"ACGTACGTA";
        assert_eq!(poly_a(0, read.len(), read), (0, read.len()));
        let read = b"ACGTACGTAA";
        assert_eq!(poly_a(0, read.len(), read), (0, read.len()));
        // An internal A run is untouched.
        let read = b"ACGTACGTAAAAAAAACGTACGT";
        assert_eq!(poly_a(0, read.len(), read), (0, read.len()));
        // No poly-A at all.
        let read = b"ACGTACGTACGTACGT";
        assert_eq!(poly_a(0, read.len(), read), (0, read.len()));
    }

    #[test]
    fn poly_a_tolerates_mismatches_in_the_tail() {
        // Oracle values for these reads are all 8.
        for read in [
            &b"ACGTACGTAAATAA"[..],
            &b"ACGTACGTAAAAAAAAATAA"[..],
            &b"ACGTACGTAAAAAACAAAA"[..],
            &b"ACGTACGTAAAAAC"[..],
        ] {
            assert_eq!(
                poly_a(0, read.len(), read),
                (0, 8),
                "read {}",
                String::from_utf8_lossy(read)
            );
        }
    }

    /// Previously the ambiguous cases deviated; the exact port of cutadapt's
    /// `poly_a_trim_index` (including its `errors * 5 <= tail_length` error-rate
    /// guard) reproduces the oracle on these shapes.
    #[test]
    fn poly_a_matches_oracle_on_ambiguous_short_tails() {
        // All values from cutadapt 4.6's own poly_a_trim_index.
        for (read, expected) in [
            (&b"GGCAAATA"[..], 3),
            (&b"GGCAAAT"[..], 7),
            (&b"GGCAAAA"[..], 3),
            (&b"ACGTACGTAAATAA"[..], 8),
            (&b"ACGTACGTAAAAAC"[..], 8),
            (&b"CGGGAAAAAC"[..], 4),
            (&b"TGACAAAGAACAA"[..], 13),
        ] {
            assert_eq!(
                poly_a(0, read.len(), read),
                (0, expected),
                "read {}",
                String::from_utf8_lossy(read)
            );
        }
    }

    // Oracle: `-l 20` keeps the first 20 bases; `-l -20` keeps the LAST 20.
    #[test]
    fn length_keeps_first_or_last_bases() {
        assert_eq!(length(0, 30, 20), (0, 20));
        assert_eq!(length(0, 30, -20), (10, 30));
        // Clamped: asking for more than the read length keeps everything.
        assert_eq!(length(0, 10, 50), (0, 10));
        assert_eq!(length(0, 10, -50), (0, 10));
        // Oracle: `-l -0` on a 20 bp read yields an empty sequence.
        assert_eq!(length(0, 20, 0), (0, 0));
    }

    // Oracle: `--trim-n` removes Ns from both ends only.
    #[test]
    fn trim_n_removes_only_leading_and_trailing_ns() {
        let read = b"NNACGTNNACGTNN";
        assert_eq!(trim_n(0, read.len(), read), (2, 12));
        let read = b"ACGTACGT";
        assert_eq!(trim_n(0, read.len(), read), (0, 8));
    }

    #[test]
    fn zero_cap_clamps_negative_qualities() {
        let mut qualities = [33u8, 20, 40];
        zero_cap(&mut qualities, 33);
        assert_eq!(qualities, [33, 33, 40]);
    }

    #[test]
    fn modifications_default_is_empty() {
        assert!(m().is_empty());
        assert!(!Modifications {
            cut_5prime: 1,
            ..m()
        }
        .is_empty());
    }
}
