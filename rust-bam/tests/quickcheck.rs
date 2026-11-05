use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

use noodles::bgzf;

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "rust-bam-quickcheck-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn write(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let path = self.0.join(name);
        fs::write(&path, bytes).unwrap();
        path
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn bam(targets: bool) -> Vec<u8> {
    let text = if targets {
        b"@HD\tVN:1.6\n@SQ\tSN:chr1\tLN:1000\n".as_slice()
    } else {
        b"@HD\tVN:1.6\n".as_slice()
    };
    let mut raw = b"BAM\x01".to_vec();
    raw.extend_from_slice(&(text.len() as u32).to_le_bytes());
    raw.extend_from_slice(text);
    raw.extend_from_slice(&u32::from(targets).to_le_bytes());
    if targets {
        raw.extend_from_slice(&5u32.to_le_bytes());
        raw.extend_from_slice(b"chr1\0");
        raw.extend_from_slice(&1000u32.to_le_bytes());
    }
    let mut writer = bgzf::io::Writer::new(Vec::new());
    writer.write_all(&raw).unwrap();
    writer.finish().unwrap()
}
fn check(paths: &[&Path], expected: i32, diagnostics: &[(&Path, &str)]) {
    let output = Command::new(env!("CARGO_BIN_EXE_rust-bam"))
        .arg("quickcheck")
        .args(paths)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(expected));
    assert!(output.stdout.is_empty());
    let expected: String = diagnostics
        .iter()
        .map(|(p, reason)| format!("{} {reason}\n", p.display()))
        .collect();
    assert_eq!(output.stderr, expected.as_bytes());
}

#[test]
fn file_failures_keep_their_masks_and_all_diagnostics() {
    let f = Fixture::new();
    let valid = f.write("valid.bam", &bam(true));
    let no_targets = bam(false);
    let no_targets = f.write("no-targets.bam", &no_targets[..no_targets.len() - 28]);
    let text = f.write("not-sequence", b"not sequence data\n");
    let missing = f.0.join("missing");
    check(&[&valid], 0, &[]);
    check(
        &[&no_targets],
        24,
        &[
            (&no_targets, "had no targets in header."),
            (
                &no_targets,
                "was missing EOF block when one should be present.",
            ),
        ],
    );
    check(
        &[&valid, &missing, &text, &no_targets],
        30,
        &[
            (&missing, "could not be opened for reading."),
            (&text, "was not identified as sequence data."),
            (&no_targets, "had no targets in header."),
            (
                &no_targets,
                "was missing EOF block when one should be present.",
            ),
        ],
    );
}

#[test]
fn header_checksum_failure_also_reports_the_decoder_close_state() {
    let f = Fixture::new();
    let mut bytes = bam(true);
    let size = usize::from(u16::from_le_bytes([bytes[16], bytes[17]])) + 1;
    bytes[size - 8] ^= 1;
    let path = f.write("crc.bam", &bytes);
    check(
        &[&path],
        40,
        &[
            (&path, "caused an error whilst reading its header."),
            (&path, "did not close cleanly."),
        ],
    );
}

#[test]
fn records_and_later_blocks_are_outside_quickcheck_scope() {
    let f = Fixture::new();
    let mut bytes = bam(true);
    let eof = bytes.split_off(bytes.len() - 28);
    // An invalid BAM record in a separate block is deliberately not decoded.
    let mut body = bgzf::io::Writer::new(Vec::new());
    body.write_all(b"invalid BAM body").unwrap();
    let mut body = body.finish().unwrap();
    body.truncate(body.len() - 28);
    let size = body.len();
    body[size - 8] ^= 1;
    bytes.extend_from_slice(&body);
    bytes.extend_from_slice(&eof);
    let path = f.write("body-corrupt.bam", &bytes);
    check(&[&path], 0, &[]);
}

