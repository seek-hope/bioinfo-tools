//! BGZF (Blocked GNU Zip Format) writing.
//!
//! BGZF is the format SAM/BAM use for random access: a gzip stream whose members
//! are capped at 64 KiB, where each member carries a `BC` extra subfield giving
//! the member's own size. A reader can therefore seek to any member without
//! decompressing the preceding ones.
//!
//! # Byte format of one block
//!
//! ```text
//! offset  size  field
//!      0     2  ID1=0x1f ID2=0x8b          (gzip magic)
//!      2     1  CM=8                       (deflate)
//!      3     1  FLG=0x04                   (FEXTRA set)
//!      4     4  MTIME=0                    (reproducible output)
//!      8     1  XFL                         (2 = max, 4 = fastest; 0 otherwise)
//!      9     1  OS=0xff                    (unknown, "not recorded")
//!     10     2  XLEN=6
//!     12     2  SI1=66 SI2=67             ("BC")
//!     14     2  SLEN=2
//!     16     2  BSIZE = total_block_size - 1
//!     18     n  raw deflate payload
//!   18+n     4  CRC32 of the uncompressed payload
//!   22+n     4  ISIZE = uncompressed payload length
//! ```
//!
//! The `XFL`/`OS` bytes are the values the reference implementation writes, so
//! the output is byte-identical to `htslib`'s for the same payload and level.
//! `MTIME` is zeroed, which makes the output reproducible — a deliberate choice
//! recorded in `docs/EVIDENCE-W1-2.md`.
//!
//! # What is not here
//!
//! No reader: BGZF reading is plain gzip reading, and
//! [`crate::compress::decompress`] handles it through `MultiGzDecoder`. No index
//! (`.gzi`) support; that belongs to the BAM work in W1-4.

use std::io::{self, Write};

/// Maximum uncompressed bytes per block.
///
/// BGZF's block size field is 16 bits and covers the *compressed* size, so the
/// reference implementation caps the uncompressed payload at 64 KiB to guarantee
/// the compressed block always fits. `htslib` uses exactly this value.
pub const MAX_BLOCK_SIZE: usize = 0xff00;

/// The fixed 18-byte header of a BGZF block.
const BLOCK_HEADER_LEN: usize = 18;
/// Header + CRC32 + ISIZE.
const BLOCK_OVERHEAD: usize = BLOCK_HEADER_LEN + 8;

/// gzip `OS` byte written by the reference implementation ("unknown").
const OS_UNKNOWN: u8 = 0xff;
/// gzip `FLG` with FEXTRA set.
const FLG_FEXTRA: u8 = 0x04;

/// A streaming BGZF writer.
///
/// Data is buffered into blocks of at most [`MAX_BLOCK_SIZE`] uncompressed bytes,
/// each written as one complete BGZF member. [`BgzfWriter::finish`] writes the
/// terminating empty block (the BGZF EOF marker) and must be called for a reader
/// to see a well-formed file.
pub struct BgzfWriter<W: Write> {
    /// `Some` until `into_inner` takes it, so `Drop` can still run.
    sink: Option<W>,
    level: u32,
    buffer: Vec<u8>,
    blocks: u64,
    finished: bool,
}

impl<W: Write> BgzfWriter<W> {
    /// Wrap `sink`; `level` is the deflate level for each block.
    pub fn new(sink: W, level: u32) -> BgzfWriter<W> {
        BgzfWriter {
            sink: Some(sink),
            level,
            buffer: Vec::with_capacity(MAX_BLOCK_SIZE),
            blocks: 0,
            finished: false,
        }
    }

    /// The number of blocks written so far, including the EOF marker.
    pub fn blocks(&self) -> u64 {
        self.blocks
    }

    /// Flush the current block, write the EOF marker and flush the sink.
    ///
    /// Writing the EOF marker matches what `htslib` produces and what
    /// `samtools quickcheck` looks for.
    pub fn finish(&mut self) -> io::Result<()> {
        self.flush_block()?;
        let eof = encode_block(&[], self.level)?;
        let sink = self.sink.as_mut().expect("sink present until into_inner");
        sink.write_all(&eof)?;
        self.blocks += 1;
        sink.flush()?;
        self.finished = true;
        Ok(())
    }

    /// The underlying sink. Only meaningful after [`BgzfWriter::finish`].
    pub fn into_inner(mut self) -> Option<W> {
        self.sink.take()
    }

    fn sink_mut(&mut self) -> io::Result<&mut W> {
        self.sink
            .as_mut()
            .ok_or_else(|| io::Error::other("BGZF writer sink already taken"))
    }

    fn flush_block(&mut self) -> io::Result<()> {
        if self.buffer.is_empty() {
            return Ok(());
        }
        let block = encode_block(&self.buffer, self.level)?;
        self.sink_mut()?.write_all(&block)?;
        self.blocks += 1;
        self.buffer.clear();
        Ok(())
    }
}

