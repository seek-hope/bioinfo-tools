//! Input format detection.
//!
//! `samtools view` accepts SAM, BAM and (with a reference) CRAM, and decides by
//! inspecting the file rather than the extension. `rust-bam` supports SAM and BAM
//! and detects them the same way, so a `.txt`-named BAM still works and a
//! mis-named file is not silently misparsed.
//!
//! Detection order matters:
//!
//! 1. **gzip/BGZF magic** (`1f 8b`). A BAM must be BGZF, so a gzip stream is
//!    either a BGZF-compressed BAM or an error; the BAM magic check after
//!    decompression decides.
//! 2. **BAM magic** `BAM\x01`.
//! 3. **SAM text** — a line starting with `@` (header) or a tab-separated record.
//!
//! CRAM (`CRAM`) is detected only so the error message can name it as
//! unimplemented rather than reporting a confusing parse failure.

use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

use crate::error::{Error, Result};

/// The container format of an alignment file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    /// Plain-text SAM.
    Sam,
    /// BGZF-compressed BAM.
    Bam,
    /// CRAM. Detected only to produce a clear "not supported" error.
    Cram,
}

impl Format {
    /// The name used in diagnostics.
    pub fn as_str(self) -> &'static str {
        match self {
            Format::Sam => "SAM",
            Format::Bam => "BAM",
            Format::Cram => "CRAM",
        }
    }
}

/// The BAM magic number, including the trailing format version byte.
pub const BAM_MAGIC: [u8; 4] = [b'B', b'A', b'M', 0x01];

/// The CRAM magic.
pub const CRAM_MAGIC: [u8; 4] = *b"CRAM";

/// The gzip magic that starts every BGZF block.
pub const GZIP_MAGIC: [u8; 2] = [0x1f, 0x8b];

/// Detect the format of `path` by reading its leading bytes.
pub fn detect_path(path: &Path) -> Result<Format> {
    if path.as_os_str() == "-" {
        // Standard input cannot be sampled without consuming it, so callers that
        // support `-` must use `detect_reader` on a buffered reader instead.
        return Err(Error::usage(
            "standard input needs a streaming reader, not path detection",
        ));
    }
    let file =
        File::open(path).map_err(|error| Error::io(format!("{}: {error}", path.display())))?;
    let mut reader = BufReader::new(file);
    detect_reader(&mut reader).map_err(|error| {
        // Re-word a detection failure to name the file, which samtools also does.
        match error.exit_code() {
            3 => Error::input(format!("{}: {}", path.display(), error.message())),
            _ => Error::io(format!("{}: {}", path.display(), error.message())),
        }
    })
}

/// Detect the format from a peekable byte stream.
///
/// Consumes at most the bytes it inspects. For a gzip stream it must decompress
/// the first block to see the BAM magic, which is what `samtools` does too.
pub fn detect_reader<R: Read>(reader: &mut R) -> Result<Format> {
    let mut head = [0u8; 4];
    let read = read_up_to(reader, &mut head)?;
    if read == 0 {
        return Err(Error::input("empty input: not a SAM or BAM file"));
    }

    if head[..2] == GZIP_MAGIC {
        // The 4 bytes above were consumed from `reader`, so they must be handed
        // back before decompressing: the gzip header starts at the first of them.
        // Chaining a cursor of the consumed prefix in front of the remaining
        // stream rebuilds the original byte sequence without needing `Seek`.
        let consumed = head[..read].to_vec();
        let mut rewound = std::io::Cursor::new(consumed).chain(reader);
        let mut decoder = flate_decode(&mut rewound)?;
        let mut inner = [0u8; 4];
        let inner_read = read_up_to(&mut decoder, &mut inner)?;
        return classify(&inner[..inner_read], true);
    }

    classify(&head[..read], false)
}

