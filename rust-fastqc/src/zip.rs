//! Minimal stored ZIP writer used for FastQC report artifacts.
//!
//! The archive contains only small text files (data, summary and HTML), so
//! storing them uncompressed keeps this crate dependency-free while producing a
//! standard ZIP that `unzip`, `zipinfo` and MultiQC can read. Plot images are not
//! fabricated; the HTML report states that explicitly.

use std::path::Path;

use crate::error::{Error, Result};

const LOCAL_HEADER: u32 = 0x0403_4b50;
const CENTRAL_HEADER: u32 = 0x0201_4b50;
const END_OF_CENTRAL: u32 = 0x0605_4b50;
const VERSION: u16 = 20;
const STORED: u16 = 0;

/// Build a valid stored ZIP archive from `(name, contents)` pairs.
pub fn stored_zip(entries: &[(String, Vec<u8>)]) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    let mut central = Vec::new();
    for (name, data) in entries {
        let name_len = u16::try_from(name.len())
            .map_err(|_| Error::io("ZIP entry name is too long".to_string()))?;
        let size = u32::try_from(data.len())
            .map_err(|_| Error::io("ZIP entry is too large".to_string()))?;
        let offset = u32::try_from(out.len())
            .map_err(|_| Error::io("ZIP archive is too large".to_string()))?;
        let crc = crc32(data);

        // Local file header, immediately followed by the stored bytes.
        put_u32(&mut out, LOCAL_HEADER);
        put_u16(&mut out, VERSION);
        put_u16(&mut out, 0); // general purpose flags
        put_u16(&mut out, STORED);
        put_u16(&mut out, 0); // DOS time
        put_u16(&mut out, 0x21); // DOS date: 1980-01-01
        put_u32(&mut out, crc);
        put_u32(&mut out, size);
        put_u32(&mut out, size);
        put_u16(&mut out, name_len);
        put_u16(&mut out, 0); // extra length
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(data);

        // Central directory entry.
        put_u32(&mut central, CENTRAL_HEADER);
        put_u16(&mut central, VERSION); // version made by
        put_u16(&mut central, VERSION); // version needed
        put_u16(&mut central, 0);
        put_u16(&mut central, STORED);
        put_u16(&mut central, 0);
        put_u16(&mut central, 0x21);
        put_u32(&mut central, crc);
        put_u32(&mut central, size);
        put_u32(&mut central, size);
        put_u16(&mut central, name_len);
        put_u16(&mut central, 0); // extra
        put_u16(&mut central, 0); // comment
        put_u16(&mut central, 0); // disk number
        put_u16(&mut central, 0); // internal attributes
        put_u32(&mut central, 0); // external attributes
        put_u32(&mut central, offset);
        central.extend_from_slice(name.as_bytes());
    }

    let central_offset =
        u32::try_from(out.len()).map_err(|_| Error::io("ZIP archive is too large".to_string()))?;
    let central_size = u32::try_from(central.len())
        .map_err(|_| Error::io("ZIP central directory is too large".to_string()))?;
    out.extend_from_slice(&central);

    put_u32(&mut out, END_OF_CENTRAL);
    put_u16(&mut out, 0); // disk number
    put_u16(&mut out, 0); // central directory disk
    put_u16(&mut out, u16::try_from(entries.len()).unwrap_or(u16::MAX));
    put_u16(&mut out, u16::try_from(entries.len()).unwrap_or(u16::MAX));
    put_u32(&mut out, central_size);
    put_u32(&mut out, central_offset);
    put_u16(&mut out, 0); // comment length
    Ok(out)
}

/// Write a stored ZIP archive, creating parent directories.
pub fn write_stored_zip(path: &Path, entries: &[(String, Vec<u8>)]) -> Result<()> {
    let bytes = stored_zip(entries)?;
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .map_err(|error| Error::io(format!("{}: {error}", parent.display())))?;
        }
    }
    std::fs::write(path, bytes).map_err(|error| Error::io(format!("{}: {error}", path.display())))
}

fn put_u16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn put_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ 0xedb8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn archive_has_local_central_and_end_headers() {
        let bytes = stored_zip(&[(
            "reads_fastqc/fastqc_data.txt".to_string(),
            b"hello".to_vec(),
        )])
        .unwrap();
        assert_eq!(&bytes[0..4], b"PK\x03\x04");
        assert!(bytes.windows(4).any(|window| window == b"PK\x01\x02"));
        assert!(bytes.windows(4).any(|window| window == b"PK\x05\x06"));
        let name = b"reads_fastqc/fastqc_data.txt";
        assert!(bytes.windows(name.len()).any(|window| window == name));
    }

    #[test]
    fn crc32_matches_known_value() {
        assert_eq!(crc32(b"123456789"), 0xcbf4_3926);
    }
}
