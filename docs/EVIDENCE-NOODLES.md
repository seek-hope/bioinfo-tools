# noodles migration evidence

Date: 2026-10-23. Scope: `rust-umitools` BAM/SAM/CRAM I/O. This is local
implementation evidence; the independent audit is recorded separately.

## Dependency change

| Before | After |
| --- | --- |
| `rust-htslib 0.49` (C/htslib, `custom_derive`, `newtype_derive`) | `noodles 0.118.0` with `bam`, `bgzf`, `core`, `cram`, `fasta`, `sam` |
| — | `noodles-cram 0.101.0` (transitive) |
| — | `noodles-fasta 0.68.0` (transitive) |

`cargo tree --locked -i rust-htslib`, `-i custom_derive` and `-i newtype_derive`
all report no package. `Cargo.lock` no longer contains those names. The adapter
is `rust-umitools/src/io/bam.rs`; the decision record is
[`rust-umitools/docs/adr/0001-noodles-migration.md`](../rust-umitools/docs/adr/0001-noodles-migration.md).

## Functional evidence

| Check | Result |
| --- | --- |
| UMI exact | **77/77 byte-identical, 0 differ** |
| formats | CRAM output/decode/input, no-reference refusal, stdin/stdout, pipes: all pass |
| spill | 9/9 bounded-memory cases |
| determinism | 10/10 thread/fan-in cases |
| random differential | 12/12 trials, 0 failures |
| extract + whitelist | 11/11 + 9/9 |
| oracle | 39/39 |
| benchmark smoke | 6 cases |
| M3 | **10/10 PASS** |
| K1 | **QUALITY PASS**; workspace release **718 passed**; FASTX 50/50; QC 22/22; MultiQC/SELFTEST PASS |
| fmt / clippy | `cargo fmt --all -- --check` exit 0; clippy `-D warnings` pass |
| licenses/sources | `cargo deny check licenses sources`: licenses ok, sources ok |

No file under `tests/gate/golden/` or `rust-cutadapt/tests/fixtures/` was
modified. The exact oracle still uses the existing golden corpus.

## Performance and memory

`tests/bench/bench.py --reps 1` (200,000 reads, same host/fixture) compared the
pre-migration rust-htslib build with the noodles build:

| Case | Old wall | New wall | Old RSS | New RSS | Old tmp | New tmp |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| dedup | 0.056 s | 0.054 s | 40,216 kB | 41,460 kB | 0 | 0 |
| group | 0.305 s | 0.183 s | 178,696 kB | 127,320 kB | 0 | 0 |
| count | 0.050 s | 0.030 s | 12,784 kB | 18,552 kB | 0 | 0 |
| count_tab | 0.008 s | 0.007 s | 836 kB | 816 kB | 0 | 0 |
| dedup low-memory | 0.049 s | 0.054 s | 36,320 kB | 43,616 kB | 0 | 0 |
| dedup high-throughput | 0.049 s | 0.061 s | 31,136 kB | 37,760 kB | 0 | 0 |

The 20k smoke run showed an apparent 0.8 MB/1-thread old result; the process was
too short for the sampler, so the 200k run is the meaningful comparison. No
temporary-space growth is observed. `group` improves; `dedup`/`count` show a
roughly 10–20% RSS increase on this host, with comparable wall time. Raw JSON:
temporary `/tmp/umi-bench-precursor-200k.json` and
`/tmp/umi-bench-new-200k.json` (not durable).

## Deliberate limitations

- BAM/SAM are streamed. CRAM input is buffered because the current
  `noodles-cram` record iterator borrows both reader and header and cannot be
  stored in the safe adapter without a self-referential type. This is a
  CRAM-only memory cost; it is not used for BAM.
- The noodles BGZF writer has no worker pool here, so `set_threads` is a no-op.
  Rayon still handles the compute layer. This is a follow-up performance item,
  not a correctness issue.
- Compressed BAM bytes can differ from htslib; decoded records/header are
  identical (exact 77/77). The legacy `--version --verbose` label
  `htslib : via rust-htslib...` is retained to preserve the recorded text
  contract; it is a compatibility label, not a dependency claim. Changing it
  would require a separate approved oracle/contract update.
- CRAM writer requires a FASTA with an index (`.fai`); the formats oracle creates
  one with `samtools faidx`. No-reference CRAM output is refused with the
  documented usage error.
- `cargo deny check advisories` no longer reports `custom_derive`; the old
  unmaintained dependency is gone rather than ignored. New noodles dependencies
  are covered by the approved license/source policy.

## Boundaries

No commit, tag, release or upload was made. Remote GitHub CI was not executed in
this environment. Independent read-only audit is the remaining completion gate;
this document does not claim it.
