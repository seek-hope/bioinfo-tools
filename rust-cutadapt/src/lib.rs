//! rust-cutadapt: an implementation of the documented cutadapt 4.6
//! command-line surface for adapter trimming of FASTQ and FASTA reads.
//!
//! The library exposes the pieces behind the CLI: [`cli`] parses the option
//! surface, [`adapter`] and [`align`] find adapters, [`modify`] and [`filter`]
//! transform and filter reads, [`pair`] coordinates mates, and [`pipeline`]
//! streams records through the writers and report assembly.

#![forbid(unsafe_code)]

pub mod adapter;
pub mod align;
pub mod auxout;
pub mod cli;
pub mod compress;
pub mod engine;
pub mod fastq;
pub mod filter;
pub mod modify;
pub mod pair;
pub mod pipeline;
pub mod report;
pub mod runreport;
pub mod trim;
