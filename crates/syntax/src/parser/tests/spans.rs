//! Span fields like `name_span`/`TypeHint::span`/`extends_spans`/etc. -
//! each must point at exactly the identifier text it claims to, not just
//! exist. `helpers::text_at` slices the original source by a span to
//! check that directly, rather than trusting the span's own byte-range
//! math is right just because it compiles.

use super::super::*;
use super::helpers::*;

#[test]
fn function_decl_name_span_points_at_the_name_only() {
    let program = parse("<?php function greet(): void {}");
    let StmtKind::FunctionDecl(f) = &program[0].kind else {
        panic!("expected FunctionDecl");
    };
    assert_eq!(
        text_at("<?php function greet(): void {}", f.name_span),
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
    assert_eq!(
        text_at(
            src,
            f.params[0]
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
    assert_eq!(text_at(src, c.name_span), "Widget");
    assert_eq!(c.extends_spans.len(), 1);
    assert_eq!(text_at(src, c.extends_spans[0]), "Base");
    assert_eq!(c.implements_spans.len(), 2);
    assert_eq!(text_at(src, c.implements_spans[0]), "Iface1");
    assert_eq!(text_at(src, c.implements_spans[1]), "Iface2");
}

#[test]
fn class_decl_uses_spans_point_at_each_trait_name() {
    let src = "<?php class Widget { use TraitA, TraitB; }";
    let program = parse(src);
    let StmtKind::ClassDecl(c) = &program[0].kind else {
        panic!("expected ClassDecl");
    };
    assert_eq!(c.uses_spans.len(), 2);
    assert_eq!(text_at(src, c.uses_spans[0]), "TraitA");
    assert_eq!(text_at(src, c.uses_spans[1]), "TraitB");
}

#[test]
fn property_and_class_const_name_and_type_hint_spans_are_precise() {
    let src = "<?php class Widget { public ?int $count; const int LIMIT = 5; }";
    let program = parse(src);
    let StmtKind::ClassDecl(c) = &program[0].kind else {
        panic!("expected ClassDecl");
    };
    let prop = &c.properties[0];
    assert_eq!(text_at(src, prop.name_span), "$count");
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
    let cst = &c.consts[0];
    assert_eq!(text_at(src, cst.name_span), "LIMIT");
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
    let StmtKind::EnumDecl(e) = &program[0].kind else {
        panic!("expected EnumDecl");
    };
    assert_eq!(text_at(src, e.name_span), "Suit");
    assert_eq!(e.implements_spans.len(), 1);
    assert_eq!(text_at(src, e.implements_spans[0]), "HasColor");
    assert_eq!(text_at(src, e.cases[0].name_span), "Hearts");
}

#[test]
fn catch_clause_types_spans_point_at_each_exception_type() {
    let src = "<?php try {} catch (TypeErrorA | TypeErrorB $e) {}";
    let program = parse(src);
    let StmtKind::Try { catches, .. } = &program[0].kind else {
        panic!("expected Try");
    };
    assert_eq!(catches[0].types_spans.len(), 2);
    assert_eq!(text_at(src, catches[0].types_spans[0]), "TypeErrorA");
    assert_eq!(text_at(src, catches[0].types_spans[1]), "TypeErrorB");
}

#[test]
fn instanceof_target_name_span_points_at_the_class_name() {
    let src = "<?php $x instanceof Foo;";
    let program = parse(src);
    let StmtKind::Expression(e) = &program[0].kind else {
        panic!("expected Expression");
    };
    let ExprKind::Instanceof(_, InstanceofTarget::Name(name, span)) = &e.kind else {
        panic!("expected Instanceof with a Name target");
    };
    assert_eq!(name, "Foo");
    assert_eq!(text_at(src, *span), "Foo");
}

#[test]
fn member_names_and_static_targets_have_their_own_spans() {
    let src = "<?php self::LIMIT; Foo::bar(); Foo::$count; $o->run(Bar::X); $o->prop;";
    let program = parse(src);
    let exprs: Vec<&Expr> = program
        .iter()
        .map(|s| match &s.kind {
            StmtKind::Expression(e) => e,
            other => panic!("expected an expression statement, got {other:?}"),
        })
        .collect();
    let ExprKind::ClassConstFetch(StaticTarget::Name(_, class_span), name) = &exprs[0].kind else {
        panic!("expected a class constant fetch");
    };
    assert_eq!(text_at(src, *class_span), "self");
    assert_eq!(text_at(src, name.span), "LIMIT");
    let ExprKind::StaticCall(StaticTarget::Name(_, class_span), name, _) = &exprs[1].kind else {
        panic!("expected a static call");
    };
    assert_eq!(text_at(src, *class_span), "Foo");
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
    let ExprKind::Interpolated(parts) = &exprs[0].kind else {
        panic!("expected an interpolated string");
    };
    let access = parts
        .iter()
        .find_map(|p| match &p.kind {
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
    impl<'ast> crate::visit::Visitor<'ast> for Find {
        fn visit_expr(&mut self, e: &'ast Expr) {
            if matches!(&e.kind, ExprKind::Variable(v) if &**v == "inner") {
                self.0 = Some(e.span);
            }
            crate::visit::walk_expr(self, e);
        }
    }
    let mut find = Find(None);
    crate::visit::walk_stmts(&mut find, &program);
    assert_eq!(text_at(src, find.0.expect("found $inner")), "$inner");
}
