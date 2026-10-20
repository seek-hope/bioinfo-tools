# W1-3 oracle pins: SeqKit 2.14.0 and fq 0.12.0

The W1-3 differential (`tests/oracle/w13_diff.py`) is only meaningful against the
exact oracle versions the plan names. This file pins them so a future run can tell
"the oracle disagreed" apart from "a different oracle version was installed".

**The oracle binaries are NOT committed.** `tests/ci/env/bin/` is a local
installation target and is git-ignored. Only this note is tracked.

## Pinned versions

| Oracle | Version | Upstream release |
| --- | --- | --- |
| SeqKit | **2.14.0** | `seqkit v2.14.0` |
| fq | **0.12.0** | `fq v0.12.0 (51779f50e 2024-07-08)` |

The versions the harness actually saw are printed on every run, so a mismatch is
visible in the log rather than silent:

```text
$ bash tests/oracle/run_w13_oracle.sh
rust-fx : target/release/rust-fx
seqkit  : .../tests/ci/env/bin/seqkit
fq      : .../tests/ci/env/bin/fq
seqkit v2.14.0
fq v0.12.0 (51779f50e 2024-07-08)
cases  : 50
```

## Pinned checksums (linux x86_64)

SHA-256 of the exact binaries used for the recorded W1-3 result:

```text
88aa8a539b1e9097220a47811367b7ef826eff8637e5284fdd32a372770d6f7d  seqkit   # v2.14.0
600e8b204086bc0d3e225b0021fd234fc452bf72414af80ad8a115a327b5fcb2  fq       # v0.12.0
```

Verify a local install with:

```sh
cd tests/ci/env/bin && sha256sum -c <<'EOF'
88aa8a539b1e9097220a47811367b7ef826eff8637e5284fdd32a372770d6f7d  seqkit
600e8b204086bc0d3e225b0021fd234fc452bf72414af80ad8a115a327b5fcb2  fq
EOF
```

Checksums are per-platform: the upstream release archives for macOS/arm64 differ.
If the platform is not linux x86_64, pin the checksum of the binary actually
installed and note the platform here rather than forcing this one to match.

## How to install (needs network, once, outside the agent sandbox)

Shell network access is forbidden inside this workspace's agent sessions, so the
binaries must be placed by a human or by CI provisioning:

```sh
mkdir -p tests/ci/env/bin

# SeqKit 2.14.0
curl -fsSL -o /tmp/seqkit.tar.gz \
  https://github.com/shenwei356/seqkit/releases/download/v2.14.0/seqkit_linux_amd64.tar.gz
tar -xzf /tmp/seqkit.tar.gz -C tests/ci/env/bin seqkit
chmod +x tests/ci/env/bin/seqkit

# fq 0.12.0
curl -fsSL -o /tmp/fq.tar.gz \
  https://github.com/stjude-rust-labs/fq/releases/download/v0.12.0/fq-0.12.0-x86_64-unknown-linux-gnu.tar.gz
tar -xzf /tmp/fq.tar.gz -C /tmp
install -m755 /tmp/fq-0.12.0-x86_64-unknown-linux-gnu/fq tests/ci/env/bin/fq
```

## Resolution order

`tests/oracle/run_w13_oracle.sh` never hard-codes a personal path. For each tool
it tries, in order:

1. `$SEQKIT_ORACLE` / `$FQ_ORACLE`, when set;
2. `tests/ci/env/bin/<tool>`;
3. `${CONDA_PREFIX}/bin/<tool>`;
4. `<tool>` on `PATH`.

If either oracle cannot be resolved the harness **exits 3** and refuses to run. It
never skips a case and never reports a pass without the oracle — a harness that
passes when the oracle is missing is worse than no harness at all.

## Why the versions matter

Both tools' output formats are version-sensitive in ways the differential depends
on. These were all measured, not assumed:

* `fq`'s log lines carry a wall-clock timestamp and environment-dependent ANSI
  colour. The harness normalises `<TS> <LEVEL> <target>:` and compares the rest
  byte for byte (`fq_log_prefix` rule).
* `fq lint` exits **1** on a validation failure. A different version exiting 3
  would silently change 8 of the 50 case outcomes.
* `fq` 0.12.0 **never enables S007** (DuplicateName) as a single-read validator;
  duplicate names exit 0. If a later version enables it, `lint_S007` flips.
* SeqKit's `stats` human layout has **no dashed rule line** and two-space column
  separators; `-T` and the human layout disagree on `Q20(%)`/`AvgQual` precision.
* SeqKit's `fx2tab -s` hashes the **lower-cased** sequence.
* SeqKit's quartiles are **Tukey hinges with round-half-to-even**, which is
  undocumented; the method in `rust-fx/src/table.rs` was reverse-engineered from
  this exact version.
