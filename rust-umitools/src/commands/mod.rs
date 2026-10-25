//! Command implementations and shared reporting helpers.

pub mod count;
pub mod count_tab;
pub mod dedup;
pub mod extract;
pub mod group;
pub mod whitelist;

use crate::error::{Error, Result};
use crate::io::spill::TempOutput;
use std::io::Write;
use std::path::PathBuf;

pub const PROGRAM: &str = "rust-umitools";

/// stderr plus optional `-L` log file.
pub struct Logger {
    file: Option<std::fs::File>,
    quiet: bool,
}

impl Logger {
    pub fn new(path: &Option<PathBuf>, quiet: bool) -> Result<Logger> {
        let file = match path {
            Some(path) => Some(
                std::fs::OpenOptions::new()
                    .create(true)
                    .write(true)
                    .truncate(true)
                    .open(path)
                    .map_err(|e| Error::io(path, e))?,
            ),
            None => None,
        };
        Ok(Logger { file, quiet })
    }

    pub fn info(&mut self, message: &str) {
        if self.quiet {
            return;
        }
        eprintln!("{message}");
        if let Some(file) = self.file.as_mut() {
            let _ = writeln!(file, "{message}");
        }
    }

    /// Warnings are part of the data contract (for example a skipped UMI) and
    /// stay visible even with `--quiet`.
    pub fn warn(&mut self, message: &str) {
        eprintln!("WARNING: {message}");
        if let Some(file) = self.file.as_mut() {
            let _ = writeln!(file, "WARNING: {message}");
        }
    }
}

/// Read level event counters, mirroring `get_bundles.read_events`.
#[derive(Default, Clone)]
pub struct Events {
    pub input_reads: u64,
    pub read2: u64,
    pub unmapped: u64,
    pub mate_unmapped: u64,
    pub chimeric: u64,
    pub unpaired: u64,
    pub low_mapq: u64,
    pub passthrough: u64,
    pub subset_excluded: u64,
}

impl Events {
    pub fn summary(&self) -> String {
        format!(
            "input={} read2={} unmapped={} mate_unmapped={} chimeric={} unpaired={} below_mapq={} passthrough={} subset_excluded={}",
            self.input_reads,
            self.read2,
            self.unmapped,
            self.mate_unmapped,
            self.chimeric,
            self.unpaired,
            self.low_mapq,
            self.passthrough,
            self.subset_excluded
        )
    }
}

/// Add the program record so the provenance of an output file is explicit.
///
/// The tag set and its order are the shared provenance contract
/// ([`bioinfo_common::provenance::ProgramRecord`]); this function only turns it
/// into an htslib record. Changing the tags or their order would change
/// `--add-pg` output bytes.
pub fn push_program_record(header: &mut crate::io::bam::Header) {
    let args: Vec<String> = std::env::args().collect();
    let program =
        bioinfo_common::provenance::ProgramRecord::new(PROGRAM, crate::cli::VERSION, &args);
    let mut record = crate::io::bam::HeaderRecord::new(b"PG");
    for (tag, value) in program.tags() {
        record.push_tag(tag, value);
    }
    header.push_record(&record);
}

/// Parse a two character BAM tag name.
pub fn parse_tag(name: &str) -> Result<[u8; 2]> {
    let bytes = name.as_bytes();
    if bytes.len() != 2 {
        return Err(Error::usage_with_hint(
            format!("tag '{name}' must have exactly two characters"),
            "for example: --gene-tag GX",
        ));
    }
    Ok([bytes[0], bytes[1]])
}

/// Append an unsigned integer tag using the smallest BAM type that fits.
///
/// htslib's `bam_aux_update_int` (used by pysam's `set_tag(int)`) picks
/// C/S/I, so matching that keeps the output bytes identical to UMI-tools.
pub fn push_aux_uint(record: &mut crate::io::bam::Record, tag: &[u8], value: u64) -> Result<()> {
    use crate::io::bam::Aux;
    let aux = if value <= u64::from(u8::MAX) {
        Aux::U8(value as u8)
    } else if value <= u64::from(u16::MAX) {
        Aux::U16(value as u16)
    } else if value <= u64::from(u32::MAX) {
        Aux::U32(value as u32)
    } else {
        return Err(Error::Resource(
            "group identifier exceeds the 32 bit range UMI-tools supports".to_string(),
        ));
    };
    record
        .push_aux(tag, aux)
        .map_err(|e| Error::Format(format!("cannot set tag: {e}")))
}

/// Resolved input/output paths. `-` means stdin/stdout: stdin is spooled to a
/// temporary BAM so two-pass commands (paired mate recovery) can re-read it,
/// and stdout is written through a temporary file so the bytes are produced
/// atomically before they reach the pipe.
pub struct Io {
    input: PathBuf,
    _input_spool: Option<TempOutput>,
    output: Option<PathBuf>,
    _output_spool: Option<TempOutput>,
    to_stdout: bool,
}

impl Io {
    pub fn resolve(
        input: &std::path::Path,
        output: Option<&std::path::Path>,
        tmpdir: &std::path::Path,
    ) -> Result<Io> {
        let (input_path, input_spool) = if input.as_os_str() == "-" {
            let temp = TempOutput::create(tmpdir, "umitools-stdin")?;
            let mut file =
                std::fs::File::create(temp.path()).map_err(|e| Error::io(temp.path(), e))?;
            std::io::copy(&mut std::io::stdin().lock(), &mut file)
                .map_err(|e| Error::io(temp.path(), e))?;
            file.flush().map_err(|e| Error::io(temp.path(), e))?;
            (temp.path().to_path_buf(), Some(temp))
        } else {
            (input.to_path_buf(), None)
        };
        let (output_path, output_spool, to_stdout) = match output {
            Some(path) if path.as_os_str() == "-" => {
                let temp = TempOutput::create(tmpdir, "umitools-stdout")?;
                (Some(temp.path().to_path_buf()), Some(temp), true)
            }
            Some(path) => (Some(path.to_path_buf()), None, false),
            None => (None, None, false),
        };
        Ok(Io {
            input: input_path,
            _input_spool: input_spool,
            output: output_path,
            _output_spool: output_spool,
            to_stdout,
        })
    }

    pub fn input(&self) -> &std::path::Path {
        &self.input
    }

    pub fn output(&self) -> Option<&std::path::Path> {
        self.output.as_deref()
    }

    /// Copy the staged stdout artifact to the real stdout.
    pub fn commit(self) -> Result<()> {
        if !self.to_stdout {
            return Ok(());
        }
        let Some(spool) = self._output_spool else {
            return Ok(());
        };
        let mut file = std::fs::File::open(spool.path()).map_err(|e| Error::io(spool.path(), e))?;
        let mut out = std::io::stdout().lock();
        match std::io::copy(&mut file, &mut out) {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => return Ok(()),
            Err(e) => return Err(Error::Format(format!("cannot write to stdout: {e}"))),
        }
        match out.flush() {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => Ok(()),
            Err(e) => Err(Error::Format(format!("cannot flush stdout: {e}"))),
        }
    }
}
