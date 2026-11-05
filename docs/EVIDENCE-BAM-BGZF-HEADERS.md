# Reject partial final BGZF headers

The pinned BGZF reader previously accepted a file ending with 1–17 bytes of a
new BGZF header. Its frame reader handles `UnexpectedEof` during the 18-byte
header read as clean EOF, including a partially consumed header. This affected
the existing serial BAM reader and the first parallel-input candidate.

The executed reproducer contains 2,048 valid records followed by 17 bytes of the
terminal BGZF member. Both native seals returned success; samtools 1.24 returned
1 with `Failed to read BGZF header` and `truncated file. Aborting`. The original
commands, input and diagnostics remain in
`target/surpass-d3d8edc/readparallel-errors/partial-trailing-header/` and
`partial-header-upstream/`. No protected fixture or comparator was changed.

## Implementation

`rust-bam/src/bgzf_source.rs` tracks compressed frame boundaries below the
existing buffered reader. It copies each 18-byte header and skips the declared
body by length. When the underlying source actually returns EOF with a partial
header pending, it returns `InvalidData: truncated BGZF header`.

Positive reads are returned unchanged. The adapter does not report errors while
prefetching, require an EOF marker after complete members, decode records, or
replace the normal BGZF header/DEFLATE/CRC and full BAM field validation. Invalid
frame sizes disable boundary tracking so the existing parser reports that
earlier error. Truncated bodies retain their existing error behavior. A read
with an empty destination does not imply EOF.

The adapter is used by ordinary BAM file/stdin reading and index construction.
It adds a fixed 18-byte header and scalar state; body processing advances over
slices without scanning every compressed byte. Existing record, sort, codec and
per-reference allocations still determine total memory. Limited reads, including
`head`, do not acquire a new obligation to consume the rest of the input. Indexed
region queries retain their existing bounded-read behavior.

Sort and index construction reject the demonstrated truncations before creating
the final output. Commands that have already emitted records or depth rows may
retain that prefix and exit nonzero, following their existing partial-output
policy. Existing native error classes are retained; these checks do not claim
that every native diagnostic or exit number matches samtools.

## Executed evidence

The corrected source is sealed in `bgzfstrict-sealed/`, with its 127 Rust/source
identities copied to `bgzfstrict-source/`. Evidence paths below are relative to
`target/surpass-d3d8edc/`.

- Four new component tests include all 17 partial-header lengths at seven input
  chunk sizes, with the unwrapped reader as a negative control. They also check
  complete/missing/concatenated EOF members, truncated bodies, earlier checksum,
  header and frame-size errors, and empty reads.
- `bgzfstrict-validation/report.json`: all 34 truncations (17 lengths replacing
  the final member and 17 lengths after a complete final member) are rejected by
  samtools and the native implementation for `view`, `view -c`, `flagstat`,
  `depth`, `coverage`, one/four-thread sort, BAI construction and CSI construction.
  Python's independent gzip reader also rejects every incomplete member.
- The same report records 27 before/after comparisons of complete streams,
  complete streams without a final EOF marker, and streams with an interior empty
  member. Complete outputs, diagnostics, exits and temporary-file cleanup match.
  Limited `head` output matches on all 34 malformed tails. A live stdin test
  completes a pending header later and produces the same BAM as the file case.
- `bgzfstrict-idxstats/report.json`: `idxstats` also rejects all 34 inputs in both
  implementations. Together these checks cover 340 malformed-input/command
  combinations, each run with both tools.
- `bgzfstrict-tests.log`: 117 release BAM tests passed. All-target Clippy with
  `-D warnings` passed in `bgzfstrict-clippy.log`.
- `bgzfstrict-gates.json`: both `bash tests/ci/check.sh` and
  `bash tests/ci/check.sh --with-fastx-qc-oracles` returned 0; source and auxiliary
  validation identities remained stable after every stage.
- `bgzfstrict-bam-oracle.log`: `20/20 cases passed`, using the sealed native
  executable and pinned samtools 1.24.

These are correctness results. They are not a formal performance comparison or
a claim of complete samtools CLI compatibility.

## Separate open observation

`bgzfstrict-extra/` records a pre-existing `quickcheck` discrepancy on the same
truncated input: the native CLI returns 1 and adds a summary diagnostic, while
samtools returns 16. The native library exposes an exit-code method, but the CLI
currently converts its failure to the shared runtime class. This correction does
not alter that path. Its error-class/diagnostic contract still needs a separate
review with the other quickcheck failure classes; it is not a new approved
compatibility exception.