#[test]
fn gzip_sam_and_bam_pipe_headers_are_supported() {
    let f = Fixture::new();
    let sam = b"@HD\tVN:1.6\n@SQ\tSN:chr1\tLN:1000\n";
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(sam).unwrap();
    let path = f.write("sam.gz", &encoder.finish().unwrap());
    check(&[&path], 0, &[]);
    for bytes in [sam.to_vec(), bam(true)] {
        let mut child = Command::new(env!("CARGO_BIN_EXE_rust-bam"))
            .args(["quickcheck", "-"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(&bytes).unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success());
        assert!(output.stdout.is_empty());
        assert!(output.stderr.is_empty());
    }
}

#[test]
fn empty_members_and_long_gzip_metadata_do_not_hide_a_valid_header() {
    let f = Fixture::new();
    let bytes = bam(true);
    let eof = &bytes[bytes.len() - 28..];
    let mut raw = Vec::new();
    flate2::read::MultiGzDecoder::new(bytes.as_slice())
        .read_to_end(&mut raw)
        .unwrap();
    for (kind, payload) in [
        ("bam", raw.as_slice()),
        ("sam", b"@SQ\tSN:chr1\tLN:1000\n".as_slice()),
    ] {
        for split in 0..=3 {
            let mut extended = Vec::new();
            for (i, chunk) in [&payload[..split], &payload[split..]]
                .into_iter()
                .enumerate()
            {
                if i == 1 || split == 0 {
                    extended.extend_from_slice(&eof.repeat(100));
                }
                let mut writer = bgzf::io::Writer::new(Vec::new());
                writer.write_all(chunk).unwrap();
                extended.extend_from_slice(&writer.finish().unwrap());
            }
            let path = f.write(&format!("empty-prefix-{split}.{kind}"), &extended);
            check(&[&path], 0, &[]);
        }
    }
    let mut encoder = flate2::GzBuilder::new()
        .filename(vec![b'a'; 5000])
        .write(Vec::new(), flate2::Compression::default());
    encoder.write_all(b"@SQ\tSN:chr1\tLN:1000\n").unwrap();
    let path = f.write("long-gzip-name.sam.gz", &encoder.finish().unwrap());
    check(&[&path], 0, &[]);
}

#[test]
fn gzip_block_boundaries_validate_present_trailers_without_reading_later_members() {
    fn gzip(bytes: &[u8]) -> Vec<u8> {
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(bytes).unwrap();
        encoder.finish().unwrap()
    }
    let f = Fixture::new();
    let header = b"@SQ\tSN:chr1\tLN:1000\n";
    let record = b"q\t0\tchr1\t1\t60\t1M\t*\t0\t0\tA\tI\n";
    for size in [65535, 65536, 65537, 131072] {
        let mut payload = header.to_vec();
        payload.extend_from_slice(b"@CO\t");
        payload.resize(size - record.len() - 1, b'a');
        payload.push(b'\n');
        payload.extend_from_slice(record);
        let encoded = gzip(&payload);
        check(&[&f.write(&format!("valid-{size}.gz"), &encoded)], 0, &[]);
        for field in [8, 4] {
            let mut invalid = encoded.clone();
            let offset = invalid.len() - field;
            invalid[offset] ^= 1;
            let path = f.write(&format!("invalid-{size}-{field}.gz"), &invalid);
            if size == 65537 {
                // The header ends in the first decoded block. Its later
                // record data and trailer are outside this quick check.
                check(&[&path], 0, &[]);
            } else {
                check(
                    &[&path],
                    40,
                    &[
                        (&path, "caused an error whilst reading its header."),
                        (&path, "did not close cleanly."),
                    ],
                );
            }
        }
        for tail in [1, 4, 8, 9] {
            let path = f.write(
                &format!("truncated-{size}-{tail}.gz"),
                &encoded[..encoded.len() - tail],
            );
            if size == 65535 {
                check(
                    &[&path],
                    40,
                    &[
                        (&path, "caused an error whilst reading its header."),
                        (&path, "did not close cleanly."),
                    ],
                );
            } else {
                check(&[&path], 0, &[]);
            }
        }
        if size == 65536 {
            let mut later = gzip(b"later record data");
            let crc = later.len() - 8;
            later[crc] ^= 1;
            let mut joined = encoded.clone();
            joined.extend_from_slice(&later);
            check(&[&f.write("bad-later-member.gz", &joined)], 0, &[]);
            let mut joined = encoded.clone();
            joined.extend_from_slice(b"invalid next member");
            check(&[&f.write("bad-later-header.gz", &joined)], 0, &[]);
        }
    }
}

#[test]
fn stdin_redirected_from_a_file_retains_the_eof_check() {
    let f = Fixture::new();
    let bytes = bam(true);
    let path = f.write("no-eof.bam", &bytes[..bytes.len() - 28]);
    let output = Command::new(env!("CARGO_BIN_EXE_rust-bam"))
        .args(["quickcheck", "-"])
        .stdin(fs::File::open(path).unwrap())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(16));
    assert!(output.stdout.is_empty());
    assert_eq!(
        output.stderr,
        b"- was missing EOF block when one should be present.\n"
    );
}
