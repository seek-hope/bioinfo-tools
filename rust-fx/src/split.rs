//! Output naming and record partitioning for `split`.
//!
//! # Naming, from the published documentation
//!
//! SeqKit's `split` writes into an output directory and names parts with a
//! zero-padded counter:
//!
//! ```text
//! hairpin.fa.gz.split/hairpin.part_00001.fa.gz
//! ```
//!
//! The published examples show **two** widths in different places — `part_001`
//! in one example and `part_00001` in another — because `-W/--part-width`
//! defaults to 3 and a longer counter widens automatically once the part count
//! needs more digits. The `-W` default of `3` is documented in the flag list, so
//! it is what this implementation uses, and DR-6 records the discrepancy in the
//! examples.
//!
//! # Ordering
//!
//! Parts are numbered in input order: part 1 holds the first `n` records, part 2
//! the next `n`, and so on. The last part holds the remainder and may be shorter.
//! That is the only ordering consistent with the documented per-part record counts
//! (`10000`, `10000`, `8645`).

use std::path::{Path, PathBuf};

/// The documented default for `-W/--part-width`.
pub const DEFAULT_PART_WIDTH: usize = 3;

/// How to partition the input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SplitBy {
    /// `-s/--by-size N`: at most `N` records per part.
    Size(u64),
    /// `-p/--by-part N`: exactly `N` parts, as evenly as possible.
    Parts(u64),
}

/// The stem used for part file names.
///
/// SeqKit strips the compression suffix and the sequence-format suffix from the
/// input name, so `hairpin.fa.gz` becomes `hairpin`. A name with neither suffix
/// keeps its full stem.
pub fn part_stem(input: &Path) -> String {
    let name = input
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut stem = name.as_str();
    // Peel a compression suffix first, then a sequence-format suffix.
    for compression in [".gz", ".xz", ".zst", ".bz2", ".lz4"] {
        if let Some(stripped) = stem.strip_suffix(compression) {
            stem = stripped;
            break;
        }
    }
    for sequence in [".fastq", ".fq", ".fasta", ".fa", ".fna", ".ffn", ".faa"] {
        if let Some(stripped) = stem.strip_suffix(sequence) {
            stem = stripped;
            break;
        }
    }
    stem.to_string()
}

/// The default output directory: `<input>.split`.
pub fn default_out_dir(input: &Path) -> PathBuf {
    let name = input
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "stdin".to_string());
    input.with_file_name(format!("{name}.split"))
}

/// The file-name extension to give each part, including any compression suffix.
///
/// Derived from the input's own suffixes so `reads.fq.gz` splits into `part_N.fq.gz`
/// rather than losing the format or the compression.
pub fn part_extension(input: &Path) -> String {
    let name = input
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    // Peel the compression suffix FIRST, then look for the sequence suffix in
    // what remains. Matching format suffixes against the full name would never
    // fire for `reads.fq.gz`, because that name does not end in `.fq`.
    let mut stem = name.as_str();
    let mut compression = "";
    for candidate in [".gz", ".xz", ".zst", ".bz2", ".lz4"] {
        if let Some(stripped) = stem.strip_suffix(candidate) {
            stem = stripped;
            compression = candidate;
            break;
        }
    }
    let mut extension = String::new();
    for sequence in [".fastq", ".fq", ".fasta", ".fa", ".fna", ".ffn", ".faa"] {
        if stem.ends_with(sequence) {
            extension.push_str(sequence);
            break;
        }
    }
    extension.push_str(compression);
    if extension.is_empty() {
        // No recognised suffix: keep the input's own extension, if any.
        if let Some(ext) = input.extension() {
            extension = format!(".{}", ext.to_string_lossy());
        }
    }
    extension
}

/// A part file's full path.
///
/// `part` is 1-based, matching the published `part_001`.
pub fn part_path(
    out_dir: &Path,
    stem: &str,
    part: u64,
    width: usize,
    extension: &str,
    explicit_extension: Option<&str>,
) -> PathBuf {
    let extension = explicit_extension.unwrap_or(extension);
    out_dir.join(format!(
        "{stem}.part_{:0width$}{extension}",
        part,
        width = width.max(1)
    ))
}

/// Partition `total` records into `parts` contiguous ranges.
///
/// Returns `(start, end)` half-open record indices, one per part. A part never
/// receives more than one record more than any other, and the earlier parts get
/// the extra record when the split is uneven — which is what "split into N parts"
/// means for sequential data.
pub fn partition_by_parts(total: u64, parts: u64) -> Vec<(u64, u64)> {
    if parts == 0 || total == 0 {
        return Vec::new();
    }
    let parts = parts.min(total);
    let base = total / parts;
    let remainder = total % parts;
    let mut ranges = Vec::with_capacity(parts as usize);
    let mut start = 0u64;
    for index in 0..parts {
        let take = base + u64::from(index < remainder);
        ranges.push((start, start + take));
        start += take;
    }
    ranges
}

