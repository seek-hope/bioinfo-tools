//! The six subcommands: `view`, `flagstat`, `idxstats`, `quickcheck`, `index`, `sort`.
//!
//! Each function is a thin layer: parse options, call into the streaming reader /
//! writer or the counting types, and emit output in `samtools`'s exact format.
//! The counting and ordering logic lives in `crate::stats` and
//! `crate::sort_order` so it can be unit-tested without touching the filesystem.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use noodles::sam;

use crate::error::{Error, Result};
use crate::format::{detect_path, Format};
use crate::index_util;
use crate::io_util::{AlignmentReader, BamWriter, Record, SamWriter};
use crate::sort_order::{self, SortKey};
use crate::stats::{FlagStat, IdxStats};

// --------------------------------------------------------------------------- //
// helpers
// --------------------------------------------------------------------------- //

/// Translate a header reference id into its name.
///
/// Returns an owned `String` because noodles stores header names as `BString`,
/// which has no borrowed `&str` view. The allocation is once per record field and
/// only in `flagstat`, whose cost is dominated by flag arithmetic.
///
/// Returns `None` for an id that is absent or out of range, so an out-of-spec
/// record cannot panic the counting loops.
fn reference_name(header: &sam::Header, id: Option<usize>) -> Option<String> {
    let id = id?;
    header
        .reference_sequences()
        .iter()
        .nth(id)
        .map(|(name, _)| String::from_utf8_lossy(name.as_ref()).into_owned())
}

/// Parse a `--reference` style id from a record, tolerating an error.
fn tolerant_reference_id(record: &Record) -> Option<usize> {
    record.reference_sequence_id()
}

// --------------------------------------------------------------------------- //
// view
// --------------------------------------------------------------------------- //

/// Options for `view`.
pub struct ViewOptions {
    /// Input path; `-` means standard input.
    pub input: PathBuf,
    /// Output path; `None` means standard output.
    pub output: Option<PathBuf>,
    /// `-h`: include the header in text output.
    pub with_header: bool,
    /// `-b`: write BAM instead of SAM text.
    pub bam_output: bool,
    /// `-H`: print only the header.
    pub header_only: bool,
    /// `-c`: print only the count of matching records.
    pub count_only: bool,
}

/// `view`: emit SAM text (default), BAM (`-b`), a header (`-H`) or a count (`-c`).
pub fn view(options: ViewOptions) -> Result<()> {
    let mut reader = AlignmentReader::open(&options.input)?;
    let header = reader.read_header()?;

    // `-H` prints just the header, in SAM text, ignoring `-b` as samtools does.
    if options.header_only {
        let mut out = open_writer(options.output.as_deref())?;
        let mut writer = SamWriter::new(&mut out);
        writer.write_header(&header)?;
        writer.finish()?;
        out.flush()
            .map_err(|error| Error::io(format!("cannot flush output: {error}")))?;
        return Ok(());
    }

    // `-c` counts without writing records, so it wins over `-b`.
    if options.count_only {
        let mut count: u64 = 0;
        let mut record = Record::default();
        loop {
            let bytes = reader.read_record(&header, &mut record)?;
            if bytes == 0 {
                break;
            }
            count += 1;
        }
        let mut out = open_writer(options.output.as_deref())?;
        writeln!(out, "{count}")
            .map_err(|error| Error::io(format!("cannot write output: {error}")))?;
        out.flush()
            .map_err(|error| Error::io(format!("cannot flush output: {error}")))?;
        return Ok(());
    }

    if options.bam_output {
        let out = open_writer(options.output.as_deref())?;
        let mut writer = BamWriter::new(out);
        writer.write_header(&header)?;
        let mut record = Record::default();
        loop {
            let bytes = reader.read_record(&header, &mut record)?;
            if bytes == 0 {
                break;
            }
            writer.write_record(&header, &record)?;
        }
        // `finish` writes the BGZF EOF marker; without it `samtools quickcheck`
        // fails with "missing EOF block".
        let mut out = writer.finish()?;
        out.flush()
            .map_err(|error| Error::io(format!("cannot flush output: {error}")))?;
        return Ok(());
    }

    // SAM text.
    let mut out = open_writer(options.output.as_deref())?;
    {
        let mut writer = SamWriter::new(&mut out);
        if options.with_header {
            writer.write_header(&header)?;
        }
        let mut record = Record::default();
        loop {
            let bytes = reader.read_record(&header, &mut record)?;
            if bytes == 0 {
                break;
            }
            writer.write_record(&header, &record)?;
        }
        writer.finish()?;
    }
    out.flush()
        .map_err(|error| Error::io(format!("cannot flush output: {error}")))?;
    Ok(())
}

