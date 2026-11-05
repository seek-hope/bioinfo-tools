//! Header/EOF checks and samtools quickcheck's per-file failure mask.
//!
//! Content sniffing follows HTSlib's bounded SAM/FASTX rules. Portions adapted
//! from hts.c, Copyright (C) 2008-2026 Genome Research Ltd. and 2012-2013 Broad
//! Institute, under the MIT license in docs/licenses/htslib-MIT.txt.

/*
HTSlib hts.c
Copyright (C) 2008, 2009, 2012-2026 Genome Research Ltd.
Copyright (C) 2012, 2013 Broad Institute.
Author: Heng Li <lh3@sanger.ac.uk>

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in
all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL
THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
DEALINGS IN THE SOFTWARE.
*/

use std::cell::Cell;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Cursor, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use flate2::{Decompress, FlushDecompress, Status};
use noodles::{bam, bgzf, sam};

use crate::bgzf_source::BgzfSource;
use crate::commands::BGZF_EOF_MARKER;
use crate::error::{Error, Result};

#[cfg(test)]
mod tests;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct QuickCheck {
    pub passed: Vec<PathBuf>,
    /// One entry per diagnostic, including multiple failures in one file.
    pub failed: Vec<(PathBuf, String)>,
    status: u8,
}

impl QuickCheck {
    pub fn ok(&self) -> bool {
        self.status == 0
    }
    pub fn exit_code(&self) -> i32 {
        i32::from(self.status)
    }
    fn fail(&mut self, path: &Path, code: u8, reason: &str) {
        self.status |= code;
        self.failed.push((path.to_owned(), reason.to_owned()));
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Bam,
    Sam,
    Fastx,
    NonSequence,
    CannotOpen,
    Cram,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Compression {
    Plain,
    Gzip,
    Bgzf,
}

/// Inspect headers and, for seekable BGZF inputs, the terminal marker. This does
/// not validate record bodies or internal compressed blocks beyond the header.
pub fn check(paths: &[PathBuf]) -> Result<QuickCheck> {
    let mut result = QuickCheck::default();
    for path in paths {
        let mut file = match if path.as_os_str() == "-" {
            stdin_file()
        } else {
            File::open(path)
        } {
            Ok(file) => file,
            Err(_) => {
                result.fail(path, 2, "could not be opened for reading.");
                continue;
            }
        };
        // Reading an unfinished trailer must not make a bounded header check
        // wait on a live pipe. Regular files can safely refill at this boundary.
        let regular_file = file.metadata().is_ok_and(|m| m.is_file());
        let first_failure = result.failed.len();
        let mut prefix = Vec::with_capacity(2048);
        if file.by_ref().take(18).read_to_end(&mut prefix).is_err() {
            result.fail(path, 2, "could not be opened for reading.");
            continue;
        }
        let compression = if prefix.starts_with(&[0x1f, 0x8b]) {
            if prefix.len() >= 18 && prefix[3] & 4 != 0 && &prefix[12..16] == b"BC\x02\0" {
                Compression::Bgzf
            } else {
                Compression::Gzip
            }
        } else {
            Compression::Plain
        };
        let limit = if compression == Compression::Plain {
            1024
        } else {
            2048
        };
        if file
            .by_ref()
            .take((limit - prefix.len()) as u64)
            .read_to_end(&mut prefix)
            .is_err()
        {
            result.fail(path, 2, "could not be opened for reading.");
            continue;
        }
        if prefix.starts_with(b"\xfd7zXZ\0")
            || prefix.starts_with(&[0x28, 0xb5, 0x2f, 0xfd])
            || (prefix.len() >= 10
                && prefix.starts_with(b"BZh")
                && (prefix[4..10] == [49, 65, 89, 38, 83, 89]
                    || prefix[4..10] == [23, 114, 69, 56, 80, 144]))
        {
            return Err(Error::usage("quickcheck supports plain and gzip/BGZF inputs; this compression format is not supported"));
        }
        let decoded = if compression == Compression::Plain {
            prefix.clone()
        } else {
            peek_gzip(&prefix)
        };
        let mut kind = classify(&decoded, path, compression);
        // A finite compressed prefix can contain empty members or a long gzip
        // optional header, even between bytes of the format signature.
        let extend_probe =
            compression != Compression::Plain && decoded.len() < 4 && prefix.len() == 2048;
        if !extend_probe && report_format_failure(kind, path, &mut result)? {
            continue;
        }
        let decoder_failed = Cell::new(false);
        let header = if kind == Kind::Fastx && !extend_probe {
            Ok(sam::Header::default())
        } else {
            let input = Cursor::new(prefix).chain(&mut file);
            let mut decoded: Box<dyn Read + '_> = match compression {
                Compression::Plain => Box::new(input),
                Compression::Bgzf => Box::new(bgzf::io::Reader::new(BufReader::new(
                    BgzfSource::new(input),
                ))),
                Compression::Gzip => Box::new(GzipBlocks::new(input, regular_file)),
            };
            if extend_probe {
                let mut leading = Vec::new();
                let first = decoded.by_ref().take(4).read_to_end(&mut leading);
                kind = classify(&leading, path, compression);
                let rest = if first.is_ok() && !matches!(kind, Kind::Bam | Kind::Sam | Kind::Cram) {
                    decoded
                        .by_ref()
                        .take((1024 - leading.len()) as u64)
                        .read_to_end(&mut leading)
                } else {
                    Ok(0)
                };
                if first.is_err() || rest.is_err() {
                    result.fail(path, 4, "was not identified as sequence data.");
                    continue;
                }
                kind = classify(&leading, path, compression);
                if report_format_failure(kind, path, &mut result)? {
                    continue;
                }
                decoded = Box::new(Cursor::new(leading).chain(decoded));
            }
            let tracked = TrackErrors {
                inner: decoded,
                failed: &decoder_failed,
            };
            match kind {
                Kind::Bam => bam::io::Reader::from(tracked).read_header(),
                Kind::Fastx => Ok(sam::Header::default()),
                _ => sam::io::Reader::new(BufReader::new(tracked)).read_header(),
            }
        };
        match header {
            Ok(header) if header.reference_sequences().is_empty() => {
                result.fail(path, 8, "had no targets in header.")
            }
            Err(_) => result.fail(path, 8, "caused an error whilst reading its header."),
            _ => {}
        }
        if compression == Compression::Bgzf {
            match check_eof(&mut file) {
                Ok(Some(false)) => result.fail(
                    path,
                    16,
                    "was missing EOF block when one should be present.",
                ),
                Err(_) => result.fail(path, 16, "caused an error whilst checking for EOF block."),
                _ => {}
            }
        }
        // HTSlib's close reports a decoder's sticky error too, even when the
        // underlying read-only OS descriptor itself closes successfully.
        if decoder_failed.get() {
            result.fail(path, 32, "did not close cleanly.");
        }
        if result.failed.len() == first_failure {
            result.passed.push(path.clone());
        }
    }
    Ok(result)
}

fn report_format_failure(kind: Kind, path: &Path, result: &mut QuickCheck) -> Result<bool> {
    match kind {
        Kind::NonSequence => result.fail(path, 4, "was not identified as sequence data."),
        Kind::CannotOpen => result.fail(path, 2, "could not be opened for reading."),
        Kind::Cram => {
            return Err(Error::usage(
                "CRAM is not supported by this build; convert it with `samtools view -b` first",
            ))
        }
        _ => return Ok(false),
    }
    Ok(true)
}

#[cfg(unix)]
fn stdin_file() -> io::Result<File> {
    use std::os::fd::AsFd;
    io::stdin().as_fd().try_clone_to_owned().map(File::from)
}
#[cfg(windows)]
fn stdin_file() -> io::Result<File> {
    use std::os::windows::io::AsHandle;
    io::stdin().as_handle().try_clone_to_owned().map(File::from)
}
#[cfg(not(any(unix, windows)))]
fn stdin_file() -> io::Result<File> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "stdin handle duplication is unavailable",
    ))
}

