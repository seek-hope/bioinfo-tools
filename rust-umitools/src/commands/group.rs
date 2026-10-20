//! `group`: tag every read of a molecule group with `UG` and the corrected UMI.

use crate::cli::GroupArgs;
use crate::cli::VERSION;
use crate::commands::{parse_tag, push_program_record, Io, Logger, PROGRAM};
use crate::core::bundle::{Bundle, Bundler, GeneBundler, Storage};
use crate::core::cluster::cluster_with_scratch;
use crate::error::{Error, Result};
use crate::io::alignment::{aux_str, read_position};
use crate::io::sort::coordinate_sort;
use crate::io::spill::{ensure_distinct_outputs, TempOutput};
use crate::runtime::pipeline::map_ordered;
use rust_htslib::bam::record::Aux;
use rust_htslib::bam::{self, Format, Read, Record};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Instant;

#[derive(Default)]
struct ProcStats {
    groups: u64,
    reads_out: u64,
}

struct Grouped {
    /// `(unique_id, top_umi, group_count, [(umi, count, reads)])`
    groups: Vec<GroupRow>,
}

type Member = (String, u64, Vec<Record>);
type GroupRow = (u64, String, u64, Vec<Member>, String);

fn group_bundle(bundle: Bundle, args: &GroupArgs, tmpdir: &Path) -> Result<Grouped> {
    let gene = bundle
        .label
        .as_ref()
        .map(|(gene, _)| String::from_utf8_lossy(gene).into_owned())
        .unwrap_or_else(|| "NA".to_string());
    let counts = bundle.counts();
    let Bundle {
        order, mut entries, ..
    } = bundle;
    let method = crate::core::cluster::Method::parse(&args.common.method)?;
    let groups =
        cluster_with_scratch(&order, &counts, method, args.common.threshold, Some(tmpdir))?;
    let mut out = Vec::with_capacity(groups.len());
    for group in groups {
        let top = order[group[0]].display();
        let group_count: u64 = group.iter().map(|index| counts[*index]).sum();
        // Move the records out of the bundle instead of cloning every read for
        // every member; group is the most memory hungry command without this.
        let mut members = Vec::with_capacity(group.len());
        for index in group {
            let umi = &order[index];
            let entry = entries
                .remove(umi)
                .ok_or_else(|| Error::Format("missing group entry".to_string()))?;
            members.push((umi.display(), counts[index], entry.reads));
        }
        out.push((0u64, top, group_count, members, gene.clone()));
    }
    Ok(Grouped { groups: out })
}

