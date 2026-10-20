//! Differential test: the k-mer prefilter must never discard a read that the
//! full dynamic program would trim, i.e. it is a strict superset of the
//! alignment decision.

use rust_cutadapt::align::{Aligner, BACK, DEFAULT_MAX_ERROR_RATE};
use rust_cutadapt::trim::{Adapter, Trim, NO_INDEL_COST};

fn full_dp_trim(adapter: &mut Adapter, read: &[u8]) -> Trim {
    let mut aligner = Aligner::new(
        adapter.sequence(),
        DEFAULT_MAX_ERROR_RATE,
        BACK,
        NO_INDEL_COST,
        adapter.min_overlap(),
    );
    match aligner.locate(read) {
        Some(a) => Trim::CutAt(a.query_start),
        None => Trim::Unchanged,
    }
}

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

#[test]
fn prefilter_never_changes_the_alignment_decision() {
    let adapters: [&[u8]; 6] = [
        b"AGATCGGAAGAGC",
        b"ACGT",
        b"TGTAATCGTTGCA",
        b"AC",
        b"T",
        b"AGATCGGAAGAGCGTCGTGTAGGGAAAGAGTGT",
    ];
    let mut rng = Rng(0x243F_6A88_85A3_08D3);
    let alphabet = b"ACGTACGTACGTACGTN";
    let mut checked = 0u64;
    let mut mismatches = 0u64;
    for adapter_seq in adapters {
        let mut fast = Adapter::new(adapter_seq);
        for trial in 0..60_000u64 {
            let len = rng.below(120);
            let mut read = Vec::with_capacity(len + 16);
            for _ in 0..len {
                read.push(alphabet[rng.below(alphabet.len())]);
            }
            if trial % 2 == 0 {
                // Splice in a (possibly mutated, possibly truncated) adapter.
                let mut ad = adapter_seq.to_vec();
                let mutations = rng.below(4);
                for _ in 0..mutations {
                    let p = rng.below(ad.len());
                    ad[p] = b"ACGT"[rng.below(4)];
                }
                let take = rng.below(ad.len() + 1);
                let pos = rng.below(read.len() + 1);
                read.splice(pos..pos, ad[..take].iter().copied());
            }
            if trial % 7 == 0 {
                read.make_ascii_lowercase();
            }
            let fast_result = fast.trim_position(&read);
            let full_result = full_dp_trim(&mut fast, &read);
            checked += 1;
            if fast_result != full_result {
                mismatches += 1;
                if mismatches <= 10 {
                    println!(
                        "MISMATCH adapter={} read={} prefilter={:?} full={:?}",
                        String::from_utf8_lossy(adapter_seq),
                        String::from_utf8_lossy(&read),
                        fast_result,
                        full_result
                    );
                }
            }
        }
    }
    println!("checked {checked} reads, {mismatches} mismatches");
    assert_eq!(mismatches, 0, "prefilter is not a superset of the aligner");
}
