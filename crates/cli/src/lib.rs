//! The command line: a port of PHPStan's `src/Command`, configuration,
//! levels and output formats. Filled in M7.

use std::io::{self, Write};

/// How a run ended, mapped to PHPStan's exit codes by the binary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// No errors: exit code 0.
    Clean,
    /// The command line could not be used: exit code 2.
    Usage,
}

/// Runs the command line `args` (without the program name), writing to `out`.
///
/// # Errors
///
/// Returns the error of a failed write; a closed pipe is `BrokenPipe`.
pub fn run(args: &[String], version: &str, out: &mut dyn Write) -> io::Result<Outcome> {
    match args.first().map(String::as_str) {
        Some("--version" | "-V") => {
            writeln!(out, "phpstan-rs {version}")?;
            Ok(Outcome::Clean)
        }
        Some("debug") => debug(&args[1..], out),
        _ => {
            writeln!(out, "Usage: phpstan-rs --version")?;
            Ok(Outcome::Usage)
        }
    }
}

/// The hidden `debug <suite>` subcommands that the parity scoreboard drives
/// (`docs/design/scoreboard.md`). The lexer and parser read one
/// `<case id>\t<file>` line per input on stdin, or take `--show <case id>
/// <file>`; phpdoc reads `<case id>\t<kind>\t<file>\t<offset>\t<length>`
/// lines, one per doc comment or type, or takes `--show` with the same five. A suite whose code is not ported yet answers with one
/// `{"unported": "<suite>"}` line.
fn debug(args: &[String], out: &mut dyn Write) -> io::Result<Outcome> {
    match args.first().map(String::as_str) {
        Some(suite @ ("lexer" | "parser" | "phpdoc" | "types" | "rules")) => {
            writeln!(out, "{{\"unported\":\"{suite}\"}}")?;
            Ok(Outcome::Clean)
        }
        _ => {
            writeln!(
                out,
                "Usage: phpstan-rs debug <lexer|parser> [--show <case id> <file>] < inputs\n       phpstan-rs debug phpdoc [--show <case id> <kind> <file> <offset> <length>] < cases\n       phpstan-rs debug <types|rules>"
            )?;
            Ok(Outcome::Usage)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_prints_the_given_version() {
        let mut out = Vec::new();
        let outcome = run(&["--version".to_owned()], "9.9.9", &mut out).unwrap();
        assert_eq!(outcome, Outcome::Clean);
        assert_eq!(out, b"phpstan-rs 9.9.9\n");
    }

    #[test]
    fn no_arguments_is_a_usage_error() {
        let mut out = Vec::new();
        assert_eq!(run(&[], "1", &mut out).unwrap(), Outcome::Usage);
    }

    #[test]
    fn debug_suites_answer_unported() {
        for suite in ["lexer", "parser", "phpdoc", "types", "rules"] {
            let mut out = Vec::new();
            let args = ["debug".to_owned(), suite.to_owned()];
            assert_eq!(run(&args, "1", &mut out).unwrap(), Outcome::Clean);
            assert_eq!(out, format!("{{\"unported\":\"{suite}\"}}\n").into_bytes());
        }
    }
}
