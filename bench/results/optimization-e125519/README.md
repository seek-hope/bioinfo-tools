# Optimization evidence after e125519

The accepted results are in `formal.json`, `tables.md` and `tables.tsv`.
[The report](../../OPTIMIZATION.md) explains the six command changes, measured
limits and reproduction; [the protocol](../../OPTIMIZATION-PROTOCOL.md) records
the population and the resource-failure amendment.

`formal.json` is a validated composition of ten complete 1M conditions from
`core-incomplete.json` and three complete 10M conditions from `streaming.json`.
Each condition contains its intact before/after/upstream blocks. The root header
comes from the core run; `stage_reports` records both execution directories,
script hashes, parameters and source-report hashes. Shared native, input,
command, timing and comparison identities are checked by
`executed-streaming/merge_optimization.py`. All 13 conditions are required.

The core run exhausted tmpfs during its first 10M conversion warm-up. Its
`passed: false` report, staged `core-report.partial`, `core.log`, empty
`core.status` and available process files remain untouched. The status file is
empty because the parent could not write it either. The incomplete warm-up has
no accepted measured samples. Retained output directories were copied to
`target/optimization-retained/`, hash-verified and removed from tmpfs; the mapping
and every file hash are in `retained-outputs.json`. Those large outputs are
locally retained outside Git. `infrastructure-failure.json` records the retry
scope. The retry keeps the full-plan permutation order and adds a free-space
preflight for the coexisting output/copy requirements.

`superseded.json` is the earlier, interrupted candidate with a FASTA record
lifetime regression. Its observations are excluded. `superseded-reason.json`,
`superseded-build-provenance.json` and `superseded-source.patch` identify it.
`validation/fasta-memory.json` records the original/superseded/final 32 MiB-record
check, including output equality. `pilot.json` is the initial six-condition 100k
pilot, identified by `phase1-build-provenance.json` and `phase1-source.patch`.
`profiling/exploratory.json` is the fixed-order counter-loop exploration. Neither
pilot nor exploratory observations enter final tables.

The baseline binaries exactly match the original README experiment. The final
native source is `e1255197285329e0c184293ef36868de30694ee0` plus
`native-source.patch`; `build-provenance.json` records both artifact hashes and
build recipes. `executed-core/` preserves the scripts used by the pilot,
superseded and core runs. `executed-streaming/` preserves the retry scripts,
merge/summary programs and independent behavior/memory/evidence checks. Run the
maintained programs from the recorded checkout as described in the report;
these byte-exact snapshots retain their original repository-layout assumptions.

`profiling/` contains separate 100k syscall/instruction-counter checks and output
hashes. Their instrumented times are excluded. `validation/` contains fresh full
base/oracle gate logs and statuses, 104-case behavior preservation, Rust process
and unit tests, eight evidence controls, twelve gate controls and document/matrix
checks. The base gate and external-oracle gate are separate results. Remote
native/dependency CI is also separate and does not establish external-oracle
provisioning.

FastQC native bundles match byte for byte. Its comparison with upstream remains
limited to independently checked counts/lengths; missing graphics and unresolved
GC/duplication differences are still visible in the raw reports. The original
[README benchmark](../readme-2026-10-31/) remains unchanged.

`sha256.json` hashes every other file in this directory after the final local
checks. It is an integrity manifest, not a signature or release artifact.
