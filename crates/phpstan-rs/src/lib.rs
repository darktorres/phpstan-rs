//! `phpstan-rs`'s library surface - every module the `phpstan-rs` binary
//! (`src/main.rs`) is built from, plus a small set of top-level re-exports
//! for the most commonly needed types, so an embedder can write
//! `phpstan_rs::Project`/`phpstan_rs::Checker` instead of reaching through
//! `phpstan_rs::project::Project`/`phpstan_rs::checker::Checker`. The full
//! module paths remain available either way.

/// The allocator every project's tables are allocated from and freed to:
/// building and dropping a project is mostly allocation, and this serves
/// many threads' allocations and frees faster than the system's.
#[cfg(not(any(
    feature = "dhat-heap",
    feature = "hotpath-alloc",
    feature = "system-allocator"
)))]
#[global_allocator]
static ALLOCATOR: mimalloc::MiMalloc = mimalloc::MiMalloc;

pub mod app;
pub mod architecture;
pub mod assertion_tables;
pub mod banned;
pub mod baseline;
pub mod checker;
pub mod deps;
pub mod doctrine_functions;
pub mod ext;
pub mod gate;
pub mod ignore_comments;
pub mod ignore_errors;
pub mod lsp;
pub mod output;
pub mod parent_calls;
// PCRE2 is reached through its C API.
#[allow(unsafe_code)]
pub(crate) mod pcre_validate;
pub mod pest_config;
pub(crate) mod php_date_parser;
pub mod project;
pub mod reflection_rule;
pub mod regex_groups;
pub mod registry;
pub mod restricted_symbols;
pub mod rule;
pub mod rules_settings;
pub(crate) mod stub_attributes;

pub use checker::Checker;
pub use diagnostics::{Diagnostic, DiagnosticBuilder, DiagnosticFix};
pub use project::Project;
pub use registry::Registry;
