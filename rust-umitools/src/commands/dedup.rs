//! `dedup`: release one representative read per inferred molecule.

use crate::cli::{DedupConfig, Policy, VERSION};
use crate::commands::{push_program_record, Events, Io, Logger, PROGRAM};
use crate::compat::pyrng::{PyRandom, StableRandom};
use crate::core::bundle::{Bundle, BundleConfig, Bundler, GeneBundler, Storage};
use crate::core::cluster::cluster_with_scratch;
use crate::core::rng::{RepresentativeStrategy, Rng};
use crate::error::{Error, InputError, Result};
use crate::io::alignment::aux_str;
use crate::io::bam::{self, Record};
use crate::io::paired::{write_mates, MateRequests};
use crate::io::sort::SortingWriter;
use crate::io::spill::{ensure_distinct_outputs, TempOutput};
use crate::io::RecordSink;
use crate::runtime::pipeline::map_ordered;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub struct RunSummary {
    pub input_reads: u64,
    pub output_reads: u64,
    pub bundles: u64,
    pub unmapped: u64,
    pub skipped_umi: u64,
    pub ties: u64,
    pub mates: u64,
    pub elapsed: Duration,
}

#[derive(Default)]
struct ProcStats {
    bundles: u64,
    clusters: u64,
    filtered: u64,
}

struct Outcome {
    reads: Vec<Record>,
    clusters: usize,
    filtered: u64,
}

/// Either a direct writer (`--no-sort-output`) or the streaming sorter.
enum Sink {
    Direct {
        temp: TempOutput,
        writer: Box<bam::Writer>,
    },
    Sorted(Box<SortingWriter>),
}

impl RecordSink for Sink {
    fn submit(&mut self, record: Record) -> Result<()> {
        match self {
            Sink::Direct { writer, .. } => writer
                .write(&record)
                .map_err(|e| Error::Format(format!("write failed: {e}"))),
            Sink::Sorted(sorter) => sorter.submit(record),
        }
    }
}

impl Sink {
    fn finish(self, out_dir: &Path, destination: &Path) -> Result<()> {
        match self {
            Sink::Direct { temp, writer } => {
                let writer = *writer;
                writer
                    .finish()
                    .map_err(|e| Error::Format(format!("cannot finish output: {e}")))?;
                temp.commit(destination)
            }
            Sink::Sorted(sorter) => {
                let final_temp = TempOutput::create(out_dir, "umitools-final")?;
                sorter.finish(final_temp.path())?;
                final_temp.commit(destination)
            }
        }
    }
}

/// Cluster one bundle and move the representative records out of it.
fn cluster_bundle(bundle: Bundle, cfg: &DedupConfig) -> Result<Outcome> {
    let counts = bundle.counts();
    let Bundle {
        order, mut entries, ..
    } = bundle;

    if cfg.ignore_umi {
        let reads = order
            .iter()
            .map(|umi| {
                entries
                    .remove(umi)
                    .and_then(|entry| entry.reads.into_iter().next())
                    .ok_or_else(|| Error::Format("missing representative record".to_string()))
            })
            .collect::<Result<Vec<_>>>()?;
        return Ok(Outcome {
            reads,
            clusters: order.len(),
            filtered: 0,
        });
    }

    let mut groups = cluster_with_scratch(
        &order,
        &counts,
        cfg.method,
        cfg.threshold,
        Some(&cfg.budget.tmpdir),
    )?;
    let mut filtered = 0u64;
    if let Some(whitelist) = &cfg.umi_whitelist {
        let mut kept = Vec::with_capacity(groups.len());
        for group in groups {
            if whitelist.contains(&order[group[0]].to_bytes()) {
                kept.push(group);
            } else {
                filtered += group.iter().map(|index| counts[*index]).sum::<u64>();
            }
        }
        groups = kept;
    }
    let mut reads = Vec::with_capacity(groups.len());
    for group in &groups {
        let representative = &order[group[0]];
        let record = entries
            .remove(representative)
            .and_then(|entry| entry.reads.into_iter().next())
            .ok_or_else(|| Error::Format("missing representative record".to_string()))?;
        reads.push(record);
    }
    Ok(Outcome {
        reads,
        clusters: groups.len(),
        filtered,
    })
}

