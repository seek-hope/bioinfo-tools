//! Explicit resource limits checked before QC statistics grow.

/// The limits accepted by the command line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QcLimits {
    /// Maximum accepted read length in bases.
    pub max_read_length: u64,
    /// Maximum number of distinct 50-base prefixes retained for duplication.
    pub max_distinct_sequences: u64,
}

/// Check one read before it is inserted into `QcStats`.
///
/// `prefix_is_new` is true when the 50-base, upper-cased prefix is not already in
/// the duplication map. A duplicate is allowed even when the map is at its cap.
pub fn check_read(
    length: usize,
    prefix_is_new: bool,
    distinct_prefixes: usize,
    limits: QcLimits,
) -> Result<(), String> {
    if length as u64 > limits.max_read_length {
        return Err(format!(
            "read length {length} exceeds --max-read-length {} (explicit resource limit)",
            limits.max_read_length
        ));
    }
    if prefix_is_new && distinct_prefixes as u64 >= limits.max_distinct_sequences {
        return Err(format!(
            "distinct sequence count would exceed --max-distinct-sequences {} (explicit resource limit)",
            limits.max_distinct_sequences
        ));
    }
    Ok(())
}
