# Release freeze

- Status: **active**
- Since: 2026-10-20
- Applies to: `rust-cutadapt` source repository and the
  `seek-hope/rust-cutadapt-dist` distribution repository

## What is frozen

No new tag, GitHub release, dist-repository upload, crates.io publication or
Bioconda submission is made while this freeze is active. The existing
`.github/workflows/` release automation is not triggered.

Documentation commits, test additions and source fixes are allowed. The freeze
is a publication freeze, not a development freeze.

## Historical artifact

The distribution repository contains **v0.1.0**, built from source commit
`d9c4c52`. That artifact predates the full-surface work and does not represent
the current tree. It stays published and unchanged, but it must not be cited as
evidence for the current compatibility claim.

There is no source tag in this repository for v0.1.0. The dist artifact is the
only historical published artifact.

## Exit criteria

The freeze may be lifted only when all of the following are true and an
explicit release decision has been recorded:

1. W0-1, W0-2, W0-3 and W0-5 in the workspace development plan are complete:
   the README matches the tested binary, the oracle is portable, current
   evidence is recorded with commit and binary checksums, and the benchmark has
   been re-run at the release commit.
2. `cargo test --release --offline`, `cargo fmt --all -- --check`,
   `cargo clippy --all-targets --locked -- -D warnings` and the differential
   matrix are green at the release commit.
3. The version in `Cargo.toml` is bumped deliberately; the release commit is
   clean and reviewed.
4. Release notes state the supported surface and every known deviation from
   cutadapt 4.6. They must link the raw differential and benchmark evidence.
5. The source tag is created only after the checks above. The dist artifact is
   built from that exact commit, its SHA256 is published, and the install
   script is updated to point at it.
6. The release owner records the decision in this repository. No release
   workflow is triggered before that record exists.

## Relationship to the README

[../README.md](../README.md) describes the current source tree. Its
installation section intentionally points source builds at the current tree
and identifies the published v0.1.0 artifact as historical.
