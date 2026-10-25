use std::collections::BTreeMap;
use std::io::Cursor;

use proptest::prelude::*;

use bioinfo_fastx::chunker::for_each_chunk;
use bioinfo_fastx::{Mode, Parser};
use noodles::sam::alignment::record::Flags;
use rust_bam::sort_order::SortKey;
use rust_cutadapt::engine::AdapterChunkStats;
use rust_cutadapt::pipeline::AdapterTotals;
use rust_fastqc::limits::{check_read, QcLimits};
use rust_fx::commands::PartPlan;
use rust_fx::split::{parts_for_size, SplitBy};
use rust_fx::table::{n50, n50_from_histogram, quartiles, quartiles_from_histogram};

fn fastq_record(name: &str, length: usize) -> String {
    format!(
        "@{name}\n{}\n+\n{}\n",
        "A".repeat(length),
        "I".repeat(length)
    )
}

proptest! {
    #[test]
    fn chunker_reassembles_to_the_exact_input(lengths in prop::collection::vec(0usize..40, 0..80)) {
        let data: String = lengths
            .iter()
            .enumerate()
            .map(|(index, length)| fastq_record(&format!("r{index}"), *length))
            .collect();
        let mut chunks = Vec::new();
        for_each_chunk::<_, _, std::io::Error>(
            &mut Cursor::new(data.clone().into_bytes()),
            4096,
            |chunk, _, _| {
                chunks.push(chunk);
                Ok(())
            },
        )
        .unwrap();
        let joined: Vec<u8> = chunks.into_iter().flatten().collect();
        prop_assert_eq!(joined, data.into_bytes());
    }

    #[test]
    fn parser_reads_generated_records(lengths in prop::collection::vec(0usize..100, 0..40)) {
        let data: String = lengths
            .iter()
            .enumerate()
            .map(|(index, length)| fastq_record(&format!("r{index}"), *length))
            .collect();
        let mut parser = Parser::new(data.as_bytes(), 0, 1).with_mode(Mode::Strict);
        let mut seen = 0usize;
        while let Some(record) = parser.next_record().unwrap() {
            prop_assert_eq!(record.sequence.len(), record.qualities.len());
            seen += 1;
        }
        prop_assert_eq!(seen, lengths.len());
    }

    #[test]
    fn histogram_statistics_match_the_sorted_path(lengths in prop::collection::vec(0u64..500, 0..80)) {
        let mut sorted = lengths.clone();
        let expected_n50 = n50(&mut sorted);
        let mut sorted_quartiles = lengths.clone();
        let expected_quartiles = quartiles(&mut sorted_quartiles);

        let mut histogram = BTreeMap::new();
        for length in &lengths {
            *histogram.entry(*length).or_insert(0u64) += 1;
        }
        prop_assert_eq!(n50_from_histogram(&histogram), expected_n50);
        prop_assert_eq!(quartiles_from_histogram(&histogram), expected_quartiles);
    }

    #[test]
    fn sort_keys_are_a_total_order(
        rows in prop::collection::vec(
            (0u16..32, prop::option::of(0usize..4), prop::option::of(0usize..100), "[ACGT]{0,8}", 0usize..20),
            1..20,
        )
    ) {
        let mut keys: Vec<SortKey> = rows
            .iter()
            .map(|(flags, reference, start, name, index)| {
                SortKey::new(
                    Flags::from_bits_truncate(*flags),
                    *reference,
                    *start,
                    name.as_bytes(),
                    *index,
                )
            })
            .collect();
        keys.sort();
        prop_assert!(keys.windows(2).all(|pair| pair[0] <= pair[1]));
    }
}

proptest! {
    #[test]
    fn part_plan_ranges_cover_the_total_once(
        total in 0u64..200,
        parts in 1u64..20,
    ) {
        let plan = PartPlan::new(total, SplitBy::Parts(parts)).unwrap();
        let mut covered = 0u64;
        for part in 0..plan.parts() {
            let (start, end) = plan.range(part);
            prop_assert_eq!(start, covered);
            prop_assert!(end >= start);
            covered = end;
        }
        prop_assert_eq!(covered, total);
    }

    #[test]
    fn qc_limits_reject_before_growth(
        length in 0usize..200,
        distinct in 0usize..100,
        prefix_is_new in any::<bool>(),
        max_length in 0u64..200,
        max_distinct in 0u64..100,
    ) {
        let limits = QcLimits {
            max_read_length: max_length,
            max_distinct_sequences: max_distinct,
        };
        let result = check_read(length, prefix_is_new, distinct, limits);
        let length_ok = (length as u64) <= max_length;
        let distinct_ok = !prefix_is_new || (distinct as u64) < max_distinct;
        prop_assert_eq!(result.is_ok(), length_ok && distinct_ok);
    }

    #[test]
    fn adapter_totals_preserve_multiset_counts(
        first_trimmed in prop::collection::vec((0usize..20, 0usize..3), 0..10),
        second_trimmed in prop::collection::vec((0usize..20, 0usize..3), 0..10),
        adjacent in prop::collection::vec(prop::option::of(0u8..4), 0..10),
    ) {
        let first = AdapterChunkStats {
            matches: 3,
            trimmed: first_trimmed.clone(),
            adjacent: adjacent.clone(),
            ..AdapterChunkStats::default()
        };
        let second = AdapterChunkStats {
            matches: 5,
            trimmed: second_trimmed.clone(),
            ..AdapterChunkStats::default()
        };
        let mut totals = AdapterTotals::default();
        totals.add(&first);
        totals.add(&second);
        let expanded = totals.to_chunk_stats();
        prop_assert_eq!(expanded.matches, 8);

        let mut expected_trimmed = first_trimmed;
        expected_trimmed.extend(second_trimmed);
        expected_trimmed.sort();
        let mut actual_trimmed = expanded.trimmed;
        actual_trimmed.sort();
        prop_assert_eq!(actual_trimmed, expected_trimmed);

        let mut expected_adjacent = adjacent;
        expected_adjacent.sort();
        let mut actual_adjacent = expanded.adjacent;
        actual_adjacent.sort();
        prop_assert_eq!(actual_adjacent, expected_adjacent);
    }
}

proptest! {
    /// Arbitrary bytes must produce `Ok` records, clean EOF, or an error; never a
    /// panic. Both strict and lenient modes are exercised.
    #[test]
    fn parser_never_panics_on_arbitrary_bytes(data in prop::collection::vec(any::<u8>(), 0..512)) {
        for mode in [Mode::Strict, Mode::Lenient] {
            let mut parser = Parser::new(data.as_slice(), 0, 1).with_mode(mode);
            loop {
                match parser.next_record() {
                    Ok(Some(record)) => {
                        prop_assert_eq!(record.sequence.len(), record.qualities.len());
                    }
                    Ok(None) | Err(_) => break,
                }
            }
        }
    }

    /// The size-based split plan must never claim more parts than records.
    #[test]
    fn parts_for_size_is_bounded(total in 0u64..200, size in 1u64..200) {
        prop_assert!(parts_for_size(total, size) <= total);
    }
}
