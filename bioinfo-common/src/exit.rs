//! Process exit-code classes shared by every bioinfo-tools binary.
//!
//! The workspace plan (`docs/DEVELOPMENT-PLAN.md`, boundary rule 3) fixes one
//! contract for all binaries:
//!
//! | Code | Class | Meaning |
//! | --- | --- | --- |
//! | 0 | success | the requested work completed |
//! | 1 | runtime | a runtime or data error while processing |
//! | 2 | usage | bad command line, or a capability this build rejects |
//! | 3 | input | an input precondition was violated (unsorted, mixed UMI lengths) |
//! | 4 | I/O or resource | filesystem, stream or resource exhaustion |
//!
//! A binary declares the subset it actually guarantees. `rust-cutadapt` uses
//! 0/1/2 (it mirrors cutadapt 4.6, which has no other failure codes), and
//! `rust-umitools` uses 0/2/3/4. Keeping the numbers in one place means a
//! workflow engine can branch on them without per-tool tables.
//!
//! This module is data and formatting only: it never calls
//! [`std::process::exit`] and never writes to a stream. Only binaries exit.

use std::fmt;

/// Exit code for a successful run.
pub const EXIT_OK: i32 = 0;
/// Exit code for a runtime or data error discovered while processing.
pub const EXIT_RUNTIME: i32 = 1;
/// Exit code for a bad command line or a rejected capability.
pub const EXIT_USAGE: i32 = 2;
/// Exit code for a violated input precondition.
pub const EXIT_INPUT: i32 = 3;
/// Exit code for an I/O failure or resource exhaustion.
pub const EXIT_IO: i32 = 4;

/// A member of the shared exit-code contract.
///
/// The variant is the *class*, not the number: call sites name the reason for
/// the failure and this type decides which number the contract assigns to it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ExitClass {
    /// The requested work completed.
    Success,
    /// A runtime or data error discovered while processing.
    Runtime,
    /// A bad command line or a capability this build rejects.
    Usage,
    /// An input precondition was violated.
    Input,
    /// An I/O failure or resource exhaustion.
    Io,
}

impl ExitClass {
    /// Every class in ascending code order.
    pub const ALL: [ExitClass; 5] = [
        ExitClass::Success,
        ExitClass::Runtime,
        ExitClass::Usage,
        ExitClass::Input,
        ExitClass::Io,
    ];

    /// The numeric code this class is assigned by the contract.
    pub const fn code(self) -> i32 {
        match self {
            ExitClass::Success => EXIT_OK,
            ExitClass::Runtime => EXIT_RUNTIME,
            ExitClass::Usage => EXIT_USAGE,
            ExitClass::Input => EXIT_INPUT,
            ExitClass::Io => EXIT_IO,
        }
    }

    /// The code as the `u8` [`std::process::ExitCode`] wants.
    ///
    /// Every contracted code is below 256, so the conversion cannot truncate.
    pub const fn as_u8(self) -> u8 {
        self.code() as u8
    }

    /// A short human label, as used by a `--version --verbose` line.
    pub const fn label(self) -> &'static str {
        match self {
            ExitClass::Success => "ok",
            ExitClass::Runtime => "runtime",
            ExitClass::Usage => "usage",
            ExitClass::Input => "input",
            ExitClass::Io => "io/resource",
        }
    }

    /// The class a numeric code belongs to, or `None` outside the contract.
    pub const fn from_code(code: i32) -> Option<ExitClass> {
        match code {
            EXIT_OK => Some(ExitClass::Success),
            EXIT_RUNTIME => Some(ExitClass::Runtime),
            EXIT_USAGE => Some(ExitClass::Usage),
            EXIT_INPUT => Some(ExitClass::Input),
            EXIT_IO => Some(ExitClass::Io),
            _ => None,
        }
    }
}

/// Render a binary's declared classes as `0 ok, 2 usage, 3 input, ...`.
///
/// This is the exact text `rust-umitools --version --verbose` prints, so the
/// provenance line and this function can never drift apart.
pub fn describe_exit_codes(classes: &[ExitClass]) -> String {
    let mut out = String::new();
    for (index, class) in classes.iter().enumerate() {
        if index > 0 {
            out.push_str(", ");
        }
        out.push_str(&class.code().to_string());
        out.push(' ');
        out.push_str(class.label());
    }
    out
}

impl fmt::Display for ExitClass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.code(), self.label())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn codes_match_the_documented_contract() {
        assert_eq!(ExitClass::Success.code(), 0);
        assert_eq!(ExitClass::Runtime.code(), 1);
        assert_eq!(ExitClass::Usage.code(), 2);
        assert_eq!(ExitClass::Input.code(), 3);
        assert_eq!(ExitClass::Io.code(), 4);
        assert_eq!(EXIT_OK, 0);
        assert_eq!(EXIT_RUNTIME, 1);
        assert_eq!(EXIT_USAGE, 2);
        assert_eq!(EXIT_INPUT, 3);
        assert_eq!(EXIT_IO, 4);
    }

    #[test]
    fn every_code_round_trips_through_its_class() {
        for class in ExitClass::ALL {
            assert_eq!(ExitClass::from_code(class.code()), Some(class));
            assert_eq!(class.as_u8() as i32, class.code());
            assert!(u8::try_from(class.code()).is_ok(), "code fits in a u8");
        }
    }

    #[test]
    fn codes_outside_the_contract_have_no_class() {
        for code in [-1, 5, 64, 255] {
            assert_eq!(ExitClass::from_code(code), None, "code {code}");
        }
    }

    #[test]
    fn classes_are_distinct_and_ordered_by_code() {
        let mut seen: HashMap<i32, ExitClass> = HashMap::new();
        for class in ExitClass::ALL {
            assert_eq!(seen.insert(class.code(), class), None, "duplicate code");
        }
        // ALL is documented as ascending, and `Ord` follows the variant order.
        let codes: Vec<i32> = ExitClass::ALL.iter().map(|c| c.code()).collect();
        let mut sorted = codes.clone();
        sorted.sort_unstable();
        assert_eq!(codes, sorted);
        assert!(ExitClass::Success < ExitClass::Runtime);
        assert!(ExitClass::Io == ExitClass::Io);
    }

    // The exact line rust-umitools --version --verbose prints (recorded from the
    // pre-refactor binary). This is the reason the function exists.
    #[test]
    fn umitools_declared_classes_render_exactly() {
        let classes = [
            ExitClass::Success,
            ExitClass::Usage,
            ExitClass::Input,
            ExitClass::Io,
        ];
        assert_eq!(
            describe_exit_codes(&classes),
            "0 ok, 2 usage, 3 input, 4 io/resource"
        );
    }

    #[test]
    fn describe_renders_any_subset_in_the_order_given() {
        assert_eq!(describe_exit_codes(&[]), "");
        assert_eq!(
            describe_exit_codes(&[ExitClass::Success, ExitClass::Runtime, ExitClass::Usage]),
            "0 ok, 1 runtime, 2 usage"
        );
        // All five classes, ascending.
        assert_eq!(
            describe_exit_codes(&ExitClass::ALL),
            "0 ok, 1 runtime, 2 usage, 3 input, 4 io/resource"
        );
    }

    #[test]
    fn display_is_code_then_label() {
        assert_eq!(ExitClass::Usage.to_string(), "2 usage");
        assert_eq!(ExitClass::Io.to_string(), "4 io/resource");
    }
}
