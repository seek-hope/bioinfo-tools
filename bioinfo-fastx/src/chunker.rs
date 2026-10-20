//! Splitting a FASTQ stream into record-aligned chunks.
//!
//! `rust-cutadapt`'s parallel pipeline decompresses the input, cuts it into
//! chunks that each contain whole records, and hands one chunk per worker. The
//! output must not depend on the worker count, so the split points must be
//! deterministic and every chunk must be independently parseable. This module
//! owns that rule.
//!
//! # Why four newlines
//!
//! A FASTQ record is four lines, so a chunk may end after any fourth newline.
//! That is deliberately *structural* rather than a parse: the chunker must work
//! at multi-gigabyte throughput without validating records twice, and a
//! malformed record still has to reach the parser so it can be reported with its
//! exact line and offset. The chunker therefore never rejects anything.
//!
//! A trailing fragment with fewer than four newlines is handed on as the last
//! chunk; the parser then reports it as truncated, exactly as it would have
//! without chunking.

/// Default chunk size in bytes.
///
/// 256 KiB matches `rust-cutadapt`'s long-standing value; it is large enough that
/// per-chunk overhead is irrelevant and small enough to stay in L2.
pub const DEFAULT_CHUNK_SIZE: usize = 256 * 1024;

/// Find the end of the last complete FASTQ record inside `buf`.
///
/// Returns the index just past the last `\n` that completes a group of four,
/// or `None` when fewer than four lines have accumulated.
pub fn last_complete_record_end(buf: &[u8]) -> Option<usize> {
    let mut newlines = 0usize;
    let mut end = None;
    for (i, &b) in buf.iter().enumerate() {
        if b == b'\n' {
            newlines += 1;
            if newlines.is_multiple_of(4) {
                end = Some(i + 1);
            }
        }
    }
    end
}

/// Split `data` into the ranges of its complete four-line records.
///
/// A trailing partial record is omitted; the parser reports it. Used by the
/// paired-end path, which must process one mate's records in groups while the
/// other mate's stay aligned.
pub fn split_records(data: &[u8]) -> Vec<RecordRange> {
    let mut ranges = Vec::new();
    let mut newlines = 0usize;
    let mut start = 0usize;
    for (i, &b) in data.iter().enumerate() {
        if b == b'\n' {
            newlines += 1;
            if newlines.is_multiple_of(4) {
                ranges.push(RecordRange { start, end: i + 1 });
                start = i + 1;
            }
        }
    }
    ranges
}

/// Byte range of one complete record inside a buffer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RecordRange {
    /// First byte of the record.
    pub start: usize,
    /// One past the last byte (the newline ending the quality line).
    pub end: usize,
}

impl RecordRange {
    /// Length in bytes.
    pub fn len(&self) -> usize {
        self.end - self.start
    }

    /// True when the range is empty.
    pub fn is_empty(&self) -> bool {
        self.end == self.start
    }
}

/// Read a stream to its end in record-aligned chunks.
///
/// `on_chunk` receives the chunk bytes, the chunk's byte offset in the
/// uncompressed input, and the 1-based line number of its first line.
///
/// # Errors
///
/// Only I/O errors and errors returned by `on_chunk`; malformed FASTQ is *not* an
/// error here, because the parser is the component that decides what malformed
/// means. This keeps the chunker usable by a caller whose parser is stricter or
/// more lenient than the default.
pub fn for_each_chunk<R, F, E>(reader: &mut R, chunk_size: usize, mut on_chunk: F) -> Result<(), E>
where
    R: std::io::Read + ?Sized,
    F: FnMut(Vec<u8>, u64, u64) -> Result<(), E>,
    E: From<std::io::Error>,
{
    let chunk_size = chunk_size.max(4096);
    let mut carry: Vec<u8> = Vec::with_capacity(chunk_size + 4096);
    let mut incoming = vec![0u8; chunk_size];
    let mut base_offset: u64 = 0;
    let mut base_line: u64 = 1;
    loop {
        let filled = read_up_to(reader, &mut incoming)?;
        if filled == 0 {
            break;
        }
        carry.extend_from_slice(&incoming[..filled]);
        if let Some(split) = last_complete_record_end(&carry) {
            let chunk: Vec<u8> = carry.drain(..split).collect();
            let newlines = chunk.iter().filter(|&&b| b == b'\n').count() as u64;
            on_chunk(chunk, base_offset, base_line)?;
            base_offset += split as u64;
            base_line += newlines;
        }
    }
    if !carry.is_empty() {
        // A trailing partial record: hand it over so the parser reports it with
        // the position it would have had in a single-pass read.
        on_chunk(std::mem::take(&mut carry), base_offset, base_line)?;
    }
    Ok(())
}

