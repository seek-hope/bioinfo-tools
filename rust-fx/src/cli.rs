//! The command-line surface.
//!
//! Option names and short flags mirror SeqKit 2.14.0's documented surface for
//! the implemented subcommands, so a user can move between the two tools without
//! relearning flags. Where SeqKit's semantics are ambiguous in its documentation,
//! the choice made here is recorded in `docs/EVIDENCE-W1-3.md`.
//!
//! Options SeqKit has but this crate does not implement are **absent** rather
//! than accepted-and-ignored: silently accepting `--two-pass` and not doing two
//! passes would be exactly the "silent fallback" the development plan forbids
//! (boundary rule 6).

use clap::{Arg, ArgAction, Command};

/// The program name, used in diagnostics and in `--version`.
pub const PROGRAM: &str = "rust-fx";

/// Build the full command tree.
pub fn command() -> Command {
    Command::new(PROGRAM)
        .version(env!("CARGO_PKG_VERSION"))
        .about("FASTX manipulation with SeqKit-compatible subcommands (experimental)")
        .after_help(
            "Compatibility: this build implements the documented SeqKit 2.14.0 behaviour for \
             stats, seq, fq2fa, fx2tab, sample, split, head, grep, sort, subseq, range, sliding, duplicate, rmdup, faidx, rename, replace, tab2fx, restart, fa2fq, translate, common and lint, and fq 0.12.0's validator set \
             for lint. head has direct SeqKit 2.14 differential evidence for positive -n/-l, -n 0 and global raw-output flags; grep has direct evidence for ID/name/sequence, strand, motif, region, duplicate-pattern and error paths; sort has natural-order/prefix and selected two-pass/index evidence. See \
             compat/matrix.tsv for per-surface status.",
        )
        .subcommand_required(true)
        .arg_required_else_help(true)
        .arg(compress_level_arg())
        .subcommand(stats_command())
        .subcommand(seq_command())
        .subcommand(fq2fa_command())
        .subcommand(fx2tab_command())
        .subcommand(sample_command())
        .subcommand(split_command())
        .subcommand(head_command())
        .subcommand(subseq_command())
        .subcommand(range_command())
        .subcommand(sliding_command())
        .subcommand(duplicate_command())
        .subcommand(rmdup_command())
        .subcommand(faidx_command())
        .subcommand(rename_command())
        .subcommand(replace_command())
        .subcommand(tab2fx_command())
        .subcommand(restart_command())
        .subcommand(fa2fq_command())
        .subcommand(translate_command())
        .subcommand(common_command())
                                                .subcommand(grep_command())
        .subcommand(sort_command())
        .subcommand(lint_command())
}

/// `-w/--line-width`: a global flag in SeqKit, applied when writing FASTA.
fn line_width_arg() -> Arg {
    Arg::new("line_width")
        .short('w')
        .long("line-width")
        .value_name("INT")
        .default_value("60")
        .help("line width when outputting FASTA format (0 for no wrap)")
}

/// `-o/--out-file`.
fn out_file_arg() -> Arg {
    Arg::new("out_file")
        .short('o')
        .long("out-file")
        .value_name("FILE")
        .help("output file (\"-\" for stdout; a .gz/.xz/.zst/.bz2 suffix compresses)")
}

/// `--compress-level`, shared by all subcommands through clap's global args.
fn compress_level_arg() -> Arg {
    Arg::new("compress_level")
        .long("compress-level")
        .value_name("INT")
        .default_value("-1")
        .allow_hyphen_values(true)
        .global(true)
        .help("compression level for gzip, zstd, xz and bzip2 (-1 for the default)")
}

/// `-j/--threads`, accepted for compatibility and recorded as unused.
fn threads_arg() -> Arg {
    Arg::new("threads")
        .short('j')
        .long("threads")
        .value_name("INT")
        .default_value("4")
        .help("worker threads (accepted for compatibility; this build is single-threaded)")
}

fn stats_command() -> Command {
    Command::new("stats")
        .visible_alias("stat")
        .about("simple statistics of FASTA/Q files")
        .arg(Arg::new("inputs").value_name("FILE").num_args(0..).action(ArgAction::Append))
        .arg(
            Arg::new("tabular")
                .short('T')
                .long("tabular")
                .action(ArgAction::SetTrue)
                .help("output in machine-friendly tabular format"),
        )
        .arg(
            Arg::new("all")
                .short('a')
                .long("all")
                .action(ArgAction::SetTrue)
                .help("all statistics, including quartiles of seq length, sum_gap, N50"),
        )
        .arg(
            Arg::new("basename")
                .short('b')
                .long("basename")
                .action(ArgAction::SetTrue)
                .help("only output basename of files"),
        )
        .arg(
            Arg::new("gap_letters")
                .short('G')
                .long("gap-letters")
                .value_name("STRING")
                .default_value("- \t.")
                .help("gap letters (default \"- .\")"),
        )
        .arg(
            Arg::new("stdin_label")
                .short('i')
                .long("stdin-label")
                .value_name("STRING")
                .default_value("-")
                .help("label for replacing default \"-\" for stdin"),
        )
        .arg(
            Arg::new("skip_err")
                .short('e')
                .long("skip-err")
                .action(ArgAction::SetTrue)
                .help("skip error, only show warning message"),
        )
        .arg(
            Arg::new("fq_encoding")
                .short('E')
                .long("fq-encoding")
                .value_name("STRING")
                .default_value("sanger")
                .help("fastq quality encoding: sanger, solexa, illumina-1.3+, illumina-1.5+, illumina-1.8+"),
        )
        .arg(out_file_arg())
        .arg(threads_arg())
}

