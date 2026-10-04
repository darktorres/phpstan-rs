//! PHPDoc: the grammar that parses a doc comment's tags and type
//! expressions into `TypeNode`s, and the `DocBlock` a declaration's doc
//! comment reduces to.

#![cfg_attr(not(test), deny(clippy::print_stdout, clippy::print_stderr))]

mod canonical;
pub mod doc_type;
pub mod docblock;
pub mod grammar;

pub use doc_type::{DocType, WrittenType};

pub use syntax::memo::MemoStats;
