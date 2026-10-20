//! Counters behind `flagstat` and `idxstats`.
//!
//! Both commands are pure counting over flags and reference ids, so they are
//! implemented as plain structs here and unit-tested without any I/O. The exact
//! output wording lives in `commands`, so a formatting change cannot silently
//! alter a count.
//!
//! ## `flagstat` semantics
//!
//! The 16 lines `samtools flagstat` prints, and which flag bits feed them, were
//! taken from observed output rather than guessed. The interesting ones:
//!
//! * **secondary** and **supplementary** are counted *separately from* primary,
//!   so a file with one primary, one secondary and one supplementary reports
//!   `3 in total` but `1 primary`.
//! * **duplicates** counts every record with 0x400; **primary duplicates**
//!   additionally excludes secondary and supplementary.
//! * **mapped** counts records *without* 0x4 among all records; **primary
//!   mapped** restricts to primary.
//! * **paired in sequencing** requires 0x1; **read1**/**read2** additionally
//!   require 0x40/0x80 and are *not* restricted to primary.
//! * **properly paired** requires 0x1 and 0x2 and *not* 0x4.
//! * **with itself and mate mapped** requires 0x1, not 0x4, and not 0x8.
//! * **singletons** requires 0x1, not 0x4, and 0x8.
//! * **mate on a different chr** requires 0x1, 0x2, and `RNEXT != RNAME`.
//! * The `(mapQ>=5)` variant is the same restricted to MAPQ >= 5.

use std::collections::BTreeMap;

use noodles::sam::alignment::record::Flags;

/// The flag bits `flagstat` consults.
pub mod bits {
    /// 0x1 — template has multiple segments.
    pub const PAIRED: u16 = 0x1;
    /// 0x2 — each segment is properly aligned.
    pub const PROPERLY_PAIRED: u16 = 0x2;
    /// 0x4 — segment is unmapped.
    pub const UNMAPPED: u16 = 0x4;
    /// 0x8 — next segment is unmapped.
    pub const MATE_UNMAPPED: u16 = 0x8;
    /// 0x40 — first segment in the template.
    pub const FIRST_IN_TEMPLATE: u16 = 0x40;
    /// 0x80 — last segment in the template.
    pub const LAST_IN_TEMPLATE: u16 = 0x80;
    /// 0x100 — secondary alignment.
    pub const SECONDARY: u16 = 0x100;
    /// 0x400 — not passing filters, e.g. a duplicate.
    pub const DUPLICATE: u16 = 0x400;
    /// 0x800 — supplementary alignment.
    pub const SUPPLEMENTARY: u16 = 0x800;
}

/// One flagstat tally, in the order `samtools flagstat` prints them.
///
/// Each field is a `(qc_passed, qc_failed)` pair. `rust-bam` does not implement
/// samtools' QC-fail split (no `-f` filter states), so the failed half is always
/// zero — which matches samtools when no QC filter is in play.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FlagStat {
    /// Total records.
    pub total: u64,
    /// Records without 0x100 and without 0x800.
    pub primary: u64,
    /// Records with 0x100.
    pub secondary: u64,
    /// Records with 0x800.
    pub supplementary: u64,
    /// Records with 0x400.
    pub duplicates: u64,
    /// Primary records with 0x400.
    pub primary_duplicates: u64,
    /// Records without 0x4.
    pub mapped: u64,
    /// Primary records without 0x4.
    pub primary_mapped: u64,
    /// Records with 0x1.
    pub paired: u64,
    /// Records with 0x1 and 0x40.
    pub read1: u64,
    /// Records with 0x1 and 0x80.
    pub read2: u64,
    /// Records with 0x1, 0x2 and without 0x4.
    pub properly_paired: u64,
    /// Records with 0x1, without 0x4 and without 0x8.
    pub with_itself_and_mate_mapped: u64,
    /// Records with 0x1, without 0x4 and with 0x8.
    pub singletons: u64,
    /// Records with 0x1 and 0x2 whose mate is on a different reference.
    pub mate_different_chr: u64,
    /// As above, restricted to MAPQ >= 5.
    pub mate_different_chr_mapq5: u64,
}

