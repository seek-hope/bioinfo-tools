//! The provenance block.
//!
//! A binary must be able to say exactly what it is and what it emulates. Two
//! surfaces carry that:
//!
//! 1. `--version --verbose` — a human-readable block on stdout.
//! 2. The `@PG` program record in a SAM/BAM header — machine-readable
//!    provenance inside the file, opt-in because it changes output bytes.
//!
//! # `--version --verbose` byte contract
//!
//! ```text
//! <program> <version>
//! <label padded to 17>: <value>
//! ...
//! ```
//!
//! The label field is a fixed [`LABEL_WIDTH`] columns, not the longest label
//! present, so adding or removing a field never re-aligns the others. Exactly
//! one trailing `\n` per line and no trailing blank line.
//!
//! Nothing here writes to a stream or exits the process: [`Provenance::render`]
//! returns the text and the binary decides where it goes.

use crate::exit::{describe_exit_codes, ExitClass};

/// Column the `:` separator is aligned to in a rendered block.
///
/// Fixed rather than computed: the block is a stable contract, so a new field
/// with a longer label must not move every colon.
pub const LABEL_WIDTH: usize = 17;

/// One labelled provenance field, e.g. `htslib : via rust-htslib`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Field {
    /// Left column label.
    pub label: &'static str,
    /// Right column value.
    pub value: String,
}

impl Field {
    /// A field with a borrowed label and an owned value.
    pub fn new(label: &'static str, value: impl Into<String>) -> Field {
        Field {
            label,
            value: value.into(),
        }
    }
}

/// The provenance block a binary prints for `--version --verbose`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Provenance {
    /// Binary name, e.g. `rust-umitools`.
    pub program: &'static str,
    /// Binary version, normally `env!("CARGO_PKG_VERSION")`.
    pub version: &'static str,
    /// Fields in the order they are printed.
    pub fields: Vec<Field>,
}

impl Provenance {
    /// A block with only the version line.
    pub fn new(program: &'static str, version: &'static str) -> Provenance {
        Provenance {
            program,
            version,
            fields: Vec::new(),
        }
    }

    /// Append a field.
    pub fn with_field(mut self, label: &'static str, value: impl Into<String>) -> Provenance {
        self.fields.push(Field::new(label, value));
        self
    }

    /// Append the `exit codes` field from the classes the binary declares.
    ///
    /// Keeping this derived from [`ExitClass`] means the printed contract and
    /// the codes the binary actually returns cannot drift apart.
    pub fn with_exit_codes(self, classes: &[ExitClass]) -> Provenance {
        self.with_field("exit codes", describe_exit_codes(classes))
    }

    /// Render the block: the version line, then one line per field.
    ///
    /// Every line ends with `\n`, including the last one; there is no blank line
    /// between the version line and the fields.
    pub fn render(&self) -> String {
        let mut out = String::with_capacity(256);
        out.push_str(self.program);
        out.push(' ');
        out.push_str(self.version);
        out.push('\n');
        for field in &self.fields {
            out.push_str(field.label);
            for _ in field.label.len()..LABEL_WIDTH {
                out.push(' ');
            }
            out.push_str(": ");
            out.push_str(&field.value);
            out.push('\n');
        }
        out
    }
}

/// The `@PG` record tags for a program record.
///
/// `ID` and `PN` are the program name, `VN` its version and `CL` its command
/// line. Returned as data so this crate stays free of an htslib dependency; the
/// binary that owns the BAM header turns these into a `HeaderRecord`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProgramRecord {
    /// `ID` tag.
    pub id: &'static str,
    /// `PN` tag.
    pub pn: &'static str,
    /// `VN` tag.
    pub vn: &'static str,
    /// `CL` tag: the full command line, argv joined by single spaces.
    pub cl: String,
}

impl ProgramRecord {
    /// Build the record for a program, version and raw argument list.
    ///
    /// `args` is the full `argv`, including the program name, matching the
    /// convention of a shell's `$0 $@`.
    pub fn new(program: &'static str, version: &'static str, args: &[String]) -> ProgramRecord {
        ProgramRecord {
            id: program,
            pn: program,
            vn: version,
            cl: args.join(" "),
        }
    }

