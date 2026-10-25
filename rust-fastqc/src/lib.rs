//! `rust-fastqc`: a FastQC-compatible QC prototype.
//!
//! Emits `fastqc_data.txt` in FastQC 0.13.0's format. HTML/plots are deliberately
//! out of scope (see `docs/EVIDENCE-QC.md`).
//!
//! ## Why the format is reproduced rather than approximated
//!
//! `fastqc_data.txt` is a machine-readable contract: MultiQC parses it, and any
//! byte difference in a field name or a numeric convention silently changes what
//! downstream tools report. Every value and field name here was therefore taken
//! from real FastQC 0.13.0 output on the fixture matrix, not from documentation.
//!
//! Two conventions are especially easy to get wrong and are pinned by tests:
//!
//! * **Per-base quality quartiles are `NaN` until a base has more than 100
//!   observations.** This is not a bug and not an artefact of small fixtures: the
//!   guard is in FastQC's own `PerBaseQualityScores.getPercentile`, which sums
//!   only over `qualityCounts[i]` whose `getTotalCount() > 100` and divides by the
//!   number of such groups. With none qualifying the divisor is zero and the
//!   result is `NaN`. Verified by decompiling the shipped 0.13.0 classes
//!   (`javap -c`) and by measuring the crossover at exactly 100 vs 101 reads.
//! * **Counts in `Per sequence GC content` and `Per sequence quality scores` are
//!   densities, not raw counts.** They are printed as floating point
//!   (`1.0`, `0.0`) even when integral.
//!
//! ## Modules emitted
//!
//! Present: Basic Statistics, Per base sequence quality, Per sequence quality
//! scores, Per base sequence content, Per base N content, Per sequence GC content,
//! Sequence Length Distribution, Sequence Duplication Levels, Overrepresented
//! sequences, Adapter Content (experimental exact-match subset).
//!
//! See [`modules`] for exactly which are computed and which are emitted as empty
//! placeholders, and [`absent`] for what the prototype omits.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod absent;
pub mod error;
pub mod grouping;
pub mod limits;
pub mod modules;
pub mod report;
pub mod stats;

pub use error::{Error, Result};
pub use report::{Report, ReportWriter};
pub use stats::QcStats;

/// The FastQC version this output format mirrors.
pub const FASTQC_VERSION: &str = "0.13.0";

/// The `##FastQC` header line, byte for byte.
pub const HEADER: &str = "##FastQC\t0.13.0";

/// The QC modules this prototype computes.
pub const COMPUTED_MODULES: [&str; 10] = [
    "Basic Statistics",
    "Per base sequence quality",
    "Per sequence quality scores",
    "Per base sequence content",
    "Per base N content",
    "Per sequence GC content",
    "Sequence Length Distribution",
    "Sequence Duplication Levels",
    "Overrepresented sequences",
    "Adapter Content",
];
