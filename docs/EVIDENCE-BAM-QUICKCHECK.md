# BAM quickcheck: exit masks and compressed-header boundaries

This is correctness evidence for the active `d3d8edc`-based batch, not a
performance result or a claim of complete samtools option parity. Artifacts are
under `target/surpass-d3d8edc/`; final archival delivery is still pending.

## Contract and implementation

`rust-bam quickcheck` now returns the pinned samtools 1.24 default failure mask
instead of collapsing every failure into generic runtime exit 1:

| Bit | Failure |
|---:|---|
| 2 | Open or unrecognized binary/index input |
| 4 | Not identified as sequence data |
| 8 | Missing targets or unreadable header |
| 16 | Missing required BGZF final marker or EOF-check error |
| 32 | Sticky decoder failure reported at close |

Bits are ORed across files and failures within a file. Every diagnostic is
retained in order, including header, EOF and close failures for one input. The
CLI emits those diagnostics directly, with the upstream punctuation and without
an extra generic summary. Other commands retain their shared exit classes.
The public `QuickCheck.failed` collection contains one entry **per diagnostic**,
not necessarily one entry per file. Actual OS close-error parity is not claimed;
bit 32 covers the decoder failure states exercised here.

The check reads the format and alignment header, then checks the final marker
for seekable BGZF input. It is **not a whole-record or whole-file integrity
check**. Corrupt later BAM blocks and some unfinished gzip tails intentionally
remain outside the bytes required for the header. A successful check does not
certify that downstream processing will succeed.

The input is opened once. `-` duplicates the actual stdin handle: a pipe remains
nonseekable, whereas a redirected regular file retains the final-marker check.
Unsupported CRAM and unsupported compression remain explicit usage errors;
`-v`, `-u` and other unimplemented upstream options are not added by this work.

## Proved finite-prefix false rejections

A fixed 2048-byte compressed sniff window can contain only empty BGZF members,
a partial SAM/BAM signature, or a long legal gzip filename. It is not EOF.
The implementation now continues discovery and replays decoded signature bytes
into the ordinary header reader, without scanning unrelated alignment bodies.
A short SAM prefix such as `@SQ` must not escape that continuation through the
FASTX classification branch.

Nine discriminating inputs are independently validated:

| Population | Cases | Pinned upstream exit | Corrected exit |
|---|---:|---:|---:|
| 74 or 100 empty BGZF members before valid BAM | 2 | 4 | 0 |
| Empty members between the first 1, 2 or 3 BAM signature bytes and the remainder | 3 | 4 | 0 |
| Equivalent split SAM `@SQ` signature | 3 | 8 | 0 |
| Valid gzip SAM with a 5000-byte NUL-terminated FNAME | 1 | 4 | 0 |

For **each** input, Python `gzip.decompress` and pinned `bgzip -dc` independently
validate/decode the compressed stream and yield the exact expected payload.
Pinned samtools accepts that decoded payload with exit 0 and empty stdout/stderr.
The corrected native check also returns 0 with empty stdout/stderr. The compressed
and decoded hashes, commands, streams and executable identities are retained in
`quickcheck-final-proof2/` and the source probe directories.

This agrees with the format specifications:

