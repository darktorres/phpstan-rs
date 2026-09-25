use super::*;
use crate::lexer::Lexer;
use crate::parser::Parser;

fn parse(src: &str) -> Vec<Stmt> {
    Parser::new(Lexer::new(src).tokenize_with_spans())
        .parse_program()
        .expect("parses")
}

/// Every variable name the walk reaches.
struct Variables(Vec<String>);

impl<'ast> Visitor<'ast> for Variables {
    fn visit_expr(&mut self, expr: &'ast Expr) {
        if let ExprKind::Variable(name) = &expr.kind {
            self.0.push(name.to_string());
        }
        walk_expr(self, expr);
    }
}

const EVERYWHERE: &str = r#"<?php
#[Attr($inAttr)]
function f(int $p = $inDefault) {
    $inBody;
    $c = function () use ($inUse) { return $inClosure; };
    $a = fn() => $inArrow;
    $o = new class($inCtorArg) {
        const K = $inConst;
        public $prop = $inPropDefault;
        public $hooked { get => $inHook; }
        public function m() { $inAnonMethod; }
    };
    static $s = $inStatic;
    match ($inSubject) { 1 => $inArm, default => $inDefaultArm };
}
enum E: int { case A = 1; const C = $inEnumConst; }
"#;

#[test]
fn the_walk_reaches_every_nested_scope_and_initializer() {
    let program = parse(EVERYWHERE);
    let mut v = Variables(Vec::new());
    walk_stmts(&mut v, &program);
    for expected in [
        "inAttr",
        "inDefault",
        "inBody",
        "inClosure",
        "inArrow",
        "inCtorArg",
        "inConst",
        "inPropDefault",
        "inHook",
        "inAnonMethod",
        "inStatic",
        "inSubject",
        "inArm",
        "inDefaultArm",
        "inEnumConst",
    ] {
        assert!(
            v.0.iter().any(|n| n == expected),
            "{expected} not visited: {:?}",
            v.0
        );
    }
}

/// Renames every variable `$x` to `$x_`.
struct Rename;

impl VisitorMut for Rename {
    fn visit_expr(&mut self, expr: &mut Expr) {
        if let ExprKind::Variable(name) = &mut expr.kind {
            *name = format!("{name}_").into();
        }
        walk_expr_mut(self, expr);
    }
}

#[test]
fn the_mutable_walk_reaches_the_same_nodes() {
    let mut program = parse(EVERYWHERE);
    walk_stmts_mut(&mut Rename, &mut program);
    let mut v = Variables(Vec::new());
    walk_stmts(&mut v, &program);
    assert!(!v.0.is_empty());
    assert!(v.0.iter().all(|n| n.ends_with('_')), "{:?}", v.0);
}
