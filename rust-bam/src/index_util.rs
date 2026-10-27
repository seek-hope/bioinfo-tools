//! BAM index construction for `rust-bam index`.
//!
//! `samtools index` writes a BAI (`.bam.bai`) by default. Building one requires
//! three things per record, which is why this is its own module rather than
//! inline in the command:
//!
//! 1. the **virtual offset** of the record's start, which only the BGZF layer
//!    knows;
//! 2. the record's **alignment interval** `(reference id, start, end)`;
//! 3. whether the record is **mapped** at all.
//!
//! ## Precondition: the file must be coordinate-sorted
//!
//! `samtools index` refuses an unsorted BAM. A BAI over unsorted records is not
//! merely useless — it produces wrong query results silently. `rust-bam index`
//! therefore checks the ordering as it goes and fails with an **exit-3 input
//! precondition** if it sees a record that sorts before its predecessor. This is
//! the same class `rust-umitools` uses for its own precondition failures.
//!
//! ## What is written
//!
//! A BAI or CSI is written through `noodles_bam::index`, selected by the
//! `build_bai`/`build_csi` entry points. Both formats are covered by region-query
//! integration tests.

use std::fs::File;
use std::io::BufWriter;
use std::path::{Path, PathBuf};

use noodles::bam;
use noodles::bam::index::Format;
use noodles::bgzf;
use noodles::sam;

use crate::error::{Error, Result};

/// The conventional BAI path for a BAM: `<path>.bai`.
///
/// `samtools index` produces `in.bam.bai` (not `in.bai`), so this matches.
pub fn bai_path(bam: &Path) -> PathBuf {
    let mut name = bam.as_os_str().to_os_string();
    name.push(".bai");
    PathBuf::from(name)
}

/// The conventional CSI path for a BAM: `<path>.csi`.
pub fn csi_path(bam: &Path) -> PathBuf {
    let mut name = bam.as_os_str().to_os_string();
    name.push(".csi");
    PathBuf::from(name)
}

/// Build a BAI for the BAM at `input` and write it to `output`.
///
/// Returns the number of records indexed, so the command can report it.
pub fn build_bai(input: &Path, output: &Path) -> Result<u64> {
    build_index(input, output, Format::Bai)
}

/// Build a CSI for the BAM at `input` and write it to `output`.
pub fn build_csi(input: &Path, output: &Path) -> Result<u64> {
    build_index(input, output, Format::Csi)
}

fn build_index(input: &Path, output: &Path, format: Format) -> Result<u64> {
    let file =
        File::open(input).map_err(|error| Error::io(format!("{}: {error}", input.display())))?;
    let mut reader = bam::io::Reader::from(bgzf::io::Reader::new(std::io::BufReader::new(file)));

    let header = reader.read_header().map_err(|error| {
        Error::input(format!("{}: cannot read header: {error}", input.display()))
    })?;
    let reference_count = header.reference_sequences().len();

    let mut indexer = bam::index::Indexer::builder()
        .set_format(format)
        .build()
        .map_err(|error| Error::runtime(format!("cannot create index builder: {error}")))?;
    let mut record = sam::alignment::RecordBuf::default();
    let mut count: u64 = 0;

    // The previous record's ordering key, for the sortedness precondition.
    let mut previous: Option<(usize, usize)> = None;

    loop {
        let start_offset = reader.get_ref().virtual_position();
        let bytes = reader
            .read_record_buf(&header, &mut record)
            .map_err(|error| Error::input(format!("{}: {error}", input.display())))?;
        if bytes == 0 {
            break;
        }

        let interval = alignment_context(&record);
        let end_offset = reader.get_ref().virtual_position();

        // Sortedness precondition: a placed record must not sort before the last
        // placed record. Unplaced records are allowed only at the end, which is
        // exactly what the coordinate order requires.
        if let Some((reference_id, start)) =
            interval.map(|(id, start, _, _)| (id, usize::from(start)))
        {
            if let Some((prev_id, prev_start)) = previous {
                if (reference_id, start) < (prev_id, prev_start) {
                    return Err(Error::input(format!(
                        "{}: cannot index an unsorted BAM: record {} is at reference {} position {}, \
                         after reference {} position {}; run `rust-bam sort` first",
                        input.display(),
                        count + 1,
                        reference_id,
                        start,
                        prev_id,
                        prev_start
                    )));
                }
            }
            previous = Some((reference_id, start));
        }

        let chunk = noodles::csi::binning_index::index::reference_sequence::bin::Chunk::new(
            start_offset,
            end_offset,
        );
        indexer.add_record(interval, chunk).map_err(|error| {
            Error::input(format!("{}: cannot index record: {error}", input.display()))
        })?;
        count += 1;
    }

    let index = indexer.build(reference_count);

    let out = File::create(output)
        .map_err(|error| Error::io(format!("{}: {error}", output.display())))?;
    match index {
        bam::index::Index::Bai(index) => {
            let mut writer = bam::bai::io::Writer::new(BufWriter::new(out));
            writer.write_index(&index).map_err(|error| {
                Error::io(format!("{}: cannot write BAI: {error}", output.display()))
            })?;
        }
        bam::index::Index::Csi(index) => {
            let mut writer = noodles::csi::io::Writer::new(BufWriter::new(out));
            writer.write_index(&index).map_err(|error| {
                Error::io(format!("{}: cannot write CSI: {error}", output.display()))
            })?;
        }
    }

    Ok(count)
}

/// The `(reference_id, start, end, is_mapped)` tuple the indexer wants.
///
/// Returns `None` for a record with no placeable interval, which the indexer
/// counts as an unplaced/unmapped record.
fn alignment_context(
    record: &sam::alignment::RecordBuf,
) -> Option<(
    usize,
    noodles::core::Position,
    noodles::core::Position,
    bool,
)> {
    let flags = record.flags();
    if flags.is_unmapped() {
        return None;
    }
    let reference_id = record.reference_sequence_id()?;
    let start = record.alignment_start()?;
    // `alignment_end` falls back to the start position for a zero-width CIGAR,
    // which is what the indexer needs (end >= start).
    let end = record.alignment_end().unwrap_or(start);
    Some((reference_id, start, end, true))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bai_path_appends_to_the_bam_name() {
        // samtools produces `in.bam.bai`, not `in.bai`.
        assert_eq!(
            bai_path(Path::new("/data/in.bam")),
            PathBuf::from("/data/in.bam.bai")
        );
        assert_eq!(
            bai_path(Path::new("relative.bam")),
            PathBuf::from("relative.bam.bai")
        );
    }
}