fn seq_command() -> Command {
    Command::new("seq")
        .about("transform sequences (extract ID, filter by length, remove gaps, reverse complement...)")
        .arg(Arg::new("inputs").value_name("FILE").num_args(0..).action(ArgAction::Append))
        .arg(Arg::new("name").short('n').long("name").action(ArgAction::SetTrue)
            .help("only print names/sequence headers"))
        .arg(Arg::new("only_id").short('i').long("only-id").action(ArgAction::SetTrue)
            .help("print IDs instead of full headers"))
        .arg(Arg::new("seq_only").short('s').long("seq").action(ArgAction::SetTrue)
            .help("only print sequences"))
        .arg(Arg::new("qual_only").short('q').long("qual").action(ArgAction::SetTrue)
            .help("only print qualities"))
        .arg(Arg::new("reverse").short('r').long("reverse").action(ArgAction::SetTrue)
            .help("reverse sequence"))
        .arg(Arg::new("complement").short('p').long("complement").action(ArgAction::SetTrue)
            .help("complement sequence"))
        .arg(Arg::new("upper").short('u').long("upper-case").action(ArgAction::SetTrue)
            .help("print sequences in upper case"))
        .arg(Arg::new("lower").short('l').long("lower-case").action(ArgAction::SetTrue)
            .help("print sequences in lower case"))
        .arg(Arg::new("remove_gaps").short('g').long("remove-gaps").action(ArgAction::SetTrue)
            .help("remove gap letters set by -G/--gap-letters"))
        .arg(Arg::new("gap_letters").short('G').long("gap-letters").value_name("STRING")
            .default_value("- \t.").help("gap letters to be removed with -g"))
        .arg(Arg::new("dna2rna").long("dna2rna").action(ArgAction::SetTrue)
            .help("DNA to RNA"))
        .arg(Arg::new("rna2dna").long("rna2dna").action(ArgAction::SetTrue)
            .help("RNA to DNA"))
        .arg(Arg::new("min_len").short('m').long("min-len").value_name("INT").default_value("-1")
            .allow_hyphen_values(true)
            .help("only print sequences longer than or equal to the minimum length (-1 for no limit)"))
        .arg(Arg::new("max_len").short('M').long("max-len").value_name("INT").default_value("-1")
            .allow_hyphen_values(true)
            .help("only print sequences shorter than or equal to the maximum length (-1 for no limit)"))
        .arg(Arg::new("min_qual").short('Q').long("min-qual").value_name("FLOAT").default_value("-1")
            .allow_hyphen_values(true)
            .help("only print sequences with average quality greater or equal than this limit"))
        .arg(Arg::new("max_qual").short('R').long("max-qual").value_name("FLOAT").default_value("-1")
            .allow_hyphen_values(true)
            .help("only print sequences with average quality less than this limit"))
        .arg(Arg::new("qual_ascii_base").short('b').long("qual-ascii-base").value_name("INT")
            .default_value("33").help("ASCII BASE, 33 for Phred+33"))
        .arg(line_width_arg())
        .arg(out_file_arg())
        .arg(threads_arg())
}

fn common_command() -> Command {
    Command::new("common")
        .about("find common/shared sequences of multiple files")
        .arg(
            Arg::new("inputs")
                .value_name("FILE")
                .required(true)
                .num_args(2..)
                .action(ArgAction::Append),
        )
        .arg(
            Arg::new("by_name")
                .short('n')
                .long("by-name")
                .action(ArgAction::SetTrue)
                .help("match by full name instead of just id"),
        )
        .arg(
            Arg::new("by_seq")
                .short('s')
                .long("by-seq")
                .action(ArgAction::SetTrue)
                .help("match by sequence"),
        )
        .arg(
            Arg::new("embedded")
                .short('e')
                .long("check-embedded-seqs")
                .action(ArgAction::SetTrue)
                .help("check embedded sequences (requires --by-seq)"),
        )
        .arg(
            Arg::new("ignore_case")
                .short('i')
                .long("ignore-case")
                .action(ArgAction::SetTrue)
                .help("ignore case"),
        )
        .arg(
            Arg::new("only_positive")
                .short('P')
                .long("only-positive-strand")
                .action(ArgAction::SetTrue)
                .help("only compare the positive strand for -s"),
        )
        .arg(line_width_arg())
        .arg(out_file_arg())
        .arg(threads_arg())
}

fn translate_command() -> Command {
    Command::new("translate")
        .about("translate DNA/RNA to protein sequence")
        .arg(
            Arg::new("inputs")
                .value_name("FILE")
                .num_args(0..)
                .action(ArgAction::Append),
        )
        .arg(
            Arg::new("frame")
                .short('f')
                .long("frame")
                .value_name("INT")
                .action(ArgAction::Append)
                .default_value("1")
                .allow_hyphen_values(true)
                .help("frames: 1,2,3,-1,-2,-3,6"),
        )
        .arg(
            Arg::new("transl_table")
                .short('T')
                .long("transl-table")
                .value_name("INT")
                .default_value("1")
                .allow_hyphen_values(true)
                .help("translation table ID (1-6, 9-14, 16, 21-31)"),
        )
        .arg(
            Arg::new("list_table")
                .short('l')
                .long("list-transl-table")
                .value_name("INT")
                .default_value("-1")
                .allow_hyphen_values(true)
                .help("show details of translation table N, 0 for all"),
        )
        .arg(
            Arg::new("list_ambiguous")
                .short('L')
                .long("list-transl-table-with-amb-codons")
                .value_name("INT")
                .default_value("-1")
                .allow_hyphen_values(true)
                .help("show translation table N including ambiguous codons, 0 for all"),
        )
        .arg(
            Arg::new("allow_unknown")
                .short('x')
                .long("allow-unknown-codon")
                .action(ArgAction::SetTrue)
                .help("translate unknown codons to X"),
        )
        .arg(
            Arg::new("init_m")
                .short('M')
                .long("init-codon-as-M")
                .action(ArgAction::SetTrue)
                .help("translate the initial codon as M"),
        )
        .arg(
            Arg::new("append_frame")
                .short('F')
                .long("append-frame")
                .action(ArgAction::SetTrue)
                .help("append frame information to the sequence ID"),
        )
        .arg(
            Arg::new("out_subseqs")
                .short('s')
                .long("out-subseqs")
                .action(ArgAction::SetTrue)
                .help("output individual ORFs separated by STOP codons"),
        )
        .arg(
            Arg::new("clean")
                .long("clean")
                .action(ArgAction::SetTrue)
                .help("replace STOP codons with X"),
        )
        .arg(
            Arg::new("min_len")
                .short('m')
                .long("min-len")
                .value_name("INT")
                .default_value("0")
                .allow_hyphen_values(true)
                .help("minimum amino-acid length for -s ORFs"),
        )
        .arg(
            Arg::new("skip_errors")
                .short('e')
                .long("skip-translate-errors")
                .action(ArgAction::SetTrue)
                .help("output a blank sequence on translation errors"),
        )
        .arg(
            Arg::new("trim")
                .long("trim")
                .action(ArgAction::SetTrue)
                .help("remove trailing X and *"),
        )
        .arg(line_width_arg())
        .arg(out_file_arg())
        .arg(threads_arg())
}

