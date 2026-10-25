//! `rust-bam`: SAM/BAM inspection and manipulation built on **noodles 0.118.0**.
//!
//! Implements the six `samtools`-shaped subcommands required by
//! `docs/DEVELOPMENT-PLAN.md` W1-4: `view`, `flagstat`, `idxstats`,
//! `quickcheck`, `index` and `sort`.
//!
//! ## Design decisions
//!
//! **D2 (noodles, not htslib).** The plan requires new SAM/BAM work to use
//! noodles so the build stays pure Rust and does not re-introduce the htslib C
//! toolchain or its CVE surface. `rust-htslib` is deliberately absent from the
//! dependency list. This means the differential against `samtools` is a genuine
//! independent-implementation comparison, not a wrapper measuring itself.
//!
//! **MSRV 1.91.** noodles declares `rust-version = "1.91.0"` on itself and every
//! sub-crate, so the workspace floor was raised from 1.88 to 1.91. See
//! `DR-W1-4-2` in `docs/EVIDENCE-W1-4.md`.
//!
//! **Shared contracts.** Exit codes, diagnostics and the provenance block come
//! from `bioinfo-common` (W1-1) rather than being re-implemented here.
//!
//! ## Exit codes
//!
//! | Code | Meaning in this tool |
//! | --- | --- |
//! | 0 | the requested work completed |
//! | 1 | a runtime or data error while processing |
//! | 2 | bad command line |
//! | 3 | an input precondition was violated (e.g. `index` on an unsorted BAM) |
//! | 4 | an I/O failure |

#![forbid(unsafe_code)]

pub mod commands;
pub mod error;
pub mod format;
pub mod index_util;
pub mod io_util;
pub mod sort_order;
pub mod stats;

pub use error::{Error, Result};

/// The subcommands this binary implements, in the order the plan lists them.
pub const COMMANDS: [&str; 6] = [
    "view",
    "flagstat",
    "idxstats",
    "quickcheck",
    "index",
    "sort",
];
