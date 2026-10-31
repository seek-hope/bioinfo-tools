# SeqKit translate compatibility closure

**Superseded intermediate evidence:** the maintainer subsequently chose not to
reproduce proven upstream defects. The current implementation corrects strand,
ORF, trim and header defects. Its raw comparison is 710/756 plus 46 explicit
reviewed exceptions, with independent correctness checks. See
[EVIDENCE-UPSTREAM-DEFECTS.md](EVIDENCE-UPSTREAM-DEFECTS.md). The 756/756 result
below describes the intermediate implementation only, not the current behavior.

This is evidence for the uncommitted implementation based on `5b71fae`; it is not
release or performance evidence. No upstream fixtures, existing oracle expectations,
Cargo dependencies or UMI implementation files were changed.

## Direct comparison

Reference: `seqkit v2.14.0`. Source inspection used SeqKit tag `v2.14.0`
(`facf0f7120483f9e39725c66dd83f5f14e048afd`) and its pinned bio `v0.15.2`
(`c289cafbaceafb27ddf7d2d9b422bf091abb61a6`).

```text
$ python3 tests/oracle/seqkit_translate_diff.py --rust target/release/rust-fx
# Before rebuilding: the release binary from the previous batch.
translate: 248/740 direct cases passed
$ cargo test -p rust-fx --test translate --test translate_errors --locked
test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
$ python3 tests/oracle/seqkit_translate_diff.py --report /tmp/bioinfo-translate-report.json
translate: 756/756 direct cases passed
```

The previous binary fails 492 cases in the initial 740-case suite. This is a
negative control: the new comparison distinguishes the missing behavior and does
not merely accept either implementation. Review then identified an additional
wrapped FASTQ boundary failure: extra quality text after an already matching
quality line could commit a malformed record before rejection. The shared SeqKit
reader now checks the next header/EOF before yielding the record. Sixteen new
boundary cases and a large-record Rust regression cover this fix; the final suite
has 756 cases. All cases generate temporary data and execute both binaries.
See `compat/README.md` for comparisons, deliberate upstream quirks and remaining
global-option/diagnostic boundaries. The new differential runs in the optional
FASTX/QC quality gate against the freshly built release binary.

## Quality gates

The final reader implementation passed both required gates. Actual output:

```text
$ bash tests/ci/check.sh
QUALITY PASS (base only; external oracles NOT RUN)
$ bash tests/ci/check.sh --with-fastx-qc-oracles
50/50 cases passed
translate: 756/756 direct cases passed
22/22 fixtures passed
PASS  multiqc            parses our fastqc_data.txt without warnings
SELFTEST PASS
QUALITY PASS (base + FASTX/QC oracles)
$ python3 tests/ci/test_check.py
Ran 12 tests in 1.398s
OK
```

Both gates include locked release builds/tests, fmt, warnings-as-errors clippy,
rustdoc, documentation/matrix validation and the cutadapt visible-fixture smoke.
The new direct suite passed against the freshly built release binary as well as
the debug binary. Final code has no new dependency or runtime reference-tool call.

The shared reader change was also checked against the existing streaming suite:

```text
$ python3 tests/oracle/seqkit_streaming_diff.py
FAIL SeqKit 2.14.0 range: 203/206 differentials
PASS SeqKit 2.14.0 tab2fx: 99/99 differentials
```

That command exits 1 solely for the three pre-existing mixed-format range cases.
No expected outputs or comparison rules were changed. Targeted range and stdin
streaming process tests passed (6 range tests and 2 streaming tests).

## Matrix and remaining scope

```text
PASS compatibility matrix: 264 rows, 5 tools, 36 upstream snapshots
status: {'verified': 193, 'experimental': 10, 'unsupported': 61}
```

Only `seqkit:translate` changed status. Other experimental and unsupported rows
remain open. In particular, the mixed FASTQ/FASTA `range` pooled-quality behavior
requires a compatibility decision; its known failures were not normalized away.

The preceding pushed commit's GitHub run `41178035242` completed successfully
(`native` and `dependency-policy`). That run predates these changes and is not
validation of this working tree.