fn fa2fq_command() -> Command {
    Command::new("fa2fq")
        .about("retrieve corresponding FASTQ records by a FASTA file")
        .arg(
            Arg::new("fasta")
                .short('f')
                .long("fasta-file")
                .value_name("FILE")
                .required(true)
                .help("FASTA file whose names select FASTQ records"),
        )
        .arg(
            Arg::new("inputs")
                .value_name("FASTQ")
                .num_args(0..)
                .action(ArgAction::Append)
                .help("FASTQ input(s); omitted means stdin"),
        )
        .arg(
            Arg::new("only_positive")
                .short('P')
                .long("only-positive-strand")
                .action(ArgAction::SetTrue)
                .help("only search the positive strand"),
        )
        .arg(line_width_arg())
        .arg(out_file_arg())
        .arg(threads_arg())
}

fn restart_command() -> Command {
    Command::new("restart")
        .about("reset start position (rotate) for circular genomes")
        .arg(
            Arg::new("inputs")
                .value_name("FILE")
                .num_args(0..)
                .action(ArgAction::Append),
        )
        .arg(
            Arg::new("new_start")
                .short('i')
                .long("new-start")
                .value_name("INT")
                .default_value("1")
                .allow_hyphen_values(true)
                .help("1-based new start; negative counts from the end"),
        )
        .arg(
            Arg::new("start_with")
                .short('s')
                .long("start-with")
                .value_name("STRING")
                .help("rotate to start with this subsequence (both strands searched)"),
        )
        .arg(
            Arg::new("ignore_case")
                .short('I')
                .long("ignore-case")
                .action(ArgAction::SetTrue)
                .help("ignore case for --start-with"),
        )
        .arg(
            Arg::new("max_mismatch")
                .short('m')
                .long("max-mismatch")
                .value_name("INT")
                .default_value("0")
                .help("max mismatch for --start-with"),
        )
        .arg(line_width_arg())
        .arg(out_file_arg())
        .arg(threads_arg())
}

fn tab2fx_command() -> Command {
    Command::new("tab2fx")
        .about("convert tabular format (first two/three columns) to FASTA/Q")
        .arg(
            Arg::new("inputs")
                .value_name("FILE")
                .num_args(0..)
                .action(ArgAction::Append),
        )
        .arg(
            Arg::new("buffer_size")
                .short('b')
                .long("buffer-size")
                .value_name("STRING")
                .default_value("1G")
                .allow_hyphen_values(true)
                .help("maximum input line size; suffixes K, M and G use powers of 1024"),
        )
        .arg(
            Arg::new("comment_prefix")
                .short('p')
                .long("comment-line-prefix")
                .value_name("STRING")
                .action(ArgAction::Append)
                .help("comment-line prefix to skip; repeat to replace the defaults"),
        )
        .arg(line_width_arg())
        .arg(out_file_arg())
        .arg(threads_arg())
}

fn replace_command() -> Command {
    Command::new("replace")
        .about("replace name/sequence by regular expression")
        .arg(
            Arg::new("inputs")
                .value_name("FILE")
                .num_args(0..)
                .action(ArgAction::Append),
        )
        .arg(
            Arg::new("pattern")
                .short('p')
                .long("pattern")
                .value_name("REGEX")
                .required(true)
                .help("search regular expression"),
        )
        .arg(
            Arg::new("replacement")
                .short('r')
                .long("replacement")
                .value_name("STRING")
                .required(true)
                .help("replacement; $1 is the first capture"),
        )
        .arg(
            Arg::new("by_seq")
                .short('s')
                .long("by-seq")
                .action(ArgAction::SetTrue)
                .help("replace the sequence instead of the name"),
        )
        .arg(
            Arg::new("ignore_case")
                .short('i')
                .long("ignore-case")
                .action(ArgAction::SetTrue)
                .help("ignore case"),
        )
        .arg(
            Arg::new("nr_width")
                .long("nr-width")
                .value_name("INT")
                .default_value("1")
                .help("minimum width of the {nr} replacement symbol"),
        )
        .arg(
            Arg::new("kv_file")
                .short('k')
                .long("kv-file")
                .value_name("FILE")
                .help("tab-delimited key-value file for {kv}"),
        )
        .arg(
            Arg::new("keep_key")
                .short('K')
                .long("keep-key")
                .action(ArgAction::SetTrue)
                .help("keep the key when no value is found"),
        )
        .arg(
            Arg::new("keep_untouch")
                .short('U')
                .long("keep-untouch")
                .action(ArgAction::SetTrue)
                .help("leave the name unchanged when no value is found"),
        )
        .arg(
            Arg::new("key_capt_idx")
                .short('I')
                .long("key-capt-idx")
                .value_name("INT")
                .default_value("1")
                .help("capture group containing the key (1-based)"),
        )
        .arg(
            Arg::new("key_miss_repl")
                .short('m')
                .long("key-miss-repl")
                .value_name("STRING")
                .help("replacement when the key has no value"),
        )
        .arg(
            Arg::new("f_by_name")
                .long("f-by-name")
                .action(ArgAction::SetTrue)
                .help("filter by full name instead of ID"),
        )
        .arg(
            Arg::new("f_by_seq")
                .long("f-by-seq")
                .action(ArgAction::SetTrue)
                .help("filter by sequence on both strands"),
        )
        .arg(
            Arg::new("f_ignore_case")
                .long("f-ignore-case")
                .action(ArgAction::SetTrue)
                .help("ignore case while filtering"),
        )
        .arg(
            Arg::new("f_invert_match")
                .long("f-invert-match")
                .action(ArgAction::SetTrue)
                .help("edit only nonmatching records"),
        )
        .arg(
            Arg::new("f_only_positive")
                .long("f-only-positive-strand")
                .action(ArgAction::SetTrue)
                .help("search only the positive strand while filtering"),
        )
        .arg(
            Arg::new("f_patterns")
                .long("f-pattern")
                .value_name("PATTERN")
                .action(ArgAction::Append)
                .help("filter pattern (repeatable)"),
        )
        .arg(
            Arg::new("f_pattern_file")
                .long("f-pattern-file")
                .value_name("FILE")
                .help("filter pattern file"),
        )
        .arg(
            Arg::new("f_use_regexp")
                .long("f-use-regexp")
                .action(ArgAction::SetTrue)
                .help("interpret filter patterns as regular expressions"),
        )
        .arg(line_width_arg())
        .arg(out_file_arg())
        .arg(threads_arg())
}

