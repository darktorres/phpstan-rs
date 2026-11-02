//! `cargo xtask <task>`: project tasks that are not part of the shipped binary.

// A task reports to the terminal.
#![allow(clippy::print_stdout, clippy::print_stderr)]

mod files;
mod parity;
mod port_map;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let root = workspace_root();
    let result = match args.first().map(String::as_str) {
        Some("port-map") => {
            port_map::run(&root, args.get(1).map(String::as_str) == Some("--check"))
        }
        Some("parity") => parity::run(&root, &args[1..]),
        _ => Err("usage: cargo xtask port-map [--check]\n       cargo xtask parity [--suite <name>]... [--filter <glob>] [--show <case id>]".to_owned()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("xtask: {message}");
            ExitCode::FAILURE
        }
    }
}

/// The workspace root: the parent of this crate's directory.
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask lives one level below the workspace root")
        .to_path_buf()
}