fn flush_batch(
    batch: &mut Vec<Bundle>,
    cfg: &DedupConfig,
    sink: &mut Sink,
    processed: &mut ProcStats,
    mates: &mut MateRequests,
) -> Result<()> {
    if batch.is_empty() {
        return Ok(());
    }
    let items = std::mem::take(batch);
    let outcomes: Vec<Result<Outcome>> = map_ordered(items, |bundle| cluster_bundle(bundle, cfg));
    for outcome in outcomes {
        let outcome = outcome?;
        processed.bundles += 1;
        processed.clusters += outcome.clusters as u64;
        processed.filtered += outcome.filtered;
        for record in outcome.reads {
            // Register the mate only for the read that survives deduplication.
            // Requesting it when the read1 is read would keep the mate of a
            // read1 that lost the representative draw, which upstream never does.
            if cfg.paired {
                mates.request(&record)?;
            }
            sink.submit(record)?;
        }
    }
    Ok(())
}

fn open_reader(path: &Path, reference: Option<&Path>) -> Result<bam::Reader> {
    let mut reader = bam::Reader::from_path(path).map_err(|e| {
        Error::Format(format!(
            "cannot open {}: {e} (check the path, the file format and read permissions)",
            path.display()
        ))
    })?;
    crate::io::set_reference(&mut reader, reference)?;
    Ok(reader)
}

