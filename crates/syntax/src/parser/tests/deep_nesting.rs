//! Input nested far deeper than a 256 KiB thread's stack can hold. Each parse
//! and walk goes through the stack guard, so the depth costs heap segments
//! and no input aborts the process.

use super::super::*;
use crate::lexer::Lexer;
use crate::visit::{self, Visitor};

const DEPTH: usize = 100_000;
const SMALL_STACK: usize = 256 * 1024;

/// Counts the statements and expressions a walk reaches.
#[derive(Default)]
struct Count(usize);

impl<'ast> Visitor<'ast> for Count {
    fn visit_stmt(&mut self, stmt: &'ast Stmt) {
        self.0 += 1;
        stack::guard(|| visit::walk_stmt(self, stmt));
    }

    fn visit_expr(&mut self, expr: &'ast Expr) {
        self.0 += 1;
        stack::guard(|| visit::walk_expr(self, expr));
    }
}

/// Parses and walks `src` on a thread with a small stack, returning the
/// number of nodes the walk reached.
fn parse_and_walk(src: String) -> usize {
    std::thread::Builder::new()
        .stack_size(SMALL_STACK)
        .spawn(move || {
            let program = Parser::new(Lexer::new(&src).tokenize_with_spans())
                .parse_program()
                .expect("the input parses");
            let mut count = Count::default();
            visit::walk_stmts(&mut count, &program);
            count.0
        })
        .expect("the thread spawns")
        .join()
        .expect("the parse and walk do not overflow")
}

#[test]
fn deeply_nested_parentheses_parse_and_walk() {
    let src = format!("<?php {}1{};", "(".repeat(DEPTH), ")".repeat(DEPTH));
    assert!(parse_and_walk(src) >= DEPTH);
}

#[test]
fn deeply_nested_array_literals_parse_and_walk() {
    let src = format!("<?php {}1{};", "[".repeat(DEPTH), "]".repeat(DEPTH));
    assert!(parse_and_walk(src) >= DEPTH);
}

#[test]
fn deeply_nested_calls_parse_and_walk() {
    let src = format!("<?php {}1{};", "f(".repeat(DEPTH), ")".repeat(DEPTH));
    assert!(parse_and_walk(src) >= DEPTH);
}

#[test]
fn deeply_nested_blocks_parse_and_walk() {
    let src = format!("<?php {}{};", "{".repeat(DEPTH), "}".repeat(DEPTH));
    assert!(parse_and_walk(src) >= DEPTH);
}

#[test]
fn a_long_left_associative_chain_parses_and_walks() {
    let src = format!("<?php 1{};", " + 1".repeat(DEPTH));
    assert!(parse_and_walk(src) >= DEPTH);
}