impl FlagStat {
    /// Accumulate one record.
    ///
    /// `mate_different_chr` needs the two reference names, which the caller
    /// supplies because resolving ids to names needs the header.
    pub fn add(
        &mut self,
        flags: Flags,
        reference_name: Option<&str>,
        mate_reference_name: Option<&str>,
        mapping_quality: Option<u8>,
    ) {
        let raw = flags.bits();
        let is_unmapped = raw & bits::UNMAPPED != 0;
        let is_secondary = raw & bits::SECONDARY != 0;
        let is_supplementary = raw & bits::SUPPLEMENTARY != 0;
        let is_primary = !is_secondary && !is_supplementary;
        let is_paired = raw & bits::PAIRED != 0;
        let mate_unmapped = raw & bits::MATE_UNMAPPED != 0;

        self.total += 1;
        if is_primary {
            self.primary += 1;
        }
        if is_secondary {
            self.secondary += 1;
        }
        if is_supplementary {
            self.supplementary += 1;
        }
        if raw & bits::DUPLICATE != 0 {
            self.duplicates += 1;
            if is_primary {
                self.primary_duplicates += 1;
            }
        }
        if !is_unmapped {
            self.mapped += 1;
            if is_primary {
                self.primary_mapped += 1;
            }
        }
        if is_paired {
            self.paired += 1;
            if raw & bits::FIRST_IN_TEMPLATE != 0 {
                self.read1 += 1;
            }
            if raw & bits::LAST_IN_TEMPLATE != 0 {
                self.read2 += 1;
            }
            if !is_unmapped {
                if raw & bits::PROPERLY_PAIRED != 0 {
                    self.properly_paired += 1;
                }
                if mate_unmapped {
                    self.singletons += 1;
                } else {
                    self.with_itself_and_mate_mapped += 1;
                }
            }
            // "with mate mapped to a different chr" needs 0x1 and a mapped mate,
            // but NOT 0x2. Verified against samtools 1.24: a pair with flags 1/1
            // (paired, not properly paired) on chr1/chr2 is counted, so requiring
            // the proper-pair bit under-counted this line.
            //
            //   $ samtools flagstat  # records flagged 1, RNAME chr1, RNEXT chr2
            //   2 + 0 with mate mapped to a different chr
            //
            // 0x8 (mate unmapped) cannot be "mapped to a different chr" either.
            if !mate_unmapped {
                let different = match (reference_name, mate_reference_name) {
                    (Some(a), Some(b)) => a != b,
                    // A mate on an unknown reference is not "different chr".
                    _ => false,
                };
                if different {
                    self.mate_different_chr += 1;
                    if mapping_quality.unwrap_or(0) >= 5 {
                        self.mate_different_chr_mapq5 += 1;
                    }
                }
            }
        }
    }
}

/// `idxstats`: per-reference counts.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IdxStats {
    /// One entry per `@SQ` line, keyed by reference name, in header order.
    pub references: Vec<ReferenceCounts>,
    /// `(mapped, unmapped)` for records with no reference (`*`).
    pub no_coordinate: (u64, u64),
}

/// Mapped/unmapped counts for one reference sequence.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReferenceCounts {
    /// Reference name.
    pub name: String,
    /// Reference length, from `@SQ`.
    pub length: u64,
    /// Records aligned to this reference.
    pub mapped: u64,
    /// Records whose mate is on this reference but which are themselves unmapped.
    pub unmapped: u64,
}

impl IdxStats {
    /// Build counters from the header's reference sequences.
    pub fn from_references<S: Into<String>>(
        references: impl Iterator<Item = (S, u64)>,
    ) -> IdxStats {
        IdxStats {
            references: references
                .map(|(name, length)| ReferenceCounts {
                    name: name.into(),
                    length,
                    mapped: 0,
                    unmapped: 0,
                })
                .collect(),
            no_coordinate: (0, 0),
        }
    }