fn open_writer(path: Option<&Path>) -> Result<Box<dyn Write>> {
    match path {
        None => Ok(Box::new(BufWriter::new(std::io::stdout()))),
        Some(path) => {
            let file = File::create(path)
                .map_err(|error| Error::io(format!("{}: {error}", path.display())))?;
            Ok(Box::new(BufWriter::new(file)))
        }
    }
}

// --------------------------------------------------------------------------- //
// flagstat
// --------------------------------------------------------------------------- //

/// `flagstat`: the 16 counters samtools prints, in its order and wording.
pub fn flagstat(input: &Path) -> Result<String> {
    let mut reader = AlignmentReader::open(input)?;
    let header = reader.read_header()?;

    let mut stat = FlagStat::default();
    let mut record = Record::default();
    loop {
        let bytes = reader.read_record(&header, &mut record)?;
        if bytes == 0 {
            break;
        }
        let flags = record.flags();
        let reference = reference_name(&header, tolerant_reference_id(&record));
        let mate_reference = reference_name(&header, record.mate_reference_sequence_id());
        let mapping_quality = record.mapping_quality().map(|q| q.get());
        stat.add(
            flags,
            reference.as_deref(),
            mate_reference.as_deref(),
            mapping_quality,
        );
    }
    Ok(render_flagstat(&stat))
}

/// Render `FlagStat` in `samtools flagstat`'s exact wording.
///
/// The `+ 0` halves are the QC-failed counts. `rust-bam` has no `-f` QC filter,
/// so the failed column is always zero, which is what samtools prints with no
/// filter applied.
pub fn render_flagstat(stat: &FlagStat) -> String {
    let mut out = String::new();
    // A plain free function rather than a closure: a `FnMut` capturing `out`
    // would hold a mutable borrow across the `out.push_str` calls below.
    fn line(out: &mut String, label: &str, passed: u64, failed: u64) {
        out.push_str(&format!("{passed} + {failed} {label}\n"));
    }
    line(
        &mut out,
        "in total (QC-passed reads + QC-failed reads)",
        stat.total,
        0,
    );
    line(&mut out, "primary", stat.primary, 0);
    line(&mut out, "secondary", stat.secondary, 0);
    line(&mut out, "supplementary", stat.supplementary, 0);
    line(&mut out, "duplicates", stat.duplicates, 0);
    line(&mut out, "primary duplicates", stat.primary_duplicates, 0);

    // The percentages: samtools prints N/A when the denominator is zero.
    let pct = |numerator: u64, denominator: u64| -> String {
        if denominator == 0 {
            "N/A".to_string()
        } else {
            format!("{:.2}%", numerator as f64 * 100.0 / denominator as f64)
        }
    };
    out.push_str(&format!(
        "{0} + 0 mapped ({1} : N/A)\n",
        stat.mapped,
        pct(stat.mapped, stat.total)
    ));
    out.push_str(&format!(
        "{0} + 0 primary mapped ({1} : N/A)\n",
        stat.primary_mapped,
        pct(stat.primary_mapped, stat.primary)
    ));
    line(&mut out, "paired in sequencing", stat.paired, 0);
    line(&mut out, "read1", stat.read1, 0);
    line(&mut out, "read2", stat.read2, 0);
    out.push_str(&format!(
        "{0} + 0 properly paired ({1} : N/A)\n",
        stat.properly_paired,
        pct(stat.properly_paired, stat.paired)
    ));
    line(
        &mut out,
        "with itself and mate mapped",
        stat.with_itself_and_mate_mapped,
        0,
    );
    out.push_str(&format!(
        "{0} + 0 singletons ({1} : N/A)\n",
        stat.singletons,
        pct(stat.singletons, stat.paired)
    ));
    line(
        &mut out,
        "with mate mapped to a different chr",
        stat.mate_different_chr,
        0,
    );
    line(
        &mut out,
        "with mate mapped to a different chr (mapQ>=5)",
        stat.mate_different_chr_mapq5,
        0,
    );
    out
}

// --------------------------------------------------------------------------- //
// idxstats
// --------------------------------------------------------------------------- //

