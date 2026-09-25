//! `phpstan-rs`'s library surface - every module the `phpstan-rs` binary
//! (`src/main.rs`) is built from, plus a small set of top-level re-exports
//! for the most commonly needed types, so an embedder can write
//! `phpstan_rs::Project`/`phpstan_rs::Checker` instead of reaching through
//! `phpstan_rs::project::Project`/`phpstan_rs::checker::Checker`. The full
//! module paths remain available either way.

pub mod architecture;
pub mod assertion_tables;
pub mod banned;
pub mod baseline;
pub mod checker;
pub mod composer;
pub mod config;
pub mod deps;
pub mod diagnostic;
pub mod docblock;
pub mod doctrine_functions;
pub mod fix;
pub mod ignore_comments;
pub mod ignore_errors;
pub mod lsp;
pub mod output;
pub mod parent_calls;
pub(crate) mod pcre_validate;
pub mod pest_config;
pub(crate) mod php_date_parser;
pub mod php_stubs;
pub mod phpdoc_grammar;
pub mod project;
pub mod reflection_rule;
pub mod regex_groups;
pub mod registry;
pub mod restricted_symbols;
pub mod rule;
pub mod sql_reflection_config;
pub mod sql_reflection_reflector;
pub mod sql_reflection_schema;
pub mod sql_reflection_schema_drift;
pub(crate) mod stub_attributes;
pub mod types;

pub use checker::Checker;
pub use diagnostic::{Diagnostic, DiagnosticBuilder, DiagnosticFix};
pub use project::Project;
pub use registry::Registry;
