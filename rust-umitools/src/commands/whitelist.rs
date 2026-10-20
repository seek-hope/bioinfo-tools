//! `whitelist`: estimate the true cell barcodes from a FASTQ file.
//!
//! Implemented subset: barcode counting from a 5' pattern (`C` marks cell
//! bases), the distance knee (same iterative chord-distance algorithm as
//! UMI-tools), a documented Gaussian-density approximation, and an optional
//! Hamming correction map. `--method umis` and paired input are rejected.

use crate::cli::WhitelistArgs;
use crate::commands::{Logger, PROGRAM};
use crate::error::{Error, Result};
use crate::io::fastq::FastqReader;
use crate::io::spill::TempOutput;
use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;
use std::time::Instant;

fn extract_cell(sequence: &[u8], pattern: &str) -> Option<String> {
    if sequence.len() < pattern.len() {
        return None;
    }
    let mut cell = Vec::new();
    for (offset, expected) in pattern.bytes().enumerate() {
        if expected == b'C' || expected == b'c' {
            cell.push(sequence[offset]);
        }
    }
    if cell.is_empty() {
        None
    } else {
        Some(String::from_utf8_lossy(&cell).into_owned())
    }
}

fn max_distance_index(values: &[f64]) -> usize {
    let n = values.len();
    if n < 2 {
        return 0;
    }
    let (x0, y0) = (0.0f64, values[0]);
    let (x1, y1) = ((n - 1) as f64, values[n - 1]);
    let (dx, dy) = (x1 - x0, y1 - y0);
    let norm = (dx * dx + dy * dy).sqrt();
    if norm == 0.0 {
        return 0;
    }
    let (ux, uy) = (dx / norm, dy / norm);
    let mut best = (0usize, f64::MIN);
    for (index, value) in values.iter().enumerate() {
        let (vx, vy) = (index as f64 - x0, value - y0);
        let scalar = vx * ux + vy * uy;
        let (px, py) = (scalar * ux, scalar * uy);
        let (ex, ey) = (vx - px, vy - py);
        let distance = (ex * ex + ey * ey).sqrt();
        if distance > best.1 {
            best = (index, distance);
        }
    }
    best.0
}

/// Iterative chord-distance knee, mirroring `getKneeEstimateDistance`.
fn knee_distance(sorted_counts: &[u64]) -> usize {
    let mut cumulative = Vec::with_capacity(sorted_counts.len());
    let mut sum = 0u64;
    for count in sorted_counts {
        sum += count;
        cumulative.push(sum as f64);
    }
    let mut previous = 0usize;
    let mut index = max_distance_index(&cumulative);
    if index == 0 {
        return 1;
    }
    let mut iterations = 0;
    while index != previous {
        previous = index;
        iterations += 1;
        if iterations > 100 {
            break;
        }
        let end = (index * 3).min(cumulative.len());
        index = max_distance_index(&cumulative[..end]);
    }
    index + 1
}

/// Gaussian-density knee (documented approximation of the upstream KDE).
fn knee_density(sorted_counts: &[u64], expect_cells: Option<usize>) -> Option<usize> {
    let max = *sorted_counts.first()? as f64;
    let threshold = 0.001 * max;
    let logs: Vec<f64> = sorted_counts
        .iter()
        .map(|count| *count as f64)
        .filter(|count| *count > threshold)
        .map(|count| count.log10())
        .collect();
    if logs.len() < 3 {
        return None;
    }
    let mean = logs.iter().sum::<f64>() / logs.len() as f64;
    let variance = logs.iter().map(|value| (value - mean).powi(2)).sum::<f64>() / logs.len() as f64;
    let std = variance.sqrt().max(1e-6);
    let bandwidth = 0.1 * std;
    let (min, max) = (
        logs.iter().cloned().fold(f64::INFINITY, f64::min),
        logs.iter().cloned().fold(f64::NEG_INFINITY, f64::max),
    );
    if max - min < 1e-9 {
        return None;
    }
    let points = 1000usize;
    let mut grid = Vec::with_capacity(points);
    let mut density = Vec::with_capacity(points);
    for step in 0..points {
        let x = min + (max - min) * step as f64 / (points - 1) as f64;
        grid.push(x);
        let mut value = 0.0;
        for log in &logs {
            let z = (x - log) / bandwidth;
            value += (-0.5 * z * z).exp();
        }
        density.push(value / (logs.len() as f64 * bandwidth * (2.0 * std::f64::consts::PI).sqrt()));
    }
    let mut minima = Vec::new();
    for index in 1..points - 1 {
        if density[index] < density[index - 1] && density[index] < density[index + 1] {
            minima.push(index);
        }
    }
    for index in minima.into_iter().rev() {
        let passing = logs.iter().filter(|log| **log > grid[index]).count();
        if let Some(expected) = expect_cells {
            if passing > expected / 10 && passing <= expected {
                return Some(passing.max(1));
            }
        } else if index >= points / 5 && (max - grid[index] > 0.5 || grid[index] < max / 2.0) {
            return Some(passing.max(1));
        }
    }
    None
}

fn hamming(left: &str, right: &str) -> usize {
    left.bytes()
        .zip(right.bytes())
        .filter(|(a, b)| a != b)
        .count()
}

