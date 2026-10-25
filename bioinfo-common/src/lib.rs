//! Shared foundation for the bioinfo-tools binaries.
//!
//! Four contracts that every binary in this workspace must honour live here, so
//! that they are defined once, unit-tested once, and cannot drift between
//! crates:
//!
//! | Module | Contract |
//! | --- | --- |
//! | [`exit`] | the 0/1/2/3/4 process exit-code classes |
//! | [`diagnostic`] | the `ERROR: ` / `WARNING: ` prefixes and the hint layout |
//! | [`summary`] | the single-line `--summary-json` document and its byte layout |
//! | [`provenance`] | the `--version --verbose` block and the `@PG` record |
//!
//! # Boundary rules
//!
//! This crate follows boundary rule 2 of the development plan literally:
//! **libraries never exit and never print.** Nothing here calls
//! [`std::process::exit`], reads [`mod@std::env`], or writes to stdout/stderr. Every
//! function returns data — a number, a `String`, a rendered document — and the
//! binary decides where it goes and with which status it leaves. That keeps the
//! contracts testable without spawning a process, and keeps
//! `rust-cutadapt`'s unusual "diagnostics on stdout after the banner" behaviour
//! a property of that binary rather than of the shared layer.
//!
//! # No dependencies
//!
//! The crate is deliberately std-only. The `--summary-json` byte contract is
//! produced by a hand-written renderer whose escaping differs from a general
//! JSON encoder (see [`summary::escape`]), so `serde_json` is not a substitute;
//! and a dependency-free foundation keeps every workspace build satisfiable with
//! `cargo --offline` and adds nothing to either existing binary's runtime set.
//! See `docs/EVIDENCE-W1-1.md` (DR-1).
//!
//! # What is *not* here
//!
//! * **cutadapt's `--json` report** — that is a compatibility contract with
//!   cutadapt 4.6's own schema, not this workspace's summary contract. It stays
//!   in `rust-cutadapt`, where its byte layout is pinned against the oracle.
//! * **Python `json.dumps` emulation** (`rust-cutadapt`'s `report::fmt`) — it
//!   escapes `\b`/`\f` with short forms, unlike [`summary::escape`]. Unifying
//!   them would change output bytes, so they stay separate.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod diagnostic;
pub mod exit;
pub mod provenance;
pub mod summary;

pub use diagnostic::{error_line, warning_line, Diagnostic, ERROR_PREFIX, WARNING_PREFIX};
pub use exit::{
    describe_exit_codes, ExitClass, EXIT_INPUT, EXIT_IO, EXIT_OK, EXIT_RUNTIME, EXIT_USAGE,
};
pub use provenance::{ProgramRecord, Provenance};
pub use summary::{directory_of, Output, Summary};

/// The exit-code contract as documentation, for a crate-level `#[doc]` or a
/// `--help` footer. Kept next to [`ExitClass`] so the prose and the code are
/// reviewed together.
pub const EXIT_CODE_CONTRACT: &str = "0 ok (success), 1 runtime (runtime/data error), \
     2 usage (usage or unsupported capability, rejected before any output is created), \
     3 input (input precondition violated), 4 io/resource (I/O or resource exhaustion)";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contract_text_lists_every_class_in_order() {
        for class in ExitClass::ALL {
            let needle = format!("{} {}", class.code(), class.label());
            assert!(
                EXIT_CODE_CONTRACT.contains(&needle),
                "`{needle}` missing from the contract text: {EXIT_CODE_CONTRACT}"
            );
            // The prose must also name the class, so a reader without the code
            // still knows what 1 and 4 mean.
            let prose = match class {
                ExitClass::Success => "success",
                ExitClass::Runtime => "runtime/data error",
                ExitClass::Usage => "usage or unsupported capability",
                ExitClass::Input => "input precondition violated",
                ExitClass::Io => "I/O or resource exhaustion",
            };
            assert!(
                EXIT_CODE_CONTRACT.contains(prose),
                "`{prose}` missing from the contract text: {EXIT_CODE_CONTRACT}"
            );
        }
        // The codes appear in ascending order.
        let positions: Vec<usize> = ExitClass::ALL
            .iter()
            .map(|c| EXIT_CODE_CONTRACT.find(&format!("{} ", c.code())).unwrap())
            .collect();
        let mut sorted = positions.clone();
        sorted.sort_unstable();
        assert_eq!(positions, sorted, "codes are out of order");
    }

    #[test]
    fn reexports_are_usable_from_the_crate_root() {
        let summary = Summary {
            program: "p",
            version: "1",
            command: "c",
            input: "-".to_string(),
            outputs: vec![Output::new("bam", "o.bam")],
            threads: 1,
            memory_bytes: 0,
            chunk_bytes: 0,
            counters: vec![],
            timings: vec![],
        };
        assert!(summary.render().starts_with("{\"program\":\"p\""));
        assert_eq!(ExitClass::Usage.code(), EXIT_USAGE);
        assert_eq!(error_line("x"), "ERROR: x");
        assert_eq!(warning_line("y"), "WARNING: y");
        assert_eq!(Provenance::new("p", "1").render(), "p 1\n");
        assert_eq!(ERROR_PREFIX, "ERROR: ");
        assert_eq!(WARNING_PREFIX, "WARNING: ");
        let record = ProgramRecord::new("p", "1", &[]);
        assert_eq!(record.id, "p");
    }
}
