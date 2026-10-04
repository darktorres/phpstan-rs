//! PHP syntax: the lexer, the parser and the tree it builds, traversal,
//! name resolution and interned names.

#![cfg_attr(not(test), deny(clippy::print_stdout, clippy::print_stderr))]

pub mod ast;
pub mod canonical;
pub mod declarations;
pub mod lexer;
pub mod memo;
pub mod name;
pub mod name_resolution;
mod parse;
pub mod parser;
pub mod php_float;
pub mod php_string;
pub mod printer;
pub mod references;
pub mod span;
pub mod stack;
pub mod token;
pub mod visit;

pub use lexer::WatchList;
pub use lexer::{SyntaxMark, SyntaxMarks};
pub use parse::{FileFacts, ParsedFile, Source, parse, parse_watching, parse_with_names};
pub use span::LineIndex;
