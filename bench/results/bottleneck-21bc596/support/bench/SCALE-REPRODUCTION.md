# Reproducing the scale comparison

These commands describe a new experiment from the repository root. The completed
run and its exact commands are in [SCALE.md](SCALE.md) and the
[evidence archive](results/scale-7a7ba28/README.md). This example has been checked
against the CLI interfaces; it is not evidence of a second executed experiment.

Run from this repository root in Bash. Install cutadapt 4.6, UMI-tools 1.1.6,
samtools 1.24, FastQC 0.13.0, SeqKit 2.14.0 and fq 0.12.0 first. The staging helper
expects samtools and Java in the supplied UMI environment, including
`lib/jvm/bin/java`; the recorded Java version is 25.0.2. It verifies versions,
downloads nothing and refuses an existing cache. Linux Python 3, Rust/Cargo, Git,
GNU time, taskset, lscpu and findmnt are also required. Complete the repository's
required gates/component checks before starting timing; MultiQC 1.35 is needed
for the optional FASTX/QC gate, but not for measured commands.

The current core mapping is in `bench/scale_common.py`:
`AFFINITIES = {1: '4', 2: '4,6', 4: '0,2,4,6'}`. On a different host use a
separate reproduction checkout, verify physical/P-core topology, and adapt that
mapping before plan creation/calibration. Merely editing plan affinity JSON
fails validation; the scale CLI has no affinity flag. The runner checks distinct
physical cores and availability, but cannot establish that they are P cores.

Use fresh directories on NVMe. Measured inputs, outputs and temporary files
must share one filesystem. Schedule builds, input generation, correctness suites, bulk transfers and
other benchmarks outside timing. Replace the installation paths below:

```bash
set -e
SCALE_WORK=/absolute/local-nvme/scale-reproduction-new
SCALE_ORACLES="$SCALE_WORK/oracles"
CUTADAPT=/absolute/cutadapt-env/bin/cutadapt

python3 bench/scale_plan.py --out "$SCALE_WORK/initial-plan.json"
python3 bench/prepare_readme_oracles.py \
  --cache "$SCALE_ORACLES" --umi-prefix /absolute/umi-env \
  --fastqc-dir /absolute/FastQC --seqkit /absolute/seqkit --fq /absolute/fq
python3 bench/readme_inputs.py --workdir "$SCALE_WORK/sources" \
  --samtools "$SCALE_ORACLES/bin/samtools" --sizes 1000000
python3 bench/scale_seal.py --outdir "$SCALE_WORK/sealed"
python3 bench/scale_inputs.py --plan "$SCALE_WORK/initial-plan.json" \
  --sources "$SCALE_WORK/sources/inputs.json" \
  --samtools "$SCALE_ORACLES/bin/samtools" --outdir "$SCALE_WORK/inputs-0"

scale_run() {
  python3 bench/scale_benchmark.py \
    --native-dir "$SCALE_WORK/sealed" \
    --canonicalizer "$SCALE_WORK/sealed/canonical-sam" \
    --oracle-dir "$SCALE_ORACLES/bin" --toolchain-cache "$SCALE_ORACLES" \
    --cutadapt "$CUTADAPT" "$@"
}
scale_run --calibration --plan "$SCALE_WORK/initial-plan.json" \
  --inputs "$SCALE_WORK/inputs-0/inputs.json" \
  --outdir "$SCALE_WORK/calibration-0"

python3 bench/scale_plan.py \
  --calibration "$SCALE_WORK/calibration-0/report.json" \
  --out "$SCALE_WORK/enlarged-plan-1.json"
python3 bench/scale_inputs.py --plan "$SCALE_WORK/enlarged-plan-1.json" \
  --sources "$SCALE_WORK/sources/inputs.json" \
  --samtools "$SCALE_ORACLES/bin/samtools" \
  --reuse-manifest "$SCALE_WORK/inputs-0/inputs.json" \
  --outdir "$SCALE_WORK/inputs-1"
scale_run --calibration --plan "$SCALE_WORK/enlarged-plan-1.json" \
  --inputs "$SCALE_WORK/inputs-1/inputs.json" \
  --outdir "$SCALE_WORK/calibration-1"
```

The source generator downloads/verifies the ENA source described by the original
README report. Use a fresh source directory: unlike the measured run directories,
it can overwrite generated source files. `--sizes 1000000` selects the required
populations; the default would also include 100k sources.

The initial plan starts at multiplicity 1 with seed 20261102, a 15 s sizing target
and 10 s minimum. Proposal rounds upward from each workload's fastest arm/core
observation and never shrinks. Repeat proposal/preparation/calibration with fresh
numbered paths until every observation is at least 10 s. Reuse requires passed
preparation, matching source/multiplicity metadata and artifact hashes; multiple
`--reuse-manifest` paths are accepted. Reused payloads stay at their old paths,
so retain those directories. The timed-process timeout defaults to 1800 s.

Assuming calibration 1 qualifies (otherwise substitute the latest paths):

```bash
python3 bench/scale_plan.py \
  --calibration "$SCALE_WORK/calibration-1/report.json" --freeze \
  --out "$SCALE_WORK/formal-plan.json"
scale_run --plan "$SCALE_WORK/formal-plan.json" \
  --inputs "$SCALE_WORK/inputs-1/inputs.json" --outdir "$SCALE_WORK/formal"
python3 bench/summarize_scale.py "$SCALE_WORK/formal/report.json" \
  --outdir "$SCALE_WORK/summary"
```

Freeze preserves multiplicities and rejects incomplete/short calibration.
Keep that calibration's input manifest; freezing does not regenerate inputs.
Formal mode omits `--calibration`. Summary requires a complete accepted formal
report and writes JSON, Markdown timing/resources and TSV. Retain and flag any
formal observation below 10 s; resizing creates another recorded experiment.

A historical formal plan contains an absolute calibration-report path/hash and
identities of inputs, scripts, executables and runtimes. New artifacts or
instrumentation require matching calibration. On another machine, collect fresh
sizing/calibration even if archived identities can be restored: formal preflight
does not enforce equal hardware. Historical before/after evidence stays separate.
