//! PHP syntax: the lexer, the parser and the tree it builds, traversal,
//! name resolution, parse-time annotations and interned names.

#![cfg_attr(not(test), deny(clippy::print_stdout, clippy::print_stderr))]

pub mod annotations;
pub mod ast;
pub mod declarations;
pub mod lexer;
pub mod name;
pub mod name_resolution;
mod parse;
pub mod parser;
pub mod php_float;
pub mod php_string;
pub mod printer;
pub mod stack;
pub mod token;
pub mod visit;

pub use parse::{Comment, ParsedFile, parse};
