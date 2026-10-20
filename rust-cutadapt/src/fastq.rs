//! FASTQ record parsing, re-exported from [`bioinfo_fastx`].
//!
//! This module used to own the parser. W1-2 moved it into the shared
//! `bioinfo-fastx` crate so that `rust-cutadapt` and `rust-umitools` cannot drift
//! apart on record structure or on the wording of a diagnostic.
//!
//! The names below are re-exports, not wrappers: `crate::fastq::Parser` *is*
//! `bioinfo_fastx::parser::Parser`, so the engine, the paired-end path and the
//! pipeline keep compiling unchanged and there is exactly one implementation.
//!
//! # Behaviour is unchanged
//!
//! The strict parser reproduces the pre-migration diagnostics byte for byte,
//! including the line/offset pair:
//!
//! ```text
//! ERROR: malformed FASTQ at line 5 (byte offset 19): sequence and quality lengths differ (8 != 3)
//! ```
//!
//! That exact string is pinned by `tests/gate/golden/cutadapt_malformed_fastq.json`,
//! recorded from the binary before the migration.
//!
//! # Strict only
//!
//! cutadapt 4.6 rejects anything the strict parser rejects, so this crate
//! deliberately does not expose the lenient mode: using it here would silently
//! accept input the oracle refuses, which is precisely the "silent fallback"
//! that boundary rule 6 forbids.

pub use bioinfo_fastx::parser::{Format as RecordFormat, Mode as RecordMode};
pub use bioinfo_fastx::{ParseError, Parser, Record};

/// The parser mode this crate uses. Named so a future lenient opt-in has to be a
/// deliberate, visible edit rather than an incidental one.
pub const MODE: RecordMode = RecordMode::Strict;
