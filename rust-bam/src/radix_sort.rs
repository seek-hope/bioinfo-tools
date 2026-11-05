//! Coordinate radix passes followed by name/index sorting within equal positions.
//!
//! For n records and position groups of sizes g, work is O(n + sum(g log g))
//! for fixed-width coordinates; the worst case remains O(n log n). Extra scratch
//! space is at most five usize values per record beyond the returned indices.

use crate::sort_order::SortKey;

fn position(key: &SortKey) -> [usize; 4] {
    [
        usize::from(key.unmapped_rank),
        key.reference_id,
        key.start,
        usize::from(key.reverse_rank),
    ]
}

fn sort_names<T>(order: &mut [usize], records: &[(SortKey, T)]) {
    order.sort_unstable_by(|&a, &b| {
        records[a]
            .0
            .name
            .cmp(&records[b].0.name)
            .then_with(|| records[a].0.input_index.cmp(&records[b].0.input_index))
    });
}

pub(crate) fn indices<T>(records: &[(SortKey, T)]) -> Vec<usize> {
    let mut order: Vec<_> = (0..records.len()).collect();
    if records.len() < 512 {
        order.sort_unstable_by(|&a, &b| records[a].0.cmp(&records[b].0));
        return order;
    }

    // Keep comparison sort's linear behavior on already ordered inputs. Reverse
    // only a strictly descending sequence so equal keys cannot be reversed.
    let mut ascending = true;
    let mut descending = true;
    for pair in records.windows(2) {
        let cmp = pair[0].0.cmp(&pair[1].0);
        ascending &= cmp.is_le();
        descending &= cmp.is_gt();
        if !ascending && !descending {
            break;
        }
    }
    if ascending {
        return order;
    }
    if descending {
        order.reverse();
        return order;
    }

    let first = position(&records[0].0);
    let mut varying = [0; 4];
    let positions: Vec<_> = records
        .iter()
        .map(|(key, _)| {
            let value = position(key);
            for field in 0..4 {
                varying[field] |= value[field] ^ first[field];
            }
            value
        })
        .collect();
    if varying == [0; 4] {
        drop(positions);
        sort_names(&mut order, records);
        return order;
    }

    let mut scratch = vec![0; records.len()];
    // Stable least-significant-byte passes, from the last tuple field to the
    // first. Uniform bytes need no pass. Full usize widths retain extreme SAM
    // coordinates and reference IDs without packing assumptions or truncation.
    for field in (0..4).rev() {
        for shift in (0..usize::BITS).step_by(8) {
            if (varying[field] >> shift) & 0xff == 0 {
                continue;
            }
            let mut counts = [0usize; 256];
            for &index in &order {
                counts[(positions[index][field] >> shift) & 0xff] += 1;
            }
            let mut offset = 0;
            for count in &mut counts {
                let size = *count;
                *count = offset;
                offset += size;
            }
            for &index in &order {
                let byte = (positions[index][field] >> shift) & 0xff;
                scratch[counts[byte]] = index;
                counts[byte] += 1;
            }
            std::mem::swap(&mut order, &mut scratch);
        }
    }

    let mut start = 0;
    while start < order.len() {
        let mut end = start + 1;
        while end < order.len() && positions[order[start]] == positions[order[end]] {
            end += 1;
        }
        sort_names(&mut order[start..end], records);
        start = end;
    }
    order
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(records: &[(SortKey, ())]) {
        let mut expected: Vec<_> = (0..records.len()).collect();
        expected.sort_unstable_by(|&a, &b| records[a].0.cmp(&records[b].0));
        assert_eq!(indices(records), expected);
    }

    #[test]
    fn matches_full_comparison_at_size_coordinate_and_name_boundaries() {
        for length in [0, 1, 31, 511, 512, 513, 4096] {
            for mode in 0..6 {
                let mut state = 0x53c4_a18d_42b7_0931u64;
                let mut next = || {
                    state ^= state << 13;
                    state ^= state >> 7;
                    state ^= state << 17;
                    state as usize
                };
                let mut records = Vec::new();
                for input_index in 0..length {
                    let value = next();
                    let name = match value % 5 {
                        0 => Vec::new(),
                        1 => b"same".to_vec(),
                        2 => b"same-prefix".to_vec(),
                        3 => vec![0, 255, (value >> 8) as u8],
                        _ => [vec![b'x'; 128], value.to_le_bytes().to_vec()].concat(),
                    };
                    let (unmapped_rank, reference_id, start, reverse_rank) = match mode {
                        0 => (0, value % 4, next() % 1024, (value & 1) as u8),
                        1 => (1, 0, 0, 0),
                        2 => (0, value, next(), 0),
                        3 => (0, usize::MAX, usize::MAX - value % 3, (value & 1) as u8),
                        4 => (0, 0, 1, (value & 1) as u8),
                        _ => ((value >> 9) as u8, next(), next(), value as u8),
                    };
                    records.push((
                        SortKey {
                            unmapped_rank,
                            reference_id,
                            start,
                            reverse_rank,
                            name,
                            input_index,
                        },
                        (),
                    ));
                }
                check(&records);
                records.sort_unstable_by(|a, b| a.0.cmp(&b.0));
                check(&records);
                records.reverse();
                check(&records);
            }
        }
    }

    #[test]
    fn identical_names_retain_input_index_order_across_strands_and_positions() {
        let records: Vec<_> = (0..4096)
            .rev()
            .map(|index| {
                (
                    SortKey {
                        unmapped_rank: 0,
                        reference_id: index % 3,
                        start: index % 11,
                        reverse_rank: (index % 2) as u8,
                        name: b"duplicate".to_vec(),
                        input_index: index,
                    },
                    (),
                )
            })
            .collect();
        check(&records);
    }
}