fn check_eof(file: &mut File) -> io::Result<Option<bool>> {
    let position = match file.stream_position() {
        Ok(position) => position,
        Err(error) if error.kind() == io::ErrorKind::NotSeekable => return Ok(None),
        Err(error) => return Err(error),
    };
    let result = (|| {
        let length = file.seek(SeekFrom::End(0))?;
        if length < 28 {
            return Ok(Some(false));
        }
        file.seek(SeekFrom::End(-28))?;
        let mut tail = [0; 28];
        file.read_exact(&mut tail)?;
        Ok(Some(tail == BGZF_EOF_MARKER))
    })();
    file.seek(SeekFrom::Start(position))?;
    result
}

struct TrackErrors<'a, R> {
    inner: R,
    failed: &'a Cell<bool>,
}
impl<R: Read> Read for TrackErrors<'_, R> {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        self.inner.read(bytes).inspect_err(|error| {
            if error.kind() != io::ErrorKind::Interrupted {
                self.failed.set(true);
            }
        })
    }
}

// HTSlib's gzip reader fills a 64 KiB decoded block before exposing it. In
// particular, a checksum failure in that block must not yield a valid header.
// Retry at the underlying byte source so an interrupted refill cannot discard
// a partially decoded block or expose it before its checksum has been checked.
struct RetryInterrupted<R>(R);
impl<R: Read> Read for RetryInterrupted<R> {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        loop {
            match self.0.read(bytes) {
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                result => return result,
            }
        }
    }
}