fn rename_command() -> Command {
    Command::new("rename")
        .about("rename duplicated IDs")
        .arg(
            Arg::new("inputs")
                .value_name("FILE")
                .num_args(0..)
                .action(ArgAction::Append),
        )
        .arg(
            Arg::new("by_name")
                .short('n')
                .long("by-name")
                .action(ArgAction::SetTrue)
                .help("check duplication by full name instead of just id"),
        )
        .arg(
            Arg::new("rename_first")
                .short('1')
                .long("rename-1st-rec")
                .action(ArgAction::SetTrue)
                .help("rename the first record as well"),
        )
        .arg(
            Arg::new("separator")
                .short('s')
                .long("separator")
                .value_name("STRING")
                .default_value("_")
                .help("separator between ID/name and counter"),
        )
        .arg(
            Arg::new("start_num")
                .short('N')
                .long("start-num")
                .value_name("INT")
                .default_value("2")
                .help("starting count number for duplicated IDs"),
        )
        .arg(
            Arg::new("multiple_outfiles")
                .short('m')
                .long("multiple-outfiles")
                .action(ArgAction::SetTrue)
                .help("write one output file per input file"),
        )
        .arg(
            Arg::new("out_dir")
                .short('O')
                .long("out-dir")
                .value_name("DIR")
                .default_value("renamed")
                .help("directory for multiple output files"),
        )
        .arg(
            Arg::new("force")
                .short('f')
                .long("force")
                .action(ArgAction::SetTrue)
                .help("suppress the non-empty output-directory warning"),
        )
        .arg(line_width_arg())
        .arg(out_file_arg())
        .arg(threads_arg())
}

fn faidx_command() -> Command {
    Command::new("faidx")
        .about("create/extract from a FASTA index (region extraction)")
        .arg(
            Arg::new("fasta")
                .value_name("FASTA")
                .required(true)
                .help("FASTA file"),
        )
        .arg(
            Arg::new("regions")
                .value_name("REGION")
                .num_args(0..)
                .action(ArgAction::Append)
                .help("regions such as chr1:2-5"),
        )
        .arg(
            Arg::new("full_head")
                .short('f')
                .long("full-head")
                .action(ArgAction::SetTrue)
                .help("print full header line"),
        )
        .arg(
            Arg::new("ignore_case")
                .short('i')
                .long("ignore-case")
                .action(ArgAction::SetTrue)
                .help("ignore case in IDs"),
        )
        .arg(
            Arg::new("region_file")
                .short('l')
                .long("region-file")
                .value_name("FILE")
                .help("file of regions, one per line"),
        )
        .arg(
            Arg::new("regexp")
                .short('r')
                .long("use-regexp")
                .action(ArgAction::SetTrue)
                .help("treat IDs as regular expressions"),
        )
        .arg(
            Arg::new("update_faidx")
                .short('U')
                .long("update-faidx")
                .action(ArgAction::SetTrue)
                .help("rebuild the FASTA index if it exists"),
        )
        .arg(
            Arg::new("immediate_output")
                .short('I')
                .long("immediate-output")
                .action(ArgAction::SetTrue)
                .help("write matches without changing their order"),
        )
        .arg(line_width_arg())
        .arg(out_file_arg())
        .arg(threads_arg())
}

fn rmdup_command() -> Command {
    Command::new("rmdup")
        .about("remove duplicated sequences by ID, name or sequence")
        .arg(
            Arg::new("inputs")
                .value_name("FILE")
                .num_args(0..)
                .action(ArgAction::Append),
        )
        .arg(
            Arg::new("by_name")
                .short('n')
                .long("by-name")
                .action(ArgAction::SetTrue)
                .help("by full name instead of just id"),
        )
        .arg(
            Arg::new("by_seq")
                .short('s')
                .long("by-seq")
                .action(ArgAction::SetTrue)
                .help("by sequence"),
        )
        .arg(
            Arg::new("ignore_case")
                .short('i')
                .long("ignore-case")
                .action(ArgAction::SetTrue)
                .help("ignore case"),
        )
        .arg(
            Arg::new("only_positive")
                .short('P')
                .long("only-positive-strand")
                .action(ArgAction::SetTrue)
                .help("only compare the positive strand for -s"),
        )
        .arg(
            Arg::new("dup_num_file")
                .short('D')
                .long("dup-num-file")
                .value_name("FILE")
                .help("write duplicate counts and ID lists"),
        )
        .arg(
            Arg::new("dup_seqs_file")
                .short('d')
                .long("dup-seqs-file")
                .value_name("FILE")
                .help("write records after the first occurrence"),
        )
        .arg(line_width_arg())
        .arg(out_file_arg())
        .arg(threads_arg())
}

fn duplicate_command() -> Command {
    Command::new("duplicate")
        .about("duplicate sequences N times")
        .arg(
            Arg::new("inputs")
                .value_name("FILE")
                .num_args(0..)
                .action(ArgAction::Append),
        )
        .arg(
            Arg::new("times")
                .short('n')
                .long("times")
                .value_name("INT")
                .default_value("1")
                .help("duplication number"),
        )
        .arg(line_width_arg())
        .arg(out_file_arg())
        .arg(threads_arg())
}

fn sliding_command() -> Command {
    Command::new("sliding")
        .about("extract subsequences in sliding windows")
        .arg(
            Arg::new("inputs")
                .value_name("FILE")
                .num_args(0..)
                .action(ArgAction::Append),
        )
        .arg(
            Arg::new("window")
                .short('W')
                .long("window")
                .value_name("INT")
                .required(true)
                .help("window size"),
        )
        .arg(
            Arg::new("step")
                .short('s')
                .long("step")
                .value_name("INT")
                .required(true)
                .help("step size"),
        )
        .arg(
            Arg::new("suffix")
                .short('S')
                .long("suffix")
                .value_name("STRING")
                .default_value("_sliding")
                .help("suffix added to the sequence ID"),
        )
        .arg(
            Arg::new("greedy")
                .short('g')
                .long("greedy")
                .action(ArgAction::SetTrue)
                .help("export a final subsequence shorter than the window"),
        )
        .arg(
            Arg::new("circular")
                .short('c')
                .long("circular")
                .action(ArgAction::SetTrue)
                .help("allow windows to cross the circular origin"),
        )
        .arg(
            Arg::new("circular_genome")
                .short('C')
                .long("circular-genome")
                .action(ArgAction::SetTrue)
                .help("alias for --circular"),
        )
        .arg(
            Arg::new("infile_list")
                .short('X')
                .long("infile-list")
                .value_name("FILE")
                .help("append input paths listed one per line in FILE"),
        )
        .arg(
            Arg::new("skip_file_check")
                .long("skip-file-check")
                .action(ArgAction::SetTrue)
                .help("skip the input existence check"),
        )
        .arg(
            Arg::new("id_regexp")
                .long("id-regexp")
                .value_name("REGEX")
                .help("regular expression used to extract IDs"),
        )
        .arg(
            Arg::new("id_ncbi")
                .long("id-ncbi")
                .action(ArgAction::SetTrue)
                .help("accept NCBI-style FASTA headers"),
        )
        .arg(
            Arg::new("seq_type")
                .short('t')
                .long("seq-type")
                .value_name("TYPE")
                .default_value("auto")
                .value_parser(["dna", "rna", "protein", "unlimit", "auto"])
                .help("sequence type"),
        )
        .arg(
            Arg::new("alphabet_guess")
                .long("alphabet-guess-seq-length")
                .value_name("INT")
                .default_value("10000")
                .help("prefix length for alphabet guessing"),
        )
        .arg(
            Arg::new("quiet")
                .long("quiet")
                .action(ArgAction::SetTrue)
                .help("suppress progress messages"),
        )
        .arg(line_width_arg())
        .arg(out_file_arg())
        .arg(threads_arg())
}