    /// The record as `(tag, value)` pairs in the order they are written.
    pub fn tags(&self) -> [(&'static [u8], &str); 4] {
        [
            (b"ID", self.id),
            (b"PN", self.pn),
            (b"VN", self.vn),
            (b"CL", &self.cl),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Current rust-umitools provenance, used here to pin the shared layout:
    //   rust-umitools 0.2.0$
    //   reference        : UMI-tools 1.1.6 (3f2cb4f41e07fe4a6cea3b3389f534bee957176b)$
    //   library          : noodles 0.118.0 (pure Rust; BAM/SAM/CRAM)$
    //   parallel         : rayon worker pool, single-threaded BGZF output$
    //   commands         : whitelist, extract, dedup, group, count, count_tab$
    //   exit codes       : 0 ok, 2 usage, 3 input, 4 io/resource$
    fn umitools() -> Provenance {
        Provenance::new("rust-umitools", "0.2.0")
            .with_field(
                "reference",
                "UMI-tools 1.1.6 (3f2cb4f41e07fe4a6cea3b3389f534bee957176b)",
            )
            .with_field("library", "noodles 0.118.0 (pure Rust; BAM/SAM/CRAM)")
            .with_field("parallel", "rayon worker pool, single-threaded BGZF output")
            .with_field(
                "commands",
                "whitelist, extract, dedup, group, count, count_tab",
            )
            .with_exit_codes(&[
                ExitClass::Success,
                ExitClass::Usage,
                ExitClass::Input,
                ExitClass::Io,
            ])
    }

    #[test]
    fn renders_the_current_umitools_block_exactly() {
        assert_eq!(
            umitools().render(),
            concat!(
                "rust-umitools 0.2.0\n",
                "reference        : UMI-tools 1.1.6 (3f2cb4f41e07fe4a6cea3b3389f534bee957176b)\n",
                "library          : noodles 0.118.0 (pure Rust; BAM/SAM/CRAM)\n",
                "parallel         : rayon worker pool, single-threaded BGZF output\n",
                "commands         : whitelist, extract, dedup, group, count, count_tab\n",
                "exit codes       : 0 ok, 2 usage, 3 input, 4 io/resource\n",
            )
        );
    }

    #[test]
    fn separator_column_is_fixed_for_every_label() {
        let text = umitools().render();
        let lines: Vec<&str> = text.lines().skip(1).collect();
        assert_eq!(lines.len(), 5);
        for line in &lines {
            assert_eq!(
                line.find(':'),
                Some(LABEL_WIDTH),
                "colon column in {line:?} should be {LABEL_WIDTH}"
            );
        }
    }

    #[test]
    fn a_longer_label_does_not_move_the_other_colons() {
        let text = Provenance::new("p", "1")
            .with_field("a", "1")
            .with_field("a-much-longer-label-than-17", "2")
            .render();
        assert_eq!(
            text,
            format!(
                "p 1\n{:<17}: 1\n{}: 2\n",
                "a", "a-much-longer-label-than-17"
            )
        );
        // The short label keeps its original column even though a longer one exists.
        assert_eq!(text.lines().nth(1).unwrap().find(':'), Some(LABEL_WIDTH));
    }

    #[test]
    fn render_ends_with_one_newline_per_line_and_no_blank_line() {
        let text = umitools().render();
        assert!(text.ends_with('\n'));
        assert!(!text.ends_with("\n\n"));
        assert_eq!(text.matches('\n').count(), 6);
    }

    #[test]
    fn a_block_with_no_fields_is_just_the_version_line() {
        assert_eq!(
            Provenance::new("rust-cutadapt", "0.1.0").render(),
            "rust-cutadapt 0.1.0\n"
        );
    }

    #[test]
    fn exit_codes_field_follows_the_declared_classes() {
        let text = Provenance::new("p", "1")
            .with_exit_codes(&[ExitClass::Success, ExitClass::Runtime, ExitClass::Usage])
            .render();
        assert_eq!(text, "p 1\nexit codes       : 0 ok, 1 runtime, 2 usage\n");
    }

    #[test]
    fn program_record_carries_all_four_tags_in_order() {
        let args = vec![
            "rust-umitools".to_string(),
            "dedup".to_string(),
            "-I".to_string(),
            "in.bam".to_string(),
        ];
        let record = ProgramRecord::new("rust-umitools", "0.2.0", &args);
        assert_eq!(record.id, "rust-umitools");
        assert_eq!(record.pn, "rust-umitools");
        assert_eq!(record.vn, "0.2.0");
        assert_eq!(record.cl, "rust-umitools dedup -I in.bam");
        let tags = record.tags();
        assert_eq!(tags[0], (&b"ID"[..], "rust-umitools"));
        assert_eq!(tags[1], (&b"PN"[..], "rust-umitools"));
        assert_eq!(tags[2], (&b"VN"[..], "0.2.0"));
        assert_eq!(tags[3], (&b"CL"[..], "rust-umitools dedup -I in.bam"));
    }

    #[test]
    fn program_record_joins_arguments_with_single_spaces() {
        let record = ProgramRecord::new("p", "1", &["p".to_string()]);
        assert_eq!(record.cl, "p");
        let empty = ProgramRecord::new("p", "1", &[]);
        assert_eq!(empty.cl, "");
    }
}