fn classify(head: &[u8], was_compressed: bool) -> Result<Format> {
    if head.len() >= 4 && head[..4] == BAM_MAGIC {
        return Ok(Format::Bam);
    }
    if head.len() >= 4 && head[..4] == CRAM_MAGIC {
        return Ok(Format::Cram);
    }
    if was_compressed {
        // A gzip stream that does not decompress to BAM is not something
        // `samtools view` accepts either.
        return Err(Error::input(
            "compressed input is not BAM (missing BAM magic after decompression)",
        ));
    }
    // SAM: either a header line or a record line. Both are printable and either
    // start with '@' or contain a tab within the first few bytes.
    Ok(Format::Sam)
}

fn read_up_to<R: Read>(reader: &mut R, buf: &mut [u8]) -> Result<usize> {
    let mut filled = 0;
    while filled < buf.len() {
        match reader.read(&mut buf[filled..]) {
            Ok(0) => break,
            Ok(n) => filled += n,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(Error::from(error)),
        }
    }
    Ok(filled)
}

/// A minimal gzip decoder for the magic peek.
///
/// `flate2` is already a workspace dependency (W1-2 uses it), so this adds no new
/// crate. Wrapping the raw reader in `Read` is enough because only the first few
/// decompressed bytes are needed.
fn flate_decode<R: Read>(reader: &mut R) -> Result<flate2::read::MultiGzDecoder<&mut R>> {
    Ok(flate2::read::MultiGzDecoder::new(reader))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn bam_magic_is_detected() {
        let bytes = [b'B', b'A', b'M', 1, 0, 0, 0, 0];
        assert_eq!(detect_reader(&mut Cursor::new(bytes)).unwrap(), Format::Bam);
    }

    #[test]
    fn cram_magic_is_detected_so_it_can_be_rejected_clearly() {
        let bytes = *b"CRAM\x03\x00";
        assert_eq!(
            detect_reader(&mut Cursor::new(bytes)).unwrap(),
            Format::Cram
        );
    }

    #[test]
    fn sam_header_is_detected() {
        let bytes = b"@HD\tVN:1.6\n@SQ\tSN:chr1\tLN:100\n";
        assert_eq!(detect_reader(&mut Cursor::new(bytes)).unwrap(), Format::Sam);
    }

    #[test]
    fn sam_record_without_header_is_detected() {
        let bytes = b"r1\t0\tchr1\t1\t60\t4M\t*\t0\t0\tACGT\tIIII\n";
        assert_eq!(detect_reader(&mut Cursor::new(bytes)).unwrap(), Format::Sam);
    }

    #[test]
    fn empty_input_is_an_input_error() {
        let error = detect_reader(&mut Cursor::new(Vec::new())).unwrap_err();
        assert_eq!(error.exit_code(), 3);
        assert!(error.message().contains("empty"));
    }

    #[test]
    fn a_bgzf_wrapped_bam_is_detected_as_bam() {
        // Compress the BAM magic as a BGZF-like gzip stream. MultiGzDecoder does
        // not require BGZF framing, so a plain gzip stream suffices to prove the
        // "decompress then look" path works.
        use flate2::write::GzEncoder;
        use flate2::Compression;
        use std::io::Write;

        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(&BAM_MAGIC).unwrap();
        encoder.write_all(&[0u8; 8]).unwrap();
        let compressed = encoder.finish().unwrap();

        assert_eq!(
            detect_reader(&mut Cursor::new(compressed)).unwrap(),
            Format::Bam
        );
    }

    #[test]
    fn gzip_that_is_not_bam_is_rejected() {
        use flate2::write::GzEncoder;
        use flate2::Compression;
        use std::io::Write;

        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(b"this is just text, not BAM").unwrap();
        let compressed = encoder.finish().unwrap();

        let error = detect_reader(&mut Cursor::new(compressed)).unwrap_err();
        assert_eq!(error.exit_code(), 3);
        assert!(error.message().contains("not BAM"));
    }

    #[test]
    fn format_names_match_the_diagnostics() {
        assert_eq!(Format::Sam.as_str(), "SAM");
        assert_eq!(Format::Bam.as_str(), "BAM");
        assert_eq!(Format::Cram.as_str(), "CRAM");
    }
}
