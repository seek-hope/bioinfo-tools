# Parallel input decompression for regular-file BAM sort (retained provisionally)

The compact sort baseline decompresses its initial input serially.
The retained candidate uses the already pinned, pure-Rust noodles BGZF multithreaded reader
for regular BAM files when sort receives more than one thread. It adds no package,
feature or direct dependency and retains full BAM `RecordBuf` decoding.

The reader receives `threads - 1` decompression workers, alongside the main record
decoder and the library's input thread. The same library frame parser checks BGZF
header, size, inflation and CRC; per-block result receivers are consumed in input
order, including errors produced by the input thread. Chunk storage/accounting,
spills, merge scheduling and final output compression are unchanged.

Selection inspects the metadata of the actual reopened file handle. Standard
input and SAM retain serial input, as do handles not identified as regular files.
The BGZF multithreaded reader joins its input thread on drop. Reading ahead on a
blocking pipe could otherwise prevent an earlier record/output error from
returning. Public `AlignmentReader::open` retains its serial behavior; only the
private sort reader selects the new path. Both reader variants use the same full
header/record decoding and error wrappers. The reader is released after loading,
before final output or merging, to release its worker pool and buffers.

The pinned BGZF implementation circulates `workers + 2` buffers, with compressed
frames limited to 65,536 bytes and validated uncompressed size at most 65,536 bytes.
Accounting must also include the current consumer buffer, channel/tasks/pool
allocations, input buffer, thread stacks and the full BAM record decoder's largest
record buffer. This is additional memory, not a new bound on total sort RSS.
Thread creation and scheduling remain costs to measure; no speed claim follows
from enabling workers. Existing thread-construction resource failures in noodles
are not converted into a new compatibility promise by this candidate.

Direct differential tests compare every decoded record, header and first error
against the serial reader. They include large records spanning BGZF blocks,
interior empty blocks, earlier CRC versus later header/frame-read failures,
oversized ISIZE, truncated bodies/headers and BAM decode errors preceding a later
CRC error. Selection tests cover zero/one/multiple sort threads and SAM input.
The first candidate passed 16 complete-output screening observations and 72 CLI
before/after comparisons, including a live stdin failure check. Its two-block,
no-warm-up screen against `header-sealed/` had a median baseline/candidate ratio
of 1.0341311675108078 at four cores. This is a native before/after exploratory
result, not an upstream comparison or formal acceptance.

Those checks exposed a shared pre-existing partial-final-header bug. The candidate
was temporarily removed while that bug was corrected and validated separately;
see [BGZF header evidence](EVIDENCE-BAM-BGZF-HEADERS.md). It is now reapplied on
`bgzfstrict-sealed/`, with both paths retaining the strict source adapter and
new parallel differential cases for every partial final header length. The
second source passed 119 release BAM tests, all-target Clippy, 72 CLI pairs and
the live stdin failure check, both full repository gates, and 20/20 sealed BAM
oracle cases. These results are recorded in `readparallel2-*`.

The completed `readparallel2-large/` screen compared that second source with
`bgzfstrict-sealed/` on 86 million records, using two balanced paired blocks at
two and four cores. All eight complete output files, diagnostics, temporary-file
checks and frozen identities passed. Median paired baseline/candidate elapsed
ratios were 1.0558597837482049 (two cores) and 1.0499223220897407 (four cores).
Every individual paired ratio exceeded one. The resource-report audit found no
failures or observer errors; maximum sampled aggregate RSS across all invocations
was 321,732,608 bytes, maximum logical file usage was 2,108,168,189 bytes, and
maximum allocated file usage was 2,108,571,648 bytes. Minimum observed filesystem
headroom was 131,949,195,264 bytes. These are sampled observations, not OS caps.
Inputs and outputs retained from earlier invocations are outside each invocation's
file-accounting workspace; filesystem headroom observes their occupied space.

`readparallel2-retention.json` records provisional retention and all eight resource
summaries. The screen has no warm-up, calibration or upstream arm and establishes
no formal superiority result. The first candidate's screening ratios remain tied
to its earlier source. Fresh calibrated upstream comparisons and the expanded
per-condition acceptance gate remain required for the final combined source.
