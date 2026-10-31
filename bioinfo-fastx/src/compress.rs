//! Compression: detection on read, selection on write, and the writers.
//!
//! # Reading
//!
//! [`decompress`] sniffs the first bytes of the stream and wraps it in the right
//! decoder. Detection is by **content**, not by file name, because that is what
//! `rust-cutadapt` already does and what makes a compressed stdin stream work:
//!
//! * gzip — including multi-member gzip and BGZF, both of which
//!   [`flate2::read::MultiGzDecoder`] handles natively;
//! * bzip2 — multi-stream;
//! * xz — multi-stream;
//! * zstd.
//!
//! # Writing
//!
//! [`compressor`] selects by the destination's extension (a file that does not
//! exist yet cannot be sniffed) and returns a boxed writer that finishes its
//! stream on drop or on [`Compressor::finish`]. Levels follow cutadapt's
//! observable behaviour, including its `-Z` fast mode:
//!
//! | format | default | `-Z` |
//! | --- | --- | --- |
//! | gzip | level 3 (cutadapt writes 6; see rust-cutadapt BENCHMARK.md) | 1 |
//! | bzip2 | 9 | 1 |
//! | xz | 6 | 0 |
//! | zstd | 3 | 1 |
//!
//! # Multi-member gzip
//!
//! [`MultiMemberGzWriter`] emits an independent gzip member per
//! [`MultiMemberGzWriter::write_member`] call. The concatenation is a valid gzip stream that every decoder (`gzip`,
//! Python `gzip`, `MultiGzDecoder`, `samtools`) reads as one, which is what makes
//! it usable for parallel or streamed output. cutadapt's own pipeline produces a
//! single member today; the multi-member writer exists for W1-3+ and is tested
//! against the decoders named above (plan risk R7).

use crate::format::{format_from_extension, Format};
use std::fs::File;
use std::io::{self, BufRead, BufReader, BufWriter, Read, Write};
use std::path::Path;

/// Buffer size used for the sniffing reader and the file writers.
pub const BUFFER_SIZE: usize = 64 * 1024;

/// Wrap `inner` in the decompressor its first bytes call for.
///
/// The stream is buffered so the header can be inspected, and the buffered
/// reader (with the inspected bytes still in place) is handed to the decoder, so
/// no byte is consumed twice.
pub fn decompress(inner: Box<dyn Read + Send>) -> io::Result<Box<dyn Read + Send>> {
    let mut buffered = BufReader::with_capacity(BUFFER_SIZE, inner);
    let head = buffered.fill_buf()?;
    let format = Format::from_magic(head);
    Ok(match format {
        Format::Gzip | Format::Bgzf => Box::new(flate2::read::MultiGzDecoder::new(buffered)),
        Format::Bzip2 => Box::new(bzip2::read::MultiBzDecoder::new(buffered)),
        Format::Xz => Box::new(xz2::read::XzDecoder::new_multi_decoder(buffered)),
        Format::Zstd => Box::new(zstd::stream::read::Decoder::new(buffered)?),
        Format::Plain => Box::new(buffered),
    })
}

/// Wrap a single-format decoder around `inner` without sniffing.
///
/// Used when the caller already knows the format (for example because it chose
/// it on the write side).
pub fn decompress_as(
    inner: Box<dyn Read + Send>,
    format: Format,
) -> io::Result<Box<dyn Read + Send>> {
    let buffered = BufReader::with_capacity(BUFFER_SIZE, inner);
    Ok(match format {
        Format::Gzip | Format::Bgzf => Box::new(flate2::read::MultiGzDecoder::new(buffered)),
        Format::Bzip2 => Box::new(bzip2::read::MultiBzDecoder::new(buffered)),
        Format::Xz => Box::new(xz2::read::XzDecoder::new_multi_decoder(buffered)),
        Format::Zstd => Box::new(zstd::stream::read::Decoder::new(buffered)?),
        Format::Plain => Box::new(buffered),
    })
}

