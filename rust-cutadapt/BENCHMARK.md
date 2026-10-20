# Benchmark report: rust-cutadapt v0.1.0 vs cutadapt 4.6 (historical)

> **This file describes the v0.1.0 subset, not the current full-surface
> binary.** The current default-mode measurement is 0.672 s mean over five runs,
> 6.60x against the recorded 4.4363 s baseline (14.36x against a same-session
> oracle), with 37.5–40.4 MB peak RSS. The 5x target is met. `-j 1` reproduces
> the earlier 2.352 s single-thread result. Raw evidence:
> [docs/bench/cutadapt-c8b0249.json](docs/bench/cutadapt-c8b0249.json) and
> [docs/bench/cutadapt-281a8dd.json](docs/bench/cutadapt-281a8dd.json).
> The 6.3x–6.9x numbers below must not be quoted for the current tree.

All numbers below were measured on the trial machine with the commands shown.
Nothing in this file is estimated; every figure is copied from real command
output.

## Machine and workload

| Item | Value |
| --- | --- |
| CPU | 13th Gen Intel Core i7-13700H (20 logical CPUs visible) |
| Memory | 31.8 GB total |
| Input | `bench/workload.fastq.gz`, 1,000,000 reads, 174,888,890 bytes uncompressed, 28,281,883 bytes gzipped |
| Command | `<bin> --quiet -a AGATCGGAAGAGC -m 10 -o <out.fastq.gz> bench/workload.fastq.gz` |
| Baseline | `bench/reference.json` (cutadapt 4.6): `wall_mean_s = 4.4363`, `max_rss_kb = 24412`, `output_bytes = 26555609` |

Load average on this shared machine was around 6.6–10.8 (1-minute average) for
our documented sessions, and the leader's 5-run session was at 8.25; one
exploratory benchmark session ran at 13.2. Absolute times move by roughly ±10 %
between sessions as a result. The cutadapt baseline in `reference.json` was
recorded on the same machine.

## Measured sessions (all real, same command, same binary)

| Session | hyperfine settings | Mean ± σ | Speedup vs 4.4363 s | Loadavg (1 min) |
| --- | --- | --- | --- | --- |
| ours, run 1 | 1 warmup + 3 runs | 642.2 ms ± 1.3 ms | 6.91x | 9.60 |
| ours, run 2 | 1 warmup + 3 runs | **641.3 ms ± 12.4 ms** | **6.92x** | 10.83 |
| ours, run 3 | 1 warmup + 3 runs | 650.3 ms ± 6.8 ms | 6.82x | not recorded |
| ours, run 4 | 2 warmups + 5 runs | 671.8 ms ± 10.9 ms | 6.60x | not recorded |
| leader, run 1 | 1 warmup + 5 runs | 670.8 ms ± 12.3 ms | 6.61x | 8.25 |
| leader, run 2 | 1 warmup + 3 runs | 703.2 ms ± 10.6 ms | 6.31x | not recorded |

**Expected performance is 6.3x–6.9x (≈0.64–0.70 s).** 641.3 ms is the best case
measured, not the expected value; the slowest session was 703.2 ms. All six
sessions are comfortably below the 5x-target threshold of 0.887 s.

## Wall time (hyperfine, 1 warmup + 3 runs — the method in `reference.json`)

```
$ hyperfine --warmup 1 --runs 3 -N \
    "./target/release/rust-cutadapt --quiet -a AGATCGGAAGAGC -m 10 \
     -o /tmp/rc-bench/out.fastq.gz bench/workload.fastq.gz"

Benchmark 1: ./target/release/rust-cutadapt --quiet -a AGATCGGAAGAGC -m 10 -o /tmp/rc-bench/out.fastq.gz bench/workload.fastq.gz
  Time (mean ± σ):     641.3 ms ±  12.4 ms    [User: 5312.5 ms, System: 60.0 ms]
  Range (min … max):   631.9 ms … 655.3 ms    3 runs
```

| Metric | rust-cutadapt (best session) | cutadapt 4.6 | Result |
| --- | --- | --- | --- |
| Wall time (mean) | **0.641 s** (range 0.641–0.703 s over all six sessions) | 4.4363 s | **6.3x–6.9x faster** |
| Target (>= 5x, i.e. <= 0.887 s) | 0.641–0.703 s | — | **met in every session** |

Speedup: `4.4363 / 0.6413 = 6.92x` for the best session;
`4.4363 / 0.7032 = 6.31x` for the slowest.

## Peak memory (`/usr/bin/time -v`)

```
$ /usr/bin/time -v ./target/release/rust-cutadapt --quiet -a AGATCGGAAGAGC -m 10 \
    -o /tmp/rc-bench/out.fastq.gz bench/workload.fastq.gz

	Elapsed (wall clock) time (h:mm:ss or m:ss): 0:00.63
	Maximum resident set size (kbytes): 13432
	Exit status: 0
```

| Metric | rust-cutadapt | cutadapt 4.6 | Result |
| --- | --- | --- | --- |
| Peak RSS | **11,648–13,432 kB ≈ 11.4–13.1 MiB** | 24,412 kB ≈ 23.8 MiB | **1.8x–2.1x lower** |