/// `idxstats`: per-reference mapped/unmapped counts plus the trailing `*` line.
pub fn idxstats(input: &Path) -> Result<String> {
    let mut reader = AlignmentReader::open(input)?;
    let header = reader.read_header()?;

    let mut stats = IdxStats::from_references(header.reference_sequences().iter().map(
        |(name, reference)| {
            (
                String::from_utf8_lossy(name.as_ref()).into_owned(),
                reference.length().get() as u64,
            )
        },
    ));

    let mut record = Record::default();
    loop {
        let bytes = reader.read_record(&header, &mut record)?;
        if bytes == 0 {
            break;
        }
        let is_unmapped = record.flags().is_unmapped();
        let reference_id = tolerant_reference_id(&record);
        stats.add(reference_id, is_unmapped);
    }
    Ok(render_idxstats(&stats))
}

/// Render `IdxStats` in `samtools idxstats`'s format: tab-separated, one line per
/// reference, then the `*` line.
pub fn render_idxstats(stats: &IdxStats) -> String {
    let mut out = String::new();
    for reference in &stats.references {
        out.push_str(&format!(
            "{}\t{}\t{}\t{}\n",
            reference.name, reference.length, reference.mapped, reference.unmapped
        ));
    }
    out.push_str(&format!(
        "*\t0\t{}\t{}\n",
        stats.no_coordinate.0, stats.no_coordinate.1
    ));
    out
}

// --------------------------------------------------------------------------- //
// quickcheck
// --------------------------------------------------------------------------- //

/// The BGZF EOF marker, as htslib compares it.
///
/// htslib checks the **final 28 bytes** against this exact pattern, `XFL = 0`
/// included. A stream whose marker differs in one byte fails — the defect W1-4
/// found in `bioinfo-fastx`, where gzip `XFL` was set to 2 from the compression
/// level and `samtools quickcheck` exited 16 while `samtools view` still worked.
pub const BGZF_EOF_MARKER: [u8; 28] = [
    0x1f, 0x8b, 0x08, 0x04, 0x00, 0x00, 0x00, 0x00, 0x00, 0xff, 0x06, 0x00, 0x42, 0x43, 0x02, 0x00,
    0x1b, 0x00, 0x03, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

/// The result of a quickcheck run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QuickCheck {
    /// Files that passed.
    pub passed: Vec<PathBuf>,
    /// `(file, reason)` for each failure.
    pub failed: Vec<(PathBuf, String)>,
}

impl QuickCheck {
    /// True when every file passed.
    pub fn ok(&self) -> bool {
        self.failed.is_empty()
    }

    /// `samtools quickcheck`'s exit code: 0 on success, 16 on any failure.
    pub fn exit_code(&self) -> i32 {
        if self.ok() {
            0
        } else {
            16
        }
    }
}

/// `quickcheck`: cheap structural validation without decoding every record.
///
/// Mirrors what `samtools quickcheck` verifies:
///
/// * the file is non-empty;
/// * a BAM has the BAM magic;
/// * a BAM ends with the 28-byte BGZF EOF marker;
/// * the header can be read and declares at least one reference sequence
///   (samtools warns "has no targets" and fails).
pub fn quickcheck(paths: &[PathBuf]) -> Result<QuickCheck> {
    let mut result = QuickCheck {
        passed: Vec::new(),
        failed: Vec::new(),
    };

    for path in paths {
        match quickcheck_one(path) {
            Ok(()) => result.passed.push(path.clone()),
            Err(reason) => result.failed.push((path.clone(), reason)),
        }
    }
    Ok(result)
}

fn quickcheck_one(path: &Path) -> std::result::Result<(), String> {
    let metadata = std::fs::metadata(path).map_err(|error| format!("{error}"))?;
    if metadata.len() == 0 {
        return Err("is empty".to_string());
    }

    let format = detect_path(path).map_err(|error| error.message().to_string())?;
    if format == Format::Cram {
        return Err("is CRAM, which this build does not support".to_string());
    }

    let mut reader = AlignmentReader::open(path).map_err(|error| error.message().to_string())?;
    let header = reader
        .read_header()
        .map_err(|error| format!("cannot read header: {}", error.message()))?;

    if header.reference_sequences().is_empty() {
        return Err("has no targets in header".to_string());
    }

    // The EOF marker check applies to BAM only: it is a BGZF concept.
    if format == Format::Bam {
        check_bgzf_eof(path)?;
    }

    Ok(())
}