/// Compression levels, expressed the way cutadapt's `-Z` switch needs them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Levels {
    /// Use each format's fastest setting (cutadapt's `-Z`).
    pub fast: bool,
}

impl Levels {
    /// cutadapt's default levels.
    pub const DEFAULT: Levels = Levels { fast: false };
    /// cutadapt's `-Z` levels.
    pub const FAST: Levels = Levels { fast: true };

    /// gzip level: 3 by default (cutadapt uses 6; see BENCHMARK.md), 1 with `-Z`.
    pub fn gzip(self) -> u32 {
        if self.fast {
            1
        } else {
            3
        }
    }

    /// bzip2 level: 9 by default, 1 with `-Z`.
    pub fn bzip2(self) -> u32 {
        if self.fast {
            1
        } else {
            9
        }
    }

    /// xz preset: 6 by default, 0 with `-Z`.
    pub fn xz(self) -> u32 {
        if self.fast {
            0
        } else {
            6
        }
    }

    /// zstd level: 3 by default, 1 with `-Z`.
    pub fn zstd(self) -> i32 {
        if self.fast {
            1
        } else {
            3
        }
    }
}

impl Default for Levels {
    fn default() -> Self {
        Levels::DEFAULT
    }
}

/// Per-format overrides for callers whose upstream exposes a numeric level.
///
/// `None` keeps the corresponding [`Levels`] default. This is separate from
/// [`Levels`] so the cutadapt `-Z` API remains source- and behaviour-compatible.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CompressionLevels {
    /// Optional gzip/zlib level.
    pub gzip: Option<u32>,
    /// Optional bzip2 level.
    pub bzip2: Option<u32>,
    /// Optional xz preset.
    pub xz: Option<u32>,
    /// Optional zstd level.
    pub zstd: Option<i32>,
}

/// A compressor that can be finished explicitly.
///
/// `finish` must be called (or the value dropped) for the trailing compressed
/// bytes to reach the sink; a gzip/xz/bzip2 frame is only complete after its
/// footer is written.
pub trait Compressor: Write {
    /// Finish the stream and flush the sink.
    fn finish(self: Box<Self>) -> io::Result<()>;
}

