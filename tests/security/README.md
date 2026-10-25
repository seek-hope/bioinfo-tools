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
keeps the known duplicate-version warnings visible without blocking on them.

## Advisories: no waiver

`deny.toml` has `[advisories] ignore = []`. The former
`RUSTSEC-2025-0058` finding (`custom_derive 0.1.7`, unmaintained) was removed
with `rust-htslib` in the noodles migration. `cargo tree -i custom_derive` and
`cargo tree -i rust-htslib` now report no matching package, and both names are
absent from `Cargo.lock`. The advisory check must pass; any future advisory
blocks CI. No suppression was added.

## Commands

```bash
# Must pass.
cargo deny --offline --locked check licenses sources
cargo deny --offline --locked check advisories

# Full report.
cargo deny --offline --locked check
```

Convenience wrapper: `bash tests/security/check.sh --licenses`,
`--advisories`, or `--all`; set `CARGO_DENY_OFFLINE=1` for offline operation.
The wrapper is also used by the `dependency-policy` CI job. Both the license/source
and advisory checks are blocking; no waiver is configured.

## Evidence

- Cargo.lock SHA-256: `8bd4060bde3b3db9c987a315857fa1058cfc840d8215588cf13bf2f2eb016b56`
- RustSec local DB commit: `f7b2f6bc663452200bbe639ebb75e0c762b3422b`
- cargo-deny 0.20.2: `advisories ok, bans ok, licenses ok, sources ok`
  (2026-10-25, after the noodles migration).
- TeamAgents independent audit: `VERDICT: PASS WITH FINDINGS`; see
  [`docs/EVIDENCE-AUDIT-TEAMAGENTS.md`](../../docs/EVIDENCE-AUDIT-TEAMAGENTS.md).
