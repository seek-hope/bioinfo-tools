//! Randomness abstraction for representative-read selection.
//!
//! Two strategies are offered and must not be confused:
//!
//! * [`crate::compat::pyrng::PyRandom`] — reproduces CPython's Mersenne Twister so that
//!   `--random-seed N` matches UMI-tools record for record.
//! * [`crate::compat::pyrng::StableRandom`] — the project's own deterministic stream,
//!   identical for every thread count but not bit-identical with upstream.

use std::fmt;

pub trait Rng: fmt::Debug {
    fn next_f64(&mut self) -> f64;
    /// Name used in logs and compatibility tables.
    fn label(&self) -> &'static str;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RepresentativeStrategy {
    /// Follow UMI-tools: CPython RNG semantics and input-order sampling.
    Upstream,
    /// Deterministic project stream; identical across thread counts.
    Stable,
}

impl RepresentativeStrategy {
    pub fn parse(raw: &str) -> crate::error::Result<RepresentativeStrategy> {
        match raw {
            "upstream" => Ok(RepresentativeStrategy::Upstream),
            "stable" => Ok(RepresentativeStrategy::Stable),
            other => Err(crate::error::Error::usage_with_hint(
                format!("unknown --representative '{other}'"),
                "choose one of: upstream, stable",
            )),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            RepresentativeStrategy::Upstream => "upstream",
            RepresentativeStrategy::Stable => "stable",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compat::pyrng::{PyRandom, StableRandom};

    #[test]
    fn strategies_parse_and_label() {
        assert_eq!(
            RepresentativeStrategy::parse("upstream").unwrap(),
            RepresentativeStrategy::Upstream
        );
        assert_eq!(
            RepresentativeStrategy::parse("stable").unwrap(),
            RepresentativeStrategy::Stable
        );
        assert!(RepresentativeStrategy::parse("random").is_err());
        let mut rng = PyRandom::from_python_seed(7);
        assert_eq!(var_label(&mut rng), "upstream");
        let mut stable = StableRandom::new(7);
        assert_eq!(var_label(&mut stable), "stable");
    }

    fn var_label(rng: &mut dyn Rng) -> &'static str {
        rng.label()
    }
}
