//! Command line surface and validated configuration.
//!
//! Options that this build does not implement are parsed (so the message is
//! actionable) and rejected before any output file is created.

use crate::core::barcode::UmiSource;
use crate::core::bundle::Detection;
use crate::core::cluster::Method;
use crate::core::rng::RepresentativeStrategy;
use crate::error::{Error, Result};
use crate::runtime::budget::Budget;
use clap::{Args, Parser, Subcommand};
use std::path::{Path, PathBuf};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

const TOP_HELP: &str = "\
EXAMPLES:
  rust-umitools dedup -I in.bam -S out.bam
  rust-umitools dedup -I in.bam -S out.bam --method=directional --random-seed 1
  rust-umitools group -I in.bam -S grouped.bam --output-bam --group-out groups.tsv
  rust-umitools count -I assigned.bam -S counts.tsv --per-gene --gene-tag GX
  rust-umitools count_tab -I reads.tsv -S counts.tsv

DEFAULTS:
  --method=directional  --edit-distance-threshold=1  --umi-separator=_
  --compresslevel=1 (use --compresslevel 6 for smaller output)
  threads = min(logical CPUs, 16); memory = half of MemAvailable (clamped)
  The run summary reports the detected hardware and the chosen budget.";

const DEDUP_HELP: &str = "\
EXAMPLES:
  rust-umitools dedup -I in.bam -S out.bam
  rust-umitools dedup -I in.bam -S out.bam --paired --random-seed 1
  rust-umitools dedup -I in.bam -S out.bam --extract-umi-method=tag --umi-tag=RX

NOTES:
  Output is coordinate sorted by default; --no-sort-output keeps processing order.
  --add-pg appends a provenance @PG line and therefore changes the output bytes.
  Unsupported options fail before any output file is created.";

const GROUP_HELP: &str = "\
EXAMPLES:
  rust-umitools group -I in.bam --group-out groups.tsv
  rust-umitools group -I in.bam -S grouped.bam --output-bam --group-out groups.tsv

NOTES:
  --output-bam is accepted for UMI-tools compatibility; a BAM is written when -S is given.
  Reads are tagged with UG (group id) and the corrected UMI (BX by default).";

const COUNT_HELP: &str = "\
EXAMPLES:
  rust-umitools count -I assigned.bam -S counts.tsv --per-gene --gene-tag GX
  rust-umitools count -I assigned.bam -S counts.tsv --per-contig

NOTES:
  Input is BAM/SAM; output is a TSV table, not BAM.
  The default table is gene<TAB>count (or gene<TAB>cell<TAB>count with --per-cell);
  --wide writes a dense gene x cell matrix.";

const WHITELIST_HELP: &str = "\
EXAMPLES:
  rust-umitools whitelist -I reads.fastq.gz -S whitelist.tsv -p NNNNCCCC
  rust-umitools whitelist -I reads.fastq -S wl.tsv -p NNNNCCCC --knee-method density

NOTES:
  N marks UMI bases and C marks cell bases in the pattern.
  The output is barcode, corrected barcodes, count, corrected counts.
  --method umis and paired input are rejected in this build.";

const EXTRACT_HELP: &str = "\
EXAMPLES:
  rust-umitools extract -I reads.fastq.gz -S out.fastq.gz -p NNNN
  rust-umitools extract -I r1.fastq -S o1.fastq -p NNNN --read2-in r2.fastq --read2-out o2.fastq

NOTES:
  N marks UMI bases in the barcode pattern; the whole pattern is removed from the read.
  The UMI is appended to the read name with --umi-separator (default _).
  gzip is selected by a .gz suffix; omit -I/-S for stdin/stdout.";

const COUNT_TAB_HELP: &str = "\
EXAMPLES:
  rust-umitools count_tab -I reads.tsv -S counts.tsv
  rust-umitools count_tab -I reads.tsv -S counts.tsv --per-cell

NOTES:
  Input is a two column table (read_id<sep>UMI and gene) sorted by gene.
  Malformed rows fail with the line number.";

