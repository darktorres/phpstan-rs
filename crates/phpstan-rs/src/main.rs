use std::path::PathBuf;

use clap::{Parser as ClapParser, Subcommand};

use phpstan_rs::{app, lsp};

#[path = "main/check.rs"]
mod check;
#[path = "main/check_project.rs"]
mod check_project;
#[path = "main/demo.rs"]
mod demo;
#[path = "main/incremental_bench.rs"]
mod incremental_bench;

#[cfg(all(feature = "dhat-heap", feature = "hotpath-alloc"))]
compile_error!("`dhat-heap` and `hotpath-alloc` each install a global allocator; enable one");

#[cfg(feature = "dhat-heap")]
#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

#[derive(ClapParser)]
#[command(subcommand_required = true, arg_required_else_help = true, version)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

// Parsed once per process, so the variant size difference costs nothing.
#[allow(clippy::large_enum_variant)]
#[derive(Subcommand)]
enum Command {
    /// Lex+parse every .php file under the given paths, reporting a
    /// pass/fail summary.
    Check { paths: Vec<PathBuf> },
    /// Load every .php file under the given paths and run the full
    /// checker over all of them, reporting every diagnostic.
    CheckProject(check_project::CheckProjectArgs),
    /// Loads every .php file under `dir` into a Project (timed), then
    /// re-parses/re-checks just one file via the incremental path (also
    /// timed), reporting both.
    IncrementalBench {
        dir: PathBuf,
        file_to_touch: Option<PathBuf>,
    },
    /// Runs the LSP server over stdio.
    Lsp,
    /// Parses the given file (or a built-in sample with none given) and
    /// dumps tokens/AST/resolved signatures/diagnostics - a debug tool,
    /// not the normal way to use this checker.
    Demo { file: Option<PathBuf> },
    /// Rewrites a baseline file, and every file it includes, in the
    /// current baseline format.
    ConvertBaseline { path: PathBuf },
    /// Runs a language-server session over the workspace `dir` in this
    /// process and writes its record to `out`, a dump with its manifest
    /// beside it (a regression-gate snapshot).
    #[command(hide = true)]
    RecordSession {
        dir: PathBuf,
        out: PathBuf,
        #[arg(long, default_value_t = 1)]
        seed: u64,
        #[arg(long, default_value_t = 40)]
        files: usize,
        #[arg(long, default_value_t = 8)]
        positions: usize,
        /// The manifest of another record: the record is written only
        /// where it differs from that one.
        #[arg(long)]
        reference: Option<PathBuf>,
    },
}

#[hotpath::main]
fn main() {
    // Stopped at the end of the run, which writes `dhat-heap.json` in the
    // `dhat-heap` build (every `check-project` error path below that
    // matters for a profiling run falls through to a normal return, not
    // `process::exit`, so this always flushes).
    phpstan_rs::heap::start_of_run();

    let cli = Cli::parse();
    match cli.command {
        Command::Check { paths } => check::run_check(&paths),
        Command::CheckProject(args) => check_project::run_check_project_command(args),
        Command::IncrementalBench { dir, file_to_touch } => {
            incremental_bench::run_incremental_bench(&dir, file_to_touch.as_deref())
        }
        Command::Lsp => {
            let exit_code = lsp::run(env!("CARGO_PKG_VERSION"), app::config_schema());
            // 0 falls through to main's own normal return (so the
            // dhat-heap profiler's, and a --features hotpath build's
            // HotpathGuard's, Drop still flushes) - only the spec-
            // mandated nonzero (protocol-violation: exit with no prior
            // shutdown) path short-circuits via process::exit, the same
            // way run_check_project's own error paths already do.
            if exit_code != 0 {
                std::process::exit(exit_code);
            }
        }
        Command::Demo { file } => demo::run_demo(file.as_deref()),
        Command::RecordSession {
            dir,
            out,
            seed,
            files,
            positions,
            reference,
        } => {
            let dir = std::fs::canonicalize(&dir).unwrap_or(dir);
            let record = lsp::session_record::record_session(&dir, seed, files, positions);
            if let Err(e) = phpstan_rs::project::session::write_dump(
                &out,
                dumps::Kind::Tree,
                reference.as_deref(),
                |w| w.write_all(record.as_bytes()),
            ) {
                eprintln!("error: {e}");
                std::process::exit(1);
            }
        }
        Command::ConvertBaseline { path } => match phpstan_rs::baseline::convert(&path) {
            Ok(files) => eprintln!("converted {files} baseline file(s)"),
            Err(e) => {
                eprintln!("error: converting {}: {e}", path.display());
                std::process::exit(1);
            }
        },
    }
    phpstan_rs::heap::end_of_run();
    if std::env::var_os("PHPSTAN_RS_MEMO_STATS").is_some() {
        for (name, count) in phpstan_rs::stats::table() {
            eprintln!("stats {name} {count}");
        }
    }
}

#[cfg(test)]
#[path = "main/tests.rs"]
mod tests;
