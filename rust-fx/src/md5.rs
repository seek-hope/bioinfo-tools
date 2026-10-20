//! MD5, needed by `fx2tab --seq-hash`.
//!
//! # Why this is implemented here rather than added as a dependency
//!
//! `fx2tab -s` prints "hash (MD5) of sequence". A checksum crate would be the
//! obvious reach, but `AGENTS.md` and the plan's dependency policy both require a
//! recorded reason before a new runtime dependency is added, and this one buys a
//! single, fully-specified algorithm of about 120 lines.
//!
//! It is pinned against the RFC 1321 test vectors and against the `md5sum`
//! binary when one is available, so the implementation is checked rather than
//! trusted. See DR-2 in `docs/EVIDENCE-W1-3.md`.

/// MD5 digest size in bytes.
pub const DIGEST_LEN: usize = 16;

const S: [u32; 64] = [
    7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9,
    14, 20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10, 15,
    21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
];

/// `K[i] = floor(2^32 × abs(sin(i + 1)))`, per RFC 1321.
const K: [u32; 64] = [
    0xd76aa478, 0xe8c7b756, 0x242070db, 0xc1bdceee, 0xf57c0faf, 0x4787c62a, 0xa8304613, 0xfd469501,
    0x698098d8, 0x8b44f7af, 0xffff5bb1, 0x895cd7be, 0x6b901122, 0xfd987193, 0xa679438e, 0x49b40821,
    0xf61e2562, 0xc040b340, 0x265e5a51, 0xe9b6c7aa, 0xd62f105d, 0x02441453, 0xd8a1e681, 0xe7d3fbc8,
    0x21e1cde6, 0xc33707d6, 0xf4d50d87, 0x455a14ed, 0xa9e3e905, 0xfcefa3f8, 0x676f02d9, 0x8d2a4c8a,
    0xfffa3942, 0x8771f681, 0x6d9d6122, 0xfde5380c, 0xa4beea44, 0x4bdecfa9, 0xf6bb4b60, 0xbebfbc70,
    0x289b7ec6, 0xeaa127fa, 0xd4ef3085, 0x04881d05, 0xd9d4d039, 0xe6db99e5, 0x1fa27cf8, 0xc4ac5665,
    0xf4292244, 0x432aff97, 0xab9423a7, 0xfc93a039, 0x655b59c3, 0x8f0ccc92, 0xffeff47d, 0x85845dd1,
    0x6fa87e4f, 0xfe2ce6e0, 0xa3014314, 0x4e0811a1, 0xf7537e82, 0xbd3af235, 0x2ad7d2bb, 0xeb86d391,
];

/// A streaming MD5 hasher.
#[derive(Clone)]
pub struct Md5 {
    state: [u32; 4],
    /// Bytes not yet folded into a 64-byte block.
    buffer: [u8; 64],
    buffered: usize,
    /// Total message length in bytes, for the length padding.
    total: u64,
}

impl Default for Md5 {
    fn default() -> Self {
        Md5::new()
    }
}

impl Md5 {
    /// A fresh hasher in the RFC 1321 initial state.
    pub fn new() -> Md5 {
        Md5 {
            state: [0x67452301, 0xefcdab89, 0x98badcfe, 0x10325476],
            buffer: [0u8; 64],
            buffered: 0,
            total: 0,
        }
    }

    /// Feed bytes into the hash.
    pub fn update(&mut self, mut data: &[u8]) {
        self.total += data.len() as u64;
        if self.buffered > 0 {
            let need = 64 - self.buffered;
            let take = need.min(data.len());
            self.buffer[self.buffered..self.buffered + take].copy_from_slice(&data[..take]);
            self.buffered += take;
            data = &data[take..];
            if self.buffered == 64 {
                let block = self.buffer;
                self.compress(&block);
                self.buffered = 0;
            }
        }
        while data.len() >= 64 {
            let (block, rest) = data.split_at(64);
            self.compress(block);
            data = rest;
        }
        if !data.is_empty() {
            self.buffer[..data.len()].copy_from_slice(data);
            self.buffered = data.len();
        }
    }

    /// Finish the hash and return the 16-byte digest.
    pub fn finish(mut self) -> [u8; DIGEST_LEN] {
        // Append 0x80, then zeros, then the 64-bit little-endian bit length.
        let bit_len = self.total.wrapping_mul(8);
        let mut tail = Vec::with_capacity(72);
        tail.push(0x80u8);
        let pad = if self.buffered < 56 {
            56 - self.buffered
        } else {
            64 + 56 - self.buffered
        };
        tail.extend(std::iter::repeat_n(0u8, pad - 1));
        tail.extend_from_slice(&bit_len.to_le_bytes());
        // `update` would re-count these bytes, so feed them directly.
        let saved_total = self.total;
        self.update(&tail);
        self.total = saved_total;
        debug_assert_eq!(self.buffered, 0);

        let mut out = [0u8; DIGEST_LEN];
        for (i, word) in self.state.iter().enumerate() {
            out[i * 4..i * 4 + 4].copy_from_slice(&word.to_le_bytes());
        }
        out
    }