#[derive(Parser, Debug)]
#[command(
    name = "rust-umitools",
    version = VERSION,
    about = "Rust high-performance implementation of UMI-tools workflows",
    args_conflicts_with_subcommands = true,
    disable_help_subcommand = true,
    after_help = TOP_HELP
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,

    /// Legacy form: <input.bam> <output.bam> [threads]
    #[arg(value_name = "INPUT")]
    pub legacy_input: Option<PathBuf>,
    #[arg(value_name = "OUTPUT")]
    pub legacy_output: Option<PathBuf>,
    #[arg(value_name = "THREADS")]
    pub legacy_threads: Option<usize>,
    /// Print detailed build information together with --version
    #[arg(long = "verbose", global = true)]
    pub verbose: bool,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Estimate true cell barcodes from FASTQ
    Whitelist(WhitelistArgs),
    /// Extract UMI from FASTQ reads into the read name
    Extract(ExtractArgs),
    /// Deduplicate reads grouped by UMI
    Dedup(DedupArgs),
    /// Group reads by UMI and tag them with UG/BX
    Group(GroupArgs),
    /// Count unique UMIs per gene (or per cell and gene)
    Count(CountArgs),
    /// Count unique UMIs from a two column read/gene table
    #[command(name = "count_tab")]
    CountTab(CountTabArgs),
}

#[derive(Args, Debug, Clone)]
pub struct CommonArgs {
    /// Input file: BAM, or SAM when the name ends in .sam (- for stdin)
    #[arg(
        short = 'I',
        long = "stdin",
        visible_alias = "input",
        value_name = "FILE"
    )]
    pub input: PathBuf,
    /// Output file; the format depends on the command (- for stdout)
    #[arg(
        short = 'S',
        long = "stdout",
        visible_alias = "output",
        value_name = "FILE"
    )]
    pub output: Option<PathBuf>,
    /// Log file (defaults to stderr)
    #[arg(short = 'L', long = "log", value_name = "FILE")]
    pub log: Option<PathBuf>,
    /// Merge method: unique, percentile, cluster, adjacency, directional
    #[arg(long = "method", default_value = "directional")]
    pub method: String,
    /// Maximum Hamming distance between UMIs in a cluster
    #[arg(long = "edit-distance-threshold", default_value_t = 1)]
    pub threshold: u32,
    /// Where the UMI comes from: read_id, tag, umis
    #[arg(long = "extract-umi-method", default_value = "read_id")]
    pub extract_umi_method: String,
    /// UMI tag when --extract-umi-method=tag
    #[arg(long = "umi-tag", default_value = "RX")]
    pub umi_tag: String,
    /// Separator for the UMI in the read name
    #[arg(long = "umi-separator", default_value = "_")]
    pub umi_separator: String,
    /// Split the UMI tag on this string and keep the first part
    #[arg(long = "umi-tag-split")]
    pub umi_tag_split: Option<String>,
    /// Remove this string from the UMI tag
    #[arg(long = "umi-tag-delim")]
    pub umi_tag_delim: Option<String>,
    /// Cell barcode tag (used with --per-cell)
    #[arg(long = "cell-tag", default_value = "CB")]
    pub cell_tag: String,
    /// Group by cell barcode as well as UMI
    #[arg(long = "per-cell")]
    pub per_cell: bool,
    /// Seed for the representative read draw
    #[arg(long = "random-seed", default_value_t = 0)]
    pub random_seed: i64,
    /// Representative read strategy: upstream, stable
    #[arg(long = "representative", default_value = "upstream")]
    pub representative: String,
    /// Soft clip length above which a read counts as spliced
    #[arg(long = "soft-clip-threshold", default_value_t = 4)]
    pub soft_clip_threshold: i64,
    /// Treat spliced reads as unique
    #[arg(long = "spliced-is-unique")]
    pub spliced_is_unique: bool,
    /// Ignore the template length when grouping
    #[arg(long = "ignore-tlen")]
    pub ignore_tlen: bool,
    /// Include the read length in the bundle key
    #[arg(long = "read-length")]
    pub read_length: bool,
    /// Discard reads below this MAPQ
    #[arg(long = "mapping-quality")]
    pub mapping_quality: Option<u8>,
    /// Multimapping detection tag: NH, X0 or XT
    #[arg(long = "multimapping-detection-method")]
    pub detection_method: Option<String>,
    /// Append a @PG provenance record (changes the output bytes)
    #[arg(long = "add-pg")]
    pub add_pg: bool,
    /// Worker threads
    #[arg(long = "threads")]
    pub threads: Option<usize>,
    /// Memory budget, e.g. 4GiB
    #[arg(long = "memory-limit")]
    pub memory_limit: Option<String>,
    /// Directory for temporary files
    #[arg(long = "tmpdir")]
    pub tmpdir: Option<PathBuf>,
    /// Suppress informational output; errors are still printed
    #[arg(long = "quiet")]
    pub quiet: bool,
    /// Progress reporting: auto (terminal only), on, off
    #[arg(long = "progress", default_value = "auto", value_name = "MODE")]
    pub progress: String,
    /// Write a machine readable run summary to this file
    #[arg(long = "summary-json", value_name = "FILE")]
    pub summary_json: Option<PathBuf>,
    /// Sort strategy: auto, low-memory, high-throughput
    #[arg(long = "sort-strategy", default_value = "auto", value_name = "MODE")]
    pub sort_strategy: String,
    /// Reference FASTA for CRAM input or output
    #[arg(long = "reference", value_name = "FASTA")]
    pub reference: Option<PathBuf>,
    /// BAM compression level (0-9; 0 stores uncompressed blocks)
    #[arg(long = "compresslevel", default_value_t = 1)]
    pub compresslevel: u32,
}