pub fn run(cfg: &DedupConfig) -> Result<RunSummary> {
    let started = Instant::now();
    let trace = std::env::var_os("RUST_UMITOOLS_TRACE").is_some();
    // Progress goes to a terminal only; pipes and CI stay quiet so logs remain
    // machine readable.
    let progress = crate::runtime::summary::progress_enabled(&cfg.progress, cfg.quiet)?;
    let mut last_progress = 0u64;
    let mut outputs: Vec<&Path> = vec![&cfg.output];
    if let Some(log) = &cfg.log {
        outputs.push(log);
    }
    ensure_distinct_outputs(&cfg.input, &outputs)?;
    crate::runtime::pipeline::configure_pool(cfg.budget.threads)?;
    let mut log = Logger::new(&cfg.log, cfg.quiet)?;
    let probe = crate::runtime::budget::HardwareProbe::detect(&cfg.budget);
    log.info(&format!(
        "{PROGRAM} {VERSION} dedup: threads={} memory={}MiB method={} threshold={} representative={}",
        cfg.budget.threads,
        cfg.budget.memory_bytes / 1_048_576,
        cfg.method.as_str(),
        cfg.threshold,
        cfg.strategy.as_str()
    ));
    log.info(&probe.summary());

    let io = Io::resolve(&cfg.input, Some(&cfg.output), &cfg.budget.tmpdir)?;
    let mut reader = open_reader(io.input(), cfg.reference.as_deref())?;
    // BGZF decompression/compression workers come out of the same thread budget.
    let io_threads = crate::runtime::budget::io_worker_threads(cfg.budget.threads);
    reader
        .set_threads(io_threads)
        .map_err(|e| Error::Format(format!("cannot set reader threads: {e}")))?;
    // Resolve --chrom against the header before reading records.
    let target_tid: Option<i32> = match &cfg.chrom {
        Some(name) => {
            let view = reader.header();
            let found = (0..view.target_count()).find(|tid| view.tid2name(*tid) == name.as_bytes());
            match found {
                Some(tid) => Some(tid as i32),
                None => {
                    return Err(Error::usage_with_hint(
                        format!("--chrom '{name}' is not a contig in the input header"),
                        "use a name that appears in the @SQ lines",
                    ))
                }
            }
        }
        None => None,
    };
    let mut header = bam::Header::from_template(reader.header());
    if cfg.add_pg {
        push_program_record(&mut header);
    }

    // A stdout target is staged in the temporary directory, so its final
    // rename must happen on the same filesystem.
    let out_dir: PathBuf = if cfg.output.as_os_str() == "-" {
        cfg.budget.tmpdir.clone()
    } else {
        cfg.output
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."))
    };

    let mut sink = if cfg.no_sort_output {
        let temp = TempOutput::create(&out_dir, "umitools-dedup")?;
        let format = crate::io::output_format(&cfg.output);
        let mut writer = bam::Writer::from_path(temp.path(), &header, format)
            .map_err(|e| Error::Format(format!("cannot write {}: {e}", temp.path().display())))?;
        crate::io::configure_writer(&mut writer, format, cfg.reference.as_deref())?;
        writer
            .set_threads(io_threads)
            .map_err(|e| Error::Format(format!("cannot set writer threads: {e}")))?;
        writer
            .set_compression_level(crate::io::compression_level(cfg.compresslevel)?)
            .map_err(|e| Error::Format(format!("cannot set compression level: {e}")))?;
        Sink::Direct {
            temp,
            writer: Box::new(writer),
        }
    } else {
        Sink::Sorted(Box::new(SortingWriter::new(
            &header,
            &cfg.budget.tmpdir,
            cfg.budget.chunk_bytes,
            cfg.budget.threads,
            cfg.compresslevel,
            crate::io::output_format(&cfg.output),
            cfg.reference.as_deref(),
        )))
    };

    let bundle_cfg = BundleConfig {
        spliced: cfg.spliced,
        ignore_tlen: cfg.ignore_tlen,
        paired: cfg.paired,
        read_length: cfg.read_length,
        per_cell: cfg.per_cell,
        source: cfg.source.clone(),
        cell_tag: cfg.cell_tag,
        cell_tag_split: Some("-".to_string()),
        cell_tag_delim: None,
        soft_clip_threshold: cfg.soft_clip_threshold,
        detection: cfg.detection,
        storage: Storage::Best,
        whole_contig: cfg.whole_contig,
        skip_missing_umi: matches!(cfg.source, crate::core::barcode::UmiSource::Tag { .. }),
    };

    let mut rng: Box<dyn Rng> = match cfg.strategy {
        RepresentativeStrategy::Upstream => Box::new(PyRandom::from_python_seed(cfg.random_seed)),
        RepresentativeStrategy::Stable => Box::new(StableRandom::new(cfg.random_seed as u64)),
    };

    let mut bundler = Bundler::new();
    let mut gene_bundler = GeneBundler::new();
    let mut processed = ProcStats::default();
    let mut events = Events::default();
    let mut batch: Vec<Bundle> = Vec::new();
    let mut batch_umis = 0usize;
    let mut emitted: Vec<Bundle> = Vec::new();
    let mut mates = MateRequests::new(cfg.budget.max_mates, &cfg.budget.tmpdir);
    // Bounded-memory path: a bundle only ever receives reads with its own
    // position, and reads are coordinate sorted, so finishing the window at a
    // position boundary is exact. Closing it early keeps live state near this
    // threshold instead of growing with the 1000 bp window.
    let partition_limit = std::env::var("RUST_UMITOOLS_PARTITION_UMIS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or_else(|| (cfg.budget.max_pending_umis / 2).max(1));
    let mut partition_coord: Option<(i32, i64)> = None;
    let mut last_coord: Option<(i32, i64)> = None;

    for record in reader.records() {
        let record = record.map_err(|e| Error::Format(format!("corrupt input: {e}")))?;
        if let Some(tid) = target_tid {
            if record.tid() != tid || record.is_unmapped() {
                continue;
            }
        }
        if record.is_last_in_template() {
            events.read2 += 1;
            continue;
        }
        events.input_reads += 1;
        if progress && events.input_reads - last_progress >= 1_000_000 {
            last_progress = events.input_reads;
            eprint!(
                "\rprocessed {:.1}M reads, {} molecules",
                events.input_reads as f64 / 1_000_000.0,
                processed.clusters
            );
        }
        if trace && events.input_reads % 50_000 == 0 {
            eprintln!(
                "[trace] reads={} pending_umis={} batch_umis={} batch_bundles={}",
                events.input_reads,
                bundler.live_umis(),
                batch_umis,
                batch.len()
            );
        }

        if cfg.paired && !record.is_paired() {
            match cfg.unpaired_reads {
                Policy::Discard => {
                    events.unpaired += 1;
                    continue;
                }
                Policy::Output => {
                    sink.submit(record)?;
                    events.passthrough += 1;
                    continue;
                }
                Policy::Use => {}
            }
        }
        if record.is_unmapped() {
            events.unmapped += 1;
            continue;
        }
        if cfg.paired && record.is_mate_unmapped() {
            match cfg.unmapped_reads {
                Policy::Discard => {
                    events.mate_unmapped += 1;
                    continue;
                }
                Policy::Output => {
                    sink.submit(record)?;
                    events.passthrough += 1;
                    continue;
                }
                Policy::Use => {}
            }
        }
        if cfg.paired && record.is_paired() && record.tid() != record.mtid() {
            match cfg.chimeric_pairs {
                Policy::Discard => {
                    events.chimeric += 1;
                    continue;
                }
                Policy::Output => {
                    sink.submit(record)?;
                    events.passthrough += 1;
                    continue;
                }
                Policy::Use => {}
            }
        }
        // Upstream draws the subset decision before the MAPQ check and before
        // UMI extraction, from the same generator used for representative ties.
        if let Some(probability) = cfg.subset {
            if rng.next_f64() >= probability {
                events.subset_excluded += 1;
                continue;
            }
        }
        if let Some(minimum) = cfg.mapping_quality {
            if record.mapq() < minimum {
                events.low_mapq += 1;
                continue;
            }
        }

        if cfg.per_gene {
            let tag = cfg.gene_tag.expect("--per-gene validates --gene-tag");
            let gene = match aux_str(&record, &tag) {
                Ok(value) if !value.is_empty() => value,
                _ => continue,
            };
            gene_bundler.push(
                record,
                gene.into_bytes().into(),
                &bundle_cfg,
                rng.as_mut(),
                &mut emitted,
            )?;
            if gene_bundler.live_umis() > cfg.budget.max_pending_umis {
                return Err(Error::Resource(format!(
                    "the gene bundle window holds {} UMIs, above the {} allowed by the memory \
                     budget; raise --memory-limit or split the input",
                    gene_bundler.live_umis(),
                    cfg.budget.max_pending_umis
                )));
            }
            for bundle in emitted.drain(..) {
                batch_umis += bundle.len();
                batch.push(bundle);
            }
            if batch_umis >= cfg.budget.max_batch_umis {
                flush_batch(&mut batch, cfg, &mut sink, &mut processed, &mut mates)?;
                batch_umis = 0;
            }
            continue;
        }

        let coord = (record.tid(), record.pos());
        if let Some(previous) = last_coord {
            if coord < previous {
                return Err(Error::Input(InputError::NotSorted {
                    record_index: events.input_reads,
                    previous: previous.1,
                    current: coord.1,
                }));
            }
        }
        last_coord = Some(coord);

        if partition_coord != Some(coord) && bundler.live_umis() > partition_limit {
            bundler.finish(&mut emitted);
            batch.append(&mut emitted);
            flush_batch(&mut batch, cfg, &mut sink, &mut processed, &mut mates)?;
            batch_umis = 0;
        }
        partition_coord = Some(coord);

        bundler.push(record, &bundle_cfg, rng.as_mut(), &mut emitted)?;
        for bundle in emitted.drain(..) {
            batch_umis += bundle.len();
            batch.push(bundle);
        }
        if bundler.live_umis() > cfg.budget.max_pending_umis {
            return Err(Error::Resource(format!(
                "the bundle window holds {} UMIs, above the {} allowed by the memory budget. \
                 This build has no disk-backed clustering for a single oversized position; \
                 raise --memory-limit or split the input.",
                bundler.live_umis(),
                cfg.budget.max_pending_umis
            )));
        }
        if batch_umis >= cfg.budget.max_batch_umis {
            if trace {
                eprintln!(
                    "[trace] flush start reads={} umis={}",
                    events.input_reads, batch_umis
                );
            }
            flush_batch(&mut batch, cfg, &mut sink, &mut processed, &mut mates)?;
            if trace {
                eprintln!("[trace] flush done reads={}", events.input_reads);
            }
            batch_umis = 0;
        }
    }

    if cfg.per_gene {
        gene_bundler.finish(&mut emitted);
    } else {
        bundler.finish(&mut emitted);
    }
    for bundle in emitted.drain(..) {
        batch.push(bundle);
    }
    flush_batch(&mut batch, cfg, &mut sink, &mut processed, &mut mates)?;

    let parse_elapsed = started.elapsed();
    let mut mate_count = 0u64;
    if cfg.paired && mates.spilled() && cfg.no_sort_output {
        return Err(Error::usage_with_hint(
            "--no-sort-output cannot be combined with spilled mate requests",
            "raise --memory-limit or keep coordinate-sorted output",
        ));
    }
    if cfg.paired && !mates.is_empty() {
        mate_count = write_mates(
            io.input(),
            &mut sink,
            &mut mates,
            cfg.reference.as_deref(),
            target_tid,
        )?;
    }
    let output_path = io
        .output()
        .expect("dedup always has an output path")
        .to_path_buf();
    sink.finish(&out_dir, &output_path)?;
    let sort_elapsed = started.elapsed();

    if progress {
        eprint!("\r");
    }
    let summary = RunSummary {
        input_reads: events.input_reads,
        output_reads: processed.clusters,
        bundles: bundler.stats.bundles,
        unmapped: events.unmapped,
        skipped_umi: bundler.stats.skipped_missing_umi,
        ties: bundler.stats.ties,
        mates: mate_count,
        elapsed: started.elapsed(),
    };
    let tie_note = if cfg.strategy == RepresentativeStrategy::Stable {
        " (stable strategy: MAPQ ties are drawn from the project RNG, not CPython)"
    } else {
        ""
    };
    log.info(&format!(
        "reads in            : {}\nbundles             : {}\noutput reads        : {}\nunmapped removed    : {}\nno-UMI skipped      : {}\nMAPQ ties sampled   : {}{}\nmates recovered     : {}\nevents              : {}\nparse+cluster       : {:.1}s\nsort+commit         : {:.1}s\nwall time           : {:.1}s",
        summary.input_reads,
        summary.bundles,
        summary.output_reads,
        summary.unmapped,
        summary.skipped_umi,
        summary.ties,
        tie_note,
        summary.mates,
        events.summary(),
        parse_elapsed.as_secs_f64(),
        (sort_elapsed - parse_elapsed).as_secs_f64(),
        summary.elapsed.as_secs_f64()
    ));
    if let Some(path) = &cfg.summary_json {
        let report = crate::runtime::summary::Summary {
            command: "dedup",
            version: VERSION,
            input: cfg.input.display().to_string(),
            outputs: vec![("bam", cfg.output.display().to_string())],
            threads: cfg.budget.threads,
            memory_bytes: cfg.budget.memory_bytes,
            chunk_bytes: cfg.budget.chunk_bytes,
            counters: vec![
                ("input_reads", summary.input_reads),
                ("output_reads", summary.output_reads),
                ("bundles", summary.bundles),
                ("unmapped", summary.unmapped),
                ("skipped_umi", summary.skipped_umi),
                ("ties", summary.ties),
                ("mates", summary.mates),
                ("passthrough", events.passthrough),
            ],
            timings: vec![
                ("parse_cluster_seconds", parse_elapsed.as_secs_f64()),
                (
                    "sort_commit_seconds",
                    (sort_elapsed - parse_elapsed).as_secs_f64(),
                ),
                ("wall_seconds", summary.elapsed.as_secs_f64()),
            ],
        };
        report.write(path)?;
    }
    io.commit()?;
    Ok(summary)
}
