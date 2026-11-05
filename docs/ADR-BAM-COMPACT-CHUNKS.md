# Compact initial BAM sort chunks (candidate)

This candidate changes the storage of initial chunks for BAM output. It keeps the
existing full input decoder, `record_memory` accounting, original run boundaries,
merge fan-in, intermediate validation, refill order and compression levels.
SAM output keeps the owned-record path. No dependency is added.

Small successfully encoded records are retained as framed, uncompressed BAM bytes
in a chunk arena, with their complete sort key. The encoder is the same ordinary
noodles encoder and quality adapter used at emission. The decoder can reuse one
RecordBuf while subsequent reads are loaded. Sorted spill/output forwards those
already normalized bytes. This does not forward raw public-input frames.

Encoding failures are not reported during loading: the original RecordBuf is
retained in a box and passed to the ordinary writer at the original sorted
emission stage. This preserves precedence against later parse errors, output
creation failures, earlier sorted records and spill boundaries. Large records also
use this fallback. A capped sink discards generated frames above 64 KiB before
copying their bodies, and a failed/oversized private encoder is discarded.

A later direct-append experiment moved the arena into this sink to eliminate
one per-frame copy. It preserved rollback and deferred errors, but the completed
three-arm elapsed screen did not establish stable whole-command gains across
CPU budgets. That extension and its histogram-scan companion were withdrawn;
the separate capped frame and arena remain the active implementation. Correctness,
instruction, elapsed and rejected-prefix capacity evidence is preserved in
[EVIDENCE-BAM-SORT-BUFFERS.md](EVIDENCE-BAM-SORT-BUFFERS.md). The historical
compact-chunk measurements below are unchanged.

The input and final headers retain identical reference sequences. The final
header changes `@HD` sort metadata, including removal of stale `GO`/`SS` fields;
the separate header correction also preserves comments previously lost by the
header builder. The pinned record encoder consults the header for reference
identifiers, so these metadata changes do not change record bytes. Header output
itself is still emitted by the ordinary writer. In-memory final output must clear
its frame when selecting an owned fallback.

The 64 KiB limit bounds the added frame copy, **not** peak encoding or total memory.
The ordinary encoder can construct a large private buffer before the capped sink
sees its length. Input readers retain their own largest buffers. Memory includes:

- the largest retained arena capacity across chunks, with Vec growth slack;
- entry and sort-key/name capacities, owned fallback records and their heaps;
- one reusable decoded record and the decoder's input buffers;
- the private encoder and capped sink (including transient oversized encoding);
- index/radix scratch, run readers and compression buffers in their active phases.

Legacy chunk accounting remains an estimate and is not a total RSS cap. The
algorithmic merge-level bound is unchanged because this candidate deliberately
keeps the original record counts per run. The exploratory 16-invocation screen
matched all original output bytes. Random 10M-record inputs used about 292 MiB
maximum observed RSS instead of about 715 MiB, and separate one/four-core paired
before/current medians were 1.1236/1.2153. The wider comparison passed 192 valid,
malformed and nonfinite-value invocations. These results belong to the preserved
compact source and predate the separate header correction. Formal upstream
acceptance remains pending; no comprehensive-superiority claim is made.
