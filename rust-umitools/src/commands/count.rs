//! `count`: unique UMI counts per gene.
//!
//! Gene bundles are held per contig, which can exceed the memory budget on
//! high-diversity input. When the streaming pass reports a resource error the
//! command retries with a two-pass gene partition: records are distributed to
//! temporary BAMs by gene hash, then each partition is clustered independently.
//! `CountOnly` bundles never draw from the representative RNG, so partitioning
//! cannot change the result.

use crate::cli::CountArgs;
use crate::cli::VERSION;
use crate::commands::{parse_tag, Io, Logger, PROGRAM};
use crate::core::bundle::{BundleConfig, CountBundle, CountGeneBundler, Storage};
use crate::core::cluster::cluster_count_with_scratch;
use crate::core::cluster::Method;
use crate::error::{Error, Result};
use crate::io::alignment::aux_str;
use crate::io::spill::{ensure_distinct_outputs, TempOutput};
use crate::runtime::budget::Budget;
use crate::runtime::pipeline::map_ordered;
use rust_htslib::bam::{self, Format, Read};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Instant;

type Counts = BTreeMap<(String, String), u64>;

fn hash_key(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

fn gene_for(
    record: &bam::Record,
    gene_tag: Option<[u8; 2]>,
    contig_names: &[String],
    mapped: Option<&[Option<String>]>,
) -> Option<String> {
    if let Some(mapped) = mapped {
        let tid = record.tid();
        if tid < 0 {
            return None;
        }
        return mapped.get(tid as usize).and_then(|gene| gene.clone());
    }
    match gene_tag {
        Some(tag) => match aux_str(record, &tag) {
            Ok(value) if !value.is_empty() => Some(value),
            _ => None,
        },
        None => match record.tid() {
            tid if tid >= 0 => Some(
                contig_names
                    .get(tid as usize)
                    .cloned()
                    .unwrap_or_else(|| tid.to_string()),
            ),
            _ => None,
        },
    }
}

/// Load `gene<TAB>transcript`, keeping only transcripts present in the header.
/// Upstream stops at the first blank line and skips `#` comments.
fn load_gene_map(path: &Path, contig_names: &[String]) -> Result<Vec<Option<String>>> {
    let text = std::fs::read_to_string(path).map_err(|e| Error::io(path, e))?;
    let known: std::collections::HashSet<&str> = contig_names.iter().map(String::as_str).collect();
    let mut by_transcript: std::collections::HashMap<&str, &str> = std::collections::HashMap::new();
    for line in text.lines() {
        if line.starts_with('#') {
            continue;
        }
        if line.trim().is_empty() {
            break;
        }
        let mut fields = line.trim_end_matches('\r').split('\t');
        let gene = fields.next().unwrap_or("");
        let transcript = fields.next().ok_or_else(|| {
            Error::usage(format!("{}: expected gene<TAB>transcript", path.display()))
        })?;
        if known.contains(transcript) {
            by_transcript.insert(transcript, gene);
        }
    }
    Ok(contig_names
        .iter()
        .map(|name| {
            by_transcript
                .get(name.as_str())
                .map(|gene| (*gene).to_string())
        })
        .collect())
}

#[allow(clippy::too_many_arguments)]
fn process(
    reader: &mut bam::Reader,
    args: &CountArgs,
    budget: &Budget,
    method: Method,
    bundle_cfg: &BundleConfig,
    contig_names: &[String],
    gene_tag: Option<[u8; 2]>,
    assigned_tag: Option<[u8; 2]>,
    skip_regex: Option<&regex::Regex>,
    mapped: Option<&[Option<String>]>,
    hold_genes: bool,
    progress: bool,
    counts: &mut Counts,
) -> Result<()> {
    let mut bundler = CountGeneBundler::new();
    bundler.set_flush_on_contig(!hold_genes);
    let mut emitted: Vec<CountBundle> = Vec::new();
    let mut batch: Vec<(String, CountBundle)> = Vec::new();
    let mut batch_umis = 0usize;
    let mut input_reads = 0u64;
    let mut last_progress = 0u64;

    for record in reader.records() {
        let record = record.map_err(|e| Error::Format(format!("corrupt input: {e}")))?;
        input_reads += 1;
        if progress && input_reads - last_progress >= 1_000_000 {
            last_progress = input_reads;
            eprint!(
                "\rprocessed {:.1}M reads, {} genes",
                input_reads as f64 / 1_000_000.0,
                counts.len()
            );
        }
        if record.is_last_in_template() || record.is_unmapped() {
            continue;
        }
        if let Some(minimum) = args.common.mapping_quality {
            if record.mapq() < minimum {
                continue;
            }
        }
        if let Some(tag) = assigned_tag {
            if let Some(regex) = skip_regex {
                let assigned = aux_str(&record, &tag).unwrap_or_default();
                if regex.is_match(&assigned) {
                    continue;
                }
            }
        }
        let Some(gene) = gene_for(&record, gene_tag, contig_names, mapped) else {
            continue;
        };
        bundler.push(record, gene.into_bytes().into(), bundle_cfg, &mut emitted)?;
        if bundler.live_umis() > budget.max_pending_umis {
            return Err(Error::Resource(format!(
                "the gene bundle window holds {} UMIs, above the {} allowed by the memory \
                 budget; retrying with gene partitions or raise --memory-limit",
                bundler.live_umis(),
                budget.max_pending_umis
            )));
        }
        for bundle in emitted.drain(..) {
            batch_umis += bundle.len();
            batch.push((String::new(), bundle));
        }
        if batch_umis >= budget.max_batch_umis {
            flush(
                &mut batch,
                method,
                args.common.threshold,
                counts,
                &budget.tmpdir,
            )?;
            batch_umis = 0;
        }
    }
    bundler.finish(&mut emitted);
    for bundle in emitted.drain(..) {
        batch.push((String::new(), bundle));
    }
    flush(
        &mut batch,
        method,
        args.common.threshold,
        counts,
        &budget.tmpdir,
    )?;
    if progress {
        eprint!("\r");
    }
    Ok(())
}

/// Two-pass fallback: hash-partition records by gene, then process each part.
#[allow(clippy::too_many_arguments)]
fn partitioned(
    input: &Path,
    partitions: usize,
    args: &CountArgs,
    budget: &Budget,
    method: Method,
    bundle_cfg: &BundleConfig,
    contig_names: &[String],
    gene_tag: Option<[u8; 2]>,
    assigned_tag: Option<[u8; 2]>,
    skip_regex: Option<&regex::Regex>,
    mapped: Option<&[Option<String>]>,
    hold_genes: bool,
    progress: bool,
    counts: &mut Counts,
) -> Result<()> {
    let mut reader = bam::Reader::from_path(input)
        .map_err(|e| Error::Format(format!("cannot reopen {}: {e}", input.display())))?;
    crate::io::set_reference(&mut reader, args.common.reference.as_deref())?;
    reader
        .set_threads(crate::runtime::budget::io_worker_threads(budget.threads))
        .map_err(|e| Error::Format(format!("cannot set reader threads: {e}")))?;
    let header = bam::Header::from_template(reader.header());

    let mut temps = Vec::with_capacity(partitions);
    let mut writers = Vec::with_capacity(partitions);
    for _ in 0..partitions {
        let temp = TempOutput::create(&budget.tmpdir, "umitools-gene-part")?;
        let mut writer = bam::Writer::from_path(temp.path(), &header, Format::Bam)
            .map_err(|e| Error::Format(format!("cannot write partition: {e}")))?;
        writer
            .set_threads(1)
            .map_err(|e| Error::Format(format!("cannot configure partition writer: {e}")))?;
        writers.push(writer);
        temps.push(temp);
    }
    for record in reader.records() {
        let record = record.map_err(|e| Error::Format(format!("corrupt input: {e}")))?;
        let key = match gene_for(&record, gene_tag, contig_names, mapped) {
            Some(key) => key,
            None if mapped.is_some() => continue,
            None => format!("contig:{}", record.tid()),
        };
        let index = (hash_key(key.as_bytes()) as usize) % partitions;
        writers[index]
            .write(&record)
            .map_err(|e| Error::Format(format!("cannot write partition: {e}")))?;
    }
    drop(writers);

    for temp in &temps {
        let mut reader = bam::Reader::from_path(temp.path())
            .map_err(|e| Error::Format(format!("cannot read partition: {e}")))?;
        process(
            &mut reader,
            args,
            budget,
            method,
            bundle_cfg,
            contig_names,
            gene_tag,
            assigned_tag,
            skip_regex,
            mapped,
            hold_genes,
            progress,
            counts,
        )?;
    }
    Ok(())
}

pub fn run(args: &CountArgs) -> Result<()> {
    let started = Instant::now();
    crate::io::compression_level(args.common.compresslevel)?;
    let progress =
        crate::runtime::summary::progress_enabled(&args.common.progress, args.common.quiet)?;
    if args.wide && !args.common.per_cell {
        return Err(Error::usage_with_hint(
            "--wide needs --per-cell",
            "wide output is a gene x cell matrix; add --per-cell --cell-tag CB",
        ));
    }
    if !args.per_gene && !args.per_contig {
        return Err(Error::usage_with_hint(
            "count needs --per-gene or --per-contig",
            "example: count -I assigned.bam -S counts.tsv --per-gene --gene-tag GX",
        ));
    }
    let gene_tag = match (&args.gene_tag, args.per_contig) {
        (Some(name), _) => Some(parse_tag(name)?),
        (None, true) => None,
        (None, false) => Some(*b"XT"),
    };
    let assigned_tag = match &args.assigned_status_tag {
        Some(name) => Some(parse_tag(name)?),
        None => gene_tag,
    };
    let skip_regex = match &args.skip_regex {
        Some(pattern) => Some(
            regex::Regex::new(pattern)
                .map_err(|e| Error::usage(format!("--skip-tags-regex is not valid: {e}")))?,
        ),
        None => None,
    };

    let output = args
        .common
        .output
        .clone()
        .ok_or_else(|| Error::usage("count needs -S/--stdout"))?;
    let mut outputs: Vec<&Path> = vec![&output];
    if let Some(log) = &args.common.log {
        outputs.push(log);
    }
    ensure_distinct_outputs(&args.common.input, &outputs)?;
    let budget = args.common.budget()?;
    let io = Io::resolve(&args.common.input, Some(&output), &budget.tmpdir)?;
    crate::runtime::pipeline::configure_pool(budget.threads)?;
    let mut log = Logger::new(&args.common.log, args.common.quiet)?;
    log.info(&format!("{PROGRAM} count: threads={}", budget.threads));

    let mut reader = bam::Reader::from_path(io.input())
        .map_err(|e| Error::Format(format!("cannot open {}: {e}", args.common.input.display())))?;
    crate::io::set_reference(&mut reader, args.common.reference.as_deref())?;
    reader
        .set_threads((budget.threads / 2).max(1))
        .map_err(|e| Error::Format(format!("cannot set reader threads: {e}")))?;
    let contig_names: Vec<String> = {
        let view = reader.header();
        (0..view.target_count())
            .map(|tid| String::from_utf8_lossy(view.tid2name(tid)).into_owned())
            .collect()
    };

    let mapped = match &args.gene_transcript_map {
        Some(path) => {
            if !args.per_contig {
                return Err(Error::usage_with_hint(
                    "--gene-transcript-map needs --per-contig",
                    "add --per-contig so transcripts are grouped by their mapped gene",
                ));
            }
            if args.gene_tag.is_some() {
                return Err(Error::usage_with_hint(
                    "--gene-transcript-map cannot be combined with --gene-tag",
                    "use --per-contig or --gene-tag, not both",
                ));
            }
            Some(load_gene_map(path, &contig_names)?)
        }
        None => None,
    };

    let dir: PathBuf = if output.as_os_str() == "-" {
        budget.tmpdir.clone()
    } else {
        output
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .map(std::path::Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."))
    };
    let temp = TempOutput::create(&dir, "umitools-count")?;

    let method = Method::parse(&args.common.method)?;
    let bundle_cfg = args.common.bundle_config(Storage::CountOnly)?;
    let mut counts: Counts = Counts::new();

    if mapped.is_some() {
        let partitions = if args.partition_genes == 0 {
            64
        } else {
            args.partition_genes
        };
        log.info(&format!(
            "{PROGRAM} count: gene-transcript map, {partitions} gene partitions"
        ));
        partitioned(
            io.input(),
            partitions,
            args,
            &budget,
            method,
            &bundle_cfg,
            &contig_names,
            gene_tag,
            assigned_tag,
            skip_regex.as_ref(),
            mapped.as_deref(),
            true,
            progress,
            &mut counts,
        )?;
    } else {
        let streamed = process(
            &mut reader,
            args,
            &budget,
            method,
            &bundle_cfg,
            &contig_names,
            gene_tag,
            assigned_tag,
            skip_regex.as_ref(),
            None,
            false,
            progress,
            &mut counts,
        );
        match streamed {
            Ok(()) => {}
            Err(Error::Resource(_)) if args.partition_genes != 1 => {
                counts.clear();
                let partitions = if args.partition_genes > 1 {
                    args.partition_genes
                } else {
                    64
                };
                log.info(&format!(
                    "{PROGRAM} count: gene window exceeded the budget, retrying with {partitions} partitions"
                ));
                partitioned(
                    io.input(),
                    partitions,
                    args,
                    &budget,
                    method,
                    &bundle_cfg,
                    &contig_names,
                    gene_tag,
                    assigned_tag,
                    skip_regex.as_ref(),
                    None,
                    false,
                    progress,
                    &mut counts,
                )?;
            }
            Err(error) => return Err(error),
        }
    }

    {
        let mut file = std::io::BufWriter::with_capacity(
            1 << 20,
            std::fs::File::create(temp.path()).map_err(|e| Error::io(temp.path(), e))?,
        );
        // Upstream writes a header line before the counts.
        if args.wide {
            // Dense gene x cell matrix; missing entries are zero. The long
            // per-cell form remains the sparse triplet representation.
            let mut cells: Vec<&str> = counts.keys().map(|(_, cell)| cell.as_str()).collect();
            cells.sort_unstable();
            cells.dedup();
            write!(file, "gene").map_err(|e| Error::io(temp.path(), e))?;
            for cell in &cells {
                write!(file, "\t{cell}").map_err(|e| Error::io(temp.path(), e))?;
            }
            writeln!(file).map_err(|e| Error::io(temp.path(), e))?;
            let mut genes: Vec<&str> = counts.keys().map(|(gene, _)| gene.as_str()).collect();
            genes.sort_unstable();
            genes.dedup();
            for gene in genes {
                write!(file, "{gene}").map_err(|e| Error::io(temp.path(), e))?;
                for cell in &cells {
                    let value = counts
                        .get(&(gene.to_string(), (*cell).to_string()))
                        .copied()
                        .unwrap_or(0);
                    write!(file, "\t{value}").map_err(|e| Error::io(temp.path(), e))?;
                }
                writeln!(file).map_err(|e| Error::io(temp.path(), e))?;
            }
        } else if args.common.per_cell {
            writeln!(file, "gene\tcell\tcount").map_err(|e| Error::io(temp.path(), e))?;
            for ((gene, cell), count) in &counts {
                writeln!(file, "{gene}\t{cell}\t{count}").map_err(|e| Error::io(temp.path(), e))?;
            }
        } else {
            writeln!(file, "gene\tcount").map_err(|e| Error::io(temp.path(), e))?;
            for ((gene, _), count) in &counts {
                writeln!(file, "{gene}\t{count}").map_err(|e| Error::io(temp.path(), e))?;
            }
        }
        file.flush().map_err(|e| Error::io(temp.path(), e))?;
    }
    temp.commit(io.output().expect("count always has an output"))?;
    log.info(&format!(
        "genes               : {}\nwall time           : {:.1}s",
        counts.len(),
        started.elapsed().as_secs_f64()
    ));
    if let Some(path) = &args.common.summary_json {
        let report = crate::runtime::summary::Summary {
            command: "count",
            version: VERSION,
            input: args.common.input.display().to_string(),
            outputs: vec![("tsv", output.display().to_string())],
            threads: budget.threads,
            memory_bytes: budget.memory_bytes,
            chunk_bytes: budget.chunk_bytes,
            counters: vec![
                ("genes", counts.len() as u64),
                ("molecules", counts.values().sum()),
            ],
            timings: vec![("wall_seconds", started.elapsed().as_secs_f64())],
        };
        report.write(path)?;
    }
    io.commit()?;
    Ok(())
}

fn flush(
    batch: &mut Vec<(String, CountBundle)>,
    method: Method,
    threshold: u32,
    counts: &mut Counts,
    tmpdir: &Path,
) -> Result<()> {
    if batch.is_empty() {
        return Ok(());
    }
    let items = std::mem::take(batch);
    let results: Vec<Result<(String, String, usize)>> = map_ordered(items, |(_, bundle)| {
        let (label, umis, values) = bundle.into_sorted();
        let groups = cluster_count_with_scratch(&umis, &values, method, threshold, Some(tmpdir))?;
        let gene = label
            .as_ref()
            .map(|(gene, _)| String::from_utf8_lossy(gene).into_owned())
            .unwrap_or_default();
        let cell = label
            .as_ref()
            .and_then(|(_, cell)| cell.as_ref())
            .map(|cell| String::from_utf8_lossy(cell).into_owned())
            .unwrap_or_default();
        Ok((gene, cell, groups))
    });
    for result in results {
        let (gene, cell, n) = result?;
        *counts.entry((gene, cell)).or_insert(0) += n as u64;
    }
    Ok(())
}