fn read_up_to<R: std::io::Read + ?Sized>(reader: &mut R, buf: &mut [u8]) -> std::io::Result<usize> {
    let mut filled = 0;
    while filled < buf.len() {
        match reader.read(&mut buf[filled..]) {
            Ok(0) => break,
            Ok(n) => filled += n,
            Err(ref e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        }
    }
    Ok(filled)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn record(name: &str, len: usize) -> String {
        format!("@{name}\n{}\n+\n{}\n", "A".repeat(len), "I".repeat(len))
    }

    #[test]
    fn finds_the_last_complete_record_boundary() {
        // Three records: the boundary is after the third, not after a line.
        let data = format!("{}{}{}", record("a", 4), record("b", 4), record("c", 4));
        assert_eq!(last_complete_record_end(data.as_bytes()), Some(data.len()));
    }

    #[test]
    fn ignores_a_trailing_partial_record() {
        let mut data = record("a", 4);
        let full = data.len();
        data.push_str("@b\nACGT\n+\n"); // missing the quality line
        assert_eq!(last_complete_record_end(data.as_bytes()), Some(full));
    }

    #[test]
    fn fewer_than_four_lines_has_no_boundary() {
        assert_eq!(last_complete_record_end(b"@a\nACGT\n+\n"), None);
        assert_eq!(last_complete_record_end(b""), None);
        assert_eq!(last_complete_record_end(b"@a\n"), None);
    }

    #[test]
    fn split_records_returns_every_complete_record() {
        let data = format!("{}{}{}", record("a", 4), record("b", 7), record("c", 1));
        let ranges = split_records(data.as_bytes());
        assert_eq!(ranges.len(), 3);
        assert_eq!(&data[ranges[0].start..ranges[0].end], record("a", 4));
        assert_eq!(&data[ranges[1].start..ranges[1].end], record("b", 7));
        assert_eq!(&data[ranges[2].start..ranges[2].end], record("c", 1));
        assert_eq!(ranges.iter().map(|r| r.len()).sum::<usize>(), data.len());
    }

    #[test]
    fn split_records_omits_a_partial_tail() {
        let mut data = record("a", 4);
        let full = data.len();
        data.push_str("@b\nACGT\n");
        let ranges = split_records(data.as_bytes());
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0].end, full);
    }

    #[test]
    fn record_range_helpers() {
        let range = RecordRange { start: 5, end: 9 };
        assert_eq!(range.len(), 4);
        assert!(!range.is_empty());
        assert!(RecordRange { start: 3, end: 3 }.is_empty());
    }

    /// Collect chunks the way the pipeline does.
    fn chunks(data: &[u8], chunk_size: usize) -> Vec<(Vec<u8>, u64, u64)> {
        let mut out = Vec::new();
        let mut cursor = Cursor::new(data.to_vec());
        for_each_chunk::<_, _, std::io::Error>(&mut cursor, chunk_size, |chunk, offset, line| {
            out.push((chunk, offset, line));
            Ok(())
        })
        .expect("in-memory read cannot fail");
        out
    }

    #[test]
    fn chunks_reassemble_to_the_input_exactly() {
        let data: String = (0..500)
            .map(|i| record(&format!("r{i}"), 10 + i % 20))
            .collect();
        for chunk_size in [4096usize, 8192, 65536, 1 << 20] {
            let got = chunks(data.as_bytes(), chunk_size);
            let joined: Vec<u8> = got.iter().flat_map(|(c, _, _)| c.clone()).collect();
            assert_eq!(joined, data.as_bytes(), "chunk size {chunk_size}");
        }
    }

    #[test]
    fn chunk_offsets_and_line_numbers_are_exact() {
        let data: String = (0..200).map(|i| record(&format!("r{i}"), 12)).collect();
        let got = chunks(data.as_bytes(), 4096);
        assert!(got.len() > 1, "the test needs several chunks");
        let mut expected_offset = 0u64;
        let mut expected_line = 1u64;
        for (chunk, offset, line) in &got {
            assert_eq!(*offset, expected_offset, "byte offset");
            assert_eq!(*line, expected_line, "line number");
            expected_offset += chunk.len() as u64;
            expected_line += chunk.iter().filter(|&&b| b == b'\n').count() as u64;
        }
        assert_eq!(expected_offset, data.len() as u64);
    }

    #[test]
    fn every_chunk_but_the_last_ends_on_a_record_boundary() {
        let data: String = (0..300).map(|i| record(&format!("r{i}"), 9)).collect();
        let got = chunks(data.as_bytes(), 4096);
        for (chunk, _, _) in &got[..got.len() - 1] {
            assert_eq!(
                chunk.iter().filter(|&&b| b == b'\n').count() % 4,
                0,
                "a non-final chunk must end after a complete record"
            );
        }
    }

    #[test]
    fn a_trailing_partial_record_is_handed_on() {
        let mut data: String = (0..50).map(|i| record(&format!("r{i}"), 9)).collect();
        data.push_str("@partial\nACGT\n");
        let got = chunks(data.as_bytes(), 4096);
        let last = &got.last().expect("at least one chunk").0;
        assert!(
            last.starts_with(b"@partial") || last.windows(8).any(|w| w == b"@partial"),
            "the partial record must reach the parser so it can be reported"
        );
    }

    #[test]
    fn empty_input_produces_no_chunks() {
        let got = chunks(b"", 4096);
        assert!(
            got.is_empty() || got.iter().all(|(c, _, _)| c.is_empty()),
            "empty input must not synthesise a chunk"
        );
    }

    #[test]
    fn a_tiny_chunk_size_is_clamped_rather_than_hanging() {
        let data = record("a", 100);
        let got = chunks(data.as_bytes(), 1);
        let joined: Vec<u8> = got.iter().flat_map(|(c, _, _)| c.clone()).collect();
        assert_eq!(joined, data.as_bytes());
    }

    #[test]
    fn errors_from_the_callback_are_propagated() {
        let mut cursor = Cursor::new(record("a", 4).into_bytes());
        let result: Result<(), std::io::Error> = for_each_chunk(&mut cursor, 4096, |_, _, _| {
            Err(std::io::Error::other("stop here"))
        });
        let err = result.expect_err("the callback error must propagate");
        assert_eq!(err.to_string(), "stop here");
    }
}