pub fn run(args: &WhitelistArgs) -> Result<()> {
    let started = Instant::now();
    let mut log = Logger::new(&args.log, args.quiet)?;
    if let Some(mode) = &args.ed_above_threshold {
        if mode != "discard" && mode != "correct" {
            return Err(Error::usage_with_hint(
                format!("unknown --ed-above-threshold '{mode}'"),
                "choose one of: discard, correct",
            ));
        }
    }
    if args.method != "reads" {
        return Err(Error::unsupported(
            "--method",
            format!(
                "'{}' is not implemented in this build; use reads",
                args.method
            ),
        ));
    }
    if args.read2_in.is_some() {
        return Err(Error::unsupported(
            "--read2-in",
            "paired whitelist estimation is not implemented in this build; use single-end input",
        ));
    }
    let pattern = args.bc_pattern.as_deref().ok_or_else(|| {
        Error::usage_with_hint("whitelist needs -p/--bc-pattern", "example: -p NNNNCCCC")
    })?;
    if !pattern.bytes().any(|byte| byte == b'C' || byte == b'c') {
        return Err(Error::usage_with_hint(
            "--bc-pattern needs C for cell bases",
            "for example: -p NNNNCCCC (N = UMI, C = cell)",
        ));
    }
    if args.knee_method != "distance" && args.knee_method != "density" {
        return Err(Error::usage_with_hint(
            format!("unknown --knee-method '{}'", args.knee_method),
            "choose one of: distance, density",
        ));
    }

    let mut reader = FastqReader::open(args.input.as_deref())?;
    let mut counts: BTreeMap<String, u64> = BTreeMap::new();
    let mut reads = 0u64;
    while let Some(record) = reader.next_record()? {
        if reads >= args.subset_reads {
            break;
        }
        reads += 1;
        if let Some(cell) = extract_cell(&record.seq, pattern) {
            *counts.entry(cell).or_insert(0) += 1;
        }
    }
    let mut ordered: Vec<(String, u64)> = counts.into_iter().collect();
    ordered.sort_by(|left, right| right.1.cmp(&left.1).then(left.0.cmp(&right.0)));
    let sorted_counts: Vec<u64> = ordered.iter().map(|(_, count)| *count).collect();

    let number = if let Some(cell_number) = args.set_cell_number {
        cell_number.min(ordered.len())
    } else {
        match args.knee_method.as_str() {
            "distance" => knee_distance(&sorted_counts),
            _ => knee_density(&sorted_counts, args.expect_cells).ok_or_else(|| {
                Error::Format(
                    "no local minimum was accepted; use --set-cell-number or --expect-cells"
                        .to_string(),
                )
            })?,
        }
    };
    let mut whitelist: Vec<(String, u64)> = ordered[..number.min(ordered.len())].to_vec();
    if whitelist.is_empty() {
        return Err(Error::Format(
            "no cell barcodes passed the threshold".to_string(),
        ));
    }

    // Above-knee error detection: a whitelisted barcode within the correction
    // distance of a more abundant whitelisted barcode is a substitution error
    // and is either discarded or folded into the parent.
    let mut corrections: BTreeMap<String, Vec<(String, u64)>> = BTreeMap::new();
    if let Some(mode) = &args.ed_above_threshold {
        let candidates = whitelist.clone();
        let mut kept = Vec::new();
        for (barcode, count) in candidates {
            let parent = whitelist.iter().find(|(other, other_count)| {
                *other_count > count
                    && hamming(&barcode, other) <= args.error_correct_threshold as usize
            });
            match parent {
                Some((other, _)) if mode == "correct" => {
                    corrections
                        .entry(other.clone())
                        .or_default()
                        .push((barcode, count));
                }
                Some(_) => {}
                None => kept.push((barcode, count)),
            }
        }
        whitelist = kept;
    }

    let directory = args
        .output
        .as_deref()
        .and_then(|path| path.parent())
        .filter(|value| !value.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let temp = TempOutput::create(directory, "umitools-whitelist")?;
    {
        let mut file = std::fs::File::create(temp.path()).map_err(|e| Error::io(temp.path(), e))?;
        for (barcode, count) in &whitelist {
            let (corrected, corrected_counts) = match corrections.get(barcode) {
                Some(entries) => {
                    let mut entries = entries.clone();
                    entries.sort_by(|left, right| left.0.cmp(&right.0));
                    (
                        entries
                            .iter()
                            .map(|(value, _)| value.as_str())
                            .collect::<Vec<_>>()
                            .join(","),
                        entries
                            .iter()
                            .map(|(_, value)| value.to_string())
                            .collect::<Vec<_>>()
                            .join(","),
                    )
                }
                None => (String::new(), String::new()),
            };
            writeln!(file, "{barcode}\t{corrected}\t{count}\t{corrected_counts}")
                .map_err(|e| Error::io(temp.path(), e))?;
        }
    }
    match &args.output {
        Some(path) if path.as_os_str() != "-" => temp.commit(path)?,
        _ => {
            let mut file =
                std::fs::File::open(temp.path()).map_err(|e| Error::io(temp.path(), e))?;
            let mut out = std::io::stdout().lock();
            std::io::copy(&mut file, &mut out)
                .map_err(|e| Error::Format(format!("cannot write stdout: {e}")))?;
            out.flush()
                .map_err(|e| Error::Format(format!("cannot flush stdout: {e}")))?;
        }
    }
    log.info(&format!(
        "{PROGRAM} whitelist: reads={} barcodes={} whitelisted={} method={} wall={:.1}s",
        reads,
        ordered.len(),
        whitelist.len(),
        args.knee_method,
        started.elapsed().as_secs_f64()
    ));
    Ok(())
}
