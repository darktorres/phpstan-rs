//! PHP syntax: the lexer, the parser and the tree it builds, traversal,
//! name resolution and interned names.

#![cfg_attr(not(test), deny(clippy::print_stdout, clippy::print_stderr))]
// `Name` holds a cell its lowercase form is made into on first use; its
// `Hash` and `Eq` read only the allocation and the text.
#![allow(clippy::mutable_key_type)]

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
pub mod stats;
pub mod stack;
pub mod token;
pub mod visit;

pub use lexer::WatchList;
pub use lexer::{SyntaxMark, SyntaxMarks};
pub use parse::{
    FileFacts, ParsedFile, Source, WorkerScratch, parse, parse_expression_into_arenas,
    parse_watching, parse_watching_transient_with_scratch, parse_watching_with_scratch,
    parse_watching_with_worker_scratch, parse_with_names,
};
pub use span::LineIndex;
pub use token::{DocEntry, TokenKind, TokenPayload, TokenPayloadEntry, TokenStream};
