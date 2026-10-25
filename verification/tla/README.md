# TLA+ protocol models

These are design-level models, not code proofs. They are checked only in the
full/nightly job when `tlc` is available.

- `CutadaptPipeline.tla`: bounded ordered handoff from trim to write; invariant
  `NoLoss` and `BoundedQueue`.
- `BamSort.tla`: chunk/read -> spill -> merge; invariants `NoLoss` and
  `NoDuplicate`.

The models do not claim compression, format or biological correctness; the
differential oracle remains the compatibility authority.
