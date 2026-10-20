//! Machine readable run summary written by `--summary-json`.
//!
//! The document, its byte layout and its escaping are the workspace contract
//! and live in [`bioinfo_common::summary`]. This module keeps only what is
//! local to `rust-umitools`: how a summary is put together from this crate's
//! fields, and how it reaches the filesystem through this crate's atomic
//! temp-file type ([`crate::io::spill::TempOutput`]).

use crate::error::{Error, Result};
use crate::io::spill::TempOutput;
use bioinfo_common::summary::{Output, Summary as Contract};
use std::path::Path;

pub use bioinfo_common::summary::SCHEMA_VERSION;

/// The program name that appears in every summary this binary writes.
pub const PROGRAM: &str = "rust-umitools";

/// A run summary for this binary.
///
/// The fields mirror the shared contract one-for-one; [`Summary::contract`]
/// turns them into the document that is written.
pub struct Summary {
    pub command: &'static str,
    pub version: &'static str,
    pub input: String,
    pub outputs: Vec<(&'static str, String)>,
    pub threads: usize,
    pub memory_bytes: u64,
    pub chunk_bytes: u64,
    pub counters: Vec<(&'static str, u64)>,
    pub timings: Vec<(&'static str, f64)>,
}

impl Summary {
    /// Render the document, including the trailing newline.
    pub fn render(&self) -> String {
        self.contract().render()
    }

    /// The shared-contract view of this summary.
    pub fn contract(&self) -> Contract {
        Contract {
            program: PROGRAM,
            version: self.version,
            command: self.command,
            input: self.input.clone(),
            outputs: self
                .outputs
                .iter()
                .map(|(role, path)| Output::new(role, path.clone()))
                .collect(),
            threads: self.threads,
            memory_bytes: self.memory_bytes,
            chunk_bytes: self.chunk_bytes,
            counters: self.counters.clone(),
            timings: self.timings.clone(),
        }
    }

    /// Write the summary atomically next to its destination.
    ///
    /// [`TempOutput`] creates the temporary file and removes it again if the
    /// write or the rename fails, so a failed run never leaves a summary that
    /// looks complete.
    pub fn write(&self, destination: &Path) -> Result<()> {
        self.contract()
            .write_atomic(destination, |directory, bytes| {
                let temp = TempOutput::create(directory, "umitools-summary")?;
                std::fs::write(temp.path(), bytes).map_err(|e| Error::io(temp.path(), e))?;
                temp.commit(destination)
            })
    }
}

/// Resolve `--progress` into a boolean. `--quiet` always wins because progress
/// is informational output; the mode is still validated so a typo is reported.
pub fn progress_enabled(mode: &str, quiet: bool) -> Result<bool> {
    let enabled = match mode {
        "on" => true,
        "off" => false,
        "auto" => {
            (unsafe { libc::isatty(2) }) == 1 && std::env::var_os("RUST_UMITOOLS_QUIET").is_none()
        }
        other => {
            return Err(Error::usage_with_hint(
                format!("unknown --progress value '{other}'"),
                "choose one of: auto, on, off",
            ))
        }
    };
    Ok(enabled && !quiet)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Summary {
        Summary {
            command: "dedup",
            version: "0.2.0",
            input: "in \"quoted\".bam".to_string(),
            outputs: vec![("bam", "out.bam".to_string())],
            threads: 4,
            memory_bytes: 64 * 1_048_576,
            chunk_bytes: 8 * 1_048_576,
            counters: vec![("molecules", 3)],
            timings: vec![("wall_seconds", 1.25)],
        }
    }

    #[test]
    fn renders_valid_json_shapes() {
        let text = sample().render();
        assert!(text.contains("\"program\":\"rust-umitools\""));
        assert!(text.contains("\"command\":\"dedup\""));
        assert!(text.contains("\\\"quoted\\\""));
        assert!(text.contains("\"memory_mib\":64"));
        assert!(text.contains("\"molecules\":3"));
        assert!(text.ends_with("}\n"));
    }

    // The exact document the pre-refactor binary wrote for this invocation:
    //   $ rust-umitools count_tab -I in.tsv -S out.tsv --summary-json s.json
    // (resources depend on the machine; the structure below is the contract).
    #[test]
    fn matches_the_recorded_pre_refactor_document() {
        let summary = Summary {
            command: "count_tab",
            version: "0.2.0",
            input: "in.tsv".to_string(),
            outputs: vec![("tsv", "out.tsv".to_string())],
            threads: 16,
            memory_bytes: 9891 * 1_048_576,
            chunk_bytes: 201_326_592,
            counters: vec![("genes", 3)],
            timings: vec![("wall_seconds", 0.0)],
        };
        assert_eq!(
            summary.render(),
            "{\"program\":\"rust-umitools\",\"version\":\"0.2.0\",\"command\":\"count_tab\",\"input\":\"in.tsv\",\"outputs\":[{\"role\":\"tsv\",\"path\":\"out.tsv\"}],\"resources\":{\"threads\":16,\"memory_mib\":9891,\"chunk_mib\":192},\"counters\":{\"genes\":3},\"timings\":{\"wall_seconds\":0.000}}\n"
        );
    }

    #[test]
    fn contract_view_carries_program_and_version() {
        let contract = sample().contract();
        assert_eq!(contract.program, PROGRAM);
        assert_eq!(contract.version, "0.2.0");
        assert_eq!(contract.command, "dedup");
        assert_eq!(contract.outputs.len(), 1);
        assert_eq!(contract.outputs[0].role, "bam");
        assert_eq!(contract.counters, vec![("molecules", 3)]);
    }

    #[test]
    fn write_produces_the_rendered_bytes_on_disk() {
        let dir = std::env::temp_dir().join(format!(
            "umitools-summary-test-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let destination = dir.join("summary.json");
        let summary = sample();
        summary.write(&destination).expect("write summary");
        let written = std::fs::read(&destination).unwrap();
        assert_eq!(written, summary.render().into_bytes());
        // The temp file is gone after the commit.
        let leftovers: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|name| name.ends_with(".tmp"))
            .collect();
        assert!(
            leftovers.is_empty(),
            "temp files left behind: {leftovers:?}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn write_into_a_missing_directory_fails_and_leaves_nothing() {
        let dir =
            std::env::temp_dir().join(format!("umitools-summary-missing-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        let error = sample()
            .write(&dir.join("summary.json"))
            .expect_err("writing into a missing directory must fail");
        assert_eq!(error.exit_code(), 4, "an I/O failure is class 4");
        assert!(!dir.exists());
    }

    #[test]
    fn rejects_unknown_progress_mode() {
        assert!(progress_enabled("auto", false).is_ok());
        assert!(progress_enabled("sometimes", false).is_err());
        assert!(!progress_enabled("on", true).unwrap());
    }
}
