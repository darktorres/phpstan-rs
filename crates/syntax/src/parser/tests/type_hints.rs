//! Native type declarations: their structure, text and name spans.

use super::super::*;
use super::helpers::*;

fn param_hint(src: &str) -> TypeHint {
    let program = parse(src);
    let StmtKind::FunctionDecl(f) = &program[0].kind else {
        panic!("expected a function");
    };
    f.params[0].type_hint.clone().expect("a type hint")
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
        assert_eq!(param_hint(src).to_string(), text, "{src}");
    }
}

#[test]
fn a_dnf_type_is_a_union_of_an_intersection() {
    let hint = param_hint("<?php function f((A&B)|null $x) {}");
    let TypeHintKind::Union(members) = &hint.kind else {
        panic!("expected a union, got {hint:?}");
    };
    assert!(matches!(&members[0].kind, TypeHintKind::Intersection(parts) if parts.len() == 2));
    assert!(matches!(
        &members[1].kind,
        TypeHintKind::Keyword(NativeType::Null, _)
    ));
}

#[test]
fn class_names_skip_keyword_types_and_carry_their_own_spans() {
    let src = "<?php function f(?\\App\\Foo|Bar|int|null $x) {}";
    let hint = param_hint(src);
    let names: Vec<(&str, String)> = hint
        .class_names()
        .into_iter()
        .map(|(name, span)| (&*name.written, text_at(src, span)))
        .collect();
    assert_eq!(
        names,
        vec![
            ("\\App\\Foo", "\\App\\Foo".to_string()),
            ("Bar", "Bar".to_string())
        ]
    );
}
