use std::path::PathBuf;

use clap::{Parser as ClapParser, Subcommand};

use phpstan_rs::project::WORKER_STACK_SIZE;
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
}

impl Command {
    /// Whether the command is a batch run, which keeps the memory it frees
    /// until it exits. The server gives memory back after a large analysis,
    /// and the bench measures the server's path.
    fn keeps_freed_memory(&self) -> bool {
        match self {
            Command::Check { .. }
            | Command::CheckProject(_)
            | Command::Demo { .. }
            | Command::ConvertBaseline { .. } => true,
            Command::IncrementalBench { .. } | Command::Lsp => false,
        }
    }
}

#[hotpath::main]
fn main() {
    // Held for the whole run; its `Drop` writes `dhat-heap.json` on
    // clean exit (every `check-project` error path below that matters
    // for a profiling run falls through to a normal return, not
    // `process::exit`, so this always flushes). Its backtraces keep ten
    // frames unless `PHPSTAN_RS_DHAT_UNTRIMMED` is set, which keeps them
    // whole.
    #[cfg(feature = "dhat-heap")]
    let _profiler = {
        let frames = std::env::var_os("PHPSTAN_RS_DHAT_UNTRIMMED")
            .is_none()
            .then_some(10);
        dhat::Profiler::builder().trim_backtraces(frames).build()
    };

    let cli = Cli::parse();
    // The profiler's guard starts threads before this body runs.
    let only_thread = !cfg!(feature = "hotpath");
    #[allow(unsafe_code)]
    if only_thread && cli.command.keeps_freed_memory() {
        // SAFETY: no other thread has started.
        unsafe { phpstan_rs::allocator::keep_freed_memory() }
    }
    on_worker_stack(|| run(cli.command));
}

/// Runs `body` on a thread with the stack every checking thread has
/// (`WORKER_STACK_SIZE`), and waits for it. The system chooses the main
/// thread's stack, and it can be small: Windows gives it 1 MiB, which the
/// stack guard's red zone (`stack`) fills by itself, and Unix gives it
/// `ulimit -s`. A panic of `body` is raised again here, so the process ends
/// as it would have if `body` had run on this thread.
fn on_worker_stack<R: Send>(body: impl FnOnce() -> R + Send) -> R {
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .name("phpstan-rs-main".to_string())
            .stack_size(WORKER_STACK_SIZE)
            .spawn_scoped(scope, body)
            .expect("spawning the thread that runs the command")
            .join()
            .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
    })
}

fn run(command: Command) {
    match command {
        Command::Check { paths } => check::run_check(&paths),
        Command::CheckProject(args) => check_project::run_check_project_command(args),
        Command::IncrementalBench { dir, file_to_touch } => {
            incremental_bench::run_incremental_bench(&dir, file_to_touch.as_deref())
        }
        Command::Lsp => {
            let exit_code = lsp::run(env!("CARGO_PKG_VERSION"), app::config_schema());
            // 0 returns, and so does main after it (so the dhat-heap
            // profiler's, and a --features hotpath build's HotpathGuard's,
            // Drop still flushes) - only the spec-mandated nonzero
            // (protocol-violation: exit with no prior shutdown) path
            // short-circuits via process::exit, the same way
            // run_check_project's own error paths already do.
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