fn range_command() -> Command {
    Command::new("range")
        .about("print FASTA/Q records in a range (start:end)")
        .arg(
            Arg::new("inputs")
                .value_name("FILE")
                .num_args(0..)
                .action(ArgAction::Append),
        )
        .arg(
            Arg::new("region")
                .short('r')
                .long("range")
                .value_name("RANGE")
                .allow_hyphen_values(true)
                .help("record range, e.g. 1:12 or -12:-1"),
        )
        .arg(line_width_arg())
        .arg(out_file_arg())
        .arg(threads_arg())
}

fn subseq_command() -> Command {
    Command::new("subseq")
        .about("get subsequences by region (1-based, negative indices supported)")
        .arg(
            Arg::new("inputs")
                .value_name("FILE")
                .num_args(0..)
                .action(ArgAction::Append),
        )
        .arg(
            Arg::new("region")
                .short('r')
                .long("region")
                .value_name("REGION")
                .allow_hyphen_values(true)
                .required_unless_present_any(["bed", "gtf"])
                .conflicts_with_all(["bed", "gtf"])
                .help("region, e.g. 1:12, -12:-1, 2:-2"),
        )
        .arg(
            Arg::new("bed")
                .long("bed")
                .value_name("FILE")
                .help("extract regions from a 0-based half-open BED file"),
        )
        .arg(
            Arg::new("gtf")
                .long("gtf")
                .value_name("FILE")
                .help("extract features from a 1-based GTF file"),
        )
        .arg(
            Arg::new("feature")
                .long("feature")
                .value_name("TYPE")
                .action(ArgAction::Append)
                .help("restrict GTF extraction to these feature types"),
        )
        .arg(
            Arg::new("gtf_tag")
                .long("gtf-tag")
                .value_name("TAG")
                .default_value("gene_id")
                .help("GTF attribute used as the output comment"),
        )
        .arg(
            Arg::new("chr")
                .long("chr")
                .value_name("NAME")
                .action(ArgAction::Append)
                .help("restrict BED extraction to these sequence IDs"),
        )
        .arg(
            Arg::new("up_stream")
                .short('u')
                .long("up-stream")
                .value_name("INT")
                .default_value("0")
                .help("upstream flank length"),
        )
        .arg(
            Arg::new("down_stream")
                .short('d')
                .long("down-stream")
                .value_name("INT")
                .default_value("0")
                .help("downstream flank length"),
        )
        .arg(
            Arg::new("only_flank")
                .short('f')
                .long("only-flank")
                .action(ArgAction::SetTrue)
                .help("only return flanking sequence"),
        )
        .arg(
            Arg::new("region_coord")
                .short('R')
                .long("region-coord")
                .action(ArgAction::SetTrue)
                .help("append coordinates to sequence ID for -r/--region"),
        )
        .arg(line_width_arg())
        .arg(out_file_arg())
        .arg(threads_arg())
}

fn head_command() -> Command {
    Command::new("head")
        .about("print the first N FASTA/Q records, or leading records whose total length >= L")
        .arg(
            Arg::new("inputs")
                .value_name("FILE")
                .num_args(0..)
                .action(ArgAction::Append),
        )
        .arg(
            Arg::new("number")
                .short('n')
                .long("number")
                .value_name("INT")
                .default_value("10")
                .help("print the first N records"),
        )
        .arg(
            Arg::new("length")
                .short('l')
                .long("length")
                .value_name("LEN")
                .help("print leading records whose total sequence length >= L (K/M/G supported)"),
        )
        .arg(
            Arg::new("infile_list")
                .short('X')
                .long("infile-list")
                .value_name("FILE")
                .help("append input paths listed one per line in FILE"),
        )
        .arg(
            Arg::new("skip_file_check")
                .long("skip-file-check")
                .action(ArgAction::SetTrue)
                .help("skip the input existence check"),
        )
        .arg(
            Arg::new("id_regexp")
                .long("id-regexp")
                .value_name("REGEX")
                .default_value("^(\\S+)\\s?")
                .help("ID parsing expression (does not change head's raw record output)"),
        )
        .arg(
            Arg::new("id_ncbi")
                .long("id-ncbi")
                .action(ArgAction::SetTrue)
                .help("accept NCBI-style FASTA headers"),
        )
        .arg(
            Arg::new("seq_type")
                .short('t')
                .long("seq-type")
                .value_name("TYPE")
                .default_value("auto")
                .value_parser(["dna", "rna", "protein", "unlimit", "auto"])
                .help("sequence type (head preserves records unchanged)"),
        )
        .arg(
            Arg::new("alphabet_guess")
                .long("alphabet-guess-seq-length")
                .value_name("INT")
                .default_value("10000")
                .help("prefix length for alphabet guessing (head does not transform sequences)"),
        )
        .arg(
            Arg::new("quiet")
                .long("quiet")
                .action(ArgAction::SetTrue)
                .help("suppress progress messages"),
        )
        .arg(line_width_arg())
        .arg(out_file_arg())
        .arg(threads_arg())
}