#[derive(Args, Debug, Clone)]
#[command(after_help = DEDUP_HELP)]
pub struct DedupArgs {
    #[command(flatten)]
    pub common: CommonArgs,
    /// Keep read pairs together
    #[arg(long = "paired")]
    pub paired: bool,
    /// Write the output in processing order instead of coordinate order
    #[arg(long = "no-sort-output")]
    pub no_sort_output: bool,
    /// Write per run statistics into this prefix (rejected: null model not implemented)
    #[arg(long = "output-stats")]
    pub output_stats: Option<String>,
    /// Do not cluster: emit the best read of every UMI
    #[arg(long = "ignore-umi", hide = true)]
    pub ignore_umi: bool,
    /// What to do with read1s whose mate is not in the BAM: use, discard, output
    #[arg(long = "unpaired-reads", default_value = "use")]
    pub unpaired_reads: String,
    /// What to do when the mate is unmapped: discard, output, use
    #[arg(long = "unmapped-reads", default_value = "discard")]
    pub unmapped_reads: String,
    /// What to do with chimeric pairs: use, discard, output
    #[arg(long = "chimeric-pairs", default_value = "use")]
    pub chimeric_pairs: String,
    /// Group by gene instead of by position (requires --gene-tag)
    #[arg(long = "per-gene")]
    pub per_gene: bool,
    /// Gene tag when --per-gene is used
    #[arg(long = "gene-tag")]
    pub gene_tag: Option<String>,
    /// Buffer a whole contig before clustering (upstream compatibility)
    #[arg(long = "buffer-whole-contig")]
    pub whole_contig: bool,
    /// Restrict processing to one contig
    #[arg(long = "chrom")]
    pub chrom: Option<String>,
    /// Keep each read with this probability (0 < p <= 1)
    #[arg(long = "subset")]
    pub subset: Option<f64>,
    /// Discard clusters whose top UMI is not in --umi-whitelist
    #[arg(long = "filter-umi")]
    pub filter_umi: bool,
    /// Accepted UMIs, one per line (first column); required by --filter-umi
    #[arg(long = "umi-whitelist", value_name = "FILE")]
    pub umi_whitelist: Option<PathBuf>,
    /// Read2 whitelist; combined with --umi-whitelist as a cartesian product
    #[arg(long = "umi-whitelist-paired", value_name = "FILE")]
    pub umi_whitelist_paired: Option<PathBuf>,
    /// Gene to transcript map (count --per-contig only; dedup rejects it)
    #[arg(long = "gene-transcript-map")]
    pub gene_transcript_map: Option<PathBuf>,
}

