//! Phase 17's additive span fields: `name_span`/`type_hint_span`/
//! `extends_spans`/etc. - each must point at exactly the identifier text
//! it claims to, not just exist. `helpers::text_at` slices the original
//! source by a span to check that directly, rather than trusting the
//! span's own byte-range math is right just because it compiles.

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
        text_at(src, f.return_type_span.expect("return type span")),
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
        text_at(src, f.params[0].type_hint_span.expect("type hint span")),
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
        text_at(src, prop.type_hint_span.expect("prop type hint span")),
        "?int"
    );
    let cst = &c.consts[0];
    assert_eq!(text_at(src, cst.name_span), "LIMIT");
    assert_eq!(
        text_at(src, cst.type_hint_span.expect("const type hint span")),
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
