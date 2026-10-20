# Differential oracle tests

`differential.py` compares rust-cutadapt against a real cutadapt 4.6 binary.
It is the acceptance gate for the full-compatibility effort: a category is only
"done" when every case in it passes.

## Requirements

* Python 3.10+ (standard library only, no network).
* A cutadapt 4.6 binary. Discovery order:
  1. `--oracle PATH`
  2. `$CUTADAPT_ORACLE`
  3. `tests/ci/env/bin/cutadapt`, the prefix declared by `tests/ci/env.yml`
  4. `$CONDA_PREFIX/bin/cutadapt` when a conda environment is active
  5. `.work/oracle-venv/bin/cutadapt` in any parent directory of the repository
  6. `cutadapt` on `PATH`

  To create the declared prefix locally (not committed):

  ```bash
  conda env create -f tests/ci/env.yml -p tests/ci/env
  python3 tests/oracle/differential.py --matrix
  ```
* A built rust-cutadapt binary: `--rust PATH`, `$RUST_CUTADAPT`, or
  `target/release/rust-cutadapt` / `target/debug/rust-cutadapt`.

If the oracle cannot be found the harness exits with status 3 and says which
binary is missing; it never silently skips cases.

## Usage

```bash
python3 tests/oracle/differential.py --self-check   # harness sanity, no binaries needed
python3 tests/oracle/differential.py --list         # list case names
python3 tests/oracle/differential.py --matrix       # one line per case
python3 tests/oracle/differential.py                # full output, problem details
python3 tests/oracle/differential.py --filter json -v
python3 tests/oracle/differential.py --category paired-end
python3 tests/oracle/differential.py --json report.json
```

Exit status: `0` all selected cases pass, `1` at least one failed, `3` a binary
is missing.

## What is compared

| Aspect | Rule |
| --- | --- |
| exit status | must be equal |
| `-o`, `-p`, stdout reads | decompressed bytes must be equal |
| `--too-short-output`, `--too-long-output`, `--untrimmed-output` | decompressed bytes must be equal |
| `--info-file`, `--rest-file`, `--wildcard-file` | bytes must be equal |
| `--json` | field-by-field after dropping `cutadapt_version`, `python_version`, `command_line_arguments`, `cores`, `tag` |
| text report | equal after normalising the version banner, command line, timing/rate lines, core count and paths |
| stderr | present in both or absent in both |

Compressed output is never compared byte-wise: deflate implementations differ.
Only the decompressed stream matters.

## Fixtures

Fixtures are generated deterministically by the harness (`prepare_fixtures`), so
there are no binary blobs to keep in sync:

* `simple.fastq` — hand-written records covering full, internal, partial,
  mismatch, absent, anchored-at-start, 5' adapter, low quality, `N` bases,
  poly-A, a CASAVA header and two adapters in one read.
* `simple.r2.fastq` — the derived mate for paired-end cases.
* `simple.fasta` — the same reads as FASTA.
* `random.fastq` / `random.r2.fastq` — 200 seeded random reads with adapters
  inserted in five patterns, for regression coverage beyond the hand cases.

`--self-check` verifies that every generated FASTQ is structurally valid
(4-line records, sequence length == quality length) and that every case
placeholder resolves, so a broken fixture cannot be mistaken for a product bug.

## Adding a case

Add a `Case(...)` entry to `cases()` in `differential.py`:

```python
Case("my_case", ["-a", A, "-m", "0", "-o", "@@out@@", "@@in@@"]),
```

Use the `@@name@@` placeholders (`in`, `r2`, `missing`, `out`, `out_gz`,
`out2`, `info`, `rest`, `wildcards`, `too_short`, `too_long`, `untrimmed`,
`json`). Do **not** use `str.format` style braces: cutadapt templates such as
`--rename '{id}'` contain braces and must pass through untouched.

Set `compare_report=False` when only the read data matters, `compare_json=True`
for `--json` cases, and `exit_code=N` when the case is expected to fail.