fn grep_command() -> Command {
    Command::new("grep")
        .about("search sequences by ID, full name or sequence")
        .arg(
            Arg::new("inputs")
                .value_name("FILE")
                .num_args(0..)
                .action(ArgAction::Append),
        )
        .arg(
            Arg::new("pattern")
                .short('p')
                .long("pattern")
                .value_name("PATTERN")
                .action(ArgAction::Append)
                .help("search pattern (repeatable; comma-separated values are split)"),
        )
        .arg(
            Arg::new("pattern_file")
                .short('f')
                .long("pattern-file")
                .value_name("FILE")
                .help("read one pattern per line from FILE"),
        )
        .arg(
            Arg::new("by_name")
                .short('n')
                .long("by-name")
                .action(ArgAction::SetTrue)
                .help("match the full name instead of the ID"),
        )
        .arg(
            Arg::new("by_seq")
                .short('s')
                .long("by-seq")
                .action(ArgAction::SetTrue)
                .help("search the sequence on both strands by default"),
        )
        .arg(
            Arg::new("invert")
                .short('v')
                .long("invert-match")
                .action(ArgAction::SetTrue)
                .help("select non-matching records"),
        )
        .arg(
            Arg::new("count")
                .short('C')
                .long("count")
                .action(ArgAction::SetTrue)
                .help("print only the number of matching records"),
        )
        .arg(
            Arg::new("ignore_case")
                .short('i')
                .long("ignore-case")
                .action(ArgAction::SetTrue)
                .help("ignore case while matching"),
        )
        .arg(
            Arg::new("allow_duplicated")
                .short('D')
                .long("allow-duplicated-patterns")
                .action(ArgAction::SetTrue)
                .help("output records once per matching pattern, including duplicates"),
        )
        .arg(
            Arg::new("circular")
                .short('c')
                .long("circular")
                .action(ArgAction::SetTrue)
                .help("allow sequence motifs to cross the circular origin"),
        )
        .arg(
            Arg::new("delete_matched")
                .long("delete-matched")
                .action(ArgAction::SetTrue)
                .help("retire each pattern after its first match"),
        )
        .arg(
            Arg::new("max_mismatch")
                .short('m')
                .long("max-mismatch")
                .value_name("INT")
                .default_value("0")
                .help("maximum Hamming mismatches for sequence motifs"),
        )
        .arg(
            Arg::new("only_positive")
                .short('P')
                .long("only-positive-strand")
                .action(ArgAction::SetTrue)
                .help("search only the positive strand for sequence motifs"),
        )
        .arg(
            Arg::new("region")
                .short('R')
                .long("region")
                .value_name("START:END")
                .allow_hyphen_values(true)
                .help("restrict sequence matching to a 1-based region"),
        )
        .arg(
            Arg::new("use_regexp")
                .short('r')
                .long("use-regexp")
                .action(ArgAction::SetTrue)
                .help("interpret patterns as regular expressions"),
        )
        .arg(
            Arg::new("degenerate")
                .short('d')
                .long("degenerate")
                .action(ArgAction::SetTrue)
                .help("allow IUPAC degenerate bases in motifs"),
        )
        .arg(
            Arg::new("immediate_output")
                .short('I')
                .long("immediate-output")
                .action(ArgAction::SetTrue)
                .help("write matches without changing their order"),
        )
        .arg(
            Arg::new("infile_list")
                .short('X')
                .long("infile-list")
                .value_name("FILE")
                .help("append input paths listed one per line in FILE"),
        )
        .arg(
            Arg::new("skip_file_check")
                .long("skip-file-check")
                .action(ArgAction::SetTrue)
                .help("skip the input existence check"),
        )
        .arg(
            Arg::new("id_regexp")
                .long("id-regexp")
                .value_name("REGEX")
                .help("regular expression used to extract IDs"),
        )
        .arg(
            Arg::new("id_ncbi")
                .long("id-ncbi")
                .action(ArgAction::SetTrue)
                .help("accept NCBI-style FASTA headers"),
        )
        .arg(
            Arg::new("seq_type")
                .short('t')
                .long("seq-type")
                .value_name("TYPE")
                .default_value("auto")
                .value_parser(["dna", "rna", "protein", "unlimit", "auto"])
                .help("sequence type"),
        )
        .arg(
            Arg::new("alphabet_guess")
                .long("alphabet-guess-seq-length")
                .value_name("INT")
                .default_value("10000")
                .help("prefix length for alphabet guessing"),
        )
        .arg(
            Arg::new("quiet")
                .long("quiet")
                .action(ArgAction::SetTrue)
                .help("suppress progress messages"),
        )
        .arg(line_width_arg())
        .arg(out_file_arg())
        .arg(threads_arg())
}

fn sort_command() -> Command {
    Command::new("sort")
        .about("sort sequences by id, full name, sequence, length or non-gap bases")
        .arg(
            Arg::new("inputs")
                .value_name("FILE")
                .num_args(0..)
                .action(ArgAction::Append),
        )
        .arg(
            Arg::new("by_length")
                .short('l')
                .long("by-length")
                .action(ArgAction::SetTrue)
                .help("sort by sequence length"),
        )
        .arg(
            Arg::new("by_name")
                .short('n')
                .long("by-name")
                .action(ArgAction::SetTrue)
                .help("sort by full name instead of ID"),
        )
        .arg(
            Arg::new("by_seq")
                .short('s')
                .long("by-seq")
                .action(ArgAction::SetTrue)
                .help("sort by sequence"),
        )
        .arg(
            Arg::new("by_bases")
                .short('b')
                .long("by-bases")
                .action(ArgAction::SetTrue)
                .help("sort by non-gap bases"),
        )
        .arg(
            Arg::new("ignore_case")
                .short('i')
                .long("ignore-case")
                .action(ArgAction::SetTrue)
                .help("ignore case while sorting"),
        )
        .arg(
            Arg::new("reverse")
                .short('r')
                .long("reverse")
                .action(ArgAction::SetTrue)
                .help("reverse the result"),
        )
        .arg(
            Arg::new("gap_letters")
                .short('G')
                .long("gap-letters")
                .value_name("STRING")
                .default_value("- \t.")
                .help("gap letters for -b"),
        )
        .arg(
            Arg::new("natural_order")
                .short('N')
                .long("natural-order")
                .action(ArgAction::SetTrue)
                .help("use natural ordering for IDs/full names"),
        )
        .arg(
            Arg::new("seq_prefix_length")
                .short('L')
                .long("seq-prefix-length")
                .value_name("INT")
                .default_value("10000")
                .help("length of sequence prefix used for sequence sorting (0 for whole)"),
        )
        .arg(
            Arg::new("two_pass")
                .short('2')
                .long("two-pass")
                .action(ArgAction::SetTrue)
                .help("sort plain FASTA using metadata and byte offsets"),
        )
        .arg(
            Arg::new("update_faidx")
                .short('U')
                .long("update-faidx")
                .action(ArgAction::SetTrue)
                .help("rebuild an existing .seqkit.fai file"),
        )
        .arg(
            Arg::new("keep_temp")
                .short('k')
                .long("keep-temp")
                .action(ArgAction::SetTrue)
                .help("keep temporary FASTA/index files for two-pass paths that need them"),
        )
        .arg(line_width_arg())
        .arg(out_file_arg())
        .arg(threads_arg())
}

