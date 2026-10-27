//! Process entry point: parse, dispatch, map errors onto exit codes.
//!
//! Exit codes come from the shared workspace contract
//! ([`bioinfo_common::exit`]); this file only decides *which* class a particular
//! failure belongs to, via [`rust_umitools::error::Error::class`].

use bioinfo_common::diagnostic::error_line;
use bioinfo_common::exit::ExitClass;
use bioinfo_common::provenance::Provenance;
use rust_umitools::cli::{self, Invocation};
use rust_umitools::error::DECLARED_EXIT_CLASSES;

/// glibc's allocator serialises badly across the reader, worker and writer
/// threads; mimalloc keeps the same records and maps without that contention.
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

fn main() {
    configure_allocator();
    if wants_verbose_version() {
        print_verbose_version();
        std::process::exit(ExitClass::Success.code());
    }
    let invocation = match cli::parse() {
        Ok(invocation) => invocation,
        Err(error) => {
            eprintln!("{}", error_line(&error.to_string()));
            std::process::exit(error.exit_code());
        }
    };
    let result = match invocation {
        Invocation::Dedup(config) => rust_umitools::commands::dedup::run(&config).map(|_| ()),
        Invocation::Group(args) => rust_umitools::commands::group::run(&args),
        Invocation::Count(args) => rust_umitools::commands::count::run(&args),
        Invocation::CountTab(args) => rust_umitools::commands::count_tab::run(&args),
        Invocation::Extract(args) => rust_umitools::commands::extract::run(&args),
        Invocation::Whitelist(args) => rust_umitools::commands::whitelist::run(&args),
    };
    match result {
        Ok(()) => std::process::exit(ExitClass::Success.code()),
        Err(error) => {
            eprintln!("{}", error_line(&error.to_string()));
            std::process::exit(error.exit_code());
        }
    }
}

/// Keep RSS at or below the glibc baseline by purging freed mimalloc segments
/// eagerly instead of after the default ten-second delay. The Rust bindings do
/// not name this option, so use its value from the pinned mimalloc 3.x enum
/// (`mi_option_purge_delay`).
fn configure_allocator() {
    const MI_OPTION_PURGE_DELAY: i32 = 15;
    // SAFETY: `mi_option_set` is the documented, thread-safe mimalloc API and
    // the option number is fixed by the pinned `libmimalloc-sys` release.
    unsafe { libmimalloc_sys::mi_option_set(MI_OPTION_PURGE_DELAY, 0) }
}

/// `--version --verbose` is handled before clap so the detailed report wins
/// over the short version string.
fn wants_verbose_version() -> bool {
    let mut version = false;
    let mut verbose = false;
    for argument in std::env::args().skip(1) {
        match argument.as_str() {
            "--version" | "-V" => version = true,
            "--verbose" => verbose = true,
            _ => {}
        }
    }
    version && verbose
}

/// The provenance block, assembled from the shared builder.
///
/// The field set, order and alignment are an output contract. Contract changes
/// require explicit approval; `bioinfo_common::provenance` owns the shared layout.
fn provenance() -> Provenance {
    Provenance::new("rust-umitools", rust_umitools::cli::VERSION)
        .with_field(
            "reference",
            "UMI-tools 1.1.6 (3f2cb4f41e07fe4a6cea3b3389f534bee957176b)",
        )
        .with_field("library", "noodles 0.118.0 (pure Rust; BAM/SAM/CRAM)")
        .with_field("parallel", "rayon worker pool, single-threaded BGZF output")
        .with_field(
            "commands",
            "whitelist, extract, dedup, group, count, count_tab",
        )
        .with_exit_codes(&DECLARED_EXIT_CLASSES)
}

fn print_verbose_version() {
    print!("{}", provenance().render());
}

#[cfg(test)]
mod tests {
    use super::*;

    // The approved output contract after the noodles/provenance refresh.
    // Column alignment, field order and line count remain significant.
    #[test]
    fn provenance_block_is_byte_identical_to_the_recorded_output() {
        assert_eq!(
            provenance().render(),
            concat!(
                "rust-umitools 0.2.0\n",
                "reference        : UMI-tools 1.1.6 (3f2cb4f41e07fe4a6cea3b3389f534bee957176b)\n",
                "library          : noodles 0.118.0 (pure Rust; BAM/SAM/CRAM)\n",
                "parallel         : rayon worker pool, single-threaded BGZF output\n",
                "commands         : whitelist, extract, dedup, group, count, count_tab\n",
                "exit codes       : 0 ok, 2 usage, 3 input, 4 io/resource\n",
            )
        );
    }

    #[test]
    fn provenance_lists_every_command_that_exists() {
        let text = provenance().render();
        let commands = text
            .lines()
            .find_map(|line| line.strip_prefix("commands         : "))
            .expect("commands field present");
        for command in [
            "whitelist",
            "extract",
            "dedup",
            "group",
            "count",
            "count_tab",
        ] {
            assert!(
                commands.contains(command),
                "{command} missing from {commands}"
            );
        }
    }
}
