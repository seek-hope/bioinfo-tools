//! Byte-preserving tabular conversion with SeqKit's per-file FASTQ state.
//!
//! This is a format converter, not a FASTQ validator: SeqKit copies unequal
//! sequence/quality lengths, treats extra columns as FASTA, and accepts non-UTF8
//! bytes. Memory is O(longest line), limited by --buffer-size, plus 64 KiB output.

use crate::commands::Tab2fxOptions;
use crate::io::CommandError;
use crate::seqkit_io::{check_paths, SeqkitOutput};
use std::io::{self, BufRead, Read, Write};

struct Scanner {
    reader: Box<dyn BufRead + Send>,
    limit: usize,
    line: Vec<u8>,
}

impl Scanner {
    fn open(path: &str, limit: i64) -> Result<Self, CommandError> {
        let raw: Box<dyn Read + Send> = if path == "-" {
            Box::new(io::stdin())
        } else {
            Box::new(std::fs::File::open(path)?)
        };
        let mut raw = io::BufReader::new(raw);
        if raw.fill_buf()?.is_empty() {
            return Err(CommandError::input("xopen: no content").with_code(255));
        }
        let decoded = bioinfo_fastx::decompress(Box::new(raw))?;
        Ok(Self {
            reader: Box::new(io::BufReader::with_capacity(65_536, decoded)),
            limit: usize::try_from(limit).unwrap_or(0),
            line: Vec::new(),
        })
    }

    fn next(&mut self) -> Result<Option<&[u8]>, CommandError> {
        self.line.clear();
        loop {
            if self.line.len() == self.limit {
                return Err(CommandError::runtime("bufio.Scanner: token too long").with_code(255));
            }
            let available = self.reader.fill_buf()?;
            if available.is_empty() {
                if self.line.is_empty() {
                    return Ok(None);
                }
                break;
            }
            let end = available
                .iter()
                .position(|byte| *byte == b'\n')
                .map_or(available.len(), |index| index + 1);
            let take = end.min(self.limit - self.line.len());
            self.line.extend_from_slice(&available[..take]);
            self.reader.consume(take);
            if self.line.last() == Some(&b'\n') {
                break;
            }
        }
        while matches!(self.line.last(), Some(b'\r' | b'\n')) {
            self.line.pop();
        }
        Ok(Some(&self.line))
    }
}

pub(crate) fn run(options: Tab2fxOptions) -> Result<(), CommandError> {
    run_inner(options).map_err(|error| error.with_code(255))
}

fn run_inner(options: Tab2fxOptions) -> Result<(), CommandError> {
    let inputs = if options.inputs.is_empty() {
        vec!["-".to_string()]
    } else {
        options.inputs
    };
    check_paths(&inputs, options.out_file.as_deref())?;
    let mut out = SeqkitOutput::create(options.out_file.as_deref(), options.compress_level)?;
    let mut header = Vec::new();
    let mut wrapped = Vec::new();
    for input in inputs {
        let mut scanner = Scanner::open(&input, options.buffer_size)?;
        let mut fastq = false;
        while let Some(line) = scanner.next()? {
            if line.is_empty()
                || options
                    .comment_prefixes
                    .iter()
                    .any(|prefix| line.starts_with(prefix.as_bytes()))
            {
                continue;
            }
            let mut fields = line.split(|byte| *byte == b'\t');
            let name = fields.next().unwrap_or_default();
            let Some(sequence) = fields.next() else {
                return Err(CommandError::input(format!(
                    "at least two columns needed: {}",
                    String::from_utf8_lossy(line)
                )));
            };
            let quality = fields.next();
            let is_fastq = quality.is_some_and(|quality| !quality.is_empty() || fastq)
                && fields.next().is_none();
            header.clear();
            header.push(if is_fastq { b'@' } else { b'>' });
            header.extend_from_slice(name);
            header.push(b'\n');
            out.write_all(&header)?;
            if is_fastq {
                fastq = true;
                out.write_all(sequence)?;
                out.write_all(b"\n+\n")?;
                out.write_all(quality.unwrap_or_default())?;
            } else if options.line_width == 0 || sequence.len() <= options.line_width {
                out.write_all(sequence)?;
            } else {
                wrapped.clear();
                for (index, chunk) in sequence.chunks(options.line_width).enumerate() {
                    if index != 0 {
                        wrapped.push(b'\n');
                    }
                    wrapped.extend_from_slice(chunk);
                }
                out.write_all(&wrapped)?;
            }
            out.write_all(b"\n")?;
        }
    }
    out.finish()
}
