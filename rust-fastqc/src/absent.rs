//! FastQC modules that are not implemented.
//!
//! The report bundle contains data, summary and data-table HTML/ZIP artifacts;
//! plot images are not emitted.

/// A FastQC 0.13.0 module that this implementation does not emit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AbsentModule {
    /// The module name FastQC uses.
    pub name: &'static str,
    /// Why it is absent.
    pub reason: &'static str,
    /// What would be needed to add it.
    pub requirement: &'static str,
}

/// FastQC 0.13.0 modules that are not implemented.
pub const ABSENT_MODULES: [AbsentModule; 2] = [
    AbsentModule {
        name: "Kmer Content",
        reason: "FastQC reports position-specific k-mer enrichment using a binomial \
                 test with its own multiple-testing correction. The binning and the \
                 correction are not documented, and the module's thresholds are \
                 empirically tuned.",
        requirement: "Reverse-engineer the k-mer binning and the enrichment statistic \
                      against the oracle, with fixtures designed to isolate each rule.",
    },
    AbsentModule {
        name: "Per tile sequence quality",
        reason: "Requires Illumina tile coordinates parsed out of the read header \
                 (the `instrument:run:flowcell:lane:tile:x:y` convention) plus a \
                 per-tile flow-cell layout. Only meaningful for Illumina data with \
                 that header format.",
        requirement: "Implement tile extraction from the CASAVA header and the per-tile \
                      deviation calculation.",
    },
];

/// The names of the absent modules, for a quick assertion.
pub fn absent_module_names() -> Vec<&'static str> {
    ABSENT_MODULES.iter().map(|module| module.name).collect()
}

/// A single human-readable summary of what is missing, for `--version --verbose`
/// and for the evidence document.
pub fn summary() -> String {
    let mut out = String::from("Modules not implemented:\n");
    for module in ABSENT_MODULES {
        out.push_str(&format!("  - {}: {}\n", module.name, module.reason));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_absent_module_has_a_reason_and_a_requirement() {
        for module in ABSENT_MODULES {
            assert!(!module.name.is_empty());
            assert!(
                module.reason.len() > 40,
                "{} needs a substantive reason",
                module.name
            );
            assert!(
                module.requirement.len() > 20,
                "{} needs a stated requirement",
                module.name
            );
            // Names must not exceed the documented limit, or drift silently.
            assert!(
                module.reason.len() < 800,
                "{} reason is rambling",
                module.name
            );
        }
    }

    #[test]
    fn absent_modules_are_not_also_computed() {
        // A module cannot be both implemented and documented as absent.
        for absent in absent_module_names() {
            for computed in crate::COMPUTED_MODULES {
                assert_ne!(absent, computed, "{absent} is both absent and computed");
            }
        }
    }

    #[test]
    fn the_summary_mentions_every_module() {
        let text = summary();
        for module in ABSENT_MODULES {
            assert!(text.contains(module.name), "{text}");
        }
    }

    #[test]
    fn adapter_content_is_no_longer_absent() {
        // It is now computed as an experimental exact-match subset.
        assert!(!absent_module_names().contains(&"Adapter Content"));
    }
}