pub fn run(args: &GroupArgs) -> Result<()> {
    let started = Instant::now();
    crate::io::compression_level(args.common.compresslevel)?;
    let progress =
        crate::runtime::summary::progress_enabled(&args.common.progress, args.common.quiet)?;
    if args.per_gene && args.gene_tag.is_none() {
        return Err(Error::usage_with_hint(
            "--per-gene needs --gene-tag",
            "for example: --per-gene --gene-tag GX",
        ));
    }
    if args.gene_tag.is_some() && !args.per_gene {
        return Err(Error::usage_with_hint(
            "--gene-tag requires --per-gene",
            "add --per-gene to key bundles by gene",
        ));
    }
    if args.common.per_cell && !args.per_gene {
        return Err(Error::usage_with_hint(
            "--per-cell requires --per-gene",
            "cell aware grouping keys bundles by gene and cell",
        ));
    }
    let gene_tag = match &args.gene_tag {
        Some(name) => Some(parse_tag(name)?),
        None => None,
    };
    let group_tag = parse_tag(&args.umi_group_tag)?;
    let output = args.common.output.clone();
    let mut outputs: Vec<&Path> = Vec::new();
    if let Some(path) = &output {
        outputs.push(path);
    }
    if let Some(path) = &args.group_out {
        outputs.push(path);
    }
    if let Some(path) = &args.common.log {
        outputs.push(path);
    }
    ensure_distinct_outputs(&args.common.input, &outputs)?;
    let summary_outputs: Vec<(&'static str, String)> = outputs
        .iter()
        .map(|path| ("output", path.display().to_string()))
        .collect();
    drop(outputs);
    let budget = args.common.budget()?;
    let io = Io::resolve(
        &args.common.input,
        args.common.output.as_deref(),
        &budget.tmpdir,
    )?;
    crate::runtime::pipeline::configure_pool(budget.threads)?;
    let mut log = Logger::new(&args.common.log, args.common.quiet)?;
    log.info(&format!("{PROGRAM} group: threads={}", budget.threads));

    let mut reader = bam::Reader::from_path(io.input())
        .map_err(|e| Error::Format(format!("cannot open {}: {e}", args.common.input.display())))?;
    crate::io::set_reference(&mut reader, args.common.reference.as_deref())?;
    reader
        .set_threads(crate::runtime::budget::io_worker_threads(budget.threads))
        .map_err(|e| Error::Format(format!("cannot set reader threads: {e}")))?;
    let mut header = bam::Header::from_template(reader.header());
    if args.common.add_pg {
        push_program_record(&mut header);
    }
    let contig_names: Vec<String> = {
        let view = reader.header();
        (0..view.target_count())
            .map(|tid| String::from_utf8_lossy(view.tid2name(tid)).into_owned())
            .collect()
    };

    let staged = match io.output() {
        Some(path) => {
            let dir: PathBuf = if path.as_os_str() == "-" {
                budget.tmpdir.clone()
            } else {
                path.parent()
                    .filter(|p| !p.as_os_str().is_empty())
                    .map(std::path::Path::to_path_buf)
                    .unwrap_or_else(|| PathBuf::from("."))
            };
            let _ = path;
            Some((TempOutput::create(&dir, "umitools-group")?, dir))
        }
        None => None,
    };
    let mut writer = match &staged {
        Some((temp, _)) => {
            let mut writer = bam::Writer::from_path(temp.path(), &header, Format::Bam)
                .map_err(|e| Error::Format(format!("cannot write: {e}")))?;
            writer
                .set_threads(crate::runtime::budget::io_worker_threads(budget.threads))
                .map_err(|e| Error::Format(format!("cannot set writer threads: {e}")))?;
            // This staged BAM is reread and recompressed by coordinate_sort;
            // spending level-6 CPU on it would be pure duplication.
            writer
                .set_compression_level(rust_htslib::bam::CompressionLevel::Fastest)
                .map_err(|e| Error::Format(format!("cannot set compression level: {e}")))?;
            Some(writer)
        }
        None => None,
    };

    let group_out = args.group_out.clone();
    let tsv_temp: Option<(TempOutput, PathBuf)> = match &group_out {
        Some(path) => {
            let dir: PathBuf = path
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .map(std::path::Path::to_path_buf)
                .unwrap_or_else(|| PathBuf::from("."));
            Some((
                TempOutput::create(&dir, "umitools-group-tsv")?,
                path.clone(),
            ))
        }
        None => None,
    };
    let mut tsv: Option<std::io::BufWriter<std::fs::File>> = match &tsv_temp {
        Some((temp, _)) => Some(std::io::BufWriter::with_capacity(
            1 << 20,
            std::fs::File::create(temp.path()).map_err(|e| Error::io(temp.path(), e))?,
        )),
        None => None,
    };
    if let Some(file) = tsv.as_mut() {
        writeln!(
            file,
            "read_id\tcontig\tposition\tgene\tumi\tumi_count\tfinal_umi\tfinal_umi_count\tunique_id"
        )
        .map_err(|e| Error::io(group_out.clone().unwrap_or_default(), e))?;
    }

    let mut bundle_cfg = args.common.bundle_config(Storage::All)?;
    bundle_cfg.whole_contig = args.whole_contig;
    let mut rng = crate::compat::pyrng::StableRandom::new(args.common.random_seed as u64);
    let mut bundler = Bundler::new();
    let mut gene_bundler = GeneBundler::new();
    let mut processed = ProcStats::default();
    let mut input_reads = 0u64;
    let mut last_progress = 0u64;
    let mut emitted: Vec<Bundle> = Vec::new();
    let mut batch: Vec<Bundle> = Vec::new();
    let mut batch_umis = 0usize;
    let mut unique_id = 0u64;

    for record in reader.records() {
        let record = record.map_err(|e| Error::Format(format!("corrupt input: {e}")))?;
        input_reads += 1;
        if progress && input_reads - last_progress >= 1_000_000 {
            last_progress = input_reads;
            eprint!(
                "\rprocessed {:.1}M reads, {} groups",
                input_reads as f64 / 1_000_000.0,
                processed.groups
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
        if let Some(tag) = gene_tag {
            let gene = match aux_str(&record, &tag) {
                Ok(value) if !value.is_empty() => value,
                _ => continue,
            };
            gene_bundler.push(
                record,
                gene.into_bytes().into(),
                &bundle_cfg,
                &mut rng,
                &mut emitted,
            )?;
        } else {
            bundler.push(record, &bundle_cfg, &mut rng, &mut emitted)?;
        }
        let live = if gene_tag.is_some() {
            gene_bundler.live_umis()
        } else {
            bundler.live_umis()
        };
        if live > budget.max_pending_umis {
            return Err(Error::Resource(format!(
                "the bundle window holds {} UMIs, above the {} allowed by the memory \
                 budget; raise --memory-limit or split the input",
                live, budget.max_pending_umis
            )));
        }
        for bundle in emitted.drain(..) {
            batch_umis += bundle.len();
            batch.push(bundle);
        }
        if batch_umis >= budget.max_batch_umis {
            flush(
                &mut batch,
                args,
                &mut writer,
                &mut tsv,
                &mut unique_id,
                &mut processed,
                &bundle_cfg,
                group_tag,
                &contig_names,
                &budget.tmpdir,
            )?;
            batch_umis = 0;
        }
    }
    if gene_tag.is_some() {
        gene_bundler.finish(&mut emitted);
    } else {
        bundler.finish(&mut emitted);
    }
    for bundle in emitted.drain(..) {
        batch.push(bundle);
    }
    flush(
        &mut batch,
        args,
        &mut writer,
        &mut tsv,
        &mut unique_id,
        &mut processed,
        &bundle_cfg,
        group_tag,
        &contig_names,
        &budget.tmpdir,
    )?;
    drop(writer);
    if progress {
        eprint!("\r");
    }

    if let Some((temp, dir)) = staged {
        let destination = io
            .output()
            .expect("staged output requires a destination")
            .to_path_buf();
        if args.no_sort_output {
            temp.commit(&destination)?;
        } else {
            let final_temp = TempOutput::create(&dir, "umitools-group-out")?;
            coordinate_sort(
                temp.path(),
                final_temp.path(),
                &budget.tmpdir,
                budget.chunk_bytes,
                budget.threads,
                args.common.compresslevel,
                crate::io::output_format(&destination),
                args.common.reference.as_deref(),
            )?;
            final_temp.commit(&destination)?;
        }
    }
    if let Some(file) = tsv.as_mut() {
        file.flush()
            .map_err(|e| Error::Format(format!("cannot flush group table: {e}")))?;
    }
    drop(tsv);
    // The table is staged like every other output: a failure before this point
    // leaves any previous file untouched.
    if let Some((temp, destination)) = tsv_temp {
        temp.commit(&destination)?;
    }
    io.commit()?;
    log.info(&format!(
        "groups              : {}\nreads out           : {}\nwall time           : {:.1}s",
        processed.groups,
        processed.reads_out,
        started.elapsed().as_secs_f64()
    ));
    if let Some(path) = &args.common.summary_json {
        let report = crate::runtime::summary::Summary {
            command: "group",
            version: VERSION,
            input: args.common.input.display().to_string(),
            outputs: summary_outputs,
            threads: budget.threads,
            memory_bytes: budget.memory_bytes,
            chunk_bytes: budget.chunk_bytes,
            counters: vec![
                ("groups", processed.groups),
                ("reads_out", processed.reads_out),
            ],
            timings: vec![("wall_seconds", started.elapsed().as_secs_f64())],
        };
        report.write(path)?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn flush(
    batch: &mut Vec<Bundle>,
    args: &GroupArgs,
    writer: &mut Option<bam::Writer>,
    tsv: &mut Option<std::io::BufWriter<std::fs::File>>,
    unique_id: &mut u64,
    processed: &mut ProcStats,
    bundle_cfg: &crate::core::bundle::BundleConfig,
    group_tag: [u8; 2],
    contig_names: &[String],
    tmpdir: &Path,
) -> Result<()> {
    if batch.is_empty() {
        return Ok(());
    }
    let items = std::mem::take(batch);
    let results: Vec<Result<Grouped>> =
        map_ordered(items, |bundle| group_bundle(bundle, args, tmpdir));
    for result in results {
        let grouped = result?;
        for (_, top, group_count, members, gene) in grouped.groups {
            let id = *unique_id;
            *unique_id += 1;
            processed.groups += 1;
            for (umi, count, reads) in members {
                for mut read in reads {
                    if let Some(writer) = writer.as_mut() {
                        // Upstream set_tag replaces an existing value; push_aux
                        // would append a duplicate field and change the bytes.
                        let _ = read.remove_aux(b"UG");
                        let _ = read.remove_aux(&group_tag);
                        crate::commands::push_aux_uint(&mut read, b"UG", id)?;
                        read.push_aux(&group_tag, Aux::String(&top))
                            .map_err(|e| Error::Format(format!("cannot set group tag: {e}")))?;
                        writer
                            .write(&read)
                            .map_err(|e| Error::Format(format!("write failed: {e}")))?;
                    }
                    if let Some(file) = tsv.as_mut() {
                        let position =
                            read_position(&read, bundle_cfg.soft_clip_threshold, false).1;
                        let reference = reference_name(&read, contig_names);
                        writeln!(
                            file,
                            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                            String::from_utf8_lossy(read.qname()),
                            reference,
                            position,
                            gene,
                            umi,
                            count,
                            top,
                            group_count,
                            id
                        )
                        .map_err(|e| Error::Format(format!("cannot write group table: {e}")))?;
                    }
                    processed.reads_out += 1;
                }
            }
        }
    }
    Ok(())
}

fn reference_name(record: &Record, contig_names: &[String]) -> String {
    match record.tid() {
        tid if tid >= 0 => contig_names
            .get(tid as usize)
            .cloned()
            .unwrap_or_else(|| tid.to_string()),
        _ => "*".to_string(),
    }
}
