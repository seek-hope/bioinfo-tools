//! `rust-fx`: FASTX manipulation with SeqKit-compatible subcommands.
//!
//! # Compatibility status
//!
//! Every subcommand here is implemented from SeqKit 2.14.0's and fq 0.12.0's
//! **published documentation**, because neither oracle binary is available in
//! this environment. The plan's acceptance for W1-3 includes a differential
//! against both; that part is **not met and is not claimed**. In the plan's own
//! vocabulary this crate is `experimental`, not `verified` — see
//! `docs/EVIDENCE-W1-3.md` and `.work/w1-3-oracle-status.md`.
//!
//! What *is* verified in-repo: the deterministic `sample --seed` contract, the
//! `split` naming and ordering rules, the `lint` validator set against fq's
//! published table, the exact byte layout of `fx2tab` and `fq2fa` output, and the
//! `stats` table layout against the published examples.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod cli;
pub mod commands;
pub mod io;
pub mod lint;
pub mod md5;
pub mod sample;
pub mod split;
pub mod stats;
pub mod table;
pub mod transform;

pub use stats::{Merge, QualStats, SeqStats};
