//! The `--summary-json` contract.
//!
//! A run summary is a small, stable, single-line JSON document that a workflow
//! manager can read without parsing the human log. It is the shared machine
//! readable surface of every bioinfo-tools binary (plan boundary rule 4).
//!
//! # Schema (version 1)
//!
//! ```json
//! {"program":"<name>","version":"<ver>","command":"<sub>","input":"<path>",
//!  "outputs":[{"role":"<role>","path":"<path>"}],
//!  "resources":{"threads":<n>,"memory_mib":<n>,"chunk_mib":<n>},
//!  "counters":{"<name>":<int>},
//!  "timings":{"<name>":<float3>}}
//! ```
//!
//! # Byte contract
//!
//! The document is rendered by hand rather than through a JSON library, and the
//! exact bytes are part of the contract:
//!
//! * no whitespace anywhere except inside string values;
//! * keys in the order above, insertion order inside `outputs`, `counters` and
//!   `timings`;
//! * integers bare, timings with exactly three decimals (`{:.3}`);
//! * exactly one trailing `\n`.
//!
//! [`escape`] follows the escaping this contract shipped with, which is *not*
//! identical to a general JSON encoder: `\b` and `\f` are emitted as `\u0008`
//! and `\u000c`, not as their short forms. Changing that would change bytes, so
//! it is documented here and pinned by a test. `serde_json` is therefore not a
//! drop-in replacement, which is one reason this crate has no dependencies.
//!
//! Nothing here writes to a stream or exits the process.

use std::fmt::Write as _;
use std::path::Path;

/// Schema version of the summary document.
pub const SCHEMA_VERSION: u64 = 1;

/// One named output artifact of a run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Output {
    /// What the file is, e.g. `bam` or `tsv`.
    pub role: &'static str,
    /// Where it was written, exactly as the user named it.
    pub path: String,
}

impl Output {
    /// A `(role, path)` output.
    pub fn new(role: &'static str, path: impl Into<String>) -> Output {
        Output {
            role,
            path: path.into(),
        }
    }
}

/// A run summary, rendered as described in the module docs.
///
/// `PartialEq` but not `Eq`: `timings` holds `f64`.
#[derive(Clone, Debug, PartialEq)]
pub struct Summary {
    /// Binary name, e.g. `rust-umitools`.
    pub program: &'static str,
    /// Binary version, normally `env!("CARGO_PKG_VERSION")`.
    pub version: &'static str,
    /// Subcommand that produced this summary.
    pub command: &'static str,
    /// Input path as the user named it, or `-` for a stream.
    pub input: String,
    /// Outputs in the order the command declares them.
    pub outputs: Vec<Output>,
    /// Worker threads actually used.
    pub threads: usize,
    /// Memory budget in bytes; rendered as whole MiB.
    pub memory_bytes: u64,
    /// Sort/batch chunk size in bytes; rendered as whole MiB.
    pub chunk_bytes: u64,
    /// Run counters in insertion order.
    pub counters: Vec<(&'static str, u64)>,
    /// Run timings in seconds, in insertion order.
    pub timings: Vec<(&'static str, f64)>,
}

impl Summary {
    /// Render the document, including the trailing newline.
    pub fn render(&self) -> String {
        let mut out = String::with_capacity(512);
        out.push_str("{\"program\":\"");
        out.push_str(&escape(self.program));
        out.push_str("\",\"version\":\"");
        out.push_str(&escape(self.version));
        out.push_str("\",\"command\":\"");
        out.push_str(&escape(self.command));
        out.push_str("\",\"input\":\"");
        out.push_str(&escape(&self.input));
        out.push_str("\",\"outputs\":[");
        for (index, output) in self.outputs.iter().enumerate() {
            if index > 0 {
                out.push(',');
            }
            let _ = write!(
                out,
                "{{\"role\":\"{}\",\"path\":\"{}\"}}",
                escape(output.role),
                escape(&output.path)
            );
        }
        out.push_str("],\"resources\":{");
        let _ = write!(
            out,
            "\"threads\":{},\"memory_mib\":{},\"chunk_mib\":{}",
            self.threads,
            self.memory_bytes / 1_048_576,
            self.chunk_bytes / 1_048_576
        );
        out.push_str("},\"counters\":{");
        for (index, (name, value)) in self.counters.iter().enumerate() {
            if index > 0 {
                out.push(',');
            }
            let _ = write!(out, "\"{}\":{}", escape(name), value);
        }
        out.push_str("},\"timings\":{");
        for (index, (name, value)) in self.timings.iter().enumerate() {
            if index > 0 {
                out.push(',');
            }
            let _ = write!(out, "\"{}\":{:.3}", escape(name), value);
        }
        out.push_str("}}\n");
        out
    }

    /// Hand the rendered document to `write`, together with the directory it
    /// must be staged in.
    ///
    /// The crate owns the bytes and the "a bare file name means `.`" rule; the
    /// calling binary owns the filesystem mechanics, because it already has a
    /// temp-file type with atomic commit and its own error type. Splitting it
    /// this way keeps every byte contract here and every failure mode there.
    pub fn write_atomic<E>(
        &self,
        destination: &Path,
        write: impl FnOnce(&Path, &[u8]) -> Result<(), E>,
    ) -> Result<(), E> {
        let directory = directory_of(destination);
        write(&directory, self.render().as_bytes())
    }
}

/// The directory a destination lives in; a bare file name means `.`.
pub fn directory_of(destination: &Path) -> std::path::PathBuf {
    destination
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| std::path::PathBuf::from("."))
}

/// Escape a string for the summary document.
///
/// This is the escaping the contract shipped with. `"` `\` `\n` `\r` `\t` get
/// their short forms, every other C0 control character is written as a
/// four-digit `\u` escape (so backspace is `\u0008`, not `\b`). Everything else,
/// including non-ASCII, is passed through as UTF-8.
pub fn escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            other if (other as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", other as u32);
            }
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Summary {
        Summary {
            program: "rust-umitools",
            version: "0.2.0",
            command: "dedup",
            input: "in.bam".to_string(),
            outputs: vec![Output::new("bam", "out.bam")],
            threads: 4,
            memory_bytes: 64 * 1_048_576,
            chunk_bytes: 8 * 1_048_576,
            counters: vec![("molecules", 3)],
            timings: vec![("wall_seconds", 1.25)],
        }
    }

