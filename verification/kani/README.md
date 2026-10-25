# Kani harnesses (targeted, bounded)

Kani is not part of the fast gate. The implemented harness set is deliberately
small and bounded; it is not a proof of the full tools.

Implemented (`src/lib.rs`, 5 harnesses):

- `bioinfo_fastx::chunker::last_complete_record_end`: the returned boundary is
  within the input.
- `rust_fx::split::parts_for_size`: the part count is bounded by the record
  total for a small domain.
- `rust_bam::sort_order::SortKey`: antisymmetry over a small key set.
- `rust_fx::table::n50`: the N50 value is bounded by the largest length and L50
  by the number of records.
- `rust_fx::table::n50_from_histogram`: the empty histogram returns `(0, 0)`.

Each harness declares a small input bound (at most four records, eight bases, or
a comparable finite domain). These are bounded proofs, not universal proofs.

Not covered by Kani: parser malformed-input behavior, allocation/overflow beyond
the listed bounds, UMI workflows, BAM index/region queries, FastQC module
semantics, and I/O errors. Miri and proptest provide separate evidence; the
differential oracle remains the compatibility authority.

The full verification gate must fail clearly when Kani is absent rather than
silently skipping: `bash tests/verify/check.sh --full`.
