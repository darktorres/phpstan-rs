//! Span fields like `Ident::span`/`TypeHint::span`/`extends_spans`/etc. -
//! each must point at exactly the identifier text it claims to, not just
//! exist. `helpers::text_at` slices the original source by a span to
//! check that directly, rather than trusting the span's own byte-range
//! math is right just because it compiles.

use super::super::*;
use super::helpers::{parse, text_at};

#[test]
fn function_decl_name_span_points_at_the_name_only() {
    let program = parse("<?php function greet(): void {}");
    let StmtKind::FunctionDecl(f) = &program[0].kind else {
        panic!("expected FunctionDecl");
    };
    let f = program.function(*f);
    assert_eq!(
        text_at("<?php function greet(): void {}", f.name.span),
        "greet"
    );
}

#[test]
fn function_decl_return_type_span_covers_the_whole_compound_hint() {
    let src = "<?php function f(): Foo|Bar {}";
    let program = parse(src);
    let StmtKind::FunctionDecl(f) = &program[0].kind else {
        panic!("expected FunctionDecl");
    };
    let f = program.function(*f);
    assert_eq!(
        text_at(
            src,
            f.return_type
                .as_ref()
                .map(|h| h.span)
                .expect("return type span")
        ),
        "Foo|Bar"
    );
}

#[test]
fn param_type_hint_span_points_at_the_hint_not_the_variable() {
    let src = "<?php function f(?Foo $x) {}";
    let program = parse(src);
    let StmtKind::FunctionDecl(f) = &program[0].kind else {
        panic!("expected FunctionDecl");
    };
    let f = program.function(*f);
    assert_eq!(
        text_at(
            src,
            program.arenas.params(f.params.clone())[0]
                .type_hint
                .as_ref()
                .map(|h| h.span)
                .expect("type hint span")
        ),
        "?Foo"
    );
}

#[test]
fn class_decl_name_extends_and_implements_spans_are_each_precise() {
    let src = "<?php class Widget extends Base implements Iface1, Iface2 {}";
    let program = parse(src);
    let StmtKind::ClassDecl(c) = &program[0].kind else {
        panic!("expected ClassDecl");
    };
    let c = program.arenas.class_decl(c.clone());
    assert_eq!(text_at(src, c.name.span), "Widget");
    assert_eq!(program.class_def(c.def).extends(&program.arenas).len(), 1);
    assert_eq!(
        text_at(
            src,
            program.class_def(c.def).extends(&program.arenas)[0].span
        ),
        "Base"
    );
    assert_eq!(
        program.class_def(c.def).implements(&program.arenas).len(),
        2
    );
    assert_eq!(
        text_at(
            src,
            program.class_def(c.def).implements(&program.arenas)[0].span
        ),
        "Iface1"
    );
    assert_eq!(
        text_at(
            src,
            program.class_def(c.def).implements(&program.arenas)[1].span
        ),
        "Iface2"
    );
}

#[test]
fn class_decl_uses_spans_point_at_each_trait_name() {
    let src = "<?php class Widget { use TraitA, TraitB; }";
    let program = parse(src);
    let StmtKind::ClassDecl(c) = &program[0].kind else {
        panic!("expected ClassDecl");
    };
    let c = program.arenas.class_decl(c.clone());
    assert_eq!(
        program.class_def(c.def).trait_uses(&program.arenas)[0]
            .traits(&program.arenas)
            .len(),
        2
    );
    assert_eq!(
        text_at(
            src,
            program.class_def(c.def).trait_uses(&program.arenas)[0].traits(&program.arenas)[0].span
        ),
        "TraitA"
    );
    assert_eq!(
        text_at(
            src,
            program.class_def(c.def).trait_uses(&program.arenas)[0].traits(&program.arenas)[1].span
        ),
        "TraitB"
    );
}

#[test]
fn property_and_class_const_name_and_type_hint_spans_are_precise() {
    let src = "<?php class Widget { public ?int $count; const int LIMIT = 5; }";
    let program = parse(src);
    let StmtKind::ClassDecl(c) = &program[0].kind else {
        panic!("expected ClassDecl");
    };
    let c = program.arenas.class_decl(c.clone());
    let prop = &program.class_def(c.def).properties(&program.arenas)[0];
    assert_eq!(text_at(src, prop.name.span), "$count");
    assert_eq!(
        text_at(
            src,
            prop.type_hint
                .as_ref()
                .map(|h| h.span)
                .expect("prop type hint span")
        ),
        "?int"
    );
    let cst = &program.class_def(c.def).consts(&program.arenas)[0];
    assert_eq!(text_at(src, cst.name.span), "LIMIT");
    assert_eq!(
        text_at(
            src,
            cst.type_hint
                .as_ref()
                .map(|h| h.span)
                .expect("const type hint span")
        ),
        "int"
    );
}

