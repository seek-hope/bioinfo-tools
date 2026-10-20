//! Error type, mapped onto the shared exit-code contract from `bioinfo-common`.
//!
//! Every failure names its *class*; `bioinfo_common::ExitClass` decides the
//! number. That way `rust-bam` cannot drift from the other binaries in the
//! workspace, and a workflow engine can branch on one table.

use std::fmt;

use bioinfo_common::{ExitClass, EXIT_INPUT, EXIT_IO, EXIT_RUNTIME, EXIT_USAGE};

/// A `rust-bam` failure.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    class: ExitClass,
    message: String,
}

impl Error {
    /// A runtime or data error while processing (exit 1).
    pub fn runtime(message: impl Into<String>) -> Error {
        Error {
            class: ExitClass::Runtime,
            message: message.into(),
        }
    }

    /// A bad command line (exit 2).
    pub fn usage(message: impl Into<String>) -> Error {
        Error {
            class: ExitClass::Usage,
            message: message.into(),
        }
    }

    /// A violated input precondition (exit 3).
    ///
    /// Used for things `samtools` also treats as precondition failures: indexing
    /// an unsorted file, viewing an unindexed file by region, a truncated record.
    pub fn input(message: impl Into<String>) -> Error {
        Error {
            class: ExitClass::Input,
            message: message.into(),
        }
    }

    /// An I/O failure or resource exhaustion (exit 4).
    pub fn io(message: impl Into<String>) -> Error {
        Error {
            class: ExitClass::Io,
            message: message.into(),
        }
    }

    /// The numeric exit code this failure maps to.
    pub fn exit_code(&self) -> i32 {
        self.class.code()
    }

    /// The message, without the `ERROR: ` prefix.
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Self {
        // A missing or unreadable file is an I/O failure (4); a malformed record
        // is a data error (1).
        //
        // The distinction between 1 and 3 is deliberate and matches samtools:
        //
        //   1  a runtime or data error -- the file is unparseable or corrupt
        //      (`samtools view nh.sam` on a headerless SAM prints "no SQ lines
        //      present in the header" and exits 1)
        //   3  a violated input *precondition* -- the file is well-formed but the
        //      tool requires something it does not satisfy, e.g. `index` on an
        //      unsorted BAM, where the user's remedy is `sort` first
        //
        // Reserving 3 for actionable preconditions keeps it meaningful; a corrupt
        // file is not a precondition the caller can satisfy, it is bad data.
        if error.kind() == std::io::ErrorKind::InvalidData
            || error.kind() == std::io::ErrorKind::UnexpectedEof
        {
            Error::runtime(error.to_string())
        } else {
            Error::io(error.to_string())
        }
    }
}

/// Convenience alias.
pub type Result<T> = std::result::Result<T, Error>;

/// The exit classes this binary actually guarantees, in ascending order.
///
/// Declared so `--help` and the provenance block can state the contract, and so
/// the set is reviewable in one place.
pub const EXIT_CLASSES: [ExitClass; 5] = [
    ExitClass::Success,
    ExitClass::Runtime,
    ExitClass::Usage,
    ExitClass::Input,
    ExitClass::Io,
];

/// The codes above, as numbers, for tests.
pub const EXIT_CODES: [i32; 5] = [0, EXIT_RUNTIME, EXIT_USAGE, EXIT_INPUT, EXIT_IO];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classes_map_to_the_shared_contract() {
        assert_eq!(Error::runtime("x").exit_code(), 1);
        assert_eq!(Error::usage("x").exit_code(), 2);
        assert_eq!(Error::input("x").exit_code(), 3);
        assert_eq!(Error::io("x").exit_code(), 4);
    }

    #[test]
    fn declared_codes_are_the_shared_ones() {
        assert_eq!(EXIT_CODES, [0, 1, 2, 3, 4]);
        assert_eq!(EXIT_CLASSES.len(), EXIT_CODES.len());
    }

    #[test]
    fn malformed_input_is_a_data_error_but_a_missing_file_is_io() {
        // 1, not 3: a corrupt record is bad data, not a satisfiable precondition.
        let malformed = std::io::Error::new(std::io::ErrorKind::InvalidData, "bad record");
        assert_eq!(Error::from(malformed).exit_code(), 1);

        let missing = std::io::Error::new(std::io::ErrorKind::NotFound, "no such file");
        assert_eq!(Error::from(missing).exit_code(), 4);
    }

    #[test]
    fn the_message_survives_display() {
        let error = Error::usage("missing <input>");
        assert_eq!(error.to_string(), "missing <input>");
        assert_eq!(error.message(), "missing <input>");
    }
}
