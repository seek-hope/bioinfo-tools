//! cutadapt 4.6 report generation.
//!
//! The module is split so that the formatting concerns stay separate from the
//! data model:
//!
//! | Module | Contents |
//! | --- | --- |
//! | [`stats`] | The input data model ([`stats::Stats`] and friends). Passive: it describes what to print, never how it was computed. |
//! | [`fmt`] | Number formatting that reproduces Python's `str.format`, `round()` and `json.dumps` for floats. |
//! | [`text`] | `--report full` ([`text::full_report`]) and `--report minimal` ([text::minimal_report]). |
//! | [`json`] | `--json` ([`json::to_json`]) with cutadapt's schema and `null` semantics. |
//!
//! All entry points are pure functions from [`stats::Stats`] to `String` (or to
//! [`json::JsonValue`]); they do not read the clock, the environment, the
//! file system or any pipeline state.
//!
//! # Example
//!
//! ```
//! use rust_cutadapt::report::{json, stats::Stats, text};
//!
//! let stats = Stats::default();
//! assert_eq!(text::full_report(&stats), "No reads processed!");
//! assert_eq!(text::minimal_report(&stats).lines().count(), 2);
//! let meta = json::Meta::new("in.fastq", vec!["-a".to_string(), "ACGT".to_string()]);
//! assert!(json::to_json(&stats, &meta).starts_with("{\n  \"tag\": \"Cutadapt report\","));
//! ```

pub mod fmt;
pub mod json;
pub mod stats;
pub mod text;

pub use json::{to_json, JsonValue, Meta};
pub use stats::Stats;
pub use text::{full_report, minimal_report};