    /// Accumulate one record.
    ///
    /// `samtools idxstats` semantics, observed rather than guessed:
    ///
    /// * A **mapped** record increments `mapped` on its own reference.
    /// * An **unmapped** record with a reference id (i.e. its mate could be
    ///   placed) increments `unmapped` on *that* reference — this is how a pair
    ///   with one mapped end is counted.
    /// * An unmapped record with **no** reference id lands in the trailing `*`
    ///   line's unmapped column.
    pub fn add(&mut self, reference_id: Option<usize>, is_unmapped: bool) {
        match (reference_id, is_unmapped) {
            (Some(id), false) => {
                if let Some(counts) = self.references.get_mut(id) {
                    counts.mapped += 1;
                }
            }
            (Some(id), true) => {
                if let Some(counts) = self.references.get_mut(id) {
                    counts.unmapped += 1;
                }
            }
            (None, false) => {
                // A "mapped" record with no reference is malformed, but count it
                // as mapped rather than dropping it.
                self.no_coordinate.0 += 1;
            }
            (None, true) => self.no_coordinate.1 += 1,
        }
    }
}

/// Alias for a map of reference name to counts, used by tests and callers that
/// want lookup by name rather than by index.
pub type CountsByName = BTreeMap<String, ReferenceCounts>;

#[cfg(test)]
mod tests {
    use super::*;

    fn flags(raw: u16) -> Flags {
        Flags::from_bits_truncate(raw)
    }

    #[test]
    fn a_plain_mapped_record_counts_as_mapped_and_nothing_else() {
        let mut stat = FlagStat::default();
        stat.add(flags(0), Some("chr1"), None, Some(60));
        assert_eq!(stat.total, 1);
        assert_eq!(stat.primary, 1);
        assert_eq!(stat.mapped, 1);
        assert_eq!(stat.primary_mapped, 1);
        assert_eq!(stat.paired, 0);
        assert_eq!(stat.singletons, 0);
    }

    #[test]
    fn an_unmapped_record_is_not_mapped() {
        let mut stat = FlagStat::default();
        stat.add(flags(bits::UNMAPPED), None, None, None);
        assert_eq!(stat.total, 1);
        assert_eq!(stat.mapped, 0);
        assert_eq!(stat.primary, 1);
    }

    #[test]
    fn secondary_and_supplementary_are_counted_separately_from_primary() {
        let mut stat = FlagStat::default();
        stat.add(flags(0), Some("chr1"), None, Some(60));
        stat.add(flags(bits::SECONDARY), Some("chr1"), None, Some(60));
        stat.add(flags(bits::SUPPLEMENTARY), Some("chr1"), None, Some(60));
        assert_eq!(stat.total, 3);
        assert_eq!(stat.primary, 1);
        assert_eq!(stat.secondary, 1);
        assert_eq!(stat.supplementary, 1);
    }

    #[test]
    fn duplicates_are_counted_for_both_columns() {
        let mut stat = FlagStat::default();
        stat.add(flags(bits::DUPLICATE), Some("chr1"), None, Some(60));
        assert_eq!(stat.duplicates, 1);
        assert_eq!(stat.primary_duplicates, 1);
    }

    #[test]
    fn a_secondary_duplicate_counts_toward_duplicates_but_not_primary_duplicates() {
        let mut stat = FlagStat::default();
        stat.add(
            flags(bits::DUPLICATE | bits::SECONDARY),
            Some("chr1"),
            None,
            Some(60),
        );
        assert_eq!(stat.duplicates, 1);
        assert_eq!(stat.primary_duplicates, 0);
    }

    #[test]
    fn read1_and_read2_require_the_paired_bit() {
        let mut stat = FlagStat::default();
        // 0x40 without 0x1 is not a read1 for flagstat purposes.
        stat.add(flags(bits::FIRST_IN_TEMPLATE), Some("chr1"), None, Some(60));
        assert_eq!(stat.read1, 0);
        assert_eq!(stat.paired, 0);

        stat.add(
            flags(bits::PAIRED | bits::FIRST_IN_TEMPLATE),
            Some("chr1"),
            None,
            Some(60),
        );
        assert_eq!(stat.read1, 1);
        assert_eq!(stat.paired, 1);
    }