/// One concrete compressor over a sink.
enum Sink {
    Plain(BufWriter<Box<dyn Write + Send>>),
    Gzip(flate2::write::GzEncoder<BufWriter<Box<dyn Write + Send>>>),
    Bzip2(bzip2::write::BzEncoder<BufWriter<Box<dyn Write + Send>>>),
    Xz(xz2::write::XzEncoder<BufWriter<Box<dyn Write + Send>>>),
    Zstd(zstd::stream::write::Encoder<'static, BufWriter<Box<dyn Write + Send>>>),
}

/// The compressor [`compressor`] returns.
pub struct InnerCompressor {
    sink: Option<Sink>,
}

impl InnerCompressor {
    fn new(
        format: Format,
        sink: Box<dyn Write + Send>,
        levels: Levels,
        overrides: CompressionLevels,
    ) -> io::Result<Self> {
        let buffered = BufWriter::with_capacity(BUFFER_SIZE, sink);
        let sink = match format {
            Format::Plain => Sink::Plain(buffered),
            Format::Gzip | Format::Bgzf => {
                // BGZF is produced by the dedicated block writer, never here; a
                // BGZF-named stream therefore falls back to ordinary gzip, which
                // is still a readable gzip file. Callers that want real BGZF use
                // `bgzf::BgzfWriter`.
                Sink::Gzip(flate2::write::GzEncoder::new(
                    buffered,
                    flate2::Compression::new(overrides.gzip.unwrap_or_else(|| levels.gzip())),
                ))
            }
            Format::Bzip2 => Sink::Bzip2(bzip2::write::BzEncoder::new(
                buffered,
                bzip2::Compression::new(
                    overrides
                        .bzip2
                        .unwrap_or_else(|| levels.bzip2())
                        .clamp(1, 9),
                ),
            )),
            Format::Xz => Sink::Xz(xz2::write::XzEncoder::new(
                buffered,
                overrides.xz.unwrap_or_else(|| levels.xz()),
            )),
            Format::Zstd => Sink::Zstd(zstd::stream::write::Encoder::new(
                buffered,
                overrides.zstd.unwrap_or_else(|| levels.zstd()),
            )?),
        };
        Ok(InnerCompressor { sink: Some(sink) })
    }
}

impl Write for InnerCompressor {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let sink = self
            .sink
            .as_mut()
            .ok_or_else(|| io::Error::other("compressor already finished"))?;
        match sink {
            Sink::Plain(w) => w.write(buf),
            Sink::Gzip(w) => w.write(buf),
            Sink::Bzip2(w) => w.write(buf),
            Sink::Xz(w) => w.write(buf),
            Sink::Zstd(w) => w.write(buf),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        let sink = self
            .sink
            .as_mut()
            .ok_or_else(|| io::Error::other("compressor already finished"))?;
        match sink {
            Sink::Plain(w) => w.flush(),
            Sink::Gzip(w) => w.flush(),
            Sink::Bzip2(w) => w.flush(),
            Sink::Xz(w) => w.flush(),
            Sink::Zstd(w) => w.flush(),
        }
    }
}

impl Compressor for InnerCompressor {
    fn finish(mut self: Box<Self>) -> io::Result<()> {
        let sink = self
            .sink
            .take()
            .ok_or_else(|| io::Error::other("compressor already finished"))?;
        match sink {
            Sink::Plain(mut w) => w.flush(),
            // Encoder completion returns a still-buffered sink. Its destructor
            // cannot report write/flush failures, so finish that layer explicitly.
            Sink::Gzip(w) => w.finish()?.flush(),
            Sink::Bzip2(w) => w.finish()?.flush(),
            Sink::Xz(w) => w.finish()?.flush(),
            Sink::Zstd(w) => w.finish()?.flush(),
        }
    }
}

/// Open `path` for writing and compress according to its extension.
pub fn compressor(path: &Path, levels: Levels) -> io::Result<Box<dyn Compressor>> {
    let format = format_from_extension(path);
    let file = File::create(path)?;
    Ok(Box::new(InnerCompressor::new(
        format,
        Box::new(file),
        levels,
        CompressionLevels::default(),
    )?))
}

/// Compress any sink according to an explicit format.
pub fn compressor_for(
    sink: Box<dyn Write + Send>,
    format: Format,
    levels: Levels,
) -> io::Result<Box<dyn Compressor>> {
    Ok(Box::new(InnerCompressor::new(
        format,
        sink,
        levels,
        CompressionLevels::default(),
    )?))
}

/// Open `path` with explicit per-format compression overrides.
///
/// Missing overrides use the ordinary [`Levels::DEFAULT`] values. Callers that
/// need an upstream-specific default should set it explicitly.
pub fn compressor_with_levels(
    path: &Path,
    overrides: CompressionLevels,
) -> io::Result<Box<dyn Compressor>> {
    let format = format_from_extension(path);
    let file = File::create(path)?;
    Ok(Box::new(InnerCompressor::new(
        format,
        Box::new(file),
        Levels::DEFAULT,
        overrides,
    )?))
}

/// Compress a sink with explicit per-format compression overrides.
pub fn compressor_for_with_levels(
    sink: Box<dyn Write + Send>,
    format: Format,
    overrides: CompressionLevels,
) -> io::Result<Box<dyn Compressor>> {
    Ok(Box::new(InnerCompressor::new(
        format,
        sink,
        Levels::DEFAULT,
        overrides,
    )?))
}

/// A raw-deflate encoder, used by the BGZF writer for each block's payload.
pub fn raw_deflate(data: &[u8], level: u32) -> io::Result<Vec<u8>> {
    use std::io::Write as _;
    let mut encoder = flate2::write::DeflateEncoder::new(
        Vec::with_capacity(data.len() / 2 + 64),
        flate2::Compression::new(level),
    );
    encoder.write_all(data)?;
    encoder.finish()
}

/// A writer that emits one independent gzip member per [`write_member`] call.
///
/// [`write_member`]: MultiMemberGzWriter::write_member
pub struct MultiMemberGzWriter<W: Write> {
    sink: W,
    level: u32,
    /// Number of members written, for tests and diagnostics.
    members: u64,
    finished: bool,
}

impl<W: Write> MultiMemberGzWriter<W> {
    /// Wrap `sink`; `level` is the gzip level for each member.
    pub fn new(sink: W, level: u32) -> Self {
        MultiMemberGzWriter {
            sink,
            level,
            members: 0,
            finished: false,
        }
    }