#[derive(Args, Debug, Clone)]
#[command(after_help = GROUP_HELP)]
pub struct GroupArgs {
    #[command(flatten)]
    pub common: CommonArgs,
    /// Write the read groups table here
    #[arg(long = "group-out")]
    pub group_out: Option<PathBuf>,
    /// Group by gene (requires gene-tagged input)
    #[arg(long = "per-gene")]
    pub per_gene: bool,
    /// Gene tag
    #[arg(long = "gene-tag")]
    pub gene_tag: Option<String>,
    /// Tag holding the corrected UMI for the group
    #[arg(long = "umi-group-tag", default_value = "BX")]
    pub umi_group_tag: String,
    /// Accepted for UMI-tools compatibility (a BAM is written whenever -S is given)
    #[arg(long = "output-bam")]
    pub output_bam: bool,
    /// Write the output in processing order
    #[arg(long = "no-sort-output")]
    pub no_sort_output: bool,
    /// Buffer a whole contig before grouping
    #[arg(long = "buffer-whole-contig", hide = true)]
    pub whole_contig: bool,
}

#[derive(Args, Debug, Clone)]
#[command(after_help = COUNT_HELP)]
pub struct CountArgs {
    #[command(flatten)]
    pub common: CommonArgs,
    /// Count per gene using this tag
    #[arg(long = "per-gene")]
    pub per_gene: bool,
    /// Gene tag
    #[arg(long = "gene-tag")]
    pub gene_tag: Option<String>,
    /// Tag marking assignment status
    #[arg(long = "assigned-status-tag")]
    pub assigned_status_tag: Option<String>,
    /// Gene partitions for the bounded spill path (0 = automatic, 1 = disable)
    #[arg(long = "partition-genes", default_value_t = 0)]
    pub partition_genes: usize,
    /// Skip genes matching this regular expression
    #[arg(long = "skip-regex")]
    pub skip_regex: Option<String>,
    /// Treat each contig as a gene
    #[arg(long = "per-contig")]
    pub per_contig: bool,
    /// Tab separated gene/transcript map (requires --per-contig)
    #[arg(long = "gene-transcript-map", value_name = "FILE")]
    pub gene_transcript_map: Option<PathBuf>,
    /// Output one line per (gene, cell) instead of a wide matrix
    #[arg(long = "wide")]
    pub wide: bool,
}

#[derive(Args, Debug, Clone)]
#[command(after_help = COUNT_TAB_HELP)]
pub struct CountTabArgs {
    /// Two column table: read_id<sep>UMI and gene, sorted by gene
    #[arg(short = 'I', long = "input", value_name = "FILE")]
    pub input: PathBuf,
    /// Output table (defaults to stdout)
    #[arg(short = 'S', long = "output", value_name = "FILE")]
    pub output: Option<PathBuf>,
    /// Log file
    #[arg(short = 'L', long = "log", value_name = "FILE")]
    pub log: Option<PathBuf>,
    /// Separator between read id and UMI
    #[arg(long = "barcode-separator", default_value = "_")]
    pub barcode_separator: String,
    /// Read names also carry a cell barcode
    #[arg(long = "per-cell")]
    pub per_cell: bool,
    /// Worker threads
    #[arg(long = "threads")]
    pub threads: Option<usize>,
    /// Memory budget, e.g. 4GiB
    #[arg(long = "memory-limit")]
    pub memory_limit: Option<String>,
    /// Directory for temporary files
    #[arg(long = "tmpdir")]
    pub tmpdir: Option<PathBuf>,
    /// Suppress informational output; errors are still printed
    #[arg(long = "quiet")]
    pub quiet: bool,
    /// Write a machine readable run summary to this file
    #[arg(long = "summary-json", value_name = "FILE")]
    pub summary_json: Option<PathBuf>,
}

