//! Stream format detection by magic bytes and by file extension.
//!
//! Reading and writing detect the format differently, and the difference is
//! deliberate — it is how both existing binaries already behave:
//!
//! * **Reading detects by content.** A gzip stream is decompressed even when the
//!   file is called `reads.fastq`, and a compressed stdin stream (which has no
//!   name at all) works. cutadapt documents this.
//! * **Writing detects by name.** `-o out.fastq.gz` compresses, `-o out.fastq`
//!   does not, and no content sniffing can help because the file does not exist
//!   yet.
//!
//! # Gzip versus BGZF
//!
//! Both start with the gzip magic bytes `1f 8b`, so magic-byte detection cannot
//! tell them apart on the first two bytes alone. [`Format::from_magic`] reports
//! [`Format::Gzip`] for both, because **any gzip decoder can read BGZF**: BGZF is
//! a valid gzip stream made of independently deflated members that each carry an
//! `FEXTRA` field. Reading BGZF therefore needs no special case.
//!
//! Writing is different: choosing BGZF requires the extension
//! (`.bgz`/`.bgzf`) or an explicit request, and [`format_from_extension`]
//! reports [`Format::Bgzf`] for those.

use std::path::Path;

/// gzip magic bytes (`1f 8b`).
pub const GZIP_MAGIC: [u8; 2] = [0x1f, 0x8b];
/// bzip2 magic bytes (`BZh`).
pub const BZIP2_MAGIC: [u8; 3] = *b"BZh";
/// xz magic bytes (`\xFD7zXZ\x00`).
pub const XZ_MAGIC: [u8; 6] = [0xfd, b'7', b'z', b'X', b'Z', 0x00];
/// zstd magic bytes (`\x28\xB5\x2F\xFD`, little-endian frame magic).
pub const ZSTD_MAGIC: [u8; 4] = [0x28, 0xb5, 0x2f, 0xfd];

/// A compression container.
///
/// `Bgzf` only ever appears on the *write* side of detection; readers treat it as
/// plain gzip, which is exactly correct.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Format {
    /// No compression.
    Plain,
    /// gzip, including multi-member gzip and BGZF.
    Gzip,
    /// BGZF, a gzip-compatible block-compressed format. Writing only.
    Bgzf,
    /// bzip2.
    Bzip2,
    /// xz.
    Xz,
    /// Zstandard.
    Zstd,
}

impl Format {
    /// Detect the format from the first bytes of a stream.
    ///
    /// Returns [`Format::Gzip`] for BGZF input, because a BGZF file *is* a gzip
    /// file and every gzip decoder must be able to read it.
    pub fn from_magic(head: &[u8]) -> Format {
        if head.len() >= 2 && head[..2] == GZIP_MAGIC {
            Format::Gzip
        } else if head.len() >= 3 && head[..3] == BZIP2_MAGIC {
            Format::Bzip2
        } else if head.len() >= 6 && head[..6] == XZ_MAGIC {
            Format::Xz
        } else if head.len() >= 4 && head[..4] == ZSTD_MAGIC {
            Format::Zstd
        } else {
            Format::Plain
        }
    }

    /// True when reading this format needs a decompressor.
    pub fn is_compressed(self) -> bool {
        !matches!(self, Format::Plain)
    }

    /// Detect the output format from a file name's extension.
    ///
    /// An inherent method as well as the free [`format_from_extension`], so a
    /// caller can read `Format::from_extension(path)` — the spelling both
    /// binaries used before the shared crate existed.
    pub fn from_extension(path: &Path) -> Format {
        format_from_extension(path)
    }

    /// The extension a file of this format conventionally carries.
    pub fn extension(self) -> &'static str {
        match self {
            Format::Plain => "",
            Format::Gzip => "gz",
            Format::Bgzf => "bgz",
            Format::Bzip2 => "bz2",
            Format::Xz => "xz",
            Format::Zstd => "zst",
        }
    }
}