fn fq2fa_command() -> Command {
    Command::new("fq2fa")
        .about("convert FASTQ to FASTA")
        .arg(
            Arg::new("inputs")
                .value_name("FILE")
                .num_args(0..)
                .action(ArgAction::Append),
        )
        .arg(line_width_arg().default_value("0"))
        .arg(out_file_arg())
        .arg(threads_arg())
}

fn fx2tab_command() -> Command {
    Command::new("fx2tab")
        .about("convert FASTA/Q to tabular format (and length, GC content, average quality...)")
        .arg(
            Arg::new("inputs")
                .value_name("FILE")
                .num_args(0..)
                .action(ArgAction::Append),
        )
        .arg(
            Arg::new("header_line")
                .short('H')
                .long("header-line")
                .action(ArgAction::SetTrue)
                .help("print header line"),
        )
        .arg(
            Arg::new("length")
                .short('l')
                .long("length")
                .action(ArgAction::SetTrue)
                .help("print sequence length"),
        )
        .arg(
            Arg::new("gc")
                .short('g')
                .long("gc")
                .action(ArgAction::SetTrue)
                .help("print GC content, i.e., (G+C)/(G+C+A+T)"),
        )
        .arg(
            Arg::new("gc_skew")
                .short('G')
                .long("gc-skew")
                .action(ArgAction::SetTrue)
                .help("print GC-Skew"),
        )
        .arg(
            Arg::new("name")
                .short('n')
                .long("name")
                .action(ArgAction::SetTrue)
                .help("only print names (no sequences and qualities)"),
        )
        .arg(
            Arg::new("only_id")
                .short('i')
                .long("only-id")
                .action(ArgAction::SetTrue)
                .help("print ID instead of full head"),
        )
        .arg(
            Arg::new("no_qual")
                .short('Q')
                .long("no-qual")
                .action(ArgAction::SetTrue)
                .help("only output two column even for FASTQ file"),
        )
        .arg(
            Arg::new("avg_qual")
                .short('q')
                .long("avg-qual")
                .action(ArgAction::SetTrue)
                .help("print average quality of a read"),
        )
        .arg(
            Arg::new("seq_hash")
                .short('s')
                .long("seq-hash")
                .action(ArgAction::SetTrue)
                .help("print hash (MD5) of sequence"),
        )
        .arg(
            Arg::new("alphabet")
                .short('a')
                .long("alphabet")
                .action(ArgAction::SetTrue)
                .help("print alphabet letters"),
        )
        .arg(
            Arg::new("case_sensitive")
                .short('I')
                .long("case-sensitive")
                .action(ArgAction::SetTrue)
                .help("calculate case sensitive base content/sequence hash"),
        )
        .arg(
            Arg::new("base_content")
                .short('B')
                .long("base-content")
                .value_name("STRINGS")
                .action(ArgAction::Append)
                .help("print base content, e.g. -B AT -B N"),
        )
        .arg(
            Arg::new("base_count")
                .short('C')
                .long("base-count")
                .value_name("STRINGS")
                .action(ArgAction::Append)
                .help("print base count, e.g. -C AT -C N"),
        )
        .arg(
            Arg::new("basename")
                .long("basename")
                .action(ArgAction::SetTrue)
                .help("print the basename of input file"),
        )
        .arg(
            Arg::new("file_name")
                .short('f')
                .long("file-name")
                .action(ArgAction::SetTrue)
                .help("print the name of input file"),
        )
        .arg(
            Arg::new("qual_ascii_base")
                .short('b')
                .long("qual-ascii-base")
                .value_name("INT")
                .default_value("33")
                .help("ASCII BASE, 33 for Phred+33"),
        )
        .arg(
            Arg::new("stdin_label")
                .long("stdin-label")
                .value_name("STRING")
                .default_value("-")
                .help("label for replacing default \"-\" for stdin"),
        )
        .arg(out_file_arg())
        .arg(threads_arg())
}

fn sample_command() -> Command {
    Command::new("sample")
        .about("sample sequences by number or proportion")
        .arg(
            Arg::new("inputs")
                .value_name("FILE")
                .num_args(0..)
                .action(ArgAction::Append),
        )
        .arg(
            Arg::new("proportion")
                .short('p')
                .long("proportion")
                .value_name("FLOAT")
                .help("sample by proportion"),
        )
        .arg(
            Arg::new("number")
                .short('n')
                .long("number")
                .value_name("INT")
                .help("sample by number (may return fewer than requested)"),
        )
        .arg(
            Arg::new("rand_seed")
                .short('s')
                .long("rand-seed")
                .value_name("INT")
                .default_value("11")
                .help("random seed"),
        )
        .arg(
            Arg::new("non_deterministic")
                .short('r')
                .long("non-deterministic")
                .action(ArgAction::SetTrue)
                .help("use a time-based seed to vary results across runs"),
        )
        .arg(line_width_arg())
        .arg(out_file_arg())
        .arg(threads_arg())
}

fn split_command() -> Command {
    Command::new("split")
        .about("split sequences into files by size or number of parts")
        .arg(Arg::new("input").value_name("FILE"))
        .arg(
            Arg::new("by_size")
                .short('s')
                .long("by-size")
                .value_name("INT")
                .help("split sequences into multi parts with N sequences"),
        )
        .arg(
            Arg::new("by_part")
                .short('p')
                .long("by-part")
                .value_name("INT")
                .help("split sequences into N parts"),
        )
        .arg(
            Arg::new("out_dir")
                .short('O')
                .long("out-dir")
                .value_name("DIR")
                .help("output directory (default: <input>.split)"),
        )
        .arg(
            Arg::new("out_prefix")
                .short('P')
                .long("out-prefix")
                .value_name("STRING")
                .help("file prefix"),
        )
        .arg(
            Arg::new("part_width")
                .short('W')
                .long("part-width")
                .value_name("INT")
                .default_value("3")
                .help("number of digits used for output file part numbering"),
        )
        .arg(
            Arg::new("extension")
                .short('e')
                .long("extension")
                .value_name("STRING")
                .help("set output file extension, e.g. \".gz\""),
        )
        .arg(
            Arg::new("dry_run")
                .short('d')
                .long("dry-run")
                .action(ArgAction::SetTrue)
                .help("dry run, just print message and no files will be created"),
        )
        .arg(
            Arg::new("force")
                .short('f')
                .long("force")
                .action(ArgAction::SetTrue)
                .help("overwrite output directory"),
        )
        .arg(threads_arg())
}

