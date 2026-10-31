//! `rust-fastqc`: FastQC-compatible QC reports.
//!
//! Emits `fastqc_data.txt` in FastQC 0.13.0's format, plus `summary.txt`, a
//! data-table HTML report and a stored ZIP. Plot images are not emitted
//! (see `docs/EVIDENCE-QC.md`).
//!
//! ## Why the format is reproduced rather than approximated
//!
//! `fastqc_data.txt` is a machine-readable contract: MultiQC parses it, and any
//! byte difference in a field name or a numeric convention silently changes what
//! downstream tools report. Field names and numeric conventions are checked
//! against pinned FastQC output. Proven upstream counting defects are corrected
//! explicitly and independently tested; see `docs/EVIDENCE-FASTQC-ADAPTERS.md`
//! in the workspace for the adapter differences.
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
//! Present: Basic Statistics, Per base sequence quality, Per tile sequence quality
//! (when sampled headers contain tile IDs), Per sequence quality scores, Per base sequence content,
//! Per base N content, Per sequence GC content,
//! Sequence Length Distribution, Sequence Duplication Levels, Overrepresented
//! sequences, Adapter Content (default or custom literal adapters).
//!
//! See [`modules`] for the emitted modules and [`absent`] for the remaining gaps.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod absent;
pub mod adapters;
pub mod contaminants;
pub mod error;
pub mod grouping;
pub mod limits;
pub mod modules;
pub mod report;
pub mod stats;
pub mod tile;
mod zip;

pub use error::{Error, Result};
pub use report::{Report, ReportWriter};
pub use stats::QcStats;

/// The FastQC version this output format mirrors.
pub const FASTQC_VERSION: &str = "0.13.0";

/// The `##FastQC` header line, byte for byte.
pub const HEADER: &str = "##FastQC\t0.13.0";

/// The QC modules this implementation computes.
pub const COMPUTED_MODULES: [&str; 11] = [
    "Basic Statistics",
    "Per base sequence quality",
    "Per tile sequence quality",
    "Per sequence quality scores",
    "Per base sequence content",
    "Per base N content",
    "Per sequence GC content",
    "Sequence Length Distribution",
    "Sequence Duplication Levels",
    "Overrepresented sequences",
    "Adapter Content",
];