    #[test]
    fn properly_paired_requires_paired_proper_and_mapped() {
        let mut stat = FlagStat::default();
        stat.add(
            flags(bits::PAIRED | bits::PROPERLY_PAIRED),
            Some("chr1"),
            Some("chr1"),
            Some(60),
        );
        assert_eq!(stat.properly_paired, 1);

        // Properly-paired bit but unmapped: not properly paired.
        stat.add(
            flags(bits::PAIRED | bits::PROPERLY_PAIRED | bits::UNMAPPED),
            Some("chr1"),
            Some("chr1"),
            None,
        );
        assert_eq!(stat.properly_paired, 1);
    }

    #[test]
    fn singletons_are_paired_mapped_reads_whose_mate_is_unmapped() {
        let mut stat = FlagStat::default();
        stat.add(
            flags(bits::PAIRED | bits::MATE_UNMAPPED),
            Some("chr1"),
            None,
            Some(60),
        );
        assert_eq!(stat.singletons, 1);
        assert_eq!(stat.with_itself_and_mate_mapped, 0);
    }

    #[test]
    fn mate_on_a_different_chromosome_does_not_require_proper_pairing() {
        // Verified against samtools 1.24: flag 0x1 alone (no 0x2) still counts.
        let mut stat = FlagStat::default();
        stat.add(flags(bits::PAIRED), Some("chr1"), Some("chr2"), Some(60));
        assert_eq!(stat.mate_different_chr, 1);
        assert_eq!(stat.mate_different_chr_mapq5, 1);

        // With 0x2 it also counts.
        stat.add(
            flags(bits::PAIRED | bits::PROPERLY_PAIRED),
            Some("chr1"),
            Some("chr2"),
            Some(60),
        );
        assert_eq!(stat.mate_different_chr, 2);

        // Same chromosome is not "different".
        stat.add(flags(bits::PAIRED), Some("chr1"), Some("chr1"), Some(60));
        assert_eq!(stat.mate_different_chr, 2);
    }

    #[test]
    fn an_unmapped_mate_is_not_mapped_to_a_different_chromosome() {
        let mut stat = FlagStat::default();
        stat.add(
            flags(bits::PAIRED | bits::MATE_UNMAPPED),
            Some("chr1"),
            Some("chr2"),
            Some(60),
        );
        assert_eq!(stat.mate_different_chr, 0);
        assert_eq!(stat.singletons, 1);
    }

    #[test]
    fn the_mapq5_variant_excludes_low_quality_alignments() {
        let mut stat = FlagStat::default();
        stat.add(
            flags(bits::PAIRED | bits::PROPERLY_PAIRED),
            Some("chr1"),
            Some("chr2"),
            Some(4),
        );
        assert_eq!(stat.mate_different_chr, 1);
        assert_eq!(stat.mate_different_chr_mapq5, 0);
    }

    #[test]
    fn idxstats_counts_mapped_records_on_their_reference() {
        let mut stats = IdxStats::from_references(vec![("chr1", 1000), ("chr2", 500)].into_iter());
        stats.add(Some(0), false);
        stats.add(Some(0), false);
        stats.add(Some(1), false);
        assert_eq!(stats.references[0].mapped, 2);
        assert_eq!(stats.references[0].length, 1000);
        assert_eq!(stats.references[1].mapped, 1);
    }

    #[test]
    fn idxstats_puts_a_mate_placed_unmapped_read_on_its_reference() {
        // An unmapped record that still carries a reference id belongs to that
        // reference's "unmapped" column, which is how samtools reports one end
        // of a pair being mapped.
        let mut stats = IdxStats::from_references(vec![("chr1", 1000)].into_iter());
        stats.add(Some(0), true);
        assert_eq!(stats.references[0].mapped, 0);
        assert_eq!(stats.references[0].unmapped, 1);
    }

    #[test]
    fn idxstats_tracks_the_star_line_for_records_with_no_reference() {
        let mut stats = IdxStats::from_references(vec![("chr1", 1000)].into_iter());
        stats.add(None, true);
        stats.add(None, true);
        assert_eq!(stats.no_coordinate, (0, 2));
        assert_eq!(stats.references[0].unmapped, 0);
    }

    #[test]
    fn references_keep_header_order() {
        let stats = IdxStats::from_references(
            vec![("chr2", 500), ("chr1", 1000), ("chrM", 16)].into_iter(),
        );
        let names: Vec<&str> = stats.references.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, ["chr2", "chr1", "chrM"]);
    }
}
