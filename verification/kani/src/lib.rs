// `#[kani::proof]` entry points are not called by a normal Rust binary, so
// rustc would otherwise flag them as dead code.
#![allow(dead_code)]

use std::collections::BTreeMap;

use bioinfo_fastx::chunker::last_complete_record_end;
use noodles::sam::alignment::record::Flags;
use rust_bam::sort_order::SortKey;
use rust_fx::split::parts_for_size;
use rust_fx::table::{n50, n50_from_histogram};

#[kani::proof]
fn chunker_boundary_is_within_input() {
    let data: [u8; 12] = kani::any();
    if let Some(end) = last_complete_record_end(&data) {
        assert!(end <= data.len());
    }
}

#[kani::proof]
fn parts_for_size_is_bounded() {
    let total: u64 = kani::any();
    let size: u64 = kani::any();
    kani::assume(total <= 100);
    kani::assume(size >= 1 && size <= 100);
    let parts = parts_for_size(total, size);
    assert!(parts <= total);
}

fn small_key(flags: u16, reference: usize, start: usize, name: &[u8], index: usize) -> SortKey {
    SortKey::new(
        Flags::from_bits_truncate(flags),
        Some(reference),
        Some(start),
        name,
        index,
    )
}

#[kani::proof]
fn sort_key_is_antisymmetric_on_small_keys() {
    let flags_a: u16 = kani::any();
    let flags_b: u16 = kani::any();
    kani::assume(matches!(flags_a, 0 | 4 | 16 | 20));
    kani::assume(matches!(flags_b, 0 | 4 | 16 | 20));
    let reference_a: u8 = kani::any();
    let reference_b: u8 = kani::any();
    kani::assume(reference_a <= 2);
    kani::assume(reference_b <= 2);
    let start_a: u8 = kani::any();
    let start_b: u8 = kani::any();
    kani::assume(start_a <= 10);
    kani::assume(start_b <= 10);
    let name_a: [u8; 2] = kani::any();
    let name_b: [u8; 2] = kani::any();
    let index_a: u8 = kani::any();
    let index_b: u8 = kani::any();
    kani::assume(index_a <= 5);
    kani::assume(index_b <= 5);
    let a = small_key(
        flags_a,
        reference_a as usize,
        start_a as usize,
        &name_a,
        index_a as usize,
    );
    let b = small_key(
        flags_b,
        reference_b as usize,
        start_b as usize,
        &name_b,
        index_b as usize,
    );
    assert_eq!(a.cmp(&b), b.cmp(&a).reverse());
}

#[kani::proof]
fn n50_is_bounded_by_the_largest_length() {
    let lengths: [u64; 4] = kani::any();
    kani::assume(lengths.iter().all(|length| *length <= 100));
    let mut values = lengths.to_vec();
    let (n50_value, l50) = n50(&mut values);
    assert!(n50_value <= 100);
    assert!(l50 <= 4);
}

#[kani::proof]
fn empty_histogram_is_zero() {
    let histogram: BTreeMap<u64, u64> = BTreeMap::new();
    assert_eq!(n50_from_histogram(&histogram), (0, 0));
}