    fn compress(&mut self, block: &[u8]) {
        debug_assert_eq!(block.len(), 64);
        let mut m = [0u32; 16];
        for (i, chunk) in block.chunks_exact(4).enumerate() {
            m[i] = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        }
        let [mut a, mut b, mut c, mut d] = self.state;
        for i in 0..64 {
            let (f, g) = match i {
                0..=15 => ((b & c) | (!b & d), i),
                16..=31 => ((d & b) | (!d & c), (5 * i + 1) % 16),
                32..=47 => (b ^ c ^ d, (3 * i + 5) % 16),
                _ => (c ^ (b | !d), (7 * i) % 16),
            };
            let tmp = d;
            d = c;
            c = b;
            let sum = a.wrapping_add(f).wrapping_add(K[i]).wrapping_add(m[g]);
            b = b.wrapping_add(sum.rotate_left(S[i]));
            a = tmp;
        }
        self.state[0] = self.state[0].wrapping_add(a);
        self.state[1] = self.state[1].wrapping_add(b);
        self.state[2] = self.state[2].wrapping_add(c);
        self.state[3] = self.state[3].wrapping_add(d);
    }
}

/// MD5 of a byte slice, as lowercase hex.
pub fn hex_digest(data: &[u8]) -> String {
    let mut hasher = Md5::new();
    hasher.update(data);
    to_hex(&hasher.finish())
}

/// Render a digest as lowercase hex.
pub fn to_hex(digest: &[u8; DIGEST_LEN]) -> String {
    let mut out = String::with_capacity(32);
    for byte in digest {
        out.push(char::from_digit((byte >> 4) as u32, 16).unwrap());
        out.push(char::from_digit((byte & 0xf) as u32, 16).unwrap());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // RFC 1321, appendix A.5: the seven published test vectors.
    #[test]
    fn rfc1321_test_vectors() {
        for (input, expected) in [
            ("", "d41d8cd98f00b204e9800998ecf8427e"),
            ("a", "0cc175b9c0f1b6a831c399e269772661"),
            ("abc", "900150983cd24fb0d6963f7d28e17f72"),
            ("message digest", "f96b697d7cb7938d525a2f31aaf161d0"),
            (
                "abcdefghijklmnopqrstuvwxyz",
                "c3fcd3d76192e4007dfb496cca67e13b",
            ),
            (
                "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789",
                "d174ab98d277d9f5a5611c2c9f419d9f",
            ),
            (
                "12345678901234567890123456789012345678901234567890123456789012345678901234567890",
                "57edf4a22be3c955ac49da2e2107b67a",
            ),
        ] {
            assert_eq!(hex_digest(input.as_bytes()), expected, "input {input:?}");
        }
    }

    #[test]
    fn block_boundary_lengths_are_padded_correctly() {
        // 55/56/63/64/65 bytes are the cases where the length padding either
        // fits in the current block or spills into an extra one.
        for len in [54usize, 55, 56, 57, 63, 64, 65, 127, 128, 129] {
            let data = vec![b'A'; len];
            let hex = hex_digest(&data);
            assert_eq!(hex.len(), 32, "len {len}");
            // A different length must give a different digest.
            if len > 0 {
                let other = hex_digest(&data[..len - 1]);
                assert_ne!(hex, other, "len {len} collides with {}", len - 1);
            }
        }
    }

    #[test]
    fn streaming_matches_one_shot() {
        let data: Vec<u8> = (0..1000u32).map(|i| (i % 251) as u8).collect();
        let one_shot = hex_digest(&data);
        for chunk in [1usize, 7, 63, 64, 65, 100, 999] {
            let mut hasher = Md5::new();
            for part in data.chunks(chunk) {
                hasher.update(part);
            }
            assert_eq!(to_hex(&hasher.finish()), one_shot, "chunk size {chunk}");
        }
    }

    #[test]
    fn update_with_empty_slices_is_a_no_op() {
        let mut hasher = Md5::new();
        hasher.update(b"");
        hasher.update(b"abc");
        hasher.update(b"");
        assert_eq!(to_hex(&hasher.finish()), hex_digest(b"abc"));
    }

    #[test]
    fn default_matches_new() {
        let mut a = Md5::default();
        a.update(b"abc");
        let mut b = Md5::new();
        b.update(b"abc");
        assert_eq!(a.finish(), b.finish());
    }

    // A sequence hash must distinguish case when asked to, which is why
    // `fx2tab -s` has `-I/--case-sensitive`.
    #[test]
    fn case_changes_the_digest() {
        assert_ne!(hex_digest(b"ACGT"), hex_digest(b"acgt"));
    }

    #[test]
    fn digest_of_a_realistic_sequence_is_well_formed() {
        let hex = hex_digest(b"AGATCGGAAGAGCACACGTCTGAACTCCAGTCAC");
        assert_eq!(hex.len(), 32);
        assert!(hex
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
    }
}
