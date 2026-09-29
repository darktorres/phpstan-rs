use super::*;
use crate::lexer::Lexer;
use crate::parser::Parser;
use crate::token::Position;
use crate::visit::{Visitor, walk_expr, walk_stmts};

fn parse(src: &str) -> Vec<Stmt> {
    Parser::new(Lexer::new(src).tokenize_with_spans())
        .parse_program()
        .expect("parses")
}

/// Every name reference in the file with its resolution, in walk order.
fn resolutions(src: &str) -> Vec<(String, ResolvedName)> {
    struct Collect(Vec<(String, ResolvedName)>);
    impl Collect {
        fn push(&mut self, name: &QualifiedName) {
            self.0
                .push((name.written.to_string(), name.resolved.clone()));
        }
    }
    impl<'ast> Visitor<'ast> for Collect {
        fn visit_expr(&mut self, e: &'ast Expr) {
            match &e.kind {
                ExprKind::Name(n) => self.push(n),
                ExprKind::ClassConstFetch(StaticTarget::Name(n), _)
                | ExprKind::StaticCall(StaticTarget::Name(n), _, _)
                | ExprKind::Instanceof(_, InstanceofTarget::Name(n)) => self.push(n),
                _ => {}
            }
            walk_expr(self, e);
        }
        fn visit_class_decl(&mut self, c: &'ast ClassDecl) {
            for n in c.def.extends.iter().chain(&c.def.implements) {
                self.push(n);
            }
            crate::visit::walk_class_decl(self, c);
        }
        fn visit_attribute(&mut self, a: &'ast Attribute) {
            self.push(&a.name);
        }
        fn visit_catch_clause(&mut self, c: &'ast CatchClause) {
            for n in &c.types {
                self.push(n);
            }
            crate::visit::walk_catch_clause(self, c);
        }
        fn visit_type_hint(&mut self, h: &'ast TypeHint) {
            if let TypeHintKind::Name(n) = &h.kind {
                self.push(n);
            }
        }
    }
    let program = parse(src);
    let mut c = Collect(Vec::new());
    walk_stmts(&mut c, &program);
    c.0
}

fn exact(s: &str) -> ResolvedName {
    ResolvedName::Exact(s.into())
}

fn find<'a>(all: &'a [(String, ResolvedName)], written: &str) -> &'a ResolvedName {
    &all.iter()
        .find(|(w, _)| w == written)
        .unwrap_or_else(|| panic!("{written} not found in {all:?}"))
        .1
}

#[test]
fn names_resolve_through_the_namespace_and_imports() {
    let all = resolutions(
        r#"<?php
        namespace App;
        use Lib\Thing;
        use Lib\Other as Alias;
        use function Lib\helper;
        use const Lib\LIMIT;
        #[Attr]
        class C extends Thing implements Alias {
            public function m(\Absolute $a, Local $b, int $c, self $d, integer $e): void {
                new Thing();
                helper();
                strlen('x');
                LIMIT;
                PHP_EOL;
                Alias::X;
                Sub\Deep::run();
                try {} catch (\Exception | Thing $e) {}
                $x instanceof Thing;
                parent::m();
            }
        }
        "#,
    );
    assert_eq!(find(&all, "Attr"), &exact("App\\Attr"));
    assert_eq!(find(&all, "Thing"), &exact("Lib\\Thing"));
    assert_eq!(find(&all, "Alias"), &exact("Lib\\Other"));
    assert_eq!(find(&all, "\\Absolute"), &exact("Absolute"));
    assert_eq!(find(&all, "Local"), &exact("App\\Local"));
    // A native keyword type isn't a name; `integer` is a class to PHP.
    assert!(all.iter().all(|(w, _)| w != "int" && w != "self"));
    assert_eq!(find(&all, "integer"), &exact("App\\integer"));
    assert_eq!(find(&all, "parent"), &ResolvedName::Unresolved);
    assert_eq!(find(&all, "helper"), &exact("Lib\\helper"));
    assert_eq!(
        find(&all, "strlen"),
        &ResolvedName::WithGlobalFallback("App\\strlen".into())
    );
    assert_eq!(find(&all, "LIMIT"), &exact("Lib\\LIMIT"));
    assert_eq!(
        find(&all, "PHP_EOL"),
        &ResolvedName::WithGlobalFallback("App\\PHP_EOL".into())
    );
    assert_eq!(find(&all, "Sub\\Deep"), &exact("App\\Sub\\Deep"));
    assert_eq!(find(&all, "\\Exception"), &exact("Exception"));
}

#[test]
fn a_later_namespace_resolves_what_follows_it_and_imports_do_not_leak() {
    let all = resolutions(
        r#"<?php
        namespace A { use X\Y; new Y(); }
        namespace B { new Y(); }
        "#,
    );
    let ys: Vec<&ResolvedName> = all
        .iter()
        .filter(|(w, _)| w == "Y")
        .map(|(_, r)| r)
        .collect();
    assert_eq!(ys, vec![&exact("X\\Y"), &exact("B\\Y")]);
}

#[test]
fn outside_any_namespace_a_function_has_no_fallback() {
    let all = resolutions("<?php strlen('x');");
    assert_eq!(find(&all, "strlen"), &exact("strlen"));
}

#[test]
fn declarations_carry_the_namespace_they_are_declared_in() {
    let program = parse(
        "<?php\nnamespace A;\nclass C {}\nconst K = 1;\nnamespace B {\n    enum E {}\n    if (true) {\n        function f() {}\n    }\n}\n",
    );
    let mut names = Vec::new();
    for_each_in_context(&program, |stmt, _| match &stmt.kind {
        StmtKind::ClassDecl(c) => names.push(Some(c.namespaced_name.clone())),
        StmtKind::GlobalConst(consts) => {
            names.extend(consts.iter().map(|c| c.namespaced_name.clone()))
        }
        StmtKind::If(IfStmt { then_branch, .. }) => {
            let StmtKind::Block(body) = &then_branch.kind else {
                panic!("expected a block, got {:?}", then_branch.kind);
            };
            let StmtKind::FunctionDecl(f) = &body[0].kind else {
                panic!("expected a function, got {:?}", body[0].kind);
            };
            names.push(f.namespaced_name.clone());
        }
        _ => {}
    });
    let names: Vec<Option<&str>> = names.iter().map(Option::as_deref).collect();
    assert_eq!(
        names,
        [Some("A\\C"), Some("A\\K"), Some("B\\E"), Some("B\\f")]
    );
}

#[test]
fn the_context_at_a_position_has_only_what_precedes_it() {
    let program = parse("<?php\nnamespace A;\nuse X\\Y;\nnamespace B;\nuse Z\\Y;\n");
    let at = |line| context_at(&program, Position { line, col: 1 });
    assert_eq!(at(3).resolve("Y"), "A\\Y");
    assert_eq!(at(4).resolve("Y"), "X\\Y");
    assert_eq!(at(6).resolve("Y"), "Z\\Y");
}