    /// Compress `bytes` as one complete, independent gzip member.
    pub fn write_member(&mut self, bytes: &[u8]) -> io::Result<()> {
        let mut encoder = flate2::write::GzEncoder::new(
            Vec::with_capacity(bytes.len() / 2 + 64),
            flate2::Compression::new(self.level),
        );
        encoder.write_all(bytes)?;
        let member = encoder.finish()?;
        self.sink.write_all(&member)?;
        self.members += 1;
        Ok(())
    }

    /// Flush the sink. Does not write an extra member.
    pub fn finish(&mut self) -> io::Result<()> {
        self.sink.flush()?;
        self.finished = true;
        Ok(())
    }

    /// The number of members written so far.
    pub fn members(&self) -> u64 {
        self.members
    }

    /// The underlying sink.
    pub fn into_inner(self) -> W {
        self.sink
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn payload() -> Vec<u8> {
        let mut out = Vec::new();
        for i in 0..2000 {
            out.extend_from_slice(format!("@read{i}\nACGTACGTACGT\n+\nIIIIIIIIIIII\n").as_bytes());
        }
        out
    }

    fn round_trip(format: Format, levels: Levels) {
        let data = payload();
        let sink: Box<dyn Write + Send> = Box::new(Vec::new());
        let mut writer = compressor_for(sink, format, levels).unwrap();
        writer.write_all(&data).unwrap();
        // `compressor_for` cannot hand the buffer back, so serialise through a
        // temporary file instead.
        drop(writer);

        let dir = std::env::temp_dir().join(format!(
            "bioinfo-fastx-rt-{}-{:?}-{:?}",
            std::process::id(),
            std::thread::current().id(),
            format
        ));
        let path = dir.with_extension(format!("fastq.{}", format.extension()));
        let _ = std::fs::remove_file(&path);
        {
            let mut file_writer = compressor(&path, levels).unwrap();
            file_writer.write_all(&data).unwrap();
            file_writer.finish().unwrap();
        }
        let raw = File::open(&path).unwrap();
        let mut reader = decompress(Box::new(raw)).unwrap();
        let mut back = Vec::new();
        reader.read_to_end(&mut back).unwrap();
        assert_eq!(back, data, "round trip failed for {format:?}");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn every_format_round_trips_at_default_level() {
        for format in [
            Format::Plain,
            Format::Gzip,
            Format::Bzip2,
            Format::Xz,
            Format::Zstd,
        ] {
            round_trip(format, Levels::DEFAULT);
        }
    }

    #[test]
    fn every_format_round_trips_at_fast_level() {
        for format in [
            Format::Plain,
            Format::Gzip,
            Format::Bzip2,
            Format::Xz,
            Format::Zstd,
        ] {
            round_trip(format, Levels::FAST);
        }
    }

    #[test]
    fn explicit_levels_round_trip_without_changing_default_levels() {
        let cases = [
            (
                Format::Gzip,
                CompressionLevels {
                    gzip: Some(1),
                    ..CompressionLevels::default()
                },
            ),
            (
                Format::Bzip2,
                CompressionLevels {
                    bzip2: Some(1),
                    ..CompressionLevels::default()
                },
            ),
            (
                Format::Xz,
                CompressionLevels {
                    xz: Some(0),
                    ..CompressionLevels::default()
                },
            ),
            (
                Format::Zstd,
                CompressionLevels {
                    zstd: Some(11),
                    ..CompressionLevels::default()
                },
            ),
        ];
        for (format, overrides) in cases {
            let path = std::env::temp_dir().join(format!(
                "bioinfo-fastx-explicit-{}-{:?}.{}",
                std::process::id(),
                std::thread::current().id(),
                format.extension()
            ));
            let data = payload();
            let mut writer = compressor_with_levels(&path, overrides).unwrap();
            writer.write_all(&data).unwrap();
            writer.finish().unwrap();
            let mut reader = decompress(Box::new(File::open(&path).unwrap())).unwrap();
            let mut back = Vec::new();
            reader.read_to_end(&mut back).unwrap();
            assert_eq!(back, data, "round trip failed for {format:?}");
            let _ = std::fs::remove_file(path);
        }
    }

    #[test]
    fn finish_propagates_buffered_sink_write_and_flush_failures() {
        struct FailingSink {
            fail_write: bool,
        }
        impl Write for FailingSink {
            fn write(&mut self, data: &[u8]) -> io::Result<usize> {
                if self.fail_write {
                    Err(io::Error::other("injected sink write failure"))
                } else {
                    Ok(data.len())
                }
            }
            fn flush(&mut self) -> io::Result<()> {
                Err(io::Error::other("injected sink flush failure"))
            }
        }
        for format in [
            Format::Plain,
            Format::Gzip,
            Format::Bzip2,
            Format::Xz,
            Format::Zstd,
        ] {
            for fail_write in [false, true] {
                let mut writer = compressor_for(
                    Box::new(FailingSink { fail_write }),
                    format,
                    Levels::DEFAULT,
                )
                .unwrap();
                writer.write_all(b">read\nACGT\n").unwrap();
                assert!(
                    writer.finish().is_err(),
                    "lost sink error for {format:?}, write={fail_write}"
                );
            }
        }
    }

    #[test]
    fn levels_match_cutadapt() {
        assert_eq!(Levels::DEFAULT.gzip(), 3);
        assert_eq!(Levels::FAST.gzip(), 1);
        assert_eq!(Levels::DEFAULT.bzip2(), 9);
        assert_eq!(Levels::FAST.bzip2(), 1);
        assert_eq!(Levels::DEFAULT.xz(), 6);
        assert_eq!(Levels::FAST.xz(), 0);
        assert_eq!(Levels::DEFAULT.zstd(), 3);
        assert_eq!(Levels::FAST.zstd(), 1);
        assert_eq!(Levels::default(), Levels::DEFAULT);
    }

    #[test]
    fn detection_is_by_content_not_name() {
        // Compress to gzip bytes, then read them through a stream whose name
        // says nothing about the format.
        let data = payload();
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::new(3));
        encoder.write_all(&data).unwrap();
        let compressed = encoder.finish().unwrap();
        let mut reader = decompress(Box::new(Cursor::new(compressed))).unwrap();
        let mut back = Vec::new();
        reader.read_to_end(&mut back).unwrap();
        assert_eq!(back, data);
    }

    #[test]
    fn plain_input_passes_through_untouched() {
        let data = b"@r1\nACGT\n+\nIIII\n".to_vec();
        let mut reader = decompress(Box::new(Cursor::new(data.clone()))).unwrap();
        let mut back = Vec::new();
        reader.read_to_end(&mut back).unwrap();
        assert_eq!(back, data);
    }

    #[test]
    fn empty_input_decompresses_to_empty() {
        let mut reader = decompress(Box::new(Cursor::new(Vec::new()))).unwrap();
        let mut back = Vec::new();
        reader.read_to_end(&mut back).unwrap();
        assert!(back.is_empty());
    }

    #[test]
    fn zstd_round_trips_through_decompress() {
        let data = payload();
        let compressed = zstd::stream::encode_all(Cursor::new(&data), 3).unwrap();
        let mut reader = decompress(Box::new(Cursor::new(compressed))).unwrap();
        let mut back = Vec::new();
        reader.read_to_end(&mut back).unwrap();
        assert_eq!(back, data);
    }

    #[test]
    fn raw_deflate_produces_a_stream_that_inflates() {
        let data = b"ACGTACGTACGTACGT".repeat(100);
        let deflated = raw_deflate(&data, 6).unwrap();
        assert!(deflated.len() < data.len());
        let mut decoder = flate2::read::DeflateDecoder::new(Cursor::new(deflated));
        let mut back = Vec::new();
        decoder.read_to_end(&mut back).unwrap();
        assert_eq!(back, data);
    }

    // ---- multi-member gzip ------------------------------------------------

    fn multigz_bytes(members: &[&[u8]], level: u32) -> Vec<u8> {
        let mut writer = MultiMemberGzWriter::new(Vec::new(), level);
        for member in members {
            writer.write_member(member).unwrap();
        }
        assert_eq!(writer.members(), members.len() as u64);
        writer.finish().unwrap();
        writer.into_inner()
    }

    #[test]
    fn multi_member_gzip_reads_back_as_the_concatenation() {
        let a = b"@r1\nACGT\n+\nIIII\n";
        let b = b"@r2\nTTTT\n+\nJJJJ\n";
        let bytes = multigz_bytes(&[a, b], 3);
        // A real gzip stream: gzip's magic, and MultiGzDecoder joins the members.
        assert_eq!(&bytes[..2], &crate::format::GZIP_MAGIC);
        let mut reader = flate2::read::MultiGzDecoder::new(Cursor::new(bytes.clone()));
        let mut back = Vec::new();
        reader.read_to_end(&mut back).unwrap();
        let mut expected = a.to_vec();
        expected.extend_from_slice(b);
        assert_eq!(back, expected);
    }

    #[test]
    fn each_member_is_independently_a_gzip_stream() {
        // Two members concatenated means two gzip headers.
        let bytes = multigz_bytes(&[b"aaaa", b"bbbb"], 3);
        let headers = bytes
            .windows(2)
            .filter(|w| w == &crate::format::GZIP_MAGIC)
            .count();
        assert_eq!(headers, 2, "expected one gzip header per member");
    }

    #[test]
    fn a_single_member_matches_a_plain_gzip_encoder() {
        let data = b"@r1\nACGT\n+\nIIII\n";
        let ours = multigz_bytes(&[data], 3);
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::new(3));
        encoder.write_all(data).unwrap();
        let theirs = encoder.finish().unwrap();
        assert_eq!(
            ours, theirs,
            "one member must be byte-identical to flate2's own GzEncoder"
        );
    }

    #[test]
    fn multi_member_round_trips_through_decompress() {
        let members: Vec<Vec<u8>> = (0..5)
            .map(|i| format!("@r{i}\nACGT\n+\nIIII\n").into_bytes())
            .collect();
        let refs: Vec<&[u8]> = members.iter().map(|m| m.as_slice()).collect();
        let bytes = multigz_bytes(&refs, 1);
        let mut reader = decompress(Box::new(Cursor::new(bytes))).unwrap();
        let mut back = Vec::new();
        reader.read_to_end(&mut back).unwrap();
        let expected: Vec<u8> = members.concat();
        assert_eq!(back, expected);
    }

    #[test]
    fn zero_members_is_an_empty_stream() {
        let bytes = multigz_bytes(&[], 3);
        assert!(bytes.is_empty());
    }

    #[test]
    fn empty_members_are_valid_and_concatenate_cleanly() {
        let bytes = multigz_bytes(&[b"", b"data", b""], 3);
        let mut reader = flate2::read::MultiGzDecoder::new(Cursor::new(bytes));
        let mut back = Vec::new();
        reader.read_to_end(&mut back).unwrap();
        assert_eq!(back, b"data");
    }
}