- [SAM/BAM specification, BGZF EOF-marker section](https://samtools.github.io/hts-specs/SAMv1.pdf):
  “Empty BGZF blocks are not otherwise special; in particular, the presence of
  an EOF marker block does not by itself signal end of file.” Its footnote says
  an embedded insignificant EOF marker should effectively be ignored.
- [RFC 1952, §2.3.1](https://www.rfc-editor.org/rfc/rfc1952):
  “If FNAME is set, an original file name is present, terminated by a zero byte.”
  It does not impose a 2048-byte filename limit.

Retrieved specification source snapshots, rather than an unrecorded current
web page, are retained:

| File | SHA-256 |
|---|---|
| `quickcheck-spec-SAMv1.tex` | `5f1326a2298b37e9e6f375456189dfb5646a285dd0adf023c6a9344fd4fbc35a` |
| `quickcheck-spec-rfc1952.txt` | `164ef0897b4cbec63abf1b57f069f3599bd0fb7c72c2a4dee21bd7e03ec9af67` |

The SAM source was retrieved from the hts-specs `master` URL, not asserted to be
the source bundled with HTSlib 1.24. Pinned executable observations establish the
behavioral comparison. No compatibility row was deleted/relabelled and no
comparison harness, expected output or protected fixture was weakened. The raw
boundary report still says `passed: false` because it preserves these differences;
a separate audit proves the nine corrections and rejects any additional mismatch.

Seventeen negative controls corrupt a header-bearing member's CRC or remove the
final BGZF marker. The native implementation rejects all seventeen; independent
gzip validation rejects all nine CRC mutations. The eight marker removals remain
valid gzip, but correctly fail the seekable BGZF marker check with exit 16.

## Gzip boundaries and review follow-up

A previous wrapper exposed a full 64 KiB decoded block before asking flate2 to
validate its trailer. At exactly 65536 and 131072 decoded bytes, corrupt CRCs
could therefore be missed even when the trailer was present. The replacement
uses the existing flate2 gzip-header parser, raw inflater and CRC helper. It
validates a present completed-member trailer before exposing the block, but does
not start a later member merely to complete that validation.

The boundary population covers 65534, 65535, 65536, 65537, 65538 and 131072 bytes,
CRC corruption, four truncation lengths, and a bad subsequent member. Process
regressions also cover ISIZE and a malformed subsequent header. Finite-file
truncation at a full decoded-block boundary is deferred until another block is
needed, matching the exercised upstream scope; this is not a statement that the
truncated stream is valid gzip.

A completed focused read-only review found two additional issues, both corrected:

1. An unconditional trailer refill could wait on a live pipe after obtaining a
   full usable decoded block. Actual regular-file metadata now permits refill;
   nonregular input validates an already-buffered full trailer or defers it until
   another decoded block is required. It does not change descriptor flags.
2. Retrying a read after decoder failure could expose bytes from the rejected
   block. Failure now discards the block and remains sticky across retries.

Tests include 1/2/17/2048/8192/65536-byte source fragments, injected `Interrupted`
reads, long gzip metadata, repeated reads after failure, and a paused source that
asserts no unnecessary refill. Twelve live runs compare upstream, the earlier
native prototype and final native behavior with 0/1/4/8 missing trailer bytes.
Both native versions finish before writer closure in all four controls; upstream
waits until closure, then returns 0. This waiting difference is explicitly retained,
not counted as exact parity. A second focused read-only review found no remaining
concrete defect in those two fixes; it did not execute tests or certify all code.

## Validation and identities

Final sealed executable:

```text
rust-bam SHA-256
5b82922b4c8edc87a0e7566d4971ce2e4b92d2acfb044d94c4e578c9addd9855
samtools 1.24 executable SHA-256
730fcc3c41bd851bfe4ff1cf8c4adf5cc48e9da01d40a1bb843fa598bcdec342
HTSlib 1.24 shared-library SHA-256
245587ef02c3ab18ec64dd2ad74e96acefe40a1ee2c9e2915d2bb42971735038
bgzip executable SHA-256
b754ec4e5cac8a48bd7719fa65aca436768e3314d3afe7bd8f79528082d2845b
```

`quickcheck-sealed/build-provenance.json` freezes the build recipe and 129 runtime
source identities. `quickcheck-full-gates.json` additionally records 12 validation
source identities, including integration tests and executed probe scripts. The
original 26 probes compare against previously captured pinned outputs; the
82-case and 40-case populations rerun the pinned executable.

| Population | Exact exit/stdout/stderr pairs | Independently proved corrections |
|---|---:|---:|
| Original quickcheck probes | 26 | 0 |
| Expanded masks, input types, indexes and stdin | 76 | 6 |
| Review boundary probes | 37 | 3 |
| Total | 139 | 9 |

`quickcheck-final-proof2/report.json` checks this complete population, all 17
negative controls and the 12 live runs. It does not derive acceptance from a
probe script's zero process exit alone.

Component validation passed 127 release tests and Clippy with warnings denied.
The full gate driver completed both required repository gates and the sealed
20-case BAM oracle. `quickcheck-full-gates.json` records all eight steps with
return code 0 and unchanged source identities. Actual final log lines were:

```text
QUALITY PASS (base only; external oracles NOT RUN)
QUALITY PASS (base + FASTX/QC oracles)
FULL GATES PASSED; SEE SEPARATE DIFFERENTIAL CORRECTION AUDIT
```
No speed measurement or 10% superiority claim follows from these tests.

## Dependencies, resources and preserved failures

No new dependency or compression feature is retained. An explicit flate2
`zlib-rs` feature prototype was withdrawn because it changed the isolated
`rust-bam` backend. The complete isolated dependency/features listing matches
the pre-quickcheck source after normalizing only local absolute paths; see
`quickcheck-feature-audit.json`. Existing workspace feature unification still
applies, so isolated and workspace-built binaries are validated separately.
There is no new runtime C/HTSlib dependency.

The implementation retains a 2048-byte raw sniff prefix, at most 1024 replayed
signature bytes, and a 64 KiB gzip output block, plus decoder buffers. **Total
memory is not capped at 64 KiB**: header text, reference maps, optional gzip
metadata and accumulated per-file diagnostics remain input-dependent. Long empty
member prefixes may require linear scanning before discovering a header.

Adapted HTSlib sniffing logic carries its MIT notice in the source and
`docs/licenses/htslib-MIT.txt`. The explanatory `hts.c` inspected was `develop`;
no claim of a pinned source hash is made.

All prototypes and failed probes are preserved, including the missing temporary
reference cache, withdrawn feature graph, failed Clippy run, and the first
negative-control proof. That proof accidentally corrupted a later record-only
block and asserted rejection; quickcheck correctly accepted it. The next proof
mutates the first nonempty header-bearing block instead. The failed assertion is
retained in `quickcheck-final-proof/failure.md`; it is not hidden or labelled as a
runtime failure. Protected `rust-umitools/` source is unchanged.