struct GzipBlocks<R: Read> {
    source: BufReader<RetryInterrupted<R>>,
    inflater: Decompress,
    crc: flate2::Crc,
    need_header: bool,
    need_trailer: bool,
    pending_error: Option<io::Error>,
    refill_trailer_at_boundary: bool,
    block: Box<[u8; 65536]>,
    offset: usize,
    length: usize,
    done: bool,
}
impl<R: Read> GzipBlocks<R> {
    fn new(source: R, refill_trailer_at_boundary: bool) -> Self {
        Self {
            source: BufReader::new(RetryInterrupted(source)),
            refill_trailer_at_boundary,
            inflater: Decompress::new(false),
            crc: flate2::Crc::new(),
            need_header: true,
            need_trailer: false,
            pending_error: None,
            block: Box::new([0; 65536]),
            offset: 0,
            length: 0,
            done: false,
        }
    }

    fn finish_member(&mut self) -> io::Result<()> {
        let mut trailer = [0; 8];
        self.source.read_exact(&mut trailer)?;
        if self.crc.sum() != u32::from_le_bytes(trailer[..4].try_into().unwrap())
            || self.crc.amount() != u32::from_le_bytes(trailer[4..].try_into().unwrap())
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid gzip trailer",
            ));
        }
        self.need_trailer = false;
        self.need_header = true;
        Ok(())
    }

    fn fill_block(&mut self) -> io::Result<()> {
        if let Some(error) = self.pending_error.take() {
            return Err(error);
        }
        self.offset = 0;
        self.length = 0;
        while self.length < self.block.len() {
            if self.need_trailer {
                self.finish_member()?;
            }
            if self.need_header {
                if self.source.fill_buf()?.is_empty() {
                    self.done = true;
                    break;
                }
                let mut header = flate2::bufread::GzDecoder::new(&mut self.source);
                if header.header().is_none() {
                    // Retrieve the parser's saved error; no body is consumed.
                    header.read_exact(&mut [0])?;
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "invalid gzip header",
                    ));
                }
                drop(header);
                self.inflater.reset(false);
                self.crc.reset();
                self.need_header = false;
            }
            let input = self.source.fill_buf()?;
            let before = (self.inflater.total_in(), self.inflater.total_out());
            let status = self.inflater.decompress(
                input,
                &mut self.block[self.length..],
                FlushDecompress::None,
            );
            let consumed = (self.inflater.total_in() - before.0) as usize;
            let produced = (self.inflater.total_out() - before.1) as usize;
            self.source.consume(consumed);
            self.crc
                .update(&self.block[self.length..self.length + produced]);
            self.length += produced;
            let status = status.map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
            if status == Status::StreamEnd {
                self.need_trailer = true;
                if self.length == self.block.len()
                    && !self.refill_trailer_at_boundary
                    && self.source.buffer().len() < 8
                {
                    // Do not wait for a writer that has supplied a full decoded
                    // block but not its trailer. Validate it if a later block
                    // is actually needed to complete the header.
                    break;
                }
                if let Err(error) = self.finish_member() {
                    if self.length == self.block.len()
                        && error.kind() == io::ErrorKind::UnexpectedEof
                    {
                        // A full decoded block is usable without demanding the
                        // tail. Preserve that failure for a later block read.
                        self.pending_error = Some(error);
                        break;
                    }
                    return Err(error);
                }
            } else if consumed == 0 && produced == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "truncated gzip body",
                ));
            }
        }
        Ok(())
    }
}
impl<R: Read> Read for GzipBlocks<R> {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        if bytes.is_empty() {
            return Ok(0);
        }
        if self.offset == self.length && !self.done {
            if let Err(error) = self.fill_block() {
                // Never expose bytes from a rejected block on a subsequent
                // read. Keep the failure sticky, including after partial input.
                self.offset = 0;
                self.length = 0;
                self.pending_error = Some(io::Error::new(error.kind(), error.to_string()));
                return Err(error);
            }
        }
        let count = bytes.len().min(self.length - self.offset);
        bytes[..count].copy_from_slice(&self.block[self.offset..self.offset + count]);
        self.offset += count;
        Ok(count)
    }
}

