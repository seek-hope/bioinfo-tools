//! Compression, re-exported from [`bioinfo_fastx`].
//!
//! This module used to own format detection and the compressors. W1-2 moved them
//! into the shared crate; the names below are re-exports so the pipeline keeps
//! compiling unchanged and there is one implementation.
//!
//! # cutadapt's specific level mapping
//!
//! cutadapt's levels are not the defaults of any compression library, so they are
//! pinned in `bioinfo_fastx::compress::Levels`:
//!
//! | format | default | `-Z` |
//! | --- | --- | --- |
//! | gzip | 3 (cutadapt writes 6; see BENCHMARK.md) | 1 |
//! | bzip2 | 9 | 1 |
//! | xz | 6 | 0 |
//!
//! [`Levels`] is re-exported here so a caller in this crate never has to spell
//! out the shared crate's path.
//!
//! # Detection direction
//!
//! Reading sniffs the content (so a compressed file with a plain name and a
//! compressed stdin stream both work); writing selects by extension. Both
//! behaviours predate the migration and are preserved by the shared crate.

pub use bioinfo_fastx::compress::{decompress, Levels, BUFFER_SIZE};
pub use bioinfo_fastx::format::{format_from_extension, Format};

/// The levels cutadapt's default mode uses.
pub const DEFAULT_LEVELS: Levels = Levels::DEFAULT;
/// The levels cutadapt's `-Z` uses.
pub const FAST_LEVELS: Levels = Levels::FAST;

/// Map cutadapt's `-Z` flag onto the shared level set.
pub fn levels(level_1: bool) -> Levels {
    if level_1 {
        Levels::FAST
    } else {
        Levels::DEFAULT
    }
}

/// Select the output format for `path`, treating `-` as plain.
///
/// cutadapt writes stdout uncompressed; a `.gz` name is the only thing that turns
/// compression on.
pub fn output_format(path: &std::path::Path) -> Format {
    if path.as_os_str() == "-" {
        Format::Plain
    } else {
        format_from_extension(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn detection_matches_the_pre_migration_behaviour() {
        assert_eq!(Format::from_magic(&[0x1f, 0x8b, 0x08]), Format::Gzip);
        assert_eq!(Format::from_magic(b"BZh9"), Format::Bzip2);
        assert_eq!(
            Format::from_magic(&[0xfd, b'7', b'z', b'X', b'Z', 0x00]),
            Format::Xz
        );
        assert_eq!(Format::from_magic(b"@r1\nACGT"), Format::Plain);
        assert_eq!(Format::from_magic(b""), Format::Plain);
    }

    #[test]
    fn extensions_match_the_pre_migration_behaviour() {
        for (name, format) in [
            ("out.fastq.gz", Format::Gzip),
            ("out.fastq.GZ", Format::Gzip),
            ("out.fastq.bz2", Format::Bzip2),
            ("out.fastq.xz", Format::Xz),
            ("out.fastq", Format::Plain),
            ("-", Format::Plain),
        ] {
            assert_eq!(format_from_extension(Path::new(name)), format, "{name}");
        }
    }

    #[test]
    fn levels_follow_the_zmapping() {
        assert_eq!(levels(false), Levels::DEFAULT);
        assert_eq!(levels(true), Levels::FAST);
        assert_eq!(DEFAULT_LEVELS.gzip(), 3);
        assert_eq!(FAST_LEVELS.gzip(), 1);
        assert_eq!(DEFAULT_LEVELS.bzip2(), 9);
        assert_eq!(FAST_LEVELS.bzip2(), 1);
        assert_eq!(DEFAULT_LEVELS.xz(), 6);
        assert_eq!(FAST_LEVELS.xz(), 0);
    }

    #[test]
    fn stdout_is_never_compressed() {
        assert_eq!(output_format(Path::new("-")), Format::Plain);
        assert_eq!(output_format(Path::new("out.fastq.gz")), Format::Gzip);
    }

    #[test]
    fn every_format_round_trips_through_the_shared_helpers() {
        use std::io::{Read, Write};
        for (name, _) in [
            ("a.fastq", Format::Plain),
            ("a.fastq.gz", Format::Gzip),
            ("a.fastq.bz2", Format::Bzip2),
            ("a.fastq.xz", Format::Xz),
        ] {
            let dir = std::env::temp_dir().join(format!(
                "rc-compress-{}-{:?}-{name}",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = std::fs::remove_file(&dir);
            let payload = b"@r1\nACGTACGT\n+\nIIIIIIII\n";
            for level_1 in [false, true] {
                let compressed = {
                    let file = std::fs::File::create(&dir).unwrap();
                    let mut writer = bioinfo_fastx::compressor_for(
                        Box::new(file),
                        output_format(Path::new(name)),
                        levels(level_1),
                    )
                    .unwrap();
                    writer.write_all(payload).unwrap();
                    writer.finish().unwrap();
                    std::fs::read(&dir).unwrap()
                };
                let mut reader = decompress(Box::new(std::io::Cursor::new(compressed))).unwrap();
                let mut back = Vec::new();
                reader.read_to_end(&mut back).unwrap();
                assert_eq!(back, payload, "{name} level_1={level_1}");
            }
            let _ = std::fs::remove_file(&dir);
        }
    }
}