#[test]
fn enum_decl_name_implements_and_case_name_spans_are_precise() {
    let src = "<?php enum Suit implements HasColor { case Hearts; }";
    let program = parse(src);
    let StmtKind::ClassDecl(e) = &program[0].kind else {
        panic!("expected an enum");
    };
    let e = program.arenas.class_decl(e.clone());
    assert_eq!(program.class_def(e.def).kind, ClassKind::Enum);
    assert_eq!(text_at(src, e.name.span), "Suit");
    assert_eq!(
        program.class_def(e.def).implements(&program.arenas).len(),
        1
    );
    assert_eq!(
        text_at(
            src,
            program.class_def(e.def).implements(&program.arenas)[0].span
        ),
        "HasColor"
    );
    assert_eq!(
        text_at(
            src,
            program.class_def(e.def).enum_cases(&program.arenas)[0]
                .name
                .span
        ),
        "Hearts"
    );
}

#[test]
fn catch_clause_types_spans_point_at_each_exception_type() {
    let src = "<?php try {} catch (TypeErrorA | TypeErrorB $e) {}";
    let program = parse(src);
    let StmtKind::Try { catches, .. } = &program[0].kind else {
        panic!("expected Try");
    };
    let [catch] = program.arenas.catch_clauses(catches.clone()) else {
        panic!("expected one catch clause");
    };
    let types = program.arenas.qualified_names(&catch.types);
    assert_eq!(types.len(), 2);
    assert_eq!(text_at(src, types[0].span), "TypeErrorA");
    assert_eq!(text_at(src, types[1].span), "TypeErrorB");
}

#[test]
fn instanceof_target_name_span_points_at_the_class_name() {
    let src = "<?php $x instanceof Foo;";
    let program = parse(src);
    let StmtKind::Expression(e) = &program[0].kind else {
        panic!("expected Expression");
    };
    let e = program.expr(*e);
    let ExprKind::Instanceof(_, InstanceofTarget::Name(name)) = &e.kind else {
        panic!("expected Instanceof with a Name target");
    };
    let name = program
        .arenas
        .qualified_name(name)
        .expect("instanceof name belongs to the parsed file");
    assert_eq!(name.written, "Foo");
    assert_eq!(text_at(src, name.span), "Foo");
}

#[test]
fn member_names_and_static_targets_have_their_own_spans() {
    let src = "<?php self::LIMIT; Foo::bar(); Foo::$count; $o->run(Bar::X); $o->prop;";
    let program = parse(src);
    let exprs: Vec<&ExprNode> = program
        .iter()
        .map(|s| match &s.kind {
            StmtKind::Expression(e) => program.expr(*e),
            other => panic!("expected an expression statement, got {other:?}"),
        })
        .collect();
    let ExprKind::ClassConstFetch(StaticTarget::Name(class), name) = &exprs[0].kind else {
        panic!("expected a class constant fetch");
    };
    assert_eq!(text_at(src, class.span), "self");
    assert_eq!(text_at(src, name.span), "LIMIT");
    let ExprKind::StaticCall(StaticTarget::Name(class), name, _) = &exprs[1].kind else {
        panic!("expected a static call");
    };
    assert_eq!(text_at(src, class.span), "Foo");
    assert_eq!(text_at(src, name.span), "bar");
    let ExprKind::StaticPropertyFetch(_, name) = &exprs[2].kind else {
        panic!("expected a static property fetch");
    };
    assert_eq!(text_at(src, name.span), "$count");
    let ExprKind::MethodCall(_, name, _, _) = &exprs[3].kind else {
        panic!("expected a method call");
    };
    assert_eq!(text_at(src, name.span), "run");
    let ExprKind::PropertyAccess(_, name, _) = &exprs[4].kind else {
        panic!("expected a property access");
    };
    assert_eq!(text_at(src, name.span), "prop");
}

#[test]
fn an_interpolated_property_name_has_its_own_span() {
    let src = r#"<?php echo "a $obj->prop b";"#;
    let program = parse(src);
    let StmtKind::Echo(exprs) = &program[0].kind else {
        panic!("expected echo");
    };
    let exprs = program.arenas.expr_ids(exprs);
    let expression = program.expr(exprs[0]);
    let ExprKind::Interpolated(parts) = &expression.kind else {
        panic!("expected an interpolated string");
    };
    let access = program
        .arenas
        .expr_ids(parts)
        .iter()
        .find_map(|p| match &program.expr(*p).kind {
            ExprKind::PropertyAccess(_, name, _) => Some(name),
            _ => None,
        })
        .expect("a property access part");
    assert_eq!(text_at(src, access.span), "prop");
}

#[test]
fn spans_inside_an_interpolated_expression_are_real_even_in_a_closure() {
    let src = "<?php\necho \"x {$f(function () { return $inner; })} y\";";
    let program = parse(src);
    struct Find(Option<Span>);
    impl crate::visit::arena::Visitor for Find {
        fn visit_expr(&mut self, arenas: &NodeArenas, id: ExprId) {
            let Some(e) = arenas.expr(id) else {
                return;
            };
            if matches!(&e.kind, ExprKind::Variable(v) if &**v == "inner") {
                self.0 = arenas.expr_span(id);
            }
            crate::visit::arena::walk_expr(self, arenas, id);
        }
    }
    let mut find = Find(None);
    crate::visit::arena::walk_stmts(&mut find, &program.arenas, program.statements());
    assert_eq!(text_at(src, find.0.expect("found $inner")), "$inner");
}
