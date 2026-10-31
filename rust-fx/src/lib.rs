//! `rust-fx`: FASTX manipulation with SeqKit-compatible subcommands.
//!
//! # Compatibility status
//!
//! Targets are pinned to SeqKit 2.14.0 and fq 0.12.0. See `compat/matrix.tsv`
//! for per-surface evidence and known differences; a verified command surface
//! does not promise every upstream global option. `tests/oracle/` contains the
//! direct differentials. In particular, `range` keeps each input's own format
//! rather than copying upstream's pooled-quality carryover across mixed files.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod cli;
pub mod commands;
pub mod grep;
pub mod io;
pub mod lint;
pub mod md5;
mod range;
pub mod replace;
pub mod sample;
mod seqkit_io;
pub mod sort;
pub mod split;
pub mod stats;
mod tab2fx;
pub mod table;
pub mod transform;
pub mod translate;

pub use stats::{Merge, QualStats, SeqStats};