#[derive(Args, Debug, Clone)]
#[command(after_help = EXTRACT_HELP)]
pub struct ExtractArgs {
    /// Input FASTQ (gzip by .gz suffix; omit or - for stdin)
    #[arg(
        short = 'I',
        long = "stdin",
        visible_alias = "input",
        value_name = "FILE"
    )]
    pub input: Option<PathBuf>,
    /// Output FASTQ (omit or - for stdout)
    #[arg(
        short = 'S',
        long = "stdout",
        visible_alias = "output",
        value_name = "FILE"
    )]
    pub output: Option<PathBuf>,
    /// Paired read 2 input
    #[arg(long = "read2-in", value_name = "FILE")]
    pub read2_in: Option<PathBuf>,
    /// Paired read 2 output
    #[arg(long = "read2-out", value_name = "FILE")]
    pub read2_out: Option<PathBuf>,
    /// Barcode pattern; N marks a UMI base
    #[arg(short = 'p', long = "bc-pattern", value_name = "PATTERN")]
    pub bc_pattern: Option<String>,
    /// Barcode pattern for read 2
    #[arg(long = "bc-pattern2", value_name = "PATTERN")]
    pub bc_pattern2: Option<String>,
    /// Extraction method: string
    #[arg(
        long = "extract-method",
        default_value = "string",
        value_name = "METHOD"
    )]
    pub extract_method: String,
    /// Barcode is on the 3' end of the read
    #[arg(long = "3prime")]
    pub three_prime: bool,
    /// Discard reads with a UMI base quality below this value
    #[arg(long = "quality-filter-threshold")]
    pub quality_filter_threshold: Option<u8>,
    /// Mask UMI bases below this quality with N
    #[arg(long = "quality-filter-mask")]
    pub quality_filter_mask: Option<u8>,
    /// Quality encoding: phred33 (other encodings are rejected)
    #[arg(
        long = "quality-encoding",
        default_value = "phred33",
        value_name = "ENCODING"
    )]
    pub quality_encoding: String,
    /// Separator between the read name and the UMI
    #[arg(long = "umi-separator", default_value = "_")]
    pub umi_separator: String,
    /// Suppress informational output
    #[arg(long = "quiet")]
    pub quiet: bool,
    /// Log file (defaults to stderr)
    #[arg(short = 'L', long = "log", value_name = "FILE")]
    pub log: Option<PathBuf>,
}

#[derive(Args, Debug, Clone)]
#[command(after_help = WHITELIST_HELP)]
pub struct WhitelistArgs {
    /// Input FASTQ (gzip by .gz; omit or - for stdin)
    #[arg(
        short = 'I',
        long = "stdin",
        visible_alias = "input",
        value_name = "FILE"
    )]
    pub input: Option<PathBuf>,
    /// Whitelist TSV output (omit or - for stdout)
    #[arg(
        short = 'S',
        long = "stdout",
        visible_alias = "output",
        value_name = "FILE"
    )]
    pub output: Option<PathBuf>,
    /// Barcode pattern; N marks UMI bases, C marks cell bases
    #[arg(short = 'p', long = "bc-pattern", value_name = "PATTERN")]
    pub bc_pattern: Option<String>,
    /// Paired read 2 input (not implemented)
    #[arg(long = "read2-in", value_name = "FILE")]
    pub read2_in: Option<PathBuf>,
    /// Count method: reads
    #[arg(long = "method", default_value = "reads", value_name = "METHOD")]
    pub method: String,
    /// Knee method: distance or density
    #[arg(
        long = "knee-method",
        default_value = "distance",
        value_name = "METHOD"
    )]
    pub knee_method: String,
    /// Prior expectation on the number of cells
    #[arg(long = "expect-cells")]
    pub expect_cells: Option<usize>,
    /// Accept exactly this many barcodes
    #[arg(long = "set-cell-number")]
    pub set_cell_number: Option<usize>,
    /// Hamming distance for correction
    #[arg(long = "error-correct-threshold", default_value_t = 1)]
    pub error_correct_threshold: u32,
    /// Correct barcodes above the knee: discard or correct
    #[arg(long = "ed-above-threshold", value_name = "MODE")]
    pub ed_above_threshold: Option<String>,
    /// Use only the first N reads
    #[arg(long = "subset-reads", default_value_t = 100_000_000)]
    pub subset_reads: u64,
    /// Suppress informational output
    #[arg(long = "quiet")]
    pub quiet: bool,
    /// Log file (defaults to stderr)
    #[arg(short = 'L', long = "log", value_name = "FILE")]
    pub log: Option<PathBuf>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Policy {
    Discard,
    Output,
    Use,
}

impl Policy {
    fn parse(raw: &str, option: &str) -> Result<Policy> {
        match raw {
            "discard" => Ok(Policy::Discard),
            "output" => Ok(Policy::Output),
            "use" => Ok(Policy::Use),
            other => Err(Error::usage_with_hint(
                format!("unknown value '{other}' for {option}"),
                "choose one of: discard, output, use",
            )),
        }
    }
}

