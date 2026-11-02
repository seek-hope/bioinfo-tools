# Bottleneck analysis after 21bc596

User requested the order: algorithmic complexity -> implementation efficiency ->
parallel processing. Do not begin with more workers. Historical scale evidence
remains immutable; new candidates and profiles go in this directory.

1. Complexity/data structure: FASTX stats is O(B + N log D) for B sequence/quality
   bytes, N records, D distinct lengths, with O(D + maximum record/buffer) state.
   It must inspect input and preserve ordered per-read error-probability sums.
   BAM external sorting uses O(N log C) chunk comparisons and merge work across
   O(log_F R) levels (C chunk records, F open-run limit, R run count). Existing
   BinaryHeap pop/push moves MergeItem (owned record/key/frame) along heap paths.
   Candidate: fixed run heads plus an index tournament tree; one root-to-leaf
   update, at most ceil(log2 R) key comparisons, no record moves within selector.
   Asymptotic comparison order remains O(log R); compare constants, data movement
   and actual pass counts explicitly. Keep full decoder and final/intermediate
   read/write order. Preserve malformed index/record behavior and total keys.
2. Implementation: profile per-base stats, decoding, allocation, comparisons,
   compression and syscalls. Preserve arbitrary gap bytes, quality offsets and
   ordered floating-point additions. Do not replace full BAM validation by raw
   field access. Do not introduce an approximation or modify oracle fixtures.
3. Parallel processing: only revisit after profiling/serial improvements, with
   measured CPU utilization and same-input correctness. Current native FX and
   flagstat are serial; sort parallelizes final BAM compression only.

Host read-only resource probe: 20 logical CPUs/14 physical cores, affinity20;
RAM available about18.0GB, disk user-available231GB. Continue using CPU4 for
single-core profiles and explicit 4;4,6;0,2,4,6 for later budgets. Do not change
clocks or caches. Small callgrind fixtures identify instruction costs, not formal
runtime claims. Larger calibrated trials must follow selected implementations.

Baseline HEAD: 21bc59628b3d0ecfc5f32d8218273b1ece992b71. Prior sealed binaries
are available in target/scale-benchmark/sealed and match this commit's native
sources. Preserve their hashes and source provenance before edits.
