//! Hamming distance between UMIs, matching `umi_tools._dedup_umi.edit_distance`.
//!
//! UMI-tools defines the distance only for equal-length sequences; different
//! lengths return infinity, i.e. no edge. This is a Hamming distance, not a
//! Levenshtein distance, so insertions and deletions are not considered.

use crate::core::umi::Umi;

/// `None` means "distance is infinite" (lengths differ).
pub fn hamming(a: &Umi, b: &Umi) -> Option<u32> {
    if a.len() != b.len() {
        return None;
    }
    match (a, b) {
        (Umi::Packed { code: ca, len }, Umi::Packed { code: cb, .. }) => {
            // Fold each 3-bit lane onto its low bit, then count lanes in one
            // popcount. Equivalent to the per-symbol loop but branch-free.
            let diff = ca ^ cb;
            if diff == 0 {
                return Some(0);
            }
            let folded = diff | (diff >> 1) | (diff >> 2);
            let lanes = u64::from(*len) as u32;
            // Bits 0, 3, 6, ... selected by (2^(3n) - 1) / 7.
            let mask = (1u64 << (3 * lanes)).wrapping_sub(1) / 7;
            Some((folded & mask).count_ones())
        }
        (Umi::Bytes(x), Umi::Bytes(y)) => {
            Some(x.iter().zip(y.iter()).filter(|(p, q)| p != q).count() as u32)
        }
        _ => {
            let (x, y) = (a.to_bytes(), b.to_bytes());
            Some(x.iter().zip(y.iter()).filter(|(p, q)| p != q).count() as u32)
        }
    }
}

/// True when the distance is at most `threshold` (false when lengths differ).
pub fn within(a: &Umi, b: &Umi, threshold: u32) -> bool {
    matches!(hamming(a, b), Some(d) if d <= threshold)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::umi::MAX_PACKED_LEN;

    fn u(raw: &str) -> Umi {
        Umi::from_bytes(raw.as_bytes())
    }

    #[test]
    fn packed_distance_matches_per_lane_count() {
        let alphabet = *b"ACGNT";
        let mut state = 0x9e37_79b9_7f4a_7c15u64;
        for len in 1..=MAX_PACKED_LEN {
            for _ in 0..40 {
                let mut left = Vec::with_capacity(len);
                let mut right = Vec::with_capacity(len);
                let mut expected = 0u32;
                for _ in 0..len {
                    state ^= state << 13;
                    state ^= state >> 7;
                    state ^= state << 17;
                    let a = alphabet[(state % 5) as usize];
                    state ^= state << 13;
                    state ^= state >> 7;
                    state ^= state << 17;
                    let b = alphabet[(state % 5) as usize];
                    if a != b {
                        expected += 1;
                    }
                    left.push(a);
                    right.push(b);
                }
                assert_eq!(
                    hamming(&Umi::from_bytes(&left), &Umi::from_bytes(&right)),
                    Some(expected),
                    "len {len}"
                );
            }
        }
    }

    #[test]
    fn counts_symbol_substitutions() {
        assert_eq!(hamming(&u("ACGT"), &u("ACGT")), Some(0));
        assert_eq!(hamming(&u("ACGT"), &u("ACGA")), Some(1));
        assert_eq!(hamming(&u("ACGT"), &u("TGCA")), Some(4));
    }

    #[test]
    fn n_is_an_ordinary_symbol() {
        assert_eq!(hamming(&u("ACGT"), &u("ACNT")), Some(1));
    }

    #[test]
    fn different_lengths_have_no_distance() {
        assert_eq!(hamming(&u("A"), &u("AA")), None);
        assert!(!within(&u("A"), &u("AA"), 1));
    }

    #[test]
    fn packed_and_byte_paths_agree() {
        let samples = ["A", "ACGT", "NNNN", "ACGTNACGTN", "TTTT", "acgt"];
        for a in samples {
            for b in samples {
                let expected: u32 = a.bytes().zip(b.bytes()).filter(|(x, y)| x != y).count() as u32;
                let got = hamming(&u(a), &u(b));
                if a.len() == b.len() {
                    assert_eq!(got, Some(expected), "{a} vs {b}");
                } else {
                    assert_eq!(got, None, "{a} vs {b}");
                }
            }
        }
    }
}
