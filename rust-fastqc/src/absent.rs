//! FastQC modules intentionally absent from this prototype.
//!
//! This module exists so the omission is a documented decision with a reason,
//! rather than an unexplained gap. It is data, so a reviewer can check the claim
//! mechanically instead of reading prose.
//!
//! Scope reminder: this prototype emits **`fastqc_data.txt` only**. No HTML, no
//! plots, no zip archive.

/// A module FastQC 0.13.0 has that this prototype does not implement.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AbsentModule {
    /// The module name FastQC uses.
    pub name: &'static str,
    /// Why it is absent.
    pub reason: &'static str,
    /// What would be needed to add it.
    pub requirement: &'static str,
}

/// Every FastQC 0.13.0 module this prototype omits.
pub const ABSENT_MODULES: [AbsentModule; 4] = [
    AbsentModule {
        name: "Adapter Content",
        reason: "FastQC measures adapter content by k-mer matching against its own \
                 adapter list with position-dependent tolerances; the list ships in \
                 Configuration/adapter_list.txt and the matching rules are not \
                 documented. Reproducing the numbers without the oracle's own list \
                 would produce plausible-looking values that disagree with FastQC, \
                 which is worse than omitting the module.",
        requirement: "Port adapter_list.txt into the crate and reverse-engineer the \
                      k-mer tolerance rules against the oracle.",
    },
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
    AbsentModule {
        name: "Per base sequence quality (grouped bases for long reads)",
        reason: "FastQC groups base positions into bins once reads exceed 50bp, using \
                 BaseGroup.makeBaseGroups with linear, exponential and logarithmic \
                 strategies. This prototype reports **every position** (`--nogroup` \
                 behaviour), so for reads longer than 50bp its per-base tables are \
                 longer than FastQC's default output.",
        requirement: "Port BaseGroup.makeBaseGroups and the grouping strategy selection. \
                      Until then, compare against FastQC run with --nogroup.",
    },
];

/// The names of the absent modules, for a quick assertion.
pub fn absent_module_names() -> Vec<&'static str> {
    ABSENT_MODULES.iter().map(|module| module.name).collect()
}

/// A single human-readable summary of what is missing, for `--version --verbose`
/// and for the evidence document.
pub fn summary() -> String {
    let mut out = String::from("Modules intentionally absent from this prototype:\n");
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
    fn adapter_content_is_documented_as_absent() {
        // The report emits an empty Adapter Content block, so the reason for that
        // emptiness must be recorded here.
        assert!(absent_module_names().contains(&"Adapter Content"));
    }
}