/// Number of parts a `by-size` split produces for `total` records.
pub fn parts_for_size(total: u64, size: u64) -> u64 {
    if size == 0 {
        return 0;
    }
    total.div_ceil(size)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stem_strips_compression_then_format() {
        assert_eq!(part_stem(Path::new("hairpin.fa.gz")), "hairpin");
        assert_eq!(part_stem(Path::new("reads_1.fq.gz")), "reads_1");
        assert_eq!(part_stem(Path::new("reads.fastq")), "reads");
        assert_eq!(part_stem(Path::new("genome.fasta.xz")), "genome");
        assert_eq!(
            part_stem(Path::new("a.bz2.fq")),
            "a.bz2",
            "compression first, then format"
        );
        assert_eq!(part_stem(Path::new("plain")), "plain");
        assert_eq!(part_stem(Path::new("/tmp/dir/x.fq")), "x");
    }

    #[test]
    fn extension_keeps_both_suffixes() {
        assert_eq!(part_extension(Path::new("reads.fq.gz")), ".fq.gz");
        assert_eq!(part_extension(Path::new("reads.fastq")), ".fastq");
        assert_eq!(part_extension(Path::new("g.fa.xz")), ".fa.xz");
        assert_eq!(part_extension(Path::new("x.fq")), ".fq");
    }

    #[test]
    fn default_out_dir_is_input_dot_split() {
        assert_eq!(
            default_out_dir(Path::new("/tmp/hairpin.fa.gz")),
            PathBuf::from("/tmp/hairpin.fa.gz.split")
        );
    }

    // The published naming: `hairpin.fa.gz.split/hairpin.part_001.fa.gz`.
    #[test]
    fn part_path_matches_the_published_naming() {
        let path = part_path(
            Path::new("hairpin.fa.gz.split"),
            "hairpin",
            1,
            DEFAULT_PART_WIDTH,
            ".fa.gz",
            None,
        );
        assert_eq!(
            path,
            PathBuf::from("hairpin.fa.gz.split/hairpin.part_001.fa.gz")
        );

        let path = part_path(
            Path::new("reads.fq.split"),
            "reads",
            12,
            DEFAULT_PART_WIDTH,
            ".fq",
            None,
        );
        assert_eq!(path, PathBuf::from("reads.fq.split/reads.part_012.fq"));
    }

    #[test]
    fn part_numbers_are_zero_padded_to_the_width() {
        for (part, expected) in [(1u64, "001"), (9, "009"), (10, "010"), (100, "100")] {
            let path = part_path(Path::new("o"), "s", part, 3, ".fq", None);
            assert!(
                path.to_string_lossy().contains(&format!("part_{expected}")),
                "{path:?}"
            );
        }
    }

    #[test]
    fn a_larger_width_pads_further() {
        let path = part_path(Path::new("o"), "s", 1, 5, ".fq", None);
        assert!(path.to_string_lossy().contains("part_00001"), "{path:?}");
    }

    #[test]
    fn a_wider_counter_than_the_width_is_not_truncated() {
        let path = part_path(Path::new("o"), "s", 1234, 3, ".fq", None);
        assert!(path.to_string_lossy().contains("part_1234"), "{path:?}");
    }

    #[test]
    fn an_explicit_extension_overrides_the_input_extension() {
        let path = part_path(Path::new("o"), "s", 1, 3, ".fq", Some(".gz"));
        assert!(path.to_string_lossy().ends_with("part_001.gz"), "{path:?}");
    }

    #[test]
    fn partition_by_parts_is_even_and_ordered() {
        let ranges = partition_by_parts(10, 3);
        assert_eq!(ranges, vec![(0, 4), (4, 7), (7, 10)]);
        // Contiguous, covering everything, and the sizes differ by at most one.
        let sizes: Vec<u64> = ranges.iter().map(|(s, e)| e - s).collect();
        assert_eq!(
            *sizes.iter().max().unwrap() - *sizes.iter().min().unwrap(),
            1
        );
        assert_eq!(ranges.first().unwrap().0, 0);
        assert_eq!(ranges.last().unwrap().1, 10);
    }

    #[test]
    fn partition_by_parts_handles_more_parts_than_records() {
        // Cannot create empty parts: the count is clamped to the record count.
        let ranges = partition_by_parts(3, 10);
        assert_eq!(ranges.len(), 3);
        assert_eq!(ranges, vec![(0, 1), (1, 2), (2, 3)]);
    }

    #[test]
    fn partition_by_parts_edge_cases() {
        assert!(partition_by_parts(0, 3).is_empty());
        assert!(partition_by_parts(5, 0).is_empty());
        assert_eq!(partition_by_parts(5, 1), vec![(0, 5)]);
        assert_eq!(
            partition_by_parts(5, 5),
            vec![(0, 1), (1, 2), (2, 3), (3, 4), (4, 5)]
        );
    }

    #[test]
    fn partition_covers_every_record_exactly_once() {
        for total in [1u64, 2, 7, 10, 100, 1001] {
            for parts in [1u64, 2, 3, 7, 13] {
                let ranges = partition_by_parts(total, parts);
                let covered: u64 = ranges.iter().map(|(s, e)| e - s).sum();
                assert_eq!(covered, total, "total {total} parts {parts}");
                let mut expected_start = 0;
                for (start, end) in &ranges {
                    assert_eq!(*start, expected_start, "ranges must be contiguous");
                    assert!(end > start, "no empty part");
                    expected_start = *end;
                }
            }
        }
    }

    // The published by-size example: 28,645 records at 10,000 per part gives
    // 10,000 / 10,000 / 8,645.
    #[test]
    fn by_size_matches_the_published_example() {
        assert_eq!(parts_for_size(28_645, 10_000), 3);
        let mut remaining = 28_645u64;
        let mut sizes = Vec::new();
        while remaining > 0 {
            let take = remaining.min(10_000);
            sizes.push(take);
            remaining -= take;
        }
        assert_eq!(sizes, vec![10_000, 10_000, 8_645]);
    }

    #[test]
    fn by_size_edge_cases() {
        assert_eq!(parts_for_size(0, 10), 0);
        assert_eq!(parts_for_size(10, 0), 0);
        assert_eq!(parts_for_size(10, 10), 1);
        assert_eq!(parts_for_size(11, 10), 2);
        assert_eq!(parts_for_size(10, 100), 1);
    }
}
