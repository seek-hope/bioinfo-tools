# Dependency policy and audit record

Status: **approved 2026-10-23** by the project maintainer in the current
session. The authoritative policy is [`deny.toml`](../../deny.toml).

## License/source policy

The policy allows these SPDX licenses (no package-specific exceptions):

`MIT`, `Apache-2.0`, `BSD-3-Clause`, `ISC`, `Unicode-3.0`, `Zlib`,
`bzip2-1.0.6`.

Only crates.io is an allowed registry; unknown registries and git sources are
denied. The list is a policy choice for this project, not merely a dump of the
current lockfile. New licenses require review before being added.

`MIT OR Apache-2.0 OR LGPL-2.1-or-later` is accepted through its MIT/Apache
branch; it does not add LGPL to the allow-list. `multiple-versions = "warn"`
keeps the six known duplicate-version warnings visible without blocking on
them. `newtype_derive` is a sibling of `custom_derive` (same era/author); it is
tracked here even though it has no RustSec advisory.

## Advisories: no waiver

`deny.toml` has `[advisories] ignore = []`. The known advisory is:

- `RUSTSEC-2025-0058`: `custom_derive 0.1.7` is unmaintained
  (`informational = "unmaintained"`, `patched = []`).
- Dependency path: `custom_derive <- rust-htslib 0.49.0 <- rust-umitools`.
- `rust-htslib 1.0.1` still declares `custom_derive = "0.1"`, so a minor
  update is not a known fix.
- `custom_derive` is a build-time `macro_rules!` helper used by `rust-htslib`
  to generate newtype trait impls (`NewtypeFrom`, `NewtypeDeref`,
  `NewtypeDerefMut`, `NewtypeIndex`, `NewtypeIndexMut`) for BAM/BCF wrapper
  types. It has no `build.rs`, no proc-macro and no `unsafe` in its source; it
  is not a BAM/BCF data-path parser. The risk is unmaintained codegen, not a
  known runtime vulnerability.

The maintainer explicitly chose **not to suppress** this finding. The license
check must pass; the advisory check must remain visibly red until the dependency
is replaced or a separately approved, time-bounded risk acceptance exists.

## Commands

```bash
# Must pass.
cargo deny --offline --locked check licenses sources

# Expected to fail with RUSTSEC-2025-0058; do not add an ignore.
cargo deny --offline --locked check advisories

# Full report: advisories FAILED, bans ok, licenses ok, sources ok.
cargo deny --offline --locked check
```

Convenience wrapper: `bash tests/security/check.sh --licenses`,
`--advisories`, or `--all`; set `CARGO_DENY_OFFLINE=1` for offline operation.
The wrapper is also used by the `dependency-policy` CI job. Its advisory step is
a blocking gate: remote CI stays red until the dependency is migrated. This is
intentional under the no-waiver decision; release review must treat the advisory
as open.

## Evidence

- Cargo.lock SHA-256: `f6a95b462b2bffa326810e451788214d59d6c1dbaad5dd41de18044297edf8bc`
- RustSec local DB commit: `f7b2f6bc663452200bbe639ebb75e0c762b3422b`
- cargo-audit 0.22.2; cargo-deny 0.20.2
- `cargo audit --no-fetch --no-yanked`: exit 0, one unmaintained warning.
- TeamAgents independent audit: `VERDICT: PASS WITH FINDINGS`; see
  [`docs/EVIDENCE-AUDIT-TEAMAGENTS.md`](../../docs/EVIDENCE-AUDIT-TEAMAGENTS.md).
