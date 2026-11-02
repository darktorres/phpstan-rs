//! The `phpstan-rs` binary.
//!
//! The command runs on a named thread whose stack size the program chooses,
//! because the main thread's stack is platform-dependent (1 MiB on Windows).

use std::io::{self, ErrorKind, Write};
use std::process::ExitCode;
use std::thread;

/// Stack size of the thread that runs the command.
const COMMAND_STACK_BYTES: usize = 64 * 1024 * 1024;

/// Exit status when the output pipe closes early, as for a process
/// killed by `SIGPIPE`.
const EXIT_BROKEN_PIPE: u8 = 141;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let command = thread::Builder::new()
        .name("phpstan-rs-main".to_owned())
        .stack_size(COMMAND_STACK_BYTES)
        .spawn(move || {
            let stdout = io::stdout();
            let mut out = stdout.lock();
            phpstan_rs::run(&args, env!("CARGO_PKG_VERSION"), &mut out)
        });
    let result = match command {
        Ok(handle) => match handle.join() {
            Ok(result) => result,
            // Re-raise the panic, so the exit status stays 101.
            Err(panic) => std::panic::resume_unwind(panic),
        },
        Err(error) => Err(error),
    };
    match result {
        Ok(phpstan_rs::Outcome::Clean) => ExitCode::SUCCESS,
        Ok(phpstan_rs::Outcome::Usage) => ExitCode::from(2),
        Err(error) if error.kind() == ErrorKind::BrokenPipe => ExitCode::from(EXIT_BROKEN_PIPE),
        Err(error) => {
            // Nothing more can be done if stderr is closed too.
            let _ = writeln!(io::stderr(), "phpstan-rs: {error}");
            ExitCode::from(2)
        }
    }
}
