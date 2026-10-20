//! Report assembly and emission.
//!
//! Produces `fastqc_data.txt` byte for byte in FastQC 0.13.0's layout. That file
//! is the only output of this prototype; HTML and plots are out of scope.

use crate::modules::{all_modules, ModuleResult};
use crate::stats::QcStats;
use crate::HEADER;

/// A complete QC report for one input file.
pub struct Report {
    /// The module results, in FastQC's order.
    pub modules: Vec<ModuleResult>,
}

impl Report {
    /// Build a report from accumulated statistics.
    pub fn from_stats(stats: &QcStats) -> Report {
        Report {
            modules: all_modules(stats),
        }
    }

    /// Render the whole `fastqc_data.txt`.
    ///
    /// The layout is: the `##FastQC` version line, then each module block, with no
    /// blank lines between blocks. FastQC ends the file with the final
    /// `>>END_MODULE` line and a newline.
    pub fn render(&self) -> String {
        let mut out = String::new();
        out.push_str(HEADER);
        out.push('\n');
        for module in &self.modules {
            out.push_str(&module.render());
        }
        out
    }

    /// The module names in report order.
    pub fn module_names(&self) -> Vec<&'static str> {
        self.modules.iter().map(|module| module.name).collect()
    }
}

/// Writes a report to a path, creating parent directories.
pub struct ReportWriter;

impl ReportWriter {
    /// Write `report` to `path`.
    pub fn write(report: &Report, path: &std::path::Path) -> crate::error::Result<()> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).map_err(|error| {
                    crate::error::Error::io(format!("{}: {error}", parent.display()))
                })?;
            }
        }
        std::fs::write(path, report.render())
            .map_err(|error| crate::error::Error::io(format!("{}: {error}", path.display())))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_report_starts_with_the_fastqc_version_line() {
        let stats = QcStats::new("x.fq");
        let report = Report::from_stats(&stats);
        let text = report.render();
        assert!(text.starts_with("##FastQC\t0.13.0\n"), "{text}");
    }

    #[test]
    fn every_module_block_is_terminated() {
        let stats = QcStats::new("x.fq");
        let report = Report::from_stats(&stats);
        let text = report.render();
        let opens = text.matches(">>").count();
        let ends = text.matches(">>END_MODULE").count();
        // Each module contributes one opening '>>' plus one '>>END_MODULE'.
        assert_eq!(ends, report.modules.len());
        assert_eq!(opens, report.modules.len() * 2);
    }

    #[test]
    fn modules_appear_in_fastqc_order() {
        let stats = QcStats::new("x.fq");
        let report = Report::from_stats(&stats);
        assert_eq!(
            report.module_names(),
            vec![
                "Basic Statistics",
                "Per base sequence quality",
                "Per sequence quality scores",
                "Per base sequence content",
                // GC content precedes N content in FastQC's report order.
                "Per sequence GC content",
                "Per base N content",
                "Sequence Length Distribution",
                "Sequence Duplication Levels",
                "Overrepresented sequences",
                "Adapter Content",
            ]
        );
    }
}