impl<W: Write> Write for BgzfWriter<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut written = 0;
        while written < buf.len() {
            let room = MAX_BLOCK_SIZE - self.buffer.len();
            let take = room.min(buf.len() - written);
            self.buffer.extend_from_slice(&buf[written..written + take]);
            written += take;
            if self.buffer.len() == MAX_BLOCK_SIZE {
                self.flush_block()?;
            }
        }
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        // A `flush` must make the data observable, so the pending block goes out.
        self.flush_block()?;
        self.sink_mut()?.flush()
    }
}

impl<W: Write> Drop for BgzfWriter<W> {
    fn drop(&mut self) {
        // Best effort: a caller that forgot `finish` still gets a complete
        // stream rather than a silently truncated one.
        if !self.finished {
            let _ = self.finish();
        }
    }
}

/// Encode one BGZF block, header and trailer included.
pub fn encode_block(payload: &[u8], level: u32) -> io::Result<Vec<u8>> {
    assert!(
        payload.len() <= MAX_BLOCK_SIZE,
        "BGZF payload {} exceeds the {MAX_BLOCK_SIZE}-byte block limit",
        payload.len()
    );
    let deflated = crate::compress::raw_deflate(payload, level)?;
    let block_size = BLOCK_OVERHEAD + deflated.len();
    if block_size > 0x1_0000 {
        return Err(io::Error::other(format!(
            "BGZF block would be {block_size} bytes, which does not fit in the 16-bit BSIZE field; \
             the payload is incompressible"
        )));
    }
    // gzip XFL is written as 0, not as a compression hint.
    //
    // RFC 1952 says XFL is 2 for maximum compression and 4 for fastest, and a
    // plain gzip writer may set it. BGZF must NOT: htslib recognises the BGZF EOF
    // marker by comparing the final 28 bytes against a fixed byte pattern whose
    // XFL field is 0. With XFL=2 `samtools quickcheck` reports
    // "missing EOF block when one should be present" and exits 16, even though the
    // marker is present and `samtools view` can read the records.
    //
    // Found by the W1-4 oracle: a payload wrapped by this writer was readable by
    // `samtools view` but rejected by `samtools quickcheck`; rewriting byte 8 of
    // every block to 0 made quickcheck pass. That is the regression this pins.
    let xfl = 0u8;
    let mut out = Vec::with_capacity(block_size);
    out.extend_from_slice(&[0x1f, 0x8b, 8, FLG_FEXTRA]);
    out.extend_from_slice(&0u32.to_le_bytes()); // MTIME = 0: reproducible output
    out.push(xfl);
    out.push(OS_UNKNOWN);
    out.extend_from_slice(&6u16.to_le_bytes()); // XLEN
    out.extend_from_slice(b"BC");
    out.extend_from_slice(&2u16.to_le_bytes()); // SLEN
    out.extend_from_slice(&((block_size - 1) as u16).to_le_bytes()); // BSIZE
    out.extend_from_slice(&deflated);
    let crc = crc32fast::hash(payload);
    out.extend_from_slice(&crc.to_le_bytes());
    out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    debug_assert_eq!(out.len(), block_size);
    Ok(out)
}