    // Byte-for-byte shape of the document. Every field, separator, the key
    // order, the three-decimal timing and the single trailing newline are
    // asserted literally so a refactor cannot drift.
    #[test]
    fn renders_the_exact_contract_bytes() {
        assert_eq!(
            sample().render(),
            concat!(
                "{\"program\":\"rust-umitools\",\"version\":\"0.2.0\",\"command\":\"dedup\",",
                "\"input\":\"in.bam\",\"outputs\":[{\"role\":\"bam\",\"path\":\"out.bam\"}],",
                "\"resources\":{\"threads\":4,\"memory_mib\":64,\"chunk_mib\":8},",
                "\"counters\":{\"molecules\":3},\"timings\":{\"wall_seconds\":1.250}}\n"
            )
        );
    }

    // Real record from the pre-refactor binary:
    //   $ rust-umitools count_tab -I in.tsv -S out.tsv --summary-json s.json
    //   {"program":"rust-umitools","version":"0.2.0","command":"count_tab",
    //    "input":"in.tsv","outputs":[{"role":"tsv","path":"out.tsv"}],
    //    "resources":{"threads":16,"memory_mib":9891,"chunk_mib":192},
    //    "counters":{"genes":3},"timings":{"wall_seconds":0.000}}
    #[test]
    fn matches_the_recorded_pre_refactor_bytes() {
        let summary = Summary {
            program: "rust-umitools",
            version: "0.2.0",
            command: "count_tab",
            input: "in.tsv".to_string(),
            outputs: vec![Output::new("tsv", "out.tsv")],
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
    fn empty_lists_render_as_empty_containers() {
        let summary = Summary {
            outputs: Vec::new(),
            counters: Vec::new(),
            timings: Vec::new(),
            ..sample()
        };
        let text = summary.render();
        assert!(text.contains("\"outputs\":[],"), "{text}");
        assert!(text.contains("\"counters\":{},"), "{text}");
        assert!(text.contains("\"timings\":{}}"), "{text}");
    }

    #[test]
    fn entries_keep_insertion_order() {
        let summary = Summary {
            outputs: vec![Output::new("a", "1"), Output::new("b", "2")],
            counters: vec![("z", 1), ("a", 2), ("m", 3)],
            timings: vec![("late", 2.0), ("early", 1.0)],
            ..sample()
        };
        let text = summary.render();
        assert!(text.contains(
            "\"outputs\":[{\"role\":\"a\",\"path\":\"1\"},{\"role\":\"b\",\"path\":\"2\"}]"
        ));
        assert!(text.contains("\"counters\":{\"z\":1,\"a\":2,\"m\":3}"));
        assert!(text.contains("\"timings\":{\"late\":2.000,\"early\":1.000}"));
    }

    #[test]
    fn timings_use_exactly_three_decimals() {
        let summary = Summary {
            timings: vec![("t", 1.23456), ("u", 0.0), ("v", 12.0)],
            ..sample()
        };
        let text = summary.render();
        assert!(text.contains("\"t\":1.235"), "{text}");
        assert!(text.contains("\"u\":0.000"), "{text}");
        assert!(text.contains("\"v\":12.000"), "{text}");
    }

    #[test]
    fn byte_budgets_render_as_whole_mib_rounded_down() {
        let summary = Summary {
            memory_bytes: 1_048_575,
            chunk_bytes: 0,
            ..sample()
        };
        let text = summary.render();
        assert!(text.contains("\"memory_mib\":0,"), "{text}");
        assert!(text.contains("\"chunk_mib\":0}"), "{text}");
    }

    #[test]
    fn escaping_uses_the_shipped_short_forms() {
        assert_eq!(escape("plain"), "plain");
        assert_eq!(escape("a\"b"), "a\\\"b");
        assert_eq!(escape("a\\b"), "a\\\\b");
        assert_eq!(escape("a\nb"), "a\\nb");
        assert_eq!(escape("a\rb"), "a\\rb");
        assert_eq!(escape("a\tb"), "a\\tb");
    }

    // Pinned because it differs from a general JSON encoder: `\b` and `\f`
    // have short forms in JSON, but this contract writes them as `\u` escapes.
    #[test]
    fn escaping_uses_four_digit_escapes_for_other_controls() {
        assert_eq!(escape("a\u{08}b"), "a\\u0008b");
        assert_eq!(escape("a\u{0c}b"), "a\\u000cb");
        assert_eq!(escape("a\u{00}b"), "a\\u0000b");
        assert_eq!(escape("a\u{1f}b"), "a\\u001fb");
    }

    #[test]
    fn escaping_passes_utf8_through() {
        assert_eq!(escape("dédup-±"), "dédup-±");
        // Non-ASCII is not escaped, and no ASCII byte is produced for it.
        assert!(escape("é").len() > 1);
    }

    #[test]
    fn a_quoted_path_stays_inside_its_string() {
        let summary = Summary {
            input: "in \"quoted\".bam".to_string(),
            ..sample()
        };
        let text = summary.render();
        assert!(
            text.contains("\"input\":\"in \\\"quoted\\\".bam\""),
            "{text}"
        );
        // The document still has the expected number of quotes: 2 per string
        // value plus the escaped ones, so a naive split would not work; the
        // real check is that the escaped quote count matches.
        assert_eq!(text.matches("\\\"").count(), 2);
    }

    #[test]
    fn document_ends_with_exactly_one_newline() {
        let text = sample().render();
        assert!(text.ends_with("}\n"));
        assert!(!text.ends_with("\n\n"));
        assert!(!text.contains('\n') || text.matches('\n').count() == 1);
    }

    #[test]
    fn directory_of_handles_bare_names_and_paths() {
        use std::path::PathBuf;
        assert_eq!(directory_of(Path::new("s.json")), PathBuf::from("."));
        assert_eq!(
            directory_of(Path::new("/tmp/a/s.json")),
            PathBuf::from("/tmp/a")
        );
        assert_eq!(directory_of(Path::new("./s.json")), PathBuf::from("."));
    }

    // The write path hands the caller exactly the rendered bytes and the
    // staging directory derived from the destination.
    #[test]
    fn write_atomic_hands_over_the_rendered_bytes_and_staging_directory() {
        let summary = sample();
        let mut seen: Option<(std::path::PathBuf, Vec<u8>)> = None;
        summary
            .write_atomic(Path::new("/out/s.json"), |directory, bytes| {
                seen = Some((directory.to_path_buf(), bytes.to_vec()));
                Ok::<(), std::io::Error>(())
            })
            .expect("write succeeds");
        let (directory, bytes) = seen.expect("callback ran");
        assert_eq!(directory, std::path::PathBuf::from("/out"));
        assert_eq!(bytes, summary.render().into_bytes());
    }

    #[test]
    fn write_atomic_uses_dot_for_a_bare_file_name() {
        let summary = sample();
        let mut directory_seen = None;
        summary
            .write_atomic(Path::new("s.json"), |directory, _| {
                directory_seen = Some(directory.to_path_buf());
                Ok::<(), std::io::Error>(())
            })
            .expect("write succeeds");
        assert_eq!(directory_seen, Some(std::path::PathBuf::from(".")));
    }

    #[test]
    fn write_atomic_propagates_the_writer_error() {
        let summary = sample();
        let result = summary.write_atomic(Path::new("/nonexistent-dir/s.json"), |_, _| {
            Err::<(), _>(std::io::Error::other("cannot create temp file"))
        });
        let error = result.expect_err("a writer failure must propagate");
        assert_eq!(error.to_string(), "cannot create temp file");
    }
}
