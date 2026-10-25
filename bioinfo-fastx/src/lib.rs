//! Shared streaming FASTQ/FASTA reading and writing.
//!
//! This crate is the workspace's shared FASTX layer (development plan decision
//! **D3**: written and owned here rather than adopted from `needletail` or
//! `seq_io`, because the requirement is a *policy* layer — strict versus lenient
//! modes and diagnostics whose byte offsets match `rust-cutadapt`'s — not just a
//! parser).
//!
//! | Module | Responsibility |
//! | --- | --- |
//! | [`mod@format`] | magic-byte and extension detection |
//! | [`parser`] | strict and lenient FASTQ/FASTA record parsing |
//! | [`chunker`] | record-aligned chunking for the parallel pipelines |
//! | [`writer`] | FASTQ/FASTA serialisation |
//! | [`compress`] | decompression, compression and multi-member gzip |
//! | [`bgzf`] | BGZF block writer |
//! | [`reader`] | `Reader`/`Writer` facades |
//!
//! # Behaviour contract
//!
//! Migrating a caller must not change one output byte. Two things are pinned by
//! tests taken from the pre-migration behaviour:
//!
//! * the strict parser's diagnostics, including
//!   `malformed FASTQ at line N (byte offset M): ...`;
//! * the writer's layout, including echoing the input's separator line (`+r1`
//!   stays `+r1`) rather than normalising it to `+`.
//!
//! # Strict by default
//!
//! [`parser::Mode::Strict`] is the default, because the FASTQ format has no
//! specification and silently accepting a malformed record is worse than
//! refusing it (plan risk **R5**). Leniency is opt-in per reader.
//!
//! # What is deliberately not here
//!
//! * **No BGZF reader.** BGZF *is* gzip, so [`compress::decompress`] reads it
//!   through `MultiGzDecoder` with no special case. Adding a separate reader
//!   would create two paths that could disagree.
//! * **No `.gzi` index.** Random access belongs to the BAM work (W1-4).
//! * **No line-oriented FASTA joining in the borrowed API.** A multi-line FASTA
//!   record is reported rather than silently truncated; see
//!   [`parser::Parser::next_record`].

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod bgzf;
pub mod chunker;
pub mod compress;
pub mod format;
pub mod parser;
pub mod reader;
pub mod writer;

pub use chunker::{last_complete_record_end, split_records, RecordRange, DEFAULT_CHUNK_SIZE};
pub use compress::{compressor, compressor_for, decompress, Levels, MultiMemberGzWriter};
pub use format::{format_from_extension, Format};
pub use parser::{Mode, ParseError, Parser, Record};
pub use reader::{Chunk, Reader, Writer};
pub use writer::{write_record, Layout};

/// The crate's view of the strict-versus-lenient contract, for a `--help`
/// footer or an ADR cross-reference.
pub const STRICTNESS_CONTRACT: &str = "strict by default (rejects anything cutadapt 4.6 rejects, \
     with cutadapt's wording); lenient mode is opt-in and only relaxes byte-range rules, never \
     record structure";