/// Detect the output format from a file name's extension.
///
/// Case-insensitive, and matches the alias list each existing binary already
/// accepts (`.gzip`, `.bzip2`, `.lzma`). A name with no recognised extension is
/// [`Format::Plain`], including `-`.
pub fn format_from_extension(path: &Path) -> Format {
    let name = path.to_string_lossy().to_ascii_lowercase();
    if name.ends_with(".bgz") || name.ends_with(".bgzf") {
        Format::Bgzf
    } else if name.ends_with(".gz") || name.ends_with(".gzip") {
        Format::Gzip
    } else if name.ends_with(".bz2") || name.ends_with(".bzip2") {
        Format::Bzip2
    } else if name.ends_with(".xz") || name.ends_with(".lzma") {
        Format::Xz
    } else if name.ends_with(".zst") || name.ends_with(".zstd") {
        Format::Zstd
    } else {
        Format::Plain
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn detects_every_magic_sequence() {
        assert_eq!(Format::from_magic(&[0x1f, 0x8b, 0x08, 0x04]), Format::Gzip);
        assert_eq!(Format::from_magic(b"BZh9"), Format::Bzip2);
        assert_eq!(
            Format::from_magic(&[0xfd, b'7', b'z', b'X', b'Z', 0x00]),
            Format::Xz
        );
        assert_eq!(Format::from_magic(&[0x28, 0xb5, 0x2f, 0xfd]), Format::Zstd);
        assert_eq!(Format::from_magic(b"@r1\nACGT\n+\nIIII\n"), Format::Plain);
        assert_eq!(Format::from_magic(b">r1\nACGT\n"), Format::Plain);
    }

    #[test]
    fn short_input_is_plain_and_never_panics() {
        for len in 0..6 {
            let head = vec![0x1f; len];
            // The only thing that matters is that this does not index out of
            // bounds; a 1-5 byte prefix cannot be identified.
            let _ = Format::from_magic(&head);
        }
        assert_eq!(Format::from_magic(b""), Format::Plain);
        assert_eq!(Format::from_magic(&[0x1f]), Format::Plain);
        assert_eq!(Format::from_magic(b"BZ"), Format::Plain);
    }

    #[test]
    fn a_truncated_magic_is_not_guessed() {
        // `BZ` is the start of bzip2; guessing here would mis-read a plain file
        // whose first record happens to begin with those bytes.
        assert_eq!(Format::from_magic(b"BZ"), Format::Plain);
        assert_eq!(Format::from_magic(b"\x1f"), Format::Plain);
        assert_eq!(Format::from_magic(&[0x28, 0xb5, 0x2f]), Format::Plain);
    }

    // BGZF input is gzip input. The two magic bytes are identical and any gzip
    // decoder handles BGZF's extra field, so reading needs no discriminator.
    #[test]
    fn bgzf_magic_reports_plain_gzip() {
        // A real BGZF header: 1f 8b 08 04 <mtime> 00 ff <XLEN=6> 'B' 'C' ...
        let bgzf_header = [
            0x1f, 0x8b, 0x08, 0x04, 0x00, 0x00, 0x00, 0x00, 0x00, 0xff, 0x06, 0x00, b'B', b'C',
            0x02, 0x00, 0x1b, 0x00, 0x03, 0x00,
        ];
        assert_eq!(Format::from_magic(&bgzf_header), Format::Gzip);
    }

    #[test]
    fn detects_every_extension() {
        for (name, format) in [
            ("out.fastq.gz", Format::Gzip),
            ("out.fastq.GZ", Format::Gzip),
            ("out.fastq.gzip", Format::Gzip),
            ("out.bgz", Format::Bgzf),
            ("out.bgzf", Format::Bgzf),
            ("out.BGZF", Format::Bgzf),
            ("out.fastq.bz2", Format::Bzip2),
            ("out.fastq.bzip2", Format::Bzip2),
            ("out.fastq.xz", Format::Xz),
            ("out.fastq.lzma", Format::Xz),
            ("out.fastq.zst", Format::Zstd),
            ("out.fastq.zstd", Format::Zstd),
            ("out.fastq", Format::Plain),
            ("out.fasta", Format::Plain),
            ("-", Format::Plain),
        ] {
            assert_eq!(
                format_from_extension(Path::new(name)),
                format,
                "extension of {name}"
            );
        }
    }

    // BGZF must win over gzip: `.bgz` also ends with "gz"? No -- it does not,
    // but `.bgzf` ends with "zf". The ordering check is still worth pinning,
    // because a careless `ends_with(".gz")` on ".bgz" would misfire if the
    // suffix list ever grew.
    #[test]
    fn bgzf_is_not_mistaken_for_gzip() {
        assert_eq!(format_from_extension(Path::new("a.bgz")), Format::Bgzf);
        assert_eq!(format_from_extension(Path::new("a.bgzf")), Format::Bgzf);
        assert_ne!(format_from_extension(Path::new("a.bgz")), Format::Gzip);
    }

    #[test]
    fn a_dotted_directory_does_not_confuse_the_extension() {
        // `/tmp/a.gz/out.fastq` must be plain: the *file* is not compressed.
        let path = PathBuf::from("/tmp/a.gz/out.fastq");
        assert_eq!(format_from_extension(&path), Format::Plain);
    }

    #[test]
    fn extension_round_trips_through_the_name() {
        for format in [
            Format::Gzip,
            Format::Bgzf,
            Format::Bzip2,
            Format::Xz,
            Format::Zstd,
        ] {
            let name = format!("out.fastq.{}", format.extension());
            assert_eq!(
                format_from_extension(Path::new(&name)),
                format,
                "round trip for {name}"
            );
            assert!(format.is_compressed());
        }
        assert_eq!(format_from_extension(Path::new("out.fastq")), Format::Plain);
        assert!(!Format::Plain.is_compressed());
    }
}