/// Verify the final 28 bytes are htslib's exact BGZF EOF marker.
fn check_bgzf_eof(path: &Path) -> std::result::Result<(), String> {
    use std::io::{Read, Seek, SeekFrom};

    let mut file = File::open(path).map_err(|error| format!("{error}"))?;
    let length = file
        .seek(SeekFrom::End(0))
        .map_err(|error| format!("{error}"))?;
    if length < BGZF_EOF_MARKER.len() as u64 {
        return Err("is truncated: shorter than a BGZF EOF block".to_string());
    }
    file.seek(SeekFrom::End(-(BGZF_EOF_MARKER.len() as i64)))
        .map_err(|error| format!("{error}"))?;
    let mut tail = [0u8; BGZF_EOF_MARKER.len()];
    file.read_exact(&mut tail)
        .map_err(|error| format!("{error}"))?;
    if tail != BGZF_EOF_MARKER {
        return Err("was missing EOF block when one should be present".to_string());
    }
    Ok(())
}

// --------------------------------------------------------------------------- //
// index
// --------------------------------------------------------------------------- //

/// `index`: write a BAI next to the BAM.
///
/// Returns the path written and the number of records indexed.
pub fn index(input: &Path, output: Option<PathBuf>) -> Result<(PathBuf, u64)> {
    let format = detect_path(input)?;
    if format != Format::Bam {
        return Err(Error::usage(format!(
            "index needs a BAM (BGZF) file; {} is {}",
            input.display(),
            format.as_str()
        )));
    }
    let output = output.unwrap_or_else(|| index_util::bai_path(input));
    let count = index_util::build_bai(input, &output)?;
    Ok((output, count))
}

// --------------------------------------------------------------------------- //
// sort
// --------------------------------------------------------------------------- //

/// Options for `sort`.
pub struct SortOptions {
    /// Input path.
    pub input: PathBuf,
    /// Output path; when `None` the result goes to standard output as BAM.
    pub output: Option<PathBuf>,
    /// Write SAM text instead of BAM.
    pub sam_output: bool,
}

/// `sort`: coordinate-sort records into the documented order.
///
/// Fully described in `crate::sort_order`. Records are buffered because sorting
/// needs random access; memory is proportional to the file, which is recorded as
/// a limitation rather than hidden.
pub fn sort(options: SortOptions) -> Result<u64> {
    let mut reader = AlignmentReader::open(&options.input)?;
    let header = reader.read_header()?;

    let mut keyed: Vec<(SortKey, Record)> = Vec::new();
    let mut record = Record::default();
    let mut index: usize = 0;
    loop {
        let bytes = reader.read_record(&header, &mut record)?;
        if bytes == 0 {
            break;
        }
        let key = SortKey::new(
            record.flags(),
            tolerant_reference_id(&record),
            record.alignment_start().map(usize::from),
            record.name().map(|n| n.as_ref()).unwrap_or(&[]),
            index,
        );
        keyed.push((key, record.clone()));
        index += 1;
    }
    let total = keyed.len() as u64;
    let sorted = sort_order::sort_by_key(keyed);

    // The header's sort-order field must say what we did. `samtools sort` sets
    // `@HD SO:coordinate`; leaving a stale `SO:unsorted` would be a lie.
    let header = header_with_coordinate_sort(header);

    if options.sam_output {
        let mut out = open_writer(options.output.as_deref())?;
        {
            let mut writer = SamWriter::new(&mut out);
            writer.write_header(&header)?;
            for record in &sorted {
                writer.write_record(&header, record)?;
            }
            writer.finish()?;
        }
        out.flush()
            .map_err(|error| Error::io(format!("cannot flush output: {error}")))?;
    } else {
        let out = open_writer(options.output.as_deref())?;
        let mut writer = BamWriter::new(out);
        writer.write_header(&header)?;
        for record in &sorted {
            writer.write_record(&header, record)?;
        }
        let mut out = writer.finish()?;
        out.flush()
            .map_err(|error| Error::io(format!("cannot flush output: {error}")))?;
    }
    Ok(total)
}

