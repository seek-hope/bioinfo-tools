//! Structured errors and process exit codes.
//!
//! The exit-code numbers themselves are the workspace contract and live in
//! [`bioinfo_common::exit`]; this module only maps each error variant onto one
//! of those classes. See docs/IMPLEMENTATION_PLAN.md section 6.1 for the
//! per-tool statement of the contract.
//!
//! 0 success, 2 usage/capability error, 3 input precondition error,
//! 4 I/O or resource error. `rust-umitools` does not return 1: a data error is
//! classed as an input (3) or I/O (4) failure, exactly as before this module was
//! moved onto the shared types.

use std::fmt;
use std::path::PathBuf;

use bioinfo_common::diagnostic::Diagnostic;
use bioinfo_common::exit::ExitClass;

#[derive(Debug)]
pub enum Error {
    /// Bad command line or an option this build cannot honour.
    Usage {
        message: String,
        hint: Option<String>,
    },
    /// A documented capability that is recognised but not implemented.
    Unsupported { option: String, detail: String },
    /// Input data does not satisfy a documented precondition.
    Input(InputError),
    /// The configured resource budget cannot be satisfied.
    Resource(String),
    /// Filesystem or stream failure.
    Io {
        path: Option<PathBuf>,
        source: std::io::Error,
    },
    /// Alignment or table parsing failure.
    Format(String),
}

#[derive(Debug)]
pub enum InputError {
    NotSorted {
        record_index: u64,
        previous: i64,
        current: i64,
    },
    MixedUmiLengths {
        min: usize,
        max: usize,
        example: String,
    },
    MissingUmi {
        detail: String,
    },
    BadTable {
        line: u64,
        detail: String,
    },
}

impl Error {
    pub fn usage(message: impl Into<String>) -> Error {
        Error::Usage {
            message: message.into(),
            hint: None,
        }
    }

    pub fn usage_with_hint(message: impl Into<String>, hint: impl Into<String>) -> Error {
        Error::Usage {
            message: message.into(),
            hint: Some(hint.into()),
        }
    }

    pub fn unsupported(option: impl Into<String>, detail: impl Into<String>) -> Error {
        Error::Unsupported {
            option: option.into(),
            detail: detail.into(),
        }
    }

    pub fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Error {
        Error::Io {
            path: Some(path.into()),
            source,
        }
    }

    /// The exit-code class this error belongs to.
    ///
    /// The classification is the shared workspace contract; only the mapping
    /// from this crate's variants onto it is local.
    pub fn class(&self) -> ExitClass {
        match self {
            Error::Usage { .. } | Error::Unsupported { .. } => ExitClass::Usage,
            Error::Input(_) => ExitClass::Input,
            Error::Resource(_) | Error::Io { .. } | Error::Format(_) => ExitClass::Io,
        }
    }

    /// The numeric exit code, as the contract assigns it.
    pub fn exit_code(&self) -> i32 {
        self.class().code()
    }

    /// The `Diagnostic` a caller renders, so the message/hint split is shared.
    pub fn diagnostic(&self) -> Diagnostic {
        match self {
            Error::Usage { message, hint } => match hint {
                Some(hint) => Diagnostic::new(message.clone()).with_hint(hint.clone()),
                None => Diagnostic::new(message.clone()),
            },
            other => Diagnostic::new(other.to_string()),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Usage { .. } => self.diagnostic().fmt_body(f),
            Error::Unsupported { option, detail } => {
                write!(
                    f,
                    "'{option}' is recognised but not implemented in this build: {detail}"
                )
            }
            Error::Input(e) => write!(f, "{e}"),
            Error::Resource(m) => write!(f, "{m}"),
            Error::Io { path, source } => match path {
                Some(p) => write!(f, "{}: {source}", p.display()),
                None => write!(f, "{source}"),
            },
            Error::Format(m) => write!(f, "{m}"),
        }
    }
}

