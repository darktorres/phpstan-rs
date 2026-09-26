//! SQL reflection, standalone: a project's `[sql_reflection]` config, a
//! static schema catalog parsed from a DDL dump, live query reflection
//! (prepare, never execute) against MySQL, Postgres or SQLite, the drift
//! check between the two, and the `SqlContext` service answering what
//! columns a query's result set has - in its own column model.

#![deny(clippy::print_stdout, clippy::print_stderr)]

pub mod column;
pub mod config;
pub mod context;
pub mod drift;
pub mod reflector;
pub mod schema;