#[derive(Clone, Debug)]
pub struct DedupConfig {
    pub input: PathBuf,
    pub output: PathBuf,
    pub log: Option<PathBuf>,
    pub method: Method,
    pub threshold: u32,
    pub source: UmiSource,
    pub ignore_umi: bool,
    pub strategy: RepresentativeStrategy,
    pub random_seed: i64,
    pub paired: bool,
    pub no_sort_output: bool,
    pub soft_clip_threshold: i64,
    pub spliced: bool,
    pub ignore_tlen: bool,
    pub read_length: bool,
    pub mapping_quality: Option<u8>,
    pub detection: Detection,
    pub unpaired_reads: Policy,
    pub unmapped_reads: Policy,
    pub chimeric_pairs: Policy,
    pub output_stats: Option<String>,
    pub whole_contig: bool,
    pub compresslevel: u32,
    pub umi_whitelist: Option<std::collections::HashSet<Vec<u8>>>,
    pub add_pg: bool,
    pub quiet: bool,
    pub progress: String,
    pub summary_json: Option<PathBuf>,
    pub reference: Option<PathBuf>,
    pub chrom: Option<String>,
    pub per_gene: bool,
    pub gene_tag: Option<[u8; 2]>,
    pub per_cell: bool,
    pub cell_tag: [u8; 2],
    pub subset: Option<f64>,
    pub budget: Budget,
}

pub fn parse_tag(name: &str) -> Result<[u8; 2]> {
    let bytes = name.as_bytes();
    if bytes.len() != 2 {
        return Err(Error::usage(format!(
            "tag '{name}' must have exactly two characters"
        )));
    }
    Ok([bytes[0], bytes[1]])
}

impl CommonArgs {
    pub fn source(&self) -> Result<UmiSource> {
        build_source(self)
    }

    pub fn budget(&self) -> Result<Budget> {
        let mut budget = Budget::resolve(
            self.threads,
            self.memory_limit.as_deref(),
            self.tmpdir.as_deref(),
        )?;
        budget.apply_sort_strategy(&self.sort_strategy)?;
        Ok(budget)
    }

    pub fn detection(&self) -> Result<Detection> {
        match &self.detection_method {
            Some(raw) => Detection::parse(raw),
            None => Ok(Detection::None),
        }
    }

    pub fn bundle_config(
        &self,
        storage: crate::core::bundle::Storage,
    ) -> Result<crate::core::bundle::BundleConfig> {
        let source = self.source()?;
        let skip_missing_umi = matches!(source, UmiSource::Tag { .. });
        Ok(crate::core::bundle::BundleConfig {
            spliced: self.spliced_is_unique,
            ignore_tlen: self.ignore_tlen,
            paired: false,
            read_length: self.read_length,
            per_cell: self.per_cell,
            source,
            cell_tag: parse_tag(&self.cell_tag)?,
            cell_tag_split: Some("-".to_string()),
            cell_tag_delim: None,
            soft_clip_threshold: self.soft_clip_threshold,
            detection: self.detection()?,
            storage,
            whole_contig: false,
            skip_missing_umi,
        })
    }
}

fn build_source(common: &CommonArgs) -> Result<UmiSource> {
    let tag = parse_tag;
    match common.extract_umi_method.as_str() {
        "read_id" => {
            let bytes = common.umi_separator.as_bytes();
            if bytes.len() != 1 {
                return Err(Error::usage_with_hint(
                    format!(
                        "--umi-separator '{}' must be a single character",
                        common.umi_separator
                    ),
                    "for example: --umi-separator _",
                ));
            }
            Ok(UmiSource::ReadId { sep: bytes[0] })
        }
        "tag" => Ok(UmiSource::Tag {
            umi_tag: tag(&common.umi_tag)?,
            umi_tag_split: common.umi_tag_split.clone(),
            umi_tag_delim: common.umi_tag_delim.clone(),
        }),
        "umis" => Ok(UmiSource::Umis),
        other => Err(Error::usage_with_hint(
            format!("unknown --extract-umi-method '{other}'"),
            "choose one of: read_id, tag, umis",
        )),
    }
}

