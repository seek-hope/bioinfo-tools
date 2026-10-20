//! Exact, length-preserving UMI representation.
//!
//! UMI-tools compares UMIs as raw bytes with a Hamming distance and requires
//! every UMI inside one bundle to have the same length. This module therefore
//! keeps the true length and never silently normalises bytes:
//!
//! * UMIs built only from `A/C/G/N/T` (upper case) up to 21 symbols are packed
//!   into three bits per symbol. Symbol values follow byte order
//!   (A<C<G<N<T) so packed order equals byte order for equal lengths.
//! * Any other byte (lower case, ambiguity codes, ...) keeps a byte string so
//!   the original comparison semantics are preserved.

use std::cmp::Ordering;

pub const MAX_PACKED_LEN: usize = 21;
pub const SYMBOL_ALPHABET: [u8; 5] = *b"ACGNT";

/// Map a byte to its packed symbol value, ordered by byte value.
#[inline]
pub fn symbol_value(byte: u8) -> Option<u8> {
    match byte {
        b'A' => Some(0),
        b'C' => Some(1),
        b'G' => Some(2),
        b'N' => Some(3),
        b'T' => Some(4),
        _ => None,
    }
}

#[inline]
pub fn symbol_byte(value: u8) -> u8 {
    SYMBOL_ALPHABET[(value as usize).min(4)]
}

/// A UMI as extracted from a read name or tag.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum Umi {
    Packed { code: u64, len: u8 },
    Bytes(Box<[u8]>),
}

impl Umi {
    /// Build a UMI without changing any byte.
    pub fn from_bytes(bytes: &[u8]) -> Umi {
        if bytes.is_empty() || bytes.len() > MAX_PACKED_LEN {
            return Umi::Bytes(bytes.into());
        }
        let mut code = 0u64;
        for &b in bytes {
            match symbol_value(b) {
                Some(v) => code = (code << 3) | u64::from(v),
                None => return Umi::Bytes(bytes.into()),
            }
        }
        Umi::Packed {
            code,
            len: bytes.len() as u8,
        }
    }

    pub fn len(&self) -> usize {
        match self {
            Umi::Packed { len, .. } => usize::from(*len),
            Umi::Bytes(b) => b.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn is_packed(&self) -> bool {
        matches!(self, Umi::Packed { .. })
    }

    #[inline]
    pub fn symbol(&self, index: usize) -> Option<u8> {
        match self {
            Umi::Packed { code, len } => {
                if index >= usize::from(*len) {
                    return None;
                }
                let shift = 3 * (usize::from(*len) - 1 - index);
                Some(((*code >> shift) & 0b111) as u8)
            }
            Umi::Bytes(b) => b.get(index).and_then(|b| symbol_value(*b)),
        }
    }

    /// Copy of this UMI with one symbol replaced, used by the k=1 neighbour search.
    pub fn with_symbol(&self, index: usize, value: u8) -> Option<Umi> {
        match self {
            Umi::Packed { code, len } => {
                let len = usize::from(*len);
                if index >= len {
                    return None;
                }
                let shift = 3 * (len - 1 - index);
                let mask = 0b111u64 << shift;
                let code = (*code & !mask) | (u64::from(value & 0b111) << shift);
                Some(Umi::Packed {
                    code,
                    len: len as u8,
                })
            }
            Umi::Bytes(b) => {
                let mut bytes = b.to_vec();
                let slot = bytes.get_mut(index)?;
                *slot = symbol_byte(value);
                Some(Umi::from_bytes(&bytes))
            }
        }
    }

    /// Packed three-bit code, when the UMI is in the packed representation.
    /// Callers can then enumerate neighbours with integer operations instead of
    /// building a new `Umi` for every candidate.
    pub fn packed_code(&self) -> Option<u64> {
        match self {
            Umi::Packed { code, .. } => Some(*code),
            Umi::Bytes(_) => None,
        }
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        match self {
            Umi::Packed { code, len } => {
                let len = usize::from(*len);
                (0..len)
                    .map(|i| {
                        let shift = 3 * (len - 1 - i);
                        symbol_byte(((*code >> shift) & 0b111) as u8)
                    })
                    .collect()
            }
            Umi::Bytes(b) => b.to_vec(),
        }
    }

    /// Lossless rendering for diagnostics (never used for comparison).
    pub fn display(&self) -> String {
        String::from_utf8_lossy(&self.to_bytes()).into_owned()
    }
}

impl Ord for Umi {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self, other) {
            (Umi::Packed { code: a, len: la }, Umi::Packed { code: b, len: lb }) => {
                // Compare symbol by symbol first so the order matches Python
                // bytes comparison even when lengths differ.
                let shared = usize::from(*la).min(usize::from(*lb));
                for index in 0..shared {
                    let sa = (a >> (3 * (usize::from(*la) - 1 - index))) & 0b111;
                    let sb = (b >> (3 * (usize::from(*lb) - 1 - index))) & 0b111;
                    match sa.cmp(&sb) {
                        Ordering::Equal => {}
                        other => return other,
                    }
                }
                la.cmp(lb)
            }
            (Umi::Bytes(a), Umi::Bytes(b)) => a.cmp(b),
            // Mixed representations are rare; compare materialised bytes so the
            // order still follows byte order.
            _ => self.to_bytes().cmp(&other.to_bytes()),
        }
    }
}

impl PartialOrd for Umi {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packs_acgtn_and_roundtrips() {
        for raw in ["A", "ACGT", "NNNN", "ACGTNACGTNACGTNACGTNA"] {
            let umi = Umi::from_bytes(raw.as_bytes());
            assert!(umi.is_packed(), "{raw} should pack");
            assert_eq!(umi.display(), raw);
            assert_eq!(umi.len(), raw.len());
        }
    }

    #[test]
    fn non_canonical_bytes_fall_back_without_normalisation() {
        let lower = Umi::from_bytes(b"acgt");
        assert!(!lower.is_packed());
        assert_eq!(lower.display(), "acgt");
        let upper = Umi::from_bytes(b"ACGT");
        assert_eq!(crate::core::distance::hamming(&lower, &upper), Some(4));
    }

    #[test]
    fn long_umi_falls_back_to_bytes() {
        let long = "A".repeat(MAX_PACKED_LEN + 1);
        let umi = Umi::from_bytes(long.as_bytes());
        assert!(!umi.is_packed());
        assert_eq!(umi.len(), MAX_PACKED_LEN + 1);
    }

    #[test]
    fn packed_order_matches_byte_order() {
        let mut raws = ["A", "C", "G", "N", "T", "AA", "AC", "CA", "NG", "TT"];
        let mut umis: Vec<Umi> = raws.iter().map(|r| Umi::from_bytes(r.as_bytes())).collect();
        umis.sort();
        raws.sort();
        let got: Vec<String> = umis.iter().map(|u| u.display()).collect();
        assert_eq!(got, raws.to_vec());
    }

    #[test]
    fn with_symbol_replaces_exactly_one_position() {
        let umi = Umi::from_bytes(b"ACGT");
        let changed = umi.with_symbol(0, 4).unwrap();
        assert_eq!(changed.display(), "TCGT");
        assert_eq!(crate::core::distance::hamming(&umi, &changed), Some(1));
        assert!(umi.with_symbol(4, 0).is_none());
    }
}
