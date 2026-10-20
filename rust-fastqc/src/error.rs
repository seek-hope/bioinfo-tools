//! Error type mapped onto the shared exit-code contract (W1-1).

use std::fmt;

use bioinfo_common::ExitClass;

/// A `rust-fastqc` failure.
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
    pub fn input(message: impl Into<String>) -> Error {
        Error {
            class: ExitClass::Input,
            message: message.into(),
        }
    }

    /// An I/O failure (exit 4).
    pub fn io(message: impl Into<String>) -> Error {
        Error {
            class: ExitClass::Io,
            message: message.into(),
        }
    }

    /// The numeric exit code.
    pub fn exit_code(&self) -> i32 {
        self.class.code()
    }

    /// The message without a prefix.
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
        if error.kind() == std::io::ErrorKind::InvalidData {
            Error::runtime(error.to_string())
        } else {
            Error::io(error.to_string())
        }
    }
}

/// Alias.
pub type Result<T> = std::result::Result<T, Error>;

/// The exit classes this binary guarantees.
pub const EXIT_CLASSES: [ExitClass; 5] = [
    ExitClass::Success,
    ExitClass::Runtime,
    ExitClass::Usage,
    ExitClass::Input,
    ExitClass::Io,
];

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
    fn malformed_data_is_a_runtime_error_not_an_input_precondition() {
        let bad = std::io::Error::new(std::io::ErrorKind::InvalidData, "bad");
        assert_eq!(Error::from(bad).exit_code(), 1);
        let missing = std::io::Error::new(std::io::ErrorKind::NotFound, "gone");
        assert_eq!(Error::from(missing).exit_code(), 4);
    }
}
