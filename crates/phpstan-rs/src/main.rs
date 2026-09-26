use std::path::PathBuf;

use clap::{Parser as ClapParser, Subcommand};

use phpstan_rs::lsp;

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
}

#[hotpath::main]
fn main() {
    // Held for the whole run; its `Drop` writes `dhat-heap.json` on
    // clean exit (every `check-project` error path below that matters
    // for a profiling run falls through to a normal return, not
    // `process::exit`, so this always flushes).
    #[cfg(feature = "dhat-heap")]
    let _profiler = dhat::Profiler::new_heap();

    let cli = Cli::parse();
    match cli.command {
        Command::Check { paths } => check::run_check(&paths),
        Command::CheckProject(args) => check_project::run_check_project_command(args),
        Command::IncrementalBench { dir, file_to_touch } => {
            incremental_bench::run_incremental_bench(&dir, file_to_touch.as_deref())
        }
        Command::Lsp => {
            let exit_code = lsp::run();
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
        Command::ConvertBaseline { path } => match phpstan_rs::baseline::convert(&path) {
            Ok(files) => eprintln!("converted {files} baseline file(s)"),
            Err(e) => {
                eprintln!("error: converting {}: {e}", path.display());
                std::process::exit(1);
            }
        },
    }
}

#[cfg(test)]
#[path = "main/tests.rs"]
mod tests;
