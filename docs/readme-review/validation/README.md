# README batch validation

Both required local gates completed with exit 0 after the implementation and
benchmark integrity fixes:

```text
$ bash tests/ci/check.sh
QUALITY PASS (base only; external oracles NOT RUN)

$ bash tests/ci/check.sh --with-fastx-qc-oracles
README boundary oracle: 22/22
PASS SeqKit corrected semantics: 434 checks
PASS FastQC limits/kmer: 141 cases; {'exact': 111, 'proved-kmer-correction': 27, 'proved-config-rejection': 1, 'exact-preflight-rejection': 2}
SELFTEST PASS
QUALITY PASS (base + FASTX/QC oracles)
```

These are selected lines from the fresh [base log](base.log) and
[external-oracle log](oracles.log), not a previous batch's evidence. Existing
FASTX, QC, adapter, tile, translation/range/tab2fx and registered-correction
checks ran in the external gate. This gate does not rerun every historical
cutadapt/UMI/samtools full oracle suite.

Additional checks completed:

- Ten benchmark evidence controls, including failure preservation and summary
  guards; `python3 -W error::ResourceWarning bench/test_readme_benchmark.py` passed.
- Twelve `tests/ci/test_check.py` harness controls passed.
- Both the [measured package build](measured-boundaries.json) and
  [workspace build](workspace-boundaries.json) passed the same 22 boundary checks.
  [summary.json](summary.json) records their different hashes.
- Thirteen native CLI calls exercised README examples on synthetic data; see
  [example-execution.json](../example-execution.json).
- New portable oracle staging passed pinned version probes and a three-condition
  [setup smoke run](staging-smoke.json) for conversion, sort and UMI dedup.
  Its timings are excluded from the formal performance tables.
- The benchmark's original source/binary hashes and archived evidence checksums
  were verified. A per-package rebuild reproduced the measured `rust-fx` hash;
  see [build provenance](../../../bench/results/readme-2026-10-31/build-provenance.json).

Documentation was checked again after adding final measurements and build notes.
The compatibility matrix remains 266 rows with 189 verified, 9 experimental,
57 unsupported and 11 exception; the remaining gaps were not relabelled.
