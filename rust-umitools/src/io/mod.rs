//! Alignment, table and temporary-file I/O.

pub mod alignment;
pub mod fastq;
pub mod paired;
pub mod sort;
pub mod spill;

use crate::error::{Error, Result};
use rust_htslib::bam::{self, Record};
use std::path::Path;

/// Output format implied by the destination extension.
pub fn output_format(path: &Path) -> bam::Format {
    match path
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase())
        .as_deref()
    {
        Some("cram") => bam::Format::Cram,
        Some("sam") => bam::Format::Sam,
        _ => bam::Format::Bam,
    }
}

/// Translate `--compresslevel` into an htslib compression level.
pub fn compression_level(level: u32) -> Result<bam::CompressionLevel> {
    match level {
        0 => Ok(bam::CompressionLevel::Uncompressed),
        1..=9 => Ok(bam::CompressionLevel::Level(level)),
        _ => Err(Error::usage_with_hint(
            "--compresslevel must be between 0 and 9",
            "0 stores uncompressed BGZF blocks; the default is 6",
        )),
    }
}

/// Attach the reference a CRAM writer needs.
pub fn configure_writer(
    writer: &mut bam::Writer,
    format: bam::Format,
    reference: Option<&Path>,
) -> Result<()> {
    if format != bam::Format::Cram {
        return Ok(());
    }
    let reference = reference.ok_or_else(|| Error::Usage {
        message: "CRAM output needs --reference".to_string(),
        hint: Some("pass --reference genome.fa so the CRAM header can carry it".to_string()),
    })?;
    writer
        .set_reference(reference)
        .map_err(|e| Error::Format(format!("cannot use reference {}: {e}", reference.display())))
}

/// Attach a CRAM reference to a reader; BAM/SAM input ignores it.
pub fn set_reference(reader: &mut bam::Reader, reference: Option<&Path>) -> Result<()> {
    if let Some(path) = reference {
        reader
            .set_reference(path)
            .map_err(|e| Error::Format(format!("cannot use reference {}: {e}", path.display())))?;
    }
    Ok(())
}

/// Consumer for output records: either a direct writer or the streaming sorter.
pub trait RecordSink {
    fn submit(&mut self, record: Record) -> Result<()>;
}
