mod children_by_node_kind;
mod helpers;

use super::*;
use crate::lexer::Lexer;
use crate::parser::Parser;
use crate::visit::arena::{self, Visitor as ArenaVisitor};

fn parse(src: &str) -> crate::parser::ParsedProgram {
    Parser::from_token_stream(Lexer::new(src).lex_token_stream())
        .parse_program()
        .expect("parses")
}

/// Every variable name the walk reaches.
struct Variables(Vec<String>);

impl ArenaVisitor for Variables {
    fn visit_expr(&mut self, arenas: &NodeArenas, id: ExprId) {
        let Some(expr) = arenas.expr(id) else {
            return;
        };
        if let ExprKind::Variable(name) = &expr.kind {
            self.0.push(name.to_string());
        }
        arena::walk_expr(self, arenas, id);
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
    arena::walk_stmts(&mut v, &program.arenas, program.statements());
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

#[test]
fn a_function_body_search_stays_in_its_scope_but_visits_anonymous_class_args() {
    let program = parse(
        "<?php function f() { $outer; $c = function () { $inner; }; new class($arg) { public $p = $member; }; }",
    );
    let StmtKind::FunctionDecl(function) =
        &program.arenas.stmt(program.statements()[0]).unwrap().kind
    else {
        panic!("expected a function declaration");
    };
    let function = program
        .arenas
        .function(*function)
        .expect("function declaration payload");
    let body = function.body_stmts(&program.arenas);
    let has_variable = |wanted: &str| {
        arena::any_expr_in_body(
            &program.arenas,
            body,
            |expr| matches!(&expr.kind, ExprKind::Variable(name) if name.as_str() == wanted),
        )
    };
    assert!(has_variable("outer"));
    assert!(has_variable("arg"));
    assert!(!has_variable("inner"));
    assert!(!has_variable("member"));
}

#[test]
fn every_name_and_closure_use_is_visited() {
    struct Names(Vec<String>);
    impl ArenaVisitor for Names {
        fn visit_name(&mut self, name: &QualifiedName) {
            self.0.push(name.written.to_string());
        }
        fn visit_closure_use(&mut self, closure_use: &ClosureUse) {
            self.0.push(format!("${}", closure_use.name));
        }
    }
    let program = parse(
        "<?php #[Attr] class C extends P implements I { use T { T::m insteadof U; } }\n\
         try { f(K, A::B, $x instanceof D); } catch (E $e) {}\n\
         $g = function () use ($u) {};",
    );
    let mut names = Names(Vec::new());
    arena::walk_stmts(&mut names, &program.arenas, program.statements());
    names.0.sort();
    assert_eq!(
        names.0,
        [
            "$u", "A", "Attr", "D", "E", "I", "K", "P", "T", "T", "U", "f"
        ]
    );
}