impl fmt::Display for InputError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InputError::NotSorted {
                record_index,
                previous,
                current,
            } => write!(
                f,
                "input is not sorted by coordinate: record {record_index} has position {current}, \
                 previous record had {previous}. Run 'samtools sort' first."
            ),
            InputError::MixedUmiLengths { min, max, example } => write!(
                f,
                "UMIs of different lengths inside one bundle ({min} and {max}, e.g. '{example}'). \
                 UMI-tools compares UMIs by Hamming distance and requires equal lengths within a \
                 position/strand bundle; fix the UMI extraction or the input data."
            ),
            InputError::MissingUmi { detail } => write!(f, "could not extract UMI: {detail}"),
            InputError::BadTable { line, detail } => write!(f, "line {line}: {detail}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<rust_htslib::errors::Error> for Error {
    fn from(e: rust_htslib::errors::Error) -> Self {
        Error::Format(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, Error>;

/// The classes this binary declares to a workflow engine.
///
/// Rendered by `--version --verbose`, so the printed contract and the returned
/// codes come from the same list.
pub const DECLARED_EXIT_CLASSES: [ExitClass; 4] = [
    ExitClass::Success,
    ExitClass::Usage,
    ExitClass::Input,
    ExitClass::Io,
];

#[cfg(test)]
mod tests {
    use super::*;
    use bioinfo_common::exit::{EXIT_INPUT, EXIT_IO, EXIT_OK, EXIT_USAGE};

    #[test]
    fn re_exported_codes_match_the_shared_contract() {
        assert_eq!(EXIT_OK, 0);
        assert_eq!(EXIT_USAGE, 2);
        assert_eq!(EXIT_INPUT, 3);
        assert_eq!(EXIT_IO, 4);
        // The binary does not use 1; no variant maps onto it.
        for error in [
            Error::usage("x"),
            Error::unsupported("--y", "z"),
            Error::Input(InputError::MissingUmi {
                detail: "d".to_string(),
            }),
            Error::Resource("r".to_string()),
            Error::io("p", std::io::Error::other("e")),
            Error::Format("f".to_string()),
        ] {
            assert_ne!(error.exit_code(), 1, "{error:?} must not exit 1");
        }
    }

    #[test]
    fn each_variant_maps_onto_its_documented_class() {
        assert_eq!(Error::usage("x").class(), ExitClass::Usage);
        assert_eq!(Error::unsupported("--x", "d").class(), ExitClass::Usage);
        assert_eq!(
            Error::Input(InputError::NotSorted {
                record_index: 1,
                previous: 5,
                current: 4
            })
            .class(),
            ExitClass::Input
        );
        assert_eq!(Error::Resource("r".to_string()).class(), ExitClass::Io);
        assert_eq!(
            Error::io("p", std::io::Error::other("e")).class(),
            ExitClass::Io
        );
        assert_eq!(Error::Format("f".to_string()).class(), ExitClass::Io);
        assert_eq!(Error::usage("x").exit_code(), EXIT_USAGE);
        assert_eq!(Error::usage("x").class().code(), EXIT_USAGE);
    }

    #[test]
    fn usage_hint_renders_through_the_shared_diagnostic() {
        let error = Error::usage_with_hint(
            "unknown --progress value 'sometimes'",
            "choose one of: auto, on, off",
        );
        assert_eq!(
            error.to_string(),
            "unknown --progress value 'sometimes'\n       choose one of: auto, on, off"
        );
        assert_eq!(
            error.diagnostic().render_error(),
            "ERROR: unknown --progress value 'sometimes'\n       choose one of: auto, on, off"
        );
        assert_eq!(
            error.diagnostic().message(),
            "unknown --progress value 'sometimes'"
        );
        assert_eq!(
            error.diagnostic().hint(),
            Some("choose one of: auto, on, off")
        );
    }

    #[test]
    fn declared_classes_render_the_historical_verbose_line() {
        assert_eq!(
            bioinfo_common::exit::describe_exit_codes(&DECLARED_EXIT_CLASSES),
            "0 ok, 2 usage, 3 input, 4 io/resource"
        );
    }

    // The exact text recorded from the pre-refactor binary for the three
    // diagnoses that the harness pins.
    #[test]
    fn recorded_diagnostics_are_unchanged() {
        assert_eq!(
            Error::usage_with_hint(
                "unknown --progress value 'sometimes'",
                "choose one of: auto, on, off"
            )
            .to_string(),
            "unknown --progress value 'sometimes'\n       choose one of: auto, on, off"
        );
        assert_eq!(
            Error::unsupported(
                "--output-stats",
                "the UMI-tools null model is not implemented, so this build will not emit \
                 statistics whose null columns would only repeat the observed values; drop the \
                 option and see docs/ROADMAP.md (R0-2)"
            )
            .to_string(),
            "'--output-stats' is recognised but not implemented in this build: the UMI-tools null \
             model is not implemented, so this build will not emit statistics whose null columns \
             would only repeat the observed values; drop the option and see docs/ROADMAP.md (R0-2)"
        );
        assert_eq!(
            Error::io("nope.tsv", std::io::Error::from_raw_os_error(2)).to_string(),
            "nope.tsv: No such file or directory (os error 2)"
        );
    }
}