// Keep produced prefix bytes even if a later CRC/DEFLATE error is encountered.
// They identify the container; the actual header reader still checks integrity.
fn peek_gzip(mut input: &[u8]) -> Vec<u8> {
    let mut output = vec![0; 1024];
    let mut written = 0;
    'members: while !input.is_empty() && written < output.len() {
        // Reuse flate2's optional-header/FHCRC parser and existing raw inflater.
        // This works with both existing Rust backends without changing features.
        let header = flate2::bufread::GzDecoder::new(Cursor::new(input));
        if header.header().is_none() {
            break;
        }
        let header_length = header.get_ref().position() as usize;
        drop(header);
        let body = &input[header_length..];
        let mut decoder = Decompress::new(false);
        let mut previous = (0, 0);
        loop {
            let status = decoder.decompress(
                &body[decoder.total_in() as usize..],
                &mut output[written + decoder.total_out() as usize..],
                FlushDecompress::Sync,
            );
            let counts = (decoder.total_in(), decoder.total_out());
            if matches!(&status, Ok(Status::StreamEnd)) {
                let start = written;
                written += counts.1 as usize;
                let trailer = &body[counts.0 as usize..];
                if trailer.len() < 8 {
                    break 'members;
                }
                let mut crc = flate2::Crc::new();
                crc.update(&output[start..written]);
                if crc.sum() != u32::from_le_bytes(trailer[..4].try_into().unwrap())
                    || crc.amount() != u32::from_le_bytes(trailer[4..8].try_into().unwrap())
                {
                    break 'members;
                }
                input = &trailer[8..];
                break;
            }
            if status.is_err() || counts == previous || written + counts.1 as usize == output.len()
            {
                written += counts.1 as usize;
                break 'members;
            }
            previous = counts;
        }
    }
    output.truncate(written);
    output
}

fn text_only(bytes: &[u8]) -> bool {
    bytes
        .iter()
        .all(|&b| b >= b' ' || matches!(b, b'\t' | b'\r' | b'\n'))
}
fn looks_fastx(bytes: &[u8]) -> bool {
    let Some(end) = bytes.iter().position(|&b| b == b'\n') else {
        return text_only(bytes);
    };
    if !text_only(&bytes[..end]) {
        return false;
    }
    let line = &bytes[end + 1..];
    let count = line
        .iter()
        .take_while(|&&b| b"=ACMGRSVTWYHKDBNUacmgrsvtwyhkdbnu0123".contains(&b))
        .count();
    !line[..count].contains(&b'=') && (count == line.len() || matches!(line[count], b'\r' | b'\n'))
}

