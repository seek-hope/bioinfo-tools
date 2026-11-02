# Complexity and implementation analysis

The analysis order was complexity, serial implementation cost, then concurrency.
No direct dependency, public CLI option or compatibility status changes here.

For statistics, let B be total FASTX input bytes (including headers/separators),
N records and D distinct lengths. Reading and counting remains O(B); maintaining
the ordered length histogram is O(N log D). State is O(D + largest record/input
buffer), plus constant lookup tables. Repeated equal-length benchmark input has
D1 and does not exercise the worst-case histogram bound. Scanning every byte is
still necessary for these statistics; the packed counters do not reduce big-O.

For sorting, let C be records per estimated-memory chunk, R initial runs and F
the fan-in limit. Chunk comparisons cost O(N log C); each merge level does
O(N log F) key comparisons and reads/decodes/compresses its records. Key comparison
itself can inspect name bytes. The number of merge levels remains about log_F R.
The new selector builds in O(k), replacing k individual heap pushes, and replaces
one head in O(log k), where k is the original number of slots in that merge.
It moves only indices and performs at most one key comparison per level. Record,
SortKey and frame Vec handles stay in fixed slots; this avoids moving those
objects through the heap, not copying sequence payload bytes that were already
stored out of line. Equal keys still choose the earlier run.

The selector has fewer than4k usize entries for nonempty k, plus the existing
run-head/reader state. Its fixed height does not shrink when runs finish. Empty
children require no key comparison, but the path is still updated. The chunk
option and64KiB added capture copy limit are not whole-process RSS caps. Headers,
record fields, reader/decoder buffers, retained capacities and open-run state
remain part of the memory bound.

Callgrind on matching100k inputs and CPU4 gave these instruction counts:

| Workload | Baseline | Serial candidate | Reduction |
| --- | ---: | ---: | ---: |
| Real stats -a -T |563983731|153092893|72.86%|
| Synthetic stats -a -T |560938463|150047438|73.25%|
| Sort,1MiB,1worker |2103324121|1976122580|6.05%|
| Flagstat, unchanged path |164865213|164865740|approximately unchanged|

The original stats callback accounted for93.35% of instructions. It repeatedly
classified bytes and counted quality thresholds. SequenceCounter now precomputes
GC/AT/N-or-X/gap contributions as four16-bit lanes; QualityDecoder precomputes
Q20/Q30 as two lanes. Summing at most65535 contributions before extracting counts
prevents cross-lane carry, even if every lane is set for every byte. Arbitrary gap
membership, all256 offsets and overlapping categories retain the old semantics.
Error probabilities still add in byte order into one accumulator across chunks,
then merge in the original per-record order; there is no histogram/multiplication
replacement for floating-point addition.

The original heap selection/movement accounted for about6.8% of the sort
diagnostic instructions. Compression, decoding, byte conversion and allocation
remain substantial. This bounds what a selector-only change can achieve on that
profile. Inclusive call-graph percentages overlap; do not add them as self costs.
Instruction counts under instrumentation are not elapsed-time speedups.

Final merge still refills before emitting the chosen record; intermediate merge
still writes its record/frame and sidecar index before refilling. Full decoding,
index EOF behavior and global input-index tie ordering remain unchanged. Flagstat
keeps the full decoder and reference-name equality contract.

The parallel temporary-compression prototype was evaluated after these serial
changes, passed output checks and was rejected by its timing screen. Accepted
concurrency therefore remains unchanged: FX statistics are serial even with -j;
BAM sort's workers compress only final BAM output. Temporary creation/merging,
chunk sorting and SAM output remain serial.