/// Parse one BGZF block's `BSIZE`, for tests and for a future reader.
///
/// Returns the total block size including the header and the 8-byte trailer.
pub fn block_size_at(bytes: &[u8]) -> Option<usize> {
    if bytes.len() < BLOCK_HEADER_LEN || bytes[..2] != [0x1f, 0x8b] || bytes[3] & FLG_FEXTRA == 0 {
        return None;
    }
    let xlen = u16::from_le_bytes([bytes[10], bytes[11]]) as usize;
    // Walk the extra fields looking for "BC".
    let mut pos = 12;
    let extra_end = 12 + xlen;
    if extra_end > bytes.len() {
        return None;
    }
    while pos + 4 <= extra_end {
        let si1 = bytes[pos];
        let si2 = bytes[pos + 1];
        let slen = u16::from_le_bytes([bytes[pos + 2], bytes[pos + 3]]) as usize;
        if si1 == b'B' && si2 == b'C' && slen == 2 && pos + 6 <= extra_end {
            let bsize = u16::from_le_bytes([bytes[pos + 4], bytes[pos + 5]]) as usize;
            return Some(bsize + 1);
        }
        pos += 4 + slen;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    fn read_bgzf(bytes: &[u8]) -> Vec<u8> {
        // BGZF is gzip: a plain multi-member gzip decoder reads it.
        let mut reader = flate2::read::MultiGzDecoder::new(io::Cursor::new(bytes));
        let mut out = Vec::new();
        reader.read_to_end(&mut out).unwrap();
        out
    }

    fn payload(n: usize) -> Vec<u8> {
        let mut out = Vec::new();
        for i in 0..n {
            out.extend_from_slice(format!("@read{i}\nACGTACGTACGT\n+\nIIIIIIIIIIII\n").as_bytes());
        }
        out
    }

    // htslib compares the last 28 bytes to this exact pattern. Any change makes
    // `samtools quickcheck` fail with "missing EOF block".
    const HTSLIB_EOF_MARKER: [u8; 28] = [
        0x1f, 0x8b, 0x08, 0x04, 0x00, 0x00, 0x00, 0x00, 0x00, 0xff, 0x06, 0x00, 0x42, 0x43, 0x02,
        0x00, 0x1b, 0x00, 0x03, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    ];

    #[test]
    fn the_eof_marker_is_byte_identical_to_htslibs() {
        let mut writer = BgzfWriter::new(Vec::new(), 6);
        writer.write_all(b"payload").unwrap();
        writer.finish().unwrap();
        let bytes = writer.into_inner().unwrap();
        assert_eq!(&bytes[bytes.len() - 28..], &HTSLIB_EOF_MARKER);
    }

    #[test]
    fn xfl_is_zero_at_every_level() {
        // A compression hint in XFL breaks htslib's EOF-marker comparison.
        for level in [0u32, 1, 3, 6, 9] {
            let block = encode_block(b"ACGT", level).unwrap();
            assert_eq!(block[8], 0, "XFL must be 0 at level {level}");
        }
    }

    #[test]
    fn block_header_is_the_documented_layout() {
        let block = encode_block(b"ACGT", 6).unwrap();
        assert_eq!(&block[0..2], &[0x1f, 0x8b], "gzip magic");
        assert_eq!(block[2], 8, "CM = deflate");
        assert_eq!(block[3], FLG_FEXTRA, "FLG has FEXTRA");
        assert_eq!(&block[4..8], &[0, 0, 0, 0], "MTIME is zero");
        assert_eq!(block[9], OS_UNKNOWN, "OS = unknown");
        assert_eq!(u16::from_le_bytes([block[10], block[11]]), 6, "XLEN");
        assert_eq!(&block[12..14], b"BC", "BC subfield");
        assert_eq!(u16::from_le_bytes([block[14], block[15]]), 2, "SLEN");
        let bsize = u16::from_le_bytes([block[16], block[17]]) as usize;
        assert_eq!(
            bsize + 1,
            block.len(),
            "BSIZE covers the whole block minus 1"
        );
        // Trailer: CRC32 then ISIZE of the uncompressed payload.
        let trailer = &block[block.len() - 8..];
        assert_eq!(
            &trailer[..4],
            &crc32fast::hash(b"ACGT").to_le_bytes(),
            "CRC32"
        );
        assert_eq!(&trailer[4..], &4u32.to_le_bytes(), "ISIZE");
    }

    #[test]
    fn a_block_inflates_to_its_payload() {
        let data = payload(10);
        let block = encode_block(&data, 6).unwrap();
        assert_eq!(read_bgzf(&block), data);
    }

    #[test]
    fn block_size_at_finds_the_size() {
        let data = payload(5);
        let block = encode_block(&data, 6).unwrap();
        assert_eq!(block_size_at(&block), Some(block.len()));
    }

    #[test]
    fn block_size_at_rejects_non_bgzf() {
        assert_eq!(block_size_at(b""), None);
        assert_eq!(block_size_at(b"@r1\nACGT\n"), None);
        // A plain gzip header has no FEXTRA.
        let mut plain = Vec::new();
        {
            use std::io::Write as _;
            let mut enc = flate2::write::GzEncoder::new(&mut plain, flate2::Compression::new(6));
            enc.write_all(b"data").unwrap();
            enc.finish().unwrap();
        }
        assert_eq!(block_size_at(&plain), None);
    }

    #[test]
    fn streaming_writes_are_readable() {
        let data = payload(500);
        let mut writer = BgzfWriter::new(Vec::new(), 6);
        // Deliberately odd-sized writes to exercise the block boundary.
        for chunk in data.chunks(997) {
            writer.write_all(chunk).unwrap();
        }
        writer.finish().unwrap();
        let bytes = writer.into_inner().unwrap();
        assert_eq!(read_bgzf(&bytes), data);
    }

    #[test]
    fn large_input_spans_multiple_blocks() {
        // `payload(n)` is ~40 bytes per record, so this is comfortably more than
        // four blocks of `MAX_BLOCK_SIZE`.
        let data = payload(30_000);
        assert!(data.len() > 3 * MAX_BLOCK_SIZE);
        let mut writer = BgzfWriter::new(Vec::new(), 6);
        writer.write_all(&data).unwrap();
        writer.finish().unwrap();
        let blocks = writer.blocks();
        let bytes = writer.into_inner().unwrap();
        // Every data block plus the EOF marker.
        assert!(blocks >= 4, "expected several blocks, got {blocks}");
        assert_eq!(read_bgzf(&bytes), data);
    }

    #[test]
    fn every_block_but_the_eof_marker_carries_at_most_the_block_limit() {
        let data = payload(3000);
        let mut writer = BgzfWriter::new(Vec::new(), 6);
        writer.write_all(&data).unwrap();
        writer.finish().unwrap();
        let bytes = writer.into_inner().unwrap();
        let mut pos = 0;
        let mut data_blocks = 0;
        while pos < bytes.len() {
            let size = block_size_at(&bytes[pos..]).expect("every block is BGZF");
            // BSIZE is 16-bit, so this is really a format assertion.
            assert!(size <= 0x1_0000, "block of {size} bytes exceeds the format");
            // The uncompressed size is the last 4 bytes of the block.
            let isize =
                u32::from_le_bytes(bytes[pos + size - 4..pos + size].try_into().unwrap()) as usize;
            assert!(
                isize <= MAX_BLOCK_SIZE,
                "uncompressed block of {isize} exceeds MAX_BLOCK_SIZE"
            );
            if isize > 0 {
                data_blocks += 1;
            }
            pos += size;
        }
        assert_eq!(pos, bytes.len(), "blocks tile the stream exactly");
        assert!(data_blocks >= 2);
    }

    #[test]
    fn the_eof_marker_is_a_28_byte_empty_block() {
        let mut writer = BgzfWriter::new(Vec::new(), 6);
        writer.write_all(b"x").unwrap();
        writer.finish().unwrap();
        let bytes = writer.into_inner().unwrap();
        let mut pos = 0;
        let mut last_size = 0;
        while pos < bytes.len() {
            last_size = block_size_at(&bytes[pos..]).unwrap();
            pos += last_size;
        }
        assert_eq!(last_size, 28, "the standard BGZF EOF marker is 28 bytes");
        let eof = &bytes[bytes.len() - 28..];
        let isize = u32::from_le_bytes(eof[24..28].try_into().unwrap());
        assert_eq!(isize, 0, "the EOF marker carries no data");
    }

    #[test]
    fn empty_input_yields_only_the_eof_marker() {
        let mut writer = BgzfWriter::new(Vec::new(), 6);
        writer.finish().unwrap();
        let bytes = writer.into_inner().unwrap();
        assert_eq!(bytes.len(), 28);
        assert!(read_bgzf(&bytes).is_empty());
    }

    #[test]
    fn output_is_reproducible() {
        // MTIME is zero, so two runs produce identical bytes. This is what lets a
        // golden fixture pin a BGZF stream.
        let data = payload(50);
        let render = || {
            let mut writer = BgzfWriter::new(Vec::new(), 6);
            writer.write_all(&data).unwrap();
            writer.finish().unwrap();
            writer.into_inner()
        };
        assert_eq!(render(), render());
    }

    #[test]
    fn finish_is_idempotent_in_effect() {
        // A second finish after drop-time finish must not append another marker
        // halfway through; the writer tracks that it is done.
        let mut writer = BgzfWriter::new(Vec::new(), 6);
        writer.write_all(b"data").unwrap();
        writer.finish().unwrap();
        let blocks = writer.blocks();
        let bytes = writer.into_inner().unwrap();
        assert_eq!(read_bgzf(&bytes), b"data");
        // 1 data block + 1 EOF marker.
        assert_eq!(blocks, 2);
    }

    #[test]
    fn explicit_flush_emits_a_block() {
        let mut writer = BgzfWriter::new(Vec::new(), 6);
        writer.write_all(b"@r1\nACGT\n+\nIIII\n").unwrap();
        writer.flush().unwrap();
        assert_eq!(writer.blocks(), 1, "flush must make data observable");
        writer.finish().unwrap();
    }

    #[test]
    fn incompressible_payload_is_rejected_instead_of_overflowing_bsize() {
        // 64 KiB of true random data cannot always deflate below the 16-bit
        // BSIZE limit. If it does not, the writer must say so rather than emit a
        // corrupt block.
        let mut state = 0x243F_6A88_85A3_08D3u64;
        let data: Vec<u8> = (0..MAX_BLOCK_SIZE)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                (state >> 24) as u8
            })
            .collect();
        match encode_block(&data, 0) {
            Ok(block) => {
                // Legal only if it fits; then it must decode correctly.
                assert!(block.len() <= 0x1_0000);
                assert_eq!(read_bgzf(&block), data);
            }
            Err(e) => {
                assert!(
                    e.to_string().contains("does not fit"),
                    "unexpected error: {e}"
                );
            }
        }
    }
}
