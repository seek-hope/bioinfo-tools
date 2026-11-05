use super::*;
use std::io::Write;

struct Fragmented {
    source: Cursor<Vec<u8>>,
    chunk: usize,
    interrupt: bool,
}
impl Read for Fragmented {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        if bytes.is_empty() {
            return Ok(0);
        }
        if self.interrupt {
            self.interrupt = false;
            return Err(io::ErrorKind::Interrupted.into());
        }
        self.interrupt = true;
        let length = bytes.len().min(self.chunk);
        self.source.read(&mut bytes[..length])
    }
}

#[test]
fn a_full_live_block_does_not_demand_its_delayed_trailer() {
    struct Paused(Cursor<Vec<u8>>);
    impl Read for Paused {
        fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
            assert!(
                self.0.position() < self.0.get_ref().len() as u64,
                "must not refill beyond the supplied full decoded block"
            );
            self.0.read(bytes)
        }
    }
    let payload = vec![b'A'; 65536];
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(&payload).unwrap();
    let encoded = encoder.finish().unwrap();
    for tail in [1, 4, 8] {
        let source = Paused(Cursor::new(encoded[..encoded.len() - tail].to_vec()));
        let mut reader = GzipBlocks::new(source, false);
        let mut actual = vec![0; 65536];
        reader.read_exact(&mut actual).unwrap();
        assert_eq!(actual, payload);
    }
}

#[test]
fn fragmented_interrupted_gzip_preserves_bytes_and_late_errors() {
    let mut seed = 0x9581_7350_0012_abcd_u64;
    let payload: Vec<_> = (0..150000)
        .map(|_| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed as u8
        })
        .collect();
    let mut encoder = flate2::GzBuilder::new()
        .filename(vec![b'a'; 5000])
        .write(Vec::new(), flate2::Compression::default());
    encoder.write_all(&payload).unwrap();
    let encoded = encoder.finish().unwrap();
    for chunk in [1, 2, 17, 2048, 8192, 65536] {
        for corrupt in [false, true] {
            let mut bytes = encoded.clone();
            if corrupt {
                let offset = bytes.len() - 8;
                bytes[offset] ^= 1;
            }
            let mut reader = GzipBlocks::new(
                Fragmented {
                    source: Cursor::new(bytes),
                    chunk,
                    interrupt: true,
                },
                true,
            );
            assert_eq!(reader.read(&mut []).unwrap(), 0);
            assert_eq!(reader.source.get_ref().0.source.position(), 0);
            let mut actual = Vec::new();
            let result = reader.read_to_end(&mut actual);
            if corrupt {
                assert_eq!(result.unwrap_err().kind(), io::ErrorKind::InvalidData);
                assert_eq!(actual, payload[..131072], "chunk {chunk}");
                let mut byte = [0x7f];
                assert_eq!(
                    reader.read(&mut byte).unwrap_err().kind(),
                    io::ErrorKind::InvalidData
                );
                assert_eq!(byte, [0x7f]);
            } else {
                result.unwrap();
                assert_eq!(actual, payload, "chunk {chunk}");
            }
        }
    }
}
