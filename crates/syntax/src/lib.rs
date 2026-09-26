//! PHP syntax: the lexer, the parser and the tree it builds, traversal,
//! name resolution, parse-time annotations and interned names.

#![cfg_attr(not(test), deny(clippy::print_stdout, clippy::print_stderr))]

pub mod annotations;
pub mod ast;
pub mod intern;
pub mod lexer;
pub mod name;
pub mod name_resolution;
pub mod parser;
pub mod printer;
pub mod token;
pub mod visit;