fn tabbed_columns(bytes: &[u8]) -> Option<(Vec<u8>, bool)> {
    let mut types = Vec::with_capacity(23);
    let mut start = 0;
    let mut seen = 0;
    let mut complete = false;
    for (i, &byte) in bytes.iter().enumerate() {
        // The pinned Linux parser's tabbed-text scanner uses signed char.
        if byte >= 128 {
            return None;
        }
        if byte >= b' ' {
            seen |= if byte.is_ascii_digit() {
                1
            } else if matches!(byte, b'+' | b'-') && i == start {
                2
            } else if b"MIDNSHP=XB".contains(&byte) && i > start && bytes[i - 1].is_ascii_digit() {
                4
            } else {
                8
            };
        } else if matches!(byte, b'\t' | b'\r' | b'\n') {
            let field = &bytes[start..i];
            let kind = if matches!(seen, 1 | 3) {
                b'i'
            } else if seen == 5 || field == b"*" {
                b'C'
            } else if field.len() == 1 && matches!(field[0], b'+' | b'-' | b'.') {
                b's'
            } else if field.len() >= 5 && field[2] == b':' && field[4] == b':' {
                b'O'
            } else {
                b'Z'
            };
            types.push(kind);
            if byte != b'\t' || types.len() == 23 {
                complete = true;
                break;
            }
            start = i + 1;
            seen = 0;
        } else {
            return None;
        }
    }
    Some((types, complete))
}

fn column_match(columns: &[u8], pattern: &[u8]) -> usize {
    if columns.len() <= pattern.len()
        && columns
            .iter()
            .zip(pattern)
            .all(|(&got, &want)| want == b'Z' || got == want)
    {
        columns.len()
    } else {
        0
    }
}

fn format_extension(path: &Path) -> &str {
    let extension = path.extension().and_then(|s| s.to_str()).unwrap_or("");
    if matches!(extension, "gz" | "bgz") {
        path.file_stem()
            .and_then(|stem| Path::new(stem).extension())
            .and_then(|s| s.to_str())
            .unwrap_or("")
    } else {
        extension
    }
}

fn classify(bytes: &[u8], path: &Path, compression: Compression) -> Kind {
    if bytes.starts_with(b"CRAM") {
        return Kind::Cram;
    }
    if bytes.starts_with(b"BAM\x01") {
        return Kind::Bam;
    }
    if [b"BAI\x01", b"CSI\x01", b"TBI\x01"]
        .iter()
        .any(|magic| bytes.starts_with(*magic))
    {
        return Kind::CannotOpen;
    }
    if [b"BCF\x02", b"BCF\x04"]
        .iter()
        .any(|magic| bytes.starts_with(*magic))
        || bytes.starts_with(b"##fileformat=VCF")
        || bytes.starts_with(b"d4\xdd\xdd")
    {
        return Kind::NonSequence;
    }
    if bytes.len() >= 4 && bytes[3] <= 4 && format_extension(path).eq_ignore_ascii_case("gzi") {
        return Kind::CannotOpen;
    }
    if [b"@HD\t", b"@SQ\t", b"@RG\t", b"@PG\t", b"@CO\t"]
        .iter()
        .any(|tag| bytes.starts_with(*tag))
    {
        return Kind::Sam;
    }
    if matches!(bytes.first(), Some(b'>' | b'@')) && looks_fastx(bytes) {
        return Kind::Fastx;
    }
    if let Some((columns, complete)) = tabbed_columns(bytes) {
        if column_match(&columns, b"ZiZiiCZiiZZOOOOOOOOOOOOOOOOOOOO")
            >= 9 + 2 * usize::from(complete)
        {
            return Kind::Sam;
        }
        let extension = format_extension(path).as_bytes();
        if (compression == Compression::Gzip && column_match(&columns, b"iiiiii") == 6)
            || (extension.windows(3).any(|s| s.eq_ignore_ascii_case(b"fqi"))
                && column_match(&columns, b"Ziiiii") == 6)
            || (extension.windows(3).any(|s| s.eq_ignore_ascii_case(b"fai"))
                && column_match(&columns, b"Ziiii") == 5)
        {
            return Kind::CannotOpen;
        }
    }
    if text_only(bytes) {
        Kind::NonSequence
    } else {
        Kind::CannotOpen
    }
}
