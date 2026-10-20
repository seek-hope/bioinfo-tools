//! Diagnostic formatting shared by every bioinfo-tools binary.
//!
//! Two things are shared and two are deliberately not:
//!
//! * **Shared:** the `ERROR: ` and `WARNING: ` prefixes, the hint layout
//!   (`\n` followed by seven spaces, then the hint) and the rule that a
//!   diagnostic is one logical message which may span several lines.
//! * **Local:** which stream a diagnostic goes to, and the program name in
//!   front of a progress banner. `rust-cutadapt` mirrors cutadapt 4.6, which
//!   prints its own `ERROR:` lines on **stdout** once processing has started;
//!   `rust-umitools` prints them on **stderr**. A workflow script depends on
//!   that split, so the stream stays a per-binary decision.
//!
//! Nothing here writes to a stream or exits the process.

use std::fmt;

/// Width the hint block is indented to: `"ERROR: "` is seven characters, so a
/// continuation line lines up under the message that follows the prefix.
pub const HINT_INDENT: &str = "       ";

/// A message with an optional actionable hint.
///
/// The hint is rendered on its own line, indented by [`HINT_INDENT`]. It exists
/// so that "what went wrong" and "what to do" stay separate fields that a test
/// can assert on independently of the rendered text.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Diagnostic {
    message: String,
    hint: Option<String>,
}

impl Diagnostic {
    /// A diagnostic with no hint.
    pub fn new(message: impl Into<String>) -> Diagnostic {
        Diagnostic {
            message: message.into(),
            hint: None,
        }
    }

    /// Attach an actionable hint.
    pub fn with_hint(mut self, hint: impl Into<String>) -> Diagnostic {
        self.hint = Some(hint.into());
        self
    }

    /// The message without the prefix or the hint.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// The hint, if one was attached.
    pub fn hint(&self) -> Option<&str> {
        self.hint.as_deref()
    }

    /// The body: the message, then the indented hint on its own line.
    ///
    /// No prefix and no trailing newline.
    pub fn body(&self) -> String {
        match &self.hint {
            Some(hint) => format!("{}\n{HINT_INDENT}{hint}", self.message),
            None => self.message.clone(),
        }
    }

    /// The full diagnostic: `ERROR: ` plus [`Diagnostic::body`].
    pub fn render_error(&self) -> String {
        format!("{ERROR_PREFIX}{}", self.body())
    }

    /// The full diagnostic: `WARNING: ` plus [`Diagnostic::body`].
    pub fn render_warning(&self) -> String {
        format!("{WARNING_PREFIX}{}", self.body())
    }

    /// Write the body into `f` without allocating a second message.
    ///
    /// Public so a binary can implement `Display` for its own error type
    /// without rendering an intermediate `String`; the layout stays shared.
    pub fn fmt_body(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)?;
        if let Some(hint) = &self.hint {
            f.write_str("\n")?;
            f.write_str(HINT_INDENT)?;
            f.write_str(hint)?;
        }
        Ok(())
    }
}

/// The prefix every error line carries.
pub const ERROR_PREFIX: &str = "ERROR: ";
/// The prefix every warning line carries.
pub const WARNING_PREFIX: &str = "WARNING: ";

/// `ERROR: {message}` for a plain message.
///
/// Used where a binary already has a rendered message string (for example
/// cutadapt's `error: ...` text after its prefix has been stripped) and only
/// needs the shared prefix. No trailing newline.
pub fn error_line(message: &str) -> String {
    format!("{ERROR_PREFIX}{message}")
}

/// `WARNING: {message}` for a plain message. No trailing newline.
pub fn warning_line(message: &str) -> String {
    format!("{WARNING_PREFIX}{message}")
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.fmt_body(f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_message_has_no_hint_line() {
        let diagnostic = Diagnostic::new("input is not valid FASTQ");
        assert_eq!(diagnostic.message(), "input is not valid FASTQ");
        assert_eq!(diagnostic.hint(), None);
        assert_eq!(diagnostic.body(), "input is not valid FASTQ");
        assert_eq!(diagnostic.render_error(), "ERROR: input is not valid FASTQ");
        assert!(!diagnostic.render_error().ends_with('\n'));
    }

    // Recorded from the pre-refactor rust-umitools binary:
    //   $ rust-umitools group -I in.tsv -S o.bam --progress sometimes
    //   ERROR: unknown --progress value 'sometimes'
    //          choose one of: auto, on, off
    // The continuation is indented by len("ERROR: ") == 7 spaces.
    #[test]
    fn hint_is_indented_to_the_message_column() {
        let diagnostic = Diagnostic::new("unknown --progress value 'sometimes'")
            .with_hint("choose one of: auto, on, off");
        assert_eq!(diagnostic.message(), "unknown --progress value 'sometimes'");
        assert_eq!(diagnostic.hint(), Some("choose one of: auto, on, off"));
        assert_eq!(
            diagnostic.render_error(),
            "ERROR: unknown --progress value 'sometimes'\n       choose one of: auto, on, off"
        );
        assert_eq!(HINT_INDENT.len(), "ERROR: ".len());
    }

    #[test]
    fn warnings_use_the_warning_prefix() {
        let diagnostic = Diagnostic::new("skipped a UMI");
        assert_eq!(diagnostic.render_warning(), "WARNING: skipped a UMI");
        assert_eq!(warning_line("skipped a UMI"), "WARNING: skipped a UMI");
    }

    #[test]
    fn display_is_the_body_without_a_prefix() {
        let diagnostic = Diagnostic::new("bad value").with_hint("use 1 or 2");
        assert_eq!(diagnostic.to_string(), "bad value\n       use 1 or 2");
        assert_eq!(
            diagnostic.render_error(),
            "ERROR: bad value\n       use 1 or 2"
        );
    }

    #[test]
    fn multiline_messages_are_not_reindented() {
        // cutadapt appends a second paragraph to a diagnostic; only the hint is
        // indented, the message itself is passed through unchanged.
        let message = "error: cannot parse\nNote: the output file is incomplete: 3 record(s)";
        let diagnostic = Diagnostic::new(message);
        assert_eq!(diagnostic.render_error(), format!("ERROR: {message}"));
        assert_eq!(error_line(message), format!("ERROR: {message}"));
    }

    #[test]
    fn empty_message_still_renders_the_prefix() {
        assert_eq!(Diagnostic::new("").render_error(), "ERROR: ");
        assert_eq!(error_line(""), "ERROR: ");
    }

    #[test]
    fn hints_may_contain_newlines_and_are_passed_through() {
        let diagnostic = Diagnostic::new("bad").with_hint("first\nsecond");
        assert_eq!(
            diagnostic.render_error(),
            "ERROR: bad\n       first\nsecond"
        );
    }
}
