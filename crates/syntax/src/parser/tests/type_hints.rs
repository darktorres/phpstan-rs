//! Native type declarations: their structure, text and name spans.

use super::super::*;
use super::helpers::text_at;

fn param_hint(src: &str) -> (NodeArenas, TypeHint) {
    let parsed = crate::parse(src.as_bytes()).expect("parses");
    let stmt = parsed
        .arenas
        .stmt(parsed.statements()[0])
        .expect("function statement");
    let StmtKind::FunctionDecl(f) = &stmt.kind else {
        panic!("expected a function");
    };
    let hint = parsed.arenas.params(
        parsed
            .arenas
            .function(*f)
            .expect("function payload")
            .params
            .clone(),
    )[0]
    .type_hint
    .clone()
    .expect("a type hint");
    (parsed.arenas, hint)
}

#[test]
fn a_type_hint_renders_as_written_without_spaces() {
    for (src, text) in [
        ("<?php function f(int $x) {}", "int"),
        ("<?php function f(?\\App\\Foo $x) {}", "?\\App\\Foo"),
        ("<?php function f(int | string $x) {}", "int|string"),
        ("<?php function f(A&B $x) {}", "A&B"),
        ("<?php function f((A&B)|null $x) {}", "(A&B)|null"),
        ("<?php function f(null|(A & B)|C $x) {}", "null|(A&B)|C"),
    ] {
        let (arenas, hint) = param_hint(src);
        assert_eq!(hint.to_string_with(&arenas), text, "{src}");
    }
}

#[test]
fn a_dnf_type_is_a_union_of_an_intersection() {
    let (arenas, hint) = param_hint("<?php function f((A&B)|null $x) {}");
    let TypeHintKind::Union(members) = &hint.kind else {
        panic!("expected a union, got {hint:?}");
    };
    let members = arenas.type_hints(members);
    assert!(
        matches!(&members[0].kind, TypeHintKind::Intersection(parts) if arenas.type_hints(parts).len() == 2)
    );
    assert!(matches!(
        &members[1].kind,
        TypeHintKind::Keyword(NativeType::Null, _)
    ));
}

#[test]
fn class_names_skip_keyword_types_and_carry_their_own_spans() {
    let src = "<?php function f(?\\App\\Foo|Bar|int|null $x) {}";
    let (arenas, hint) = param_hint(src);
    let names: Vec<(&str, String)> = hint
        .class_names(&arenas)
        .into_iter()
        .map(|name| (&*name.written, text_at(src, name.span)))
        .collect();
    assert_eq!(
        names,
        vec![
            ("\\App\\Foo", "\\App\\Foo".to_string()),
            ("Bar", "Bar".to_string())
        ]
    );
}
