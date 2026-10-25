# ADR 0001: migrate BAM I/O from rust-htslib to noodles

- Status: accepted
- Date: 2026-10-23
- Decision owner: `rust-umitools` maintainer (user-authorized migration)
- Supersedes: the earlier decision to keep `rust-htslib` for the first CRAM/BAM
  implementation (`IMPLEMENTATION_PLAN.md`)

## Context

`rust-umitools` used `rust-htslib 0.49`, which brings `custom_derive 0.1` and
`newtype_derive 0.1` transitively. `custom_derive` is reported as unmaintained
by RUSTSEC-2025-0058. The workspace already uses noodles 0.118 for `rust-bam`,
and noodles meets the workspace MSRV of 1.91. Keeping a C htslib dependency
only for UMI I/O therefore adds an unmaintained code-generation dependency and
a second BAM implementation for no compatibility benefit.

## Decision

Use noodles for all `rust-umitools` alignment I/O:

| Crate | Version | Features/role |
| --- | --- | --- |
| `noodles` | 0.118.0 | `bam`, `bgzf`, `core`, `cram`, `fasta`, `sam` |
| `noodles-cram` | 0.101.0 | transitive CRAM reader/writer |
| `noodles-fasta` | 0.68.0 | indexed reference repository for CRAM |

The adapter lives in `rust-umitools/src/io/bam.rs`. It keeps the small
`Record`/`Cigar`/`Aux`/`Header`/`Reader`/`Writer` surface the business layer
already used, while storing noodles `sam::alignment::RecordBuf` internally.
`rust-htslib`, `custom_derive` and `newtype_derive` are removed from
`Cargo.toml` and `Cargo.lock`.

## Consequences

- BAM and SAM input/output are streamed through noodles BGZF/SAM readers and
  writers. Coordinate sorting and temporary runs keep the existing bounded
  chunking and merge logic.
- CRAM input/output uses `noodles-cram` with a `noodles-fasta` indexed
  reference repository. The formats oracle covers CRAM output, decode, input and
  the no-reference refusal.
- `noodles-cram`'s record iterator borrows the reader and header, so the
  current safe adapter buffers CRAM input records before exposing them. This is
  a CRAM-only memory cost; BAM/SAM remain streaming. A future noodles API that
  permits a stored iterator can remove it.
- The noodles BGZF writer used here has no worker pool, so the adapter's
  `set_threads` is a no-op. The higher-level Rayon work remains. This is visible
  in the 200k benchmark: temp space stays zero and throughput/RSS are comparable
  to the rust-htslib precursor; `group` is faster and lower-RSS, while
  `dedup`/`count` are about 10–20% higher RSS on this host.
- Compressed BAM bytes can differ from htslib (BGZF block/compression choices),
  but the oracle compares the decoded records/header and passes 77/77. The
  legacy `--version --verbose` field `htslib : via rust-htslib...` is retained
  verbatim for the recorded text contract; it is now a compatibility label, not
  a dependency claim. Changing it requires a separately approved contract
  update.
- License/source policy: `cargo deny check licenses sources` passes. The
  license allow-list in `deny.toml` already covers the noodles dependencies.

## Verification

See [`docs/EVIDENCE-NOODLES.md`](../../docs/EVIDENCE-NOODLES.md). Short form:
exact 77/77, formats/spill/determinism/random/extract/oracle pass, M3 10/10,
K1 718 passed, clippy/fmt clean, and no old dependency remains in the tree.

## Rejected alternatives

- **Patch only `custom_derive` in rust-htslib.** Keeps a fork of the C-backed
  BAM library and does not address the broader dependency surface.
- **Upgrade rust-htslib.** 1.0.2 still declares `custom_derive 0.1`.
- **Keep the unmaintained dependency with an ignore.** Explicitly rejected by
  the maintainer; the advisory remains visible and CI is blocking.
- **Migrate only BAM and drop CRAM.** Rejected because the existing formats
  oracle treats CRAM as part of the observable contract.