/// Set `@HD SO:coordinate` on a header.
///
/// `samtools sort` rewrites `@HD` to `SO:coordinate`; leaving a stale
/// `SO:unsorted` after sorting would be a false statement about the file. The
/// sort order lives in the header map's *other fields* (`SO`), not in a typed
/// accessor, so it is set through `other_fields_mut`.
fn header_with_coordinate_sort(header: sam::Header) -> sam::Header {
    use noodles::sam::header::record::value::map::header::{sort_order, tag, Version};
    use noodles::sam::header::record::value::map::Header;
    use noodles::sam::header::record::value::Map;

    // Start from the existing @HD when there is one, so VN and any custom fields
    // survive; otherwise default to VN:1.6 like samtools does.
    let mut hd = header.header().cloned().unwrap_or_else(|| {
        Map::<Header>::builder()
            .set_version(Version::new(1, 6))
            .build()
            .expect("VN:1.6 is always valid")
    });
    // `SO` is a standard tag with a public constant, so no raw key is needed.
    hd.other_fields_mut().insert(
        tag::SORT_ORDER,
        String::from_utf8_lossy(sort_order::COORDINATE)
            .into_owned()
            .into(),
    );

    let mut builder = sam::Header::builder().set_header(hd);
    for (name, reference) in header.reference_sequences() {
        builder = builder.add_reference_sequence(name.clone(), reference.clone());
    }
    for (name, read_group) in header.read_groups() {
        builder = builder.add_read_group(name.clone(), read_group.clone());
    }
    for (id, program) in header.programs().as_ref() {
        builder = builder.add_program(id.clone(), program.clone());
    }
    builder.build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bgzf_eof_marker_is_the_28_byte_htslib_pattern() {
        assert_eq!(BGZF_EOF_MARKER.len(), 28);
        // gzip magic, FEXTRA, XFL = 0 (not 2), and the BC subfield.
        assert_eq!(&BGZF_EOF_MARKER[..4], &[0x1f, 0x8b, 0x08, 0x04]);
        assert_eq!(BGZF_EOF_MARKER[8], 0x00, "XFL must be 0");
        assert_eq!(&BGZF_EOF_MARKER[12..14], b"BC");
    }

    #[test]
    fn flagstat_renders_all_sixteen_lines_in_samtools_order() {
        let stat = FlagStat {
            total: 3,
            primary: 3,
            primary_mapped: 2,
            mapped: 2,
            ..FlagStat::default()
        };
        let text = render_flagstat(&stat);
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 16, "samtools flagstat prints 16 lines");
        assert_eq!(
            lines[0],
            "3 + 0 in total (QC-passed reads + QC-failed reads)"
        );
        assert_eq!(lines[1], "3 + 0 primary");
        assert_eq!(lines[6], "2 + 0 mapped (66.67% : N/A)");
        assert_eq!(lines[7], "2 + 0 primary mapped (66.67% : N/A)");
        assert_eq!(
            lines[15],
            "0 + 0 with mate mapped to a different chr (mapQ>=5)"
        );
    }

    #[test]
    fn flagstat_prints_na_when_the_denominator_is_zero() {
        let stat = FlagStat::default();
        let text = render_flagstat(&stat);
        assert!(text.contains("mapped (N/A : N/A)"), "{text}");
        assert!(text.contains("properly paired (N/A : N/A)"), "{text}");
    }

    #[test]
    fn flagstat_percentages_use_two_decimals() {
        let stat = FlagStat {
            total: 3,
            mapped: 2,
            ..FlagStat::default()
        };
        let text = render_flagstat(&stat);
        assert!(text.contains("66.67%"), "{text}");

        let stat = FlagStat {
            total: 3,
            mapped: 3,
            ..FlagStat::default()
        };
        let text = render_flagstat(&stat);
        assert!(text.contains("100.00%"), "{text}");
    }

    #[test]
    fn idxstats_renders_a_trailing_star_line() {
        let stats = IdxStats::from_references(vec![("chr1", 1000), ("chr2", 500)].into_iter());
        let text = render_idxstats(&stats);
        assert_eq!(text, "chr1\t1000\t0\t0\nchr2\t500\t0\t0\n*\t0\t0\t0\n");
    }

    #[test]
    fn idxstats_numbers_appear_in_mapped_then_unmapped_order() {
        let mut stats = IdxStats::from_references(vec![("chr1", 1000)].into_iter());
        stats.add(Some(0), false);
        stats.add(Some(0), false);
        stats.add(Some(0), true);
        let text = render_idxstats(&stats);
        assert_eq!(text.lines().next().unwrap(), "chr1\t1000\t2\t1");
    }

    #[test]
    fn quickcheck_exit_code_is_16_on_failure_and_0_on_success() {
        let ok = QuickCheck {
            passed: vec![PathBuf::from("a.bam")],
            failed: Vec::new(),
        };
        assert_eq!(ok.exit_code(), 0);
        assert!(ok.ok());

        let bad = QuickCheck {
            passed: Vec::new(),
            failed: vec![(PathBuf::from("b.bam"), "is empty".to_string())],
        };
        assert_eq!(bad.exit_code(), 16);
        assert!(!bad.ok());
    }
}
