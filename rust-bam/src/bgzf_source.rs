//! Preserve compressed reads while rejecting a partial final BGZF header.
//!
//! The pinned BGZF frame reader treats UnexpectedEof while reading its 18-byte
//! header as clean EOF, even if it consumed a partial header. Track only compressed
//! frame boundaries, and report that case as InvalidData when the underlying input
//! actually returns EOF. Do not validate ahead or report errors on positive reads:
//! ordinary decoding must still report an earlier header, CRC or BAM field error.

use std::io::{self, Read};

const HEADER_SIZE: usize = 18;
const MIN_FRAME_SIZE: usize = HEADER_SIZE + 8;

pub(crate) struct BgzfSource<R> {
    inner: R,
    header: [u8; HEADER_SIZE],
    header_len: usize,
    body_remaining: usize,
    tracking: bool,
}

impl<R> BgzfSource<R> {
    pub(crate) fn new(inner: R) -> Self {
        Self {
            inner,
            header: [0; HEADER_SIZE],
            header_len: 0,
            body_remaining: 0,
            tracking: true,
        }
    }

    fn advance(&mut self, mut bytes: &[u8]) {
        while self.tracking && !bytes.is_empty() {
            if self.body_remaining != 0 {
                let count = bytes.len().min(self.body_remaining);
                self.body_remaining -= count;
                bytes = &bytes[count..];
                continue;
            }
            let count = bytes.len().min(HEADER_SIZE - self.header_len);
            self.header[self.header_len..self.header_len + count].copy_from_slice(&bytes[..count]);
            self.header_len += count;
            bytes = &bytes[count..];
            if self.header_len == HEADER_SIZE {
                let length =
                    usize::from(u16::from_le_bytes([self.header[16], self.header[17]])) + 1;
                if length < MIN_FRAME_SIZE {
                    // The ordinary frame parser owns this earlier error. Its
                    // malformed size makes subsequent boundary tracking unsafe.
                    self.tracking = false;
                    return;
                }
                self.header_len = 0;
                self.body_remaining = length - HEADER_SIZE;
            }
        }
    }
}

impl<R: Read> Read for BgzfSource<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let count = self.inner.read(buf)?;
        if count == 0 {
            if self.tracking && self.header_len != 0 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "truncated BGZF header",
                ));
            }
        } else {
            self.advance(&buf[..count]);
        }
        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use noodles::bgzf;
    use std::io::{BufReader, Cursor, Write};

    struct Chunked {
        inner: Cursor<Vec<u8>>,
        chunk: usize,
    }
    impl Read for Chunked {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            let count = buf.len().min(self.chunk);
            self.inner.read(&mut buf[..count])
        }
    }

    fn encode(bytes: &[u8]) -> Vec<u8> {
        let mut writer = bgzf::io::Writer::new(Vec::new());
        writer.write_all(bytes).unwrap();
        writer.finish().unwrap()
    }

    fn decode(
        bytes: Vec<u8>,
        chunk: usize,
        checked: bool,
    ) -> (Vec<u8>, Option<(io::ErrorKind, String)>) {
        let inner = Chunked {
            inner: Cursor::new(bytes),
            chunk,
        };
        let source: Box<dyn Read> = if checked {
            Box::new(BgzfSource::new(inner))
        } else {
            Box::new(inner)
        };
        let mut reader = bgzf::io::Reader::new(BufReader::with_capacity(31, source));
        let mut output = Vec::new();
        let error = reader
            .read_to_end(&mut output)
            .err()
            .map(|e| (e.kind(), e.to_string()));
        (output, error)
    }

    #[test]
    fn every_partial_header_length_is_rejected_without_losing_prior_data() {
        let payload = b"valid block data".repeat(5000);
        let complete = encode(&payload);
        let eof = encode(&[]);
        let body = &complete[..complete.len() - eof.len()];
        for tail in 1..HEADER_SIZE {
            for chunk in [1, 2, 17, 18, 19, 128, 8192] {
                let input = [body, &eof[..tail]].concat();
                let old = decode(input.clone(), chunk, false);
                assert_eq!(old, (payload.clone(), None), "negative control tail={tail}");
                let checked = decode(input, chunk, true);
                assert_eq!(checked.0, payload);
                assert_eq!(
                    checked.1,
                    Some((io::ErrorKind::InvalidData, "truncated BGZF header".into()))
                );
            }
        }
    }

    #[test]
    fn complete_members_and_truncated_bodies_retain_original_behavior() {
        let a = encode(b"first member");
        let b = encode(b"second member");
        let inputs = [
            Vec::new(),
            a.clone(),
            a[..a.len() - 28].to_vec(),
            [a.clone(), b].concat(),
            a[..a.len() - 29].to_vec(),
        ];
        for input in inputs {
            for chunk in [1, 17, 31, 8192] {
                assert_eq!(
                    decode(input.clone(), chunk, true),
                    decode(input.clone(), chunk, false)
                );
            }
        }
    }

    #[test]
    fn earlier_checksum_header_and_frame_size_errors_keep_precedence() {
        let complete = encode(b"valid payload");
        let end = complete.len() - 28;
        let mut crc = complete[..end].to_vec();
        crc[end - 8] ^= 1;
        let mut header = complete[..end].to_vec();
        header[0] ^= 1;
        let mut size = complete[..end].to_vec();
        size[16..18].copy_from_slice(&0u16.to_le_bytes());
        for invalid in [crc, header, size] {
            let input = [invalid, complete[..17].to_vec()].concat();
            for chunk in [1, 18, 8192] {
                let expected = decode(input.clone(), chunk, false);
                assert!(expected.1.is_some());
                assert_eq!(decode(input.clone(), chunk, true), expected);
            }
        }
    }

    #[test]
    fn empty_read_is_not_mistaken_for_end_of_input() {
        let mut source = BgzfSource::new(Cursor::new(vec![0x1f]));
        assert_eq!(source.read(&mut [0]).unwrap(), 1);
        assert_eq!(source.read(&mut []).unwrap(), 0);
        assert_eq!(
            source.read(&mut [0]).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
    }
}