fn reject(option: &str, detail: &str) -> Error {
    Error::unsupported(option, detail)
}

fn load_whitelist(path: &Path) -> Result<Vec<Vec<u8>>> {
    let text = std::fs::read_to_string(path).map_err(|e| Error::io(path, e))?;
    let mut umis = Vec::new();
    for line in text.lines() {
        if line.starts_with('#') {
            continue;
        }
        let first = line.trim_end_matches('\r').split('\t').next().unwrap_or("");
        umis.push(first.as_bytes().to_vec());
    }
    Ok(umis)
}

impl DedupConfig {
    fn validate_unsupported(args: &DedupArgs) -> Result<()> {
        if args.common.compresslevel > 9 {
            return Err(Error::usage_with_hint(
                "--compresslevel must be between 0 and 9",
                "0 stores uncompressed BGZF blocks; the default is 1",
            ));
        }
        if args.output_stats.is_some() {
            return Err(reject(
                "--output-stats",
                "the UMI-tools null model is not implemented, so this build will not emit \
                 statistics whose null columns would only repeat the observed values; drop the \
                 option and see docs/ROADMAP.md (R0-2)",
            ));
        }
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
        if args.filter_umi && args.umi_whitelist.is_none() {
            return Err(Error::usage_with_hint(
                "--filter-umi needs --umi-whitelist",
                "supply a file with one accepted UMI in the first column of each line",
            ));
        }
        if args.gene_transcript_map.is_some() {
            return Err(reject(
                "--gene-transcript-map",
                "this map is only supported by count --per-contig; dedup has no --per-contig",
            ));
        }
        if let Some(probability) = args.subset {
            if !(probability > 0.0 && probability <= 1.0) {
                return Err(Error::usage_with_hint(
                    "--subset must be in (0, 1]",
                    "for example: --subset 0.1 keeps about ten percent of the reads",
                ));
            }
        }
        if args.common.per_cell && !args.per_gene {
            return Err(Error::usage_with_hint(
                "--per-cell requires --per-gene",
                "cell aware deduplication keys bundles by gene and cell",
            ));
        }
        Ok(())
    }

    pub fn from_args(args: &DedupArgs, input: PathBuf, output: PathBuf) -> Result<DedupConfig> {
        DedupConfig::validate_unsupported(args)?;
        let common = &args.common;
        let source = build_source(common)?;
        let method = Method::parse(&common.method)?;
        let strategy = RepresentativeStrategy::parse(&common.representative)?;
        let detection = match &common.detection_method {
            Some(raw) => Detection::parse(raw)?,
            // Upstream leaves the multimapping comparison off unless the user asks.
            None => Detection::None,
        };
        let budget = common.budget()?;
        let umi_whitelist = if args.filter_umi {
            let first = load_whitelist(args.umi_whitelist.as_ref().expect("validated above"))?;
            let set = if let Some(second) = &args.umi_whitelist_paired {
                let second = load_whitelist(second)?;
                let mut combined = std::collections::HashSet::new();
                for left in &first {
                    for right in &second {
                        let mut umi = left.clone();
                        umi.extend_from_slice(right);
                        combined.insert(umi);
                    }
                }
                combined
            } else {
                first.into_iter().collect()
            };
            Some(set)
        } else {
            None
        };
        Ok(DedupConfig {
            input,
            output,
            log: common.log.clone(),
            method,
            threshold: common.threshold,
            source,
            ignore_umi: args.ignore_umi,
            strategy,
            random_seed: common.random_seed,
            paired: args.paired,
            no_sort_output: args.no_sort_output,
            soft_clip_threshold: common.soft_clip_threshold,
            spliced: common.spliced_is_unique,
            ignore_tlen: common.ignore_tlen,
            read_length: common.read_length,
            mapping_quality: common.mapping_quality,
            detection,
            unpaired_reads: Policy::parse(&args.unpaired_reads, "--unpaired-reads")?,
            unmapped_reads: Policy::parse(&args.unmapped_reads, "--unmapped-reads")?,
            chimeric_pairs: Policy::parse(&args.chimeric_pairs, "--chimeric-pairs")?,
            output_stats: args.output_stats.clone(),
            whole_contig: args.whole_contig,
            compresslevel: common.compresslevel,
            umi_whitelist,
            add_pg: common.add_pg,
            quiet: common.quiet,
            progress: common.progress.clone(),
            summary_json: common.summary_json.clone(),
            reference: common.reference.clone(),
            chrom: args.chrom.clone(),
            per_gene: args.per_gene,
            gene_tag: match &args.gene_tag {
                Some(name) => Some(parse_tag(name)?),
                None => None,
            },
            per_cell: common.per_cell,
            cell_tag: parse_tag(&common.cell_tag)?,
            subset: args.subset,
            budget,
        })
    }

