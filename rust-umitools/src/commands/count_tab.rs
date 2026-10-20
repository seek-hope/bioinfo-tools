//! `count_tab`: unique UMI counts from a two column read/gene table.

use crate::cli::CountTabArgs;
use crate::cli::VERSION;
use crate::commands::{Io, Logger, PROGRAM};
use crate::error::{Error, InputError, Result};
use crate::io::spill::{ensure_distinct_outputs, TempOutput};
use rustc_hash::FxHashSet;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::time::Instant;

/// `(cell, umi)` pair used to count distinct molecules.
type CellUmi = (Option<Box<[u8]>>, Box<[u8]>);

pub fn run(args: &CountTabArgs) -> Result<()> {
    let started = Instant::now();
    let budget = crate::runtime::budget::Budget::resolve(
        args.threads,
        args.memory_limit.as_deref(),
        args.tmpdir.as_deref(),
    )?;
    let io = Io::resolve(&args.input, None, &budget.tmpdir)?;
    let mut log = Logger::new(&args.log, args.quiet)?;
    log.info(&format!("{PROGRAM} count_tab"));

    let separator = {
        let bytes = args.barcode_separator.as_bytes();
        if bytes.len() != 1 {
            return Err(Error::usage_with_hint(
                format!(
                    "--barcode-separator '{}' must be a single character",
                    args.barcode_separator
                ),
                "for example: --barcode-separator _",
            ));
        }
        bytes[0]
    };

    let reader =
        BufReader::new(std::fs::File::open(io.input()).map_err(|e| Error::io(io.input(), e))?);

    let mut outputs: Vec<&std::path::Path> = Vec::new();
    if let Some(path) = &args.output {
        outputs.push(path);
    }
    if let Some(path) = &args.log {
        outputs.push(path);
    }
    ensure_distinct_outputs(&args.input, &outputs)?;

    let stdout_target = args
        .output
        .as_ref()
        .is_none_or(|path| path.as_os_str() == "-");
    let (writer, temp): (Box<dyn Write>, Option<(TempOutput, PathBuf)>) = match &args.output {
        Some(path) if !stdout_target => {
            let dir: PathBuf = path
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .map(std::path::Path::to_path_buf)
                .unwrap_or_else(|| PathBuf::from("."));
            let temp = TempOutput::create(&dir, "umitools-count-tab")?;
            let file = std::fs::File::create(temp.path()).map_err(|e| Error::io(temp.path(), e))?;
            (
                Box::new(std::io::BufWriter::with_capacity(1 << 20, file)),
                Some((temp, path.clone())),
            )
        }
        _ => (
            Box::new(std::io::BufWriter::with_capacity(
                1 << 20,
                std::io::stdout(),
            )),
            None,
        ),
    };
    let mut writer = writer;

    if args.per_cell {
        writeln!(writer, "cell\tgene\tcount").map_err(stream_error)?;
    } else {
        writeln!(writer, "gene\tcount").map_err(stream_error)?;
    }

    let mut current_gene: Option<String> = None;
    let mut seen: FxHashSet<CellUmi> = FxHashSet::default();
    let mut cell_order: Vec<Option<Box<[u8]>>> = Vec::new();
    let mut per_cell: std::collections::HashMap<Option<Box<[u8]>>, u64> =
        std::collections::HashMap::new();
    let mut genes = 0u64;

    let flush_gene = |gene: &str,
                      seen: &mut FxHashSet<CellUmi>,
                      cell_order: &mut Vec<Option<Box<[u8]>>>,
                      per_cell: &mut std::collections::HashMap<Option<Box<[u8]>>, u64>,
                      writer: &mut dyn Write|
     -> Result<()> {
        if args.per_cell {
            for cell in cell_order.iter() {
                let count = per_cell.get(cell).copied().unwrap_or(0);
                let label = cell
                    .as_ref()
                    .map(|c| String::from_utf8_lossy(c).into_owned())
                    .unwrap_or_default();
                writeln!(writer, "{label}\t{gene}\t{count}").map_err(stream_error)?;
            }
        } else {
            writeln!(writer, "{gene}\t{}", seen.len()).map_err(stream_error)?;
        }
        seen.clear();
        cell_order.clear();
        per_cell.clear();
        Ok(())
    };

    for (index, line) in reader.lines().enumerate() {
        let line =
            line.map_err(|e| Error::Format(format!("cannot read {}: {e}", args.input.display())))?;
        if line.trim().is_empty() {
            continue;
        }
        let values: Vec<&str> = line.trim_end().split('\t').collect();
        if values.len() != 2 {
            return Err(Error::Input(InputError::BadTable {
                line: index as u64 + 1,
                detail: format!("expected exactly 2 columns, found {}", values.len()),
            }));
        }
        let (read_id, gene) = (values[0], values[1].to_string());
        if current_gene.as_deref() != Some(gene.as_str()) {
            if let Some(previous) = current_gene.clone() {
                flush_gene(
                    &previous,
                    &mut seen,
                    &mut cell_order,
                    &mut per_cell,
                    writer.as_mut(),
                )?;
                genes += 1;
            }
            current_gene = Some(gene);
        }
        let fields: Vec<&[u8]> = read_id.as_bytes().split(|b| *b == separator).collect();
        if fields.is_empty() {
            return Err(Error::Input(InputError::BadTable {
                line: index as u64 + 1,
                detail: "empty read id".to_string(),
            }));
        }
        let umi: Box<[u8]> = fields[fields.len() - 1].into();
        let cell: Option<Box<[u8]>> = if args.per_cell {
            if fields.len() < 2 {
                return Err(Error::Input(InputError::BadTable {
                    line: index as u64 + 1,
                    detail: "read id has no cell barcode field".to_string(),
                }));
            }
            Some(fields[fields.len() - 2].into())
        } else {
            None
        };
        if seen.insert((cell.clone(), umi)) && args.per_cell {
            if !per_cell.contains_key(&cell) {
                cell_order.push(cell.clone());
            }
            *per_cell.entry(cell).or_insert(0) += 1;
        }
        if seen.len() > budget.max_pending_umis {
            return Err(Error::Resource(format!(
                "one gene holds {} distinct UMIs, above the {} allowed by the memory budget; \
                 raise --memory-limit or split the table by gene",
                seen.len(),
                budget.max_pending_umis
            )));
        }
    }
    if let Some(previous) = current_gene {
        flush_gene(
            &previous,
            &mut seen,
            &mut cell_order,
            &mut per_cell,
            writer.as_mut(),
        )?;
        genes += 1;
    }
    writer.flush().map_err(stream_error)?;
    drop(writer);

    if let Some((temp, destination)) = temp {
        temp.commit(&destination)?;
    }
    log.info(&format!(
        "genes               : {}\nwall time           : {:.1}s",
        genes,
        started.elapsed().as_secs_f64()
    ));
    if let Some(path) = &args.summary_json {
        let mut outputs = Vec::new();
        if let Some(path) = &args.output {
            outputs.push(("tsv", path.display().to_string()));
        }
        let report = crate::runtime::summary::Summary {
            command: "count_tab",
            version: VERSION,
            input: args.input.display().to_string(),
            outputs,
            threads: budget.threads,
            memory_bytes: budget.memory_bytes,
            chunk_bytes: budget.chunk_bytes,
            counters: vec![("genes", genes)],
            timings: vec![("wall_seconds", started.elapsed().as_secs_f64())],
        };
        report.write(path)?;
    }
    Ok(())
}

fn stream_error(error: std::io::Error) -> Error {
    Error::Format(format!("cannot write count table: {error}"))
}