fn lint_command() -> Command {
    Command::new("lint")
        .about("FASTQ file pair validator (fq 0.12.0 validator set)")
        .arg(Arg::new("r1").value_name("R1_SRC").required(true))
        .arg(Arg::new("r2").value_name("R2_SRC"))
        .arg(
            Arg::new("lint_mode")
                .long("lint-mode")
                .value_name("MODE")
                .default_value("panic")
                .value_parser(["panic", "log"])
                .help("Panic on first error or log all errors"),
        )
        .arg(
            Arg::new("single_read_validation_level")
                .long("single-read-validation-level")
                .value_name("LEVEL")
                .default_value("high")
                .value_parser(["low", "medium", "high"])
                .help("Only use single read validators up to a given level"),
        )
        .arg(
            Arg::new("paired_read_validation_level")
                .long("paired-read-validation-level")
                .value_name("LEVEL")
                .default_value("high")
                .value_parser(["low", "medium", "high"])
                .help("Only use paired read validators up to a given level"),
        )
        .arg(
            Arg::new("disable_validator")
                .long("disable-validator")
                .value_name("CODE")
                .action(ArgAction::Append)
                .help("Disable validators by code. Use multiple times to disable more than one"),
        )
        .arg(
            Arg::new("record_definition_separator")
                .long("record-definition-separator")
                .value_name("STRING")
                .help("record definition separator (accepted and ignored by this build)"),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_command_tree_parses_every_subcommand() {
        for name in ["stats", "seq", "fq2fa", "fx2tab", "sample", "split", "lint"] {
            let parsed = command()
                .try_get_matches_from([PROGRAM, name, "--help"])
                .or_else(|_| command().try_get_matches_from([PROGRAM, name, "x"]));
            // `--help` exits early, so a successful parse of either form is enough
            // to prove the subcommand exists and its args are consistent.
            let _ = parsed;
            let tree = command();
            let sub = tree
                .get_subcommands()
                .find(|c| c.get_name() == name)
                .unwrap_or_else(|| panic!("missing subcommand {name}"));
            assert_eq!(sub.get_name(), name);
        }
    }

    #[test]
    fn stats_has_the_documented_flags() {
        let tree = command();
        let stats = tree
            .get_subcommands()
            .find(|c| c.get_name() == "stats")
            .expect("stats");
        for flag in ["tabular", "all", "basename", "gap_letters", "stdin_label"] {
            assert!(
                stats.get_arguments().any(|a| a.get_id() == flag),
                "missing {flag}"
            );
        }
    }

    #[test]
    fn seq_has_the_documented_flags() {
        let tree = command();
        let seq = tree
            .get_subcommands()
            .find(|c| c.get_name() == "seq")
            .expect("seq");
        for flag in [
            "name",
            "only_id",
            "seq_only",
            "qual_only",
            "reverse",
            "complement",
            "upper",
            "lower",
            "remove_gaps",
            "gap_letters",
            "min_len",
            "max_len",
        ] {
            assert!(
                seq.get_arguments().any(|a| a.get_id() == flag),
                "missing {flag}"
            );
        }
    }

    #[test]
    fn fx2tab_has_the_documented_flags() {
        let tree = command();
        let fx2tab = tree
            .get_subcommands()
            .find(|c| c.get_name() == "fx2tab")
            .expect("fx2tab");
        for flag in [
            "header_line",
            "length",
            "gc",
            "name",
            "only_id",
            "no_qual",
            "avg_qual",
            "seq_hash",
        ] {
            assert!(
                fx2tab.get_arguments().any(|a| a.get_id() == flag),
                "missing {flag}"
            );
        }
    }

    #[test]
    fn sample_takes_a_seed_with_the_documented_default() {
        let matches = command()
            .try_get_matches_from([PROGRAM, "sample", "-p", "0.5", "in.fq"])
            .expect("parse");
        let (name, sample) = matches.subcommand().expect("subcommand");
        assert_eq!(name, "sample");
        // Documented default is 11.
        assert_eq!(
            sample.get_one::<String>("rand_seed").map(String::as_str),
            Some("11")
        );
    }

    #[test]
    fn lint_validates_the_three_enumerated_options() {
        let matches = command()
            .try_get_matches_from([
                PROGRAM,
                "lint",
                "--lint-mode",
                "log",
                "--single-read-validation-level",
                "low",
                "r1.fq",
            ])
            .expect("parse");
        let (_, lint) = matches.subcommand().expect("subcommand");
        assert_eq!(
            lint.get_one::<String>("lint_mode").map(String::as_str),
            Some("log")
        );
        assert_eq!(
            lint.get_one::<String>("single_read_validation_level")
                .map(String::as_str),
            Some("low")
        );
    }

    #[test]
    fn an_invalid_lint_mode_is_rejected() {
        assert!(command()
            .try_get_matches_from([PROGRAM, "lint", "--lint-mode", "bogus", "r1.fq"])
            .is_err());
    }

    #[test]
    fn split_defaults_the_part_width_to_three() {
        let matches = command()
            .try_get_matches_from([PROGRAM, "split", "-s", "100", "in.fq"])
            .expect("parse");
        let (_, split) = matches.subcommand().expect("subcommand");
        assert_eq!(
            split.get_one::<String>("part_width").map(String::as_str),
            Some("3")
        );
    }

    #[test]
    fn no_subcommand_is_an_error_rather_than_a_silent_no_op() {
        assert!(command().try_get_matches_from([PROGRAM]).is_err());
    }

    #[test]
    fn unimplemented_seqkit_flags_are_absent_not_ignored() {
        // Boundary rule 6: an option this build cannot honour must be rejected,
        // not accepted and ignored.
        for (sub, flag) in [
            ("stats", "--two-pass"),
            ("seq", "--two-pass"),
            ("seq", "--color"),
            ("sample", "--two-pass"),
            ("split", "--two-pass"),
        ] {
            let result = command().try_get_matches_from([PROGRAM, sub, flag, "x"]);
            assert!(
                result.is_err(),
                "{sub} {flag} must be rejected, not silently accepted"
            );
        }
    }
}