    /// Default configuration for the legacy positional invocation.
    pub fn legacy(input: PathBuf, output: PathBuf, threads: Option<usize>) -> Result<DedupConfig> {
        Ok(DedupConfig {
            input,
            output,
            log: None,
            method: Method::Directional,
            threshold: 1,
            source: UmiSource::ReadId { sep: b'_' },
            ignore_umi: false,
            strategy: RepresentativeStrategy::Upstream,
            random_seed: 0,
            paired: false,
            no_sort_output: false,
            soft_clip_threshold: 4,
            spliced: false,
            ignore_tlen: false,
            read_length: false,
            mapping_quality: None,
            detection: Detection::None,
            unpaired_reads: Policy::Use,
            unmapped_reads: Policy::Discard,
            chimeric_pairs: Policy::Use,
            output_stats: None,
            whole_contig: false,
            compresslevel: 6,
            umi_whitelist: None,
            add_pg: false,
            quiet: false,
            progress: "auto".to_string(),
            summary_json: None,
            reference: None,
            chrom: None,
            per_gene: false,
            gene_tag: None,
            per_cell: false,
            cell_tag: *b"CB",
            subset: None,
            budget: Budget::resolve(threads, None, None)?,
        })
    }
}

pub enum Invocation {
    Dedup(Box<DedupConfig>),
    Extract(Box<ExtractArgs>),
    Whitelist(Box<WhitelistArgs>),
    Group(Box<GroupArgs>),
    Count(Box<CountArgs>),
    CountTab(Box<CountTabArgs>),
}

pub fn parse() -> Result<Invocation> {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(e) => {
            if matches!(
                e.kind(),
                clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion
            ) {
                print!("{e}");
                std::process::exit(bioinfo_common::exit::ExitClass::Success.code());
            }
            return Err(Error::usage(e.to_string().trim_end().to_string()));
        }
    };
    match cli.command {
        Some(Command::Dedup(args)) => {
            let input = args.common.input.clone();
            let output = args
                .common
                .output
                .clone()
                .ok_or_else(|| Error::usage_with_hint("dedup needs -S/--stdout", "example: dedup -I in.bam -S out.bam"))?;
            let config = DedupConfig::from_args(&args, input, output)?;
            Ok(Invocation::Dedup(Box::new(config)))
        }
        Some(Command::Group(args)) => {
            if args.common.output.is_none() && args.group_out.is_none() {
                return Err(Error::usage("group needs at least one of -S/--stdout or --group-out"));
            }
            Ok(Invocation::Group(Box::new(args)))
        }
        Some(Command::Count(args)) => {
            if args.common.output.is_none() {
                return Err(Error::usage("count needs -S/--stdout"));
            }
            Ok(Invocation::Count(Box::new(args)))
        }
        Some(Command::CountTab(args)) => Ok(Invocation::CountTab(Box::new(args))),
        Some(Command::Extract(args)) => Ok(Invocation::Extract(Box::new(args))),
        Some(Command::Whitelist(args)) => Ok(Invocation::Whitelist(Box::new(args))),
        None => match (cli.legacy_input, cli.legacy_output) {
            (Some(input), Some(output)) => {
                Ok(Invocation::Dedup(Box::new(DedupConfig::legacy(input, output, cli.legacy_threads)?)))
            }
            _ => Err(Error::usage_with_hint(
                "no command given",
                "usage: rust-umitools <in.bam> <out.bam> [threads]  or  rust-umitools dedup -I in.bam -S out.bam",
            )),
        },
    }
}
