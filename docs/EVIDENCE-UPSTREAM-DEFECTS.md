# Proven upstream defects and approved native-runtime exceptions

This records the maintainer's decision: “错误行为不复现，但需给出确凿的证据说明上游行为有误而我们的实现是正确的；2.A。”
The second choice retains a pure Rust FastQC implementation with explicit JVM
launcher exceptions. These are deliberate compatibility differences, **not full
upstream-equivalence passes**. No upstream executable is called at runtime.

The matrix uses a separate `exception` status. Its finite registry is
[`compat/approved-exceptions.json`](../compat/approved-exceptions.json). Every
exception needs an explicit rule, proof document and executable verifier; an
unregistered exception or a silently relabelled verified row fails the matrix gate.
Unimplemented scientific capabilities remain `unsupported`.

## Reference and reproduction

- SeqKit **2.14.0**, source commit
  [`facf0f7120483f9e39725c66dd83f5f14e048afd`](https://github.com/shenwei356/seqkit/tree/facf0f7120483f9e39725c66dd83f5f14e048afd).
- Its bio dependency **0.15.2**, source commit
  [`c289cafbaceafb27ddf7d2d9b422bf091abb61a6`](https://github.com/shenwei356/bio/tree/c289cafbaceafb27ddf7d2d9b422bf091abb61a6).
- Independent FASTQ validator: **fq 0.12.0 (51779f50e 2024-07-08)**.
- FastQC **0.13.0**, pinned launcher help in `compat/upstream/fastqc.help.txt`.

```bash
cargo build -p rust-fx -p rust-fastqc --locked
python3 tests/oracle/check_seqkit_exceptions.py \
  --evidence-dir /tmp/bioinfo-approved-exceptions
cargo test -p rust-fastqc --test native_runtime --locked
```

The verifier retains full raw logs, observation hashes and minimal examples in
that directory. The same result was reproduced with the final release build:

```text
PASS SeqKit corrected semantics: 434 checks
translate raw: 710/756 exact; 46 reviewed differences with matching hashes
range raw: 203/206 exact; 3 approved quality-provenance differences
tab2fx raw: 99/99 exact
PASS approved exceptions: independent correctness + unchanged raw comparisons
```

The original exact differential programs still return **1** for these differences.
Their comparisons, expected outputs and failure conditions are unchanged. The
translate report only gained observation hashes. A separate verifier requires
both independent correctness checks and exact correspondence with the reviewed
46-case observation manifest. A new mismatch, changed payload/hash or a vanished
exception fails and requires investigation; the verifier never regenerates the
manifest. Range's pooled quality contents can vary, so its candidate is checked
against typed input records, not arbitrary upstream quality bytes.

## 1. `range`: qualities without a source

Minimal inputs:

```text
# reads.fq
@q
ACGT
+
JKLM

# reads.fa
>f
TGCAAC
```

Run each binary with `range -r 1:-1 -w 0 reads.fq reads.fa`. Observed SeqKit output:

```text
@q
ACGT
+
JKLM
@f
TGCAAC
+
JKLM
```

The second input has **no quality information**. The output nevertheless assigns
it the first read's `JKLM` quality string. Moreover, six sequence bases have only
four quality characters. This is a format violation, not a preferred ordering or
a different valid encoding. The independent validator reports:

```text
fq lint upstream.fastq
[S005] ConsistentSeqQualValidator: sequence-quality scores lengths mismatch: expected 6, got 4
```

The validator exits 1. It accepts the original FASTQ control with exit 0. Across
eight repeated minimal runs the upstream output is rejected; the quality buffer
may instead be empty when a different pool object is used, which is still invalid
and is rejected specifically as S004 (`empty quality`). Each run checks the
observed output fields and requires the corresponding S004/S005 diagnostic.
A wider 96-case matrix tests two qualities, equal/unequal sequence lengths, four
ranges, three wrapping widths and both file orders. In the recorded run, upstream
violates input-record provenance in **48 cases**, including **4 observed exact
stale-quality copies**. The copy count is observational and can vary with pool reuse.

Rust emits:

```text
@q
ACGT
+
JKLM
>f
TGCAAC
```

Correctness is checked without deriving expected data from Rust: select the
requested records from the typed source lists, retain sequence/name/quality and
format, and render at the requested width. It must also satisfy
`range(file1, file2) == range(file1) + range(file2)` for these per-file selections.
All 96 provenance and 96 composition checks pass. Mixed input produces a
concatenation of typed FASTA/FASTQ records; this is **not** a claim that the whole
mixed stream is a homogeneous FASTQ file. Consumers requiring one format should
process the input formats separately; quality cannot be inferred for FASTA.

Source mechanism:

- [`range.go`](https://github.com/shenwei356/seqkit/blob/facf0f7120483f9e39725c66dd83f5f14e048afd/seqkit/cmd/range.go#L147)
  sets process-global `ForcelyOutputFastq` on a FASTQ input and does not clear it
  for the following FASTA input.
- [`reader.go`](https://github.com/shenwei356/bio/blob/c289cafbaceafb27ddf7d2d9b422bf091abb61a6/seqio/fastx/reader.go#L86)
  resets the reader's quality buffer but not the pooled record's quality slice;
  the FASTA assignment updates sequence fields without clearing record quality.
- [`records.go`](https://github.com/shenwei356/bio/blob/c289cafbaceafb27ddf7d2d9b422bf091abb61a6/seqio/fastx/records.go#L118)
  uses that flag/quality slice to emit FASTQ.

## 2. `translate`: incomplete reverse complement

| Input and command | SeqKit 2.14.0 | Correct Rust output | Independent reason |
| --- | --- | --- | --- |
| `YTC`, `translate -f -1` | `D` | `E` | Y is C/T. CTC and TTC reverse-complement to GAG and GAA; both translate to E. |
| `UUG`, `translate -f -1` | `L` | `Q` | Reverse complement is CAA, coding Q; RNA UUG must agree with DNA TTG. |

The upstream translation library uses the non-redundant DNA complement table
and ignores lookup errors, leaving U and ambiguity letters unchanged. This breaks
strand symmetry. Rust uses the existing complete IUPAC complement implementation.

The independent test expands all **3375 IUPAC codons** for each of the **24 tables**,
three reverse frames and both initiation modes: 144 batch checks. Negative
translation is compared to upstream **positive** translation of an independently
complemented input. This tests the correction through a separate path. Twelve
additional RNA cases require equivalence after U/T normalization. The concrete
YTC proof independently checks both fully specified codon expansions.

## 3. Reverse ORF endpoint is off by one

For input sequence `TTAGGGCAT` (an input sequence, not a genome assembly), run
`translate -s -f -1`:

```text
SeqKit: >r_frame=-1_begin=3_end=9
        MP
Rust:   >r_frame=-1_begin=4_end=9
        MP
```

Coordinates are **1-based inclusive on the input sequence**. Two amino acids
require six nucleotide positions. `3..9` contains seven; `4..9` contains six.
The input slice `GGGCAT` reverse-complements to `ATGCCC`, which translates to
exactly `MP`. The upstream STOP-terminated branch includes an extra nucleotide;
its terminal-ORF branch does not.

The test computes ORFs from positive translation of independently oriented input,
projects each half-open nucleotide span back onto the original sequence, and
compares complete emitted records. Every interval must have
`end - begin + 1 == 3 * amino_acid_length`. Six frame-batch checks cover 71 source
sequences, including consecutive stops, terminal ORFs and truncated codons.

## 4. Right-trim contradicts the documented operation

Upstream help states that `--trim` removes X and STOP characters “from the right
end of the translation”. For `ATGNNNCCC`, untrimmed translation is `MXP`; there is
nothing to trim at its right end. SeqKit emits `M`, deleting internal X and the
following proline. Rust emits `MXP`. The test derives the expected result from
untrimmed upstream translation followed by a right-end strip of X/STOP, covering
all 24 tables with and without `--clean` (48 checks). Errors after internal X/STOP
are no longer hidden by premature termination; the full sequence is validated.

## 5. Description characters are deleted

For `>r  description` and sequence `ATG`, `translate -F` emits:

```text
SeqKit: >r_frame=1 escription
Rust:   >r_frame=1 description
```

Adding a second header separator must not delete the first description letter.
The upstream parser increments its cursor both inside and at the end of its loop.
Rust consumes separators once and preserves description bytes. Fifteen cases
vary separators and header-producing modes; they compare with the same description
under a single separator. No Unicode replacement is introduced.

## 6. FastQC stays native Rust

`-j/--java` selects an upstream JVM executable; `--debug` prints information from
that launcher. These are not biological algorithms and are not upstream bugs.
The maintainer explicitly chose native Rust instead of delegating these paths to
Java/FastQC. Both options are recognized and rejected before input/output access,
with exit 2 and a diagnostic explaining that no JVM is launched. They are never
silently accepted as ineffective options.

`rust-fastqc/tests/native_runtime.rs` verifies both aliases and debug rejection,
no output artifacts, and successful ordinary QC with an empty executable PATH
and a nonexistent JAVA_HOME. Actual result: **2/2 tests passed**.

## 7. FastQC adapter boundary and configuration defects

The native `-a/--adapters` implementation has a separate
[44-case evidence set](EVIDENCE-FASTQC-ADAPTERS.md): 35 exact comparisons, six
independently checked counting corrections and three empty-list rejections.
Upstream loses complete equal-length adapter matches, and merely reversing the
two reads `AGATCGGAAGAG` and 25 T bases changes its reported rate from 0% to 50%.
Rust reports the mathematically required 50% in either order. Mixed adapter
lengths expose a second order-dependent loss when later reads expand the report
range; first-hit histograms now preserve those counts too. An empty adapter
list causes an upstream array-index exception during report creation; native
validation fails before reading input or producing artifacts. These are explicit
`FASTQC-ADAPTER-CORRECTNESS` differences, not byte-equivalence passes.

## Negative controls and validation boundary

The independent suite was run against the preceding release binary that copied
the upstream translation quirks. It exits 1 at `strand/table-frame-init`, proving
that upstream-equivalent output does not satisfy the corrected contract.
The independent fq validator accepts a valid input and rejects the malformed
upstream output specifically with S005 (length mismatch) or S004 (empty quality),
matching the observed defect, rather than merely failing to launch.

Exception-gate negative controls additionally reject an exit-42 crash under an
approved range label, changed output/quality, a changed upstream failure and a
reversed input order. Reports are removed before every child run, and a regression
control proves an aborted comparison cannot reuse a previous report. All six
validator controls and all four matrix-policy controls pass.

Final local validation after the adapter histogram and CR-line fixes ran:

```sh
bash tests/ci/check.sh
bash tests/ci/check.sh --with-fastx-qc-oracles
```

Both exited **0**. Their actual final lines were:

```text
QUALITY PASS (base only; external oracles NOT RUN)
QUALITY PASS (base + FASTX/QC oracles)
```

The gates include fmt, warnings-as-errors clippy, locked release tests/builds,
rustdoc, documentation and matrix checks, exception policy/validator controls,
and the cutadapt visible-fixture smoke. The fresh release oracle run reported
**50/50 FASTX**, **434 SeqKit correctness checks**, the raw **710/756 translate**,
**203/206 range**, **99/99 tab2fx** results above, **22/22 QC** plus MultiQC, and:

```text
PASS FastQC adapters: 44 cases; {'exact': 35, 'approved-counting-difference': 6, 'approved-config-rejection': 3}
SELFTEST PASS
```

Release regressions include 17 translation tests, six adapter tests and two
native-runtime tests. The separately run CI harness tests passed 12/12.
The full unrelated UMI, samtools and cutadapt differential suites were not rerun
for this batch; their older evidence is not presented as a current run.

Logs: `/tmp/bioinfo-policy-adapters-final-base.log` and
`/tmp/bioinfo-policy-adapters-final-oracles.log`, with matching `.status` files
containing 0. These results apply to the uncommitted working tree based on
`5b71fae`, not a new remote CI run or release. The matrix has 266 rows: 189
verified, 9 experimental, 60 unsupported, 8 exception. The two added Kmer/tile
rows expose previously narrative-only gaps; no prior matrix row was removed.

No performance improvement is claimed. Memory remains O(longest record) plus
codec/output buffers for translation and positive range; negative ranges retain
the selected tail record bytes as documented previously.

## References

- [NCBI SRA file format guide](https://www.ncbi.nlm.nih.gov/sra/docs/submitformats/):
  FASTQ carries per-base qualities and the quality string length equals sequence length.
- [IUPAC ambiguity code definitions](https://bioinformatics.org/sms2/iupac.html):
  Y=C/T, R=A/G, and other ambiguity sets; complements follow base pairing.
- [SeqKit translate source and flag contract](https://github.com/shenwei356/seqkit/blob/facf0f7120483f9e39725c66dd83f5f14e048afd/seqkit/cmd/translate.go).
- [Pinned codon implementation](https://github.com/shenwei356/bio/blob/c289cafbaceafb27ddf7d2d9b422bf091abb61a6/seq/codon_tables.go#L205).