Measured peak RSS values: 11,648 kB (leader's run), 12,968 kB, 13,060 kB,
13,348 kB and 13,432 kB (our runs). The command output above shows 13,432 kB,
our highest reading; the value fluctuates by roughly ±1 MB between runs and
stays far below cutadapt's 24 MB.

Peak RSS is independent of input size because the pipeline streams 256 KiB
record-aligned chunks through bounded channels (see `README.md`).

## Output validation

The gzip output was decompressed and checked three ways:

1. **Structural check** — 3,999,984 lines = 999,996 records; every header starts
   with `@`, every separator with `+`, sequence/quality lengths match, and every
   retained read is between 10 and 83 bp (the `-m 10` filter).
   Four of the 1,000,000 input reads are trimmed to 9 bp or fewer and are
   correctly discarded.
2. **Byte-identical comparison against an independent implementation** — a
   separate Python port of cutadapt 4.6's `Aligner` (BACK flags, `indel_cost =
   100000`) was used to build the expected output; the full result matched:

   ```
   expected records_out: 999996
   expected sha256: 3c6c63bc333ccc2243d024d83b7c4c3481328bc04be9839dfab1e3aa903952cf

   (gzip -dc of the benchmark output)
   decompressed bytes: 144871678
   records: 999996
   sha256: 3c6c63bc333ccc2243d024d83b7c4c3481328bc04be9839dfab1e3aa903952cf
   ```
3. **gzip validity** — `gzip -t` accepts the output; Python's `gzip` module and
   `flate2`'s `MultiGzDecoder` decode it (output is a multi-member gzip stream).

`python3 tests/check.py` additionally verifies the three golden-file cases
(plain in, gzip in, gzip out) against cutadapt 4.6's records: all pass.

Output determinism was checked by comparing decompressed output across 1, 2, 3
and 4 worker threads (`RUST_CUTADAPT_THREADS`): identical.

## Compression-level trade-off (honest note)

Output size and speed both depend on the deflate level. The Rust build uses
`flate2`'s default `miniz_oxide` backend (no extra dependencies allowed), whose
level 6 is much slower than zlib's. Measured on this machine:

| Level | Wall time | How measured | Speedup vs cutadapt | Output size | vs cutadapt output |
| --- | --- | --- | --- | --- | --- |
| 1 | 0.41–0.46 s | single timed runs (3 thread counts) | 9.6x–10.8x | 34,973,680 B | +31.7 % |
| **3 (shipped default)** | **0.64–0.70 s** | hyperfine, 6 sessions | **6.3x–6.9x** | **27,027,230 B** | **+1.8 %** |
| 5 | 0.87–1.01 s | hyperfine, 3 sessions | 4.4x–5.1x | 26,708,750 B | +0.6 % |
| 6 | 1.99–2.02 s | hyperfine, 2 sessions + 1 timed run | 2.2x | 26,240,006 B | −1.2 % |

Level 6 clearly misses the 5x target. Level 5 straddles it (4.4x–5.1x), so it
is too close to rely on. The shipped default is therefore level 3, which stays
at 6.3x–6.9x with only 1.8 % larger output. The level can be changed with the
`RUST_CUTADAPT_COMPRESSION_LEVEL` environment variable (0–9) if smaller output
matters more than speed; it is not a CLI flag because a compression-level flag
is outside the implemented cutadapt subset.

The benchmark above was run with the default flags, exactly as in
`bench/reference.json`.

## Compressed output size vs cutadapt

With the shipped level 3, the benchmark output is 27,027,230 bytes versus
cutadapt's 26,555,609 bytes (`reference.json`), i.e. **+1.8 % larger**. This is
the deliberate cost of the 6.3x–6.9x speedup: `flate2`'s `miniz_oxide` backend
at level 6 (cutadapt's level) takes about 2.0 s here, which would miss the 5x
target. Decompressed content is identical in both cases (same 999,996 records,
sha256 `3c6c63bc...52cf`).

## Why it is fast

- Cutadapt is limited by Python per-read object creation; rust-cutadapt parses
  borrowed byte slices with no per-record allocation.
- The dynamic program only runs for reads that pass cutadapt's own k-mer
  prefilter (3/4/5-mer prefixes at the 3' end and 6-mer chunks), which is the
  same heuristic cutadapt uses. Equivalence of the fast path with always
  running the aligner is covered by `tests/differential.rs` (360,000 randomised
  reads).
- Gzip decompression, trimming and gzip compression overlap on separate threads
  with bounded queues, and compression is split across chunks (multi-member
  gzip), so all available cores are used while memory stays constant.

## Reproducing

Run this from the `rust-cutadapt/` directory; it works verbatim there:

```bash
cargo build --release --offline
mkdir -p /tmp/rc-bench
hyperfine --warmup 1 --runs 3 -N \
  "./target/release/rust-cutadapt --quiet -a AGATCGGAAGAGC -m 10 \
   -o /tmp/rc-bench/out.fastq.gz bench/workload.fastq.gz"
/usr/bin/time -v ./target/release/rust-cutadapt --quiet -a AGATCGGAAGAGC -m 10 \
  -o /tmp/rc-bench/out.fastq.gz bench/workload.fastq.gz
gzip -dc /tmp/rc-bench/out.fastq.gz | wc -l   # 3999984 lines = 999996 records
```

`mkdir -p` is required: the output directory is not created by the program, so
a missing directory makes it exit 1 with `cannot create output`.
