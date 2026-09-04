//! `phpstan-rs`'s library surface - every module the `phpstan-rs` binary
//! (`src/main.rs`) is built from, plus a small set of top-level re-exports
//! for the most commonly needed types, so an embedder can write
//! `phpstan_rs::Project`/`phpstan_rs::Checker` instead of reaching through
//! `phpstan_rs::project::Project`/`phpstan_rs::checker::Checker`. The full
//! module paths remain available either way.

pub mod ast;
pub mod ast_walk;
pub mod baseline;
pub mod checker;
pub mod composer;
pub mod config;
pub mod deps;
pub mod docblock;
pub mod lexer;
pub mod lsp;
pub mod output;
pub mod parser;
pub mod pest_config;
pub mod php_stubs;
pub mod php_version;
pub mod project;
pub mod regex_groups;
pub mod registry;
pub mod token;
pub mod types;

pub use checker::{Checker, Diagnostic, DiagnosticBuilder, DiagnosticFix};
pub use project::Project;
pub use registry::Registry;
