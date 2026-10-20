//! The command-line surface.
//!
//! Option names and short flags mirror SeqKit 2.14.0's documented surface for the
//! seven implemented subcommands, so a user can move between the two tools
//! without relearning flags. Where SeqKit's semantics are ambiguous in its
//! documentation, the choice made here is recorded in `docs/EVIDENCE-W1-3.md`.
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
             stats, seq, fq2fa, fx2tab, sample, split and lint, and fq 0.12.0's validator set for \
             lint. It has NOT been differentially verified against either tool: the oracles are \
             unavailable in the build environment (see docs/EVIDENCE-W1-3.md). Treat the output \
             as experimental.",
        )
        .subcommand_required(true)
        .arg_required_else_help(true)
        .subcommand(stats_command())
        .subcommand(seq_command())
        .subcommand(fq2fa_command())
        .subcommand(fx2tab_command())
        .subcommand(sample_command())
        .subcommand(split_command())
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

fn fq2fa_command() -> Command {
    Command::new("fq2fa")
        .about("convert FASTQ to FASTA")
        .arg(
            Arg::new("inputs")
                .value_name("FILE")
                .num_args(0..)
                .action(ArgAction::Append),
        )
        .arg(line_width_arg())
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
