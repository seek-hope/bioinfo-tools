# Benchmark workload

`workload.fastq.gz` is a deterministic 1,000,000-read single-end FASTQ (70 bp prefix plus a
3' `AGATCGGAAGAGC` adapter, with partial/internal/mismatch/no-adapter variants).

`reference.json` is the cutadapt 4.6 baseline on this machine:

- hyperfine 1.20.0, 1 warmup + 3 runs
- peak RSS from `/usr/bin/time -v` on one run
- output is gzip; compare decompressed records only

Measure the Rust binary with the same command shape and report wall time and peak RSS.
