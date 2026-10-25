# ADR 0001: dependency policy and bzip2/xz support

- Status: accepted
- Date: 2026-10-20
- Decision owners: rust-cutadapt maintainers
- Supersedes: the trial rule that limited dependencies to the standard library,
  `flate2` and `clap`

## Context

The original v0.1.0 trial restricted the crate to the standard library plus
`flate2` and `clap`. That restriction described the original single-end subset,
which only needed plain/gzip FASTQ I/O.

The current tree implements the documented cutadapt 4.6 surface, including
bzip2 and xz input/output. Those formats require `bzip2` and xz (via `xz2`); the
implementation also already depends on `clap` for the full option grammar and
`flate2` for gzip. The current `Cargo.toml` therefore lists:

- `clap` (option grammar)
- `flate2` (gzip)
- `bzip2` (bzip2)
- `xz2` with its `static` feature (xz)

Keeping the old two-dependency rule would either drop documented formats or
reimplement compression codecs, both of which are worse outcomes. Leaving the
old rule unrecorded would make the manifest look like an accidental violation.

## Decision

The dependency policy is:

1. Prefer the standard library, then an existing dependency, then new code.
2. Keep the direct runtime dependency set limited to what the documented
   cutadapt 4.6 surface needs. The current set is `clap`, `flate2`, `bzip2` and
   `xz2` (with its `static` feature); adding another direct dependency requires
   an ADR or an explicit update to this one.
3. Keep test and build-only dependencies out of the runtime surface.
4. Build with the pinned toolchain and `--locked`; `Cargo.lock` is authoritative
   for reproducible builds. Network access is allowed; offline builds are
   supported when the lockfile is vendored or cached.
5. Do not use a dependency to hide an unsupported format or an unimplemented
   option. Unsupported input must fail closed with the documented exit code.

The earlier `clap` + `flate2`-only rule is historical and is superseded by this
decision.

## Amendment: `liblzma` → `xz2` (W1-1)

- Date: 2026-10-20
- Related: `docs/EVIDENCE-W1-1.md` DR-2

> Historical amendment. The `rust-htslib` constraint described below ended with
the 2026-10-25 noodles migration. The direct `lzma-sys` pins were then dropped
in favour of `xz2`'s `static` feature; the text records why `xz2` was chosen at
the time.

Moving this crate into the `bioinfo-tools` Cargo workspace made the original
`liblzma` choice impossible. Cargo permits only one package per `links` value in
a resolution graph, and `liblzma-sys` and `lzma-sys` both declare
`links = "lzma"`. `rust-umitools` pulls in `lzma-sys` through `rust-htslib`,
which needs it for its verified CRAM support, so the xz binding on this side had
to be the `lzma-sys` one.

`xz2` is the predecessor of the `liblzma` fork (same author, same C library,
identical `XzDecoder`/`XzEncoder` API including `new_multi_decoder`) and depends
on `lzma-sys`, the crate htslib already uses. This is therefore a substitution,
not an addition: the same C library is linked, through the provider the
workspace already had.

The substitution was measured before it was adopted. xz output is
**byte-identical** to the old binding at preset 0 (`-Z`) and preset 6 on a 20 MB
realistic FASTQ excerpt and on a 3 000-read trimming run, and the five xz
differential cases against cutadapt 4.6 pass unchanged. At the time,
`rust-htslib` enabled `lzma-sys/static`, so the bundled xz-5.2 sources were
compiled in rather than the system liblzma being linked dynamically; the current
manifest enables the same static build through `xz2/static` directly.

`--debug` still prints `liblzma` in its dependency list, because that string is
observable output (`docs/EVIDENCE-W1-1.md` DR-5).

## Consequences

- bzip2 and xz are first-class documented formats, not silent extensions.
- The MSRV check must cover the new dependencies (currently Rust 1.91).
- Offline builds need these crates vendored or present in the local registry,
  exactly as they already need `clap` and `flate2`; online builds can fetch them.
- The dependency review in `Cargo.toml` and the CI lockfile check remain the
  enforcement points. A future format or reporting feature that needs another
  crate must update this ADR rather than silently expanding the manifest.
- Inside the workspace, keep `xz2`'s `static` feature enabled so the bundled xz
  sources are used. The former `rust-htslib` provider is gone; no direct
  `lzma-sys` pin is needed.
