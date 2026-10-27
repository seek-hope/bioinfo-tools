//! `rust-fx`: FASTX manipulation with SeqKit-compatible subcommands.
//!
//! # Compatibility status
//!
//! Every subcommand here is implemented from SeqKit 2.14.0's and fq 0.12.0's
//! documented behavior. The pinned oracle binaries are available through the
//! project toolchain; `head` now has direct SeqKit differential evidence for
//! positive `-n/-l` cases, while the remaining commands and head's global flags
//! remain experimental. See `compat/matrix.tsv` for the per-surface status and
//! `docs/EVIDENCE-W1-3.md` for earlier evidence.
//!
//! What *is* verified in-repo: the deterministic `sample --seed` contract, the
//! `split` naming and ordering rules, the `lint` validator set against fq's
//! published table, the exact byte layout of `fx2tab` and `fq2fa` output, and the
//! `stats` table layout against the published examples.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod cli;
pub mod commands;
pub mod grep;
pub mod io;
pub mod lint;
pub mod md5;
pub mod replace;
pub mod sample;
pub mod sort;
pub mod split;
pub mod stats;
pub mod table;
pub mod transform;
pub mod translate;

pub use stats::{Merge, QualStats, SeqStats};
