# QC oracle pins: FastQC 0.13.0 and MultiQC 1.35

The QC differential (`tests/oracle/qc_diff.py`) is only meaningful against the exact
oracle versions the plan names. This file pins them so a future run can tell "the
oracle disagreed" apart from "a different oracle version was installed".

**No oracle bytes are committed.** `tests/ci/env/` is git-ignored. Only this note is
tracked. The FastQC distribution is ~15 MB and the MultiQC virtualenv ~1.1 GB, which
is another reason they live outside version control.

## Pinned versions

| Oracle | Version | Banner |
| --- | --- | --- |
| FastQC | **0.13.0** | `FastQC v0.13.0` |
| MultiQC | **1.35** | `multiqc, version 1.35` |
| Java (runtime for FastQC) | 25.0.2 | `openjdk version "25.0.2-internal"` |

The versions the harness actually saw are printed on every run, so a mismatch is
visible in the log rather than silent:

```text
$ python3 tests/oracle/qc_diff.py --rust target/release/rust-fastqc
rust-fastqc : …/target/release/rust-fastqc
fastqc      : …/tests/ci/env/bin/fastqc
multiqc     : …/tests/ci/env/bin/multiqc
FastQC v0.13.0
fixtures    : 22
```

## Pinned checksums

FastQC ships as an unpacked directory of class files plus four JARs, so the
distribution is identified by its JARs and by the `Configuration/limits.txt` that
carries the module thresholds:

```text
e0c8f4afbf6c82ce50e1a45bbcc26682be98b59357f5df5bb967444fdc94ab3e  htsjdk.jar
```

Verify a local install with:

```sh
cd tests/ci/env/fastqc && sha256sum -c <<'EOF'
e0c8f4afbf6c82ce50e1a45bbcc26682be98b59357f5df5bb967444fdc94ab3e  htsjdk.jar
EOF
```

For MultiQC, pin the Python package rather than a binary:

```sh
tests/ci/env/multiqc-venv/bin/python3 -c "import multiqc; print(multiqc.__version__)"
# 1.35
```

## Behavioural dependencies on the version

These were each measured against 0.13.0 and are what the differential would notice
if the version changed:

* **`Configuration/limits.txt`** supplies every module threshold. The values used
  (and quoted in `rust-fastqc/src/modules.rs`) are `quality_base_lower 10/5`,
  `quality_base_median 25/20`, `n_content 5/20`, `duplication 70/50`,
  `sequence 10/20`, `gc_sequence 15/30`, `quality_sequence 27/20`,
  `overrepresented 0.1/1`. A version that retunes these changes verdicts without
  changing any number in the tables.
* **Per-base quartiles are `NaN` until a position has more than 100 observations**
  (`PerBaseQualityScores.getPercentile`). A version that lowers this shows numbers
  where 0.13.0 shows `NaN`.
* **Base grouping starts at 75bp** (`BaseGroup.getLinearInterval` tries interval 1
  first). A different ladder shifts every row label for long reads.
* **`Per sequence GC content`** spreads each read over percentage bins via
  `GCModel`, and its verdict comes from a theoretical normal distribution whose
  mean uses the "either walk survived" rule. Both are 0.13.0-specific.
* **`%GC` truncates** (`(int)` cast) while the overrepresented percentage
  **rounds to two decimals**.
* **Adapter Content** is computed from `Configuration/adapter_list.txt`, which this
  prototype does not implement; see `docs/EVIDENCE-QC.md`.

## How to install (needs network, once, outside the agent sandbox)

Shell network access is forbidden inside this workspace's agent sessions, so the
oracles must be placed by a human or by CI provisioning:

```sh
mkdir -p tests/ci/env

# FastQC 0.13.0 (unpacks to a directory of classes plus JARs).
curl -fsSL -o /tmp/fastqc.zip \
  https://www.bioinformatics.babraham.ac.uk/projects/fastqc/fastqc_v0.13.0.zip
unzip -q /tmp/fastqc.zip -d tests/ci/env
chmod +x tests/ci/env/FastQC/fastqc

# MultiQC 1.35 in its own virtualenv.
python3 -m venv tests/ci/env/multiqc-venv
tests/ci/env/multiqc-venv/bin/pip install 'multiqc==1.35'
```

The launcher is then exposed at `tests/ci/env/bin/fastqc`, which is what the
resolver looks for.

## Resolution order

`tests/oracle/qc_diff.py` and `run_qc_oracle.sh` never hard-code a personal path.
For each tool:

1. `--fastqc` / `--multiqc` on the command line;
2. `$FASTQC_ORACLE` / `$MULTIQC_ORACLE`;
3. `tests/ci/env/bin/<tool>`;
4. `<tool>` on `PATH`.

If FastQC cannot be resolved the harness **exits 3** and refuses to run. It never
skips a fixture and never reports a pass without the oracle.

## Note on the FastQC launcher

`tests/ci/env/bin/fastqc` is FastQC's own Python launcher script, not a wrapper
this project wrote. It locates `java`, builds the classpath from the four JARs in
the parent directory and runs `uk.ac.babraham.FastQC.FastQCApplication`. Because it
is a script, its checksum is meaningful only together with the JARs it loads, which
is why `htsjdk.jar` is pinned above.
