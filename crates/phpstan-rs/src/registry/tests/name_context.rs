//! `collect_class_name_case_refs` - the read-only, pair-collecting
//! sibling of `resolve_names_in_type`.

use super::super::*;
use syntax::name_resolution::NameContext;
use types::Name;
use types::{self, CallSiteVariance, Type};

fn ctx_for(namespace: &str) -> NameContext {
    let mut ctx = NameContext::default();
    ctx.set_namespace(namespace);
    ctx
}

#[test]
fn collects_a_representative_nested_type_s_own_class_name_refs() {
    // A `Union`/`ClassString`/`ArrayShape` combination:
    // `class-string<Foo>|array{owner: Bar}`.
    let ctx = ctx_for("App");
    let ty = types::parse_written_type("class-string<Foo>|array{owner: Bar}");
    let mut out = Vec::new();
    collect_class_name_case_refs(&ty, &ctx, &mut out);
    assert_eq!(
        out,
        vec![
            (Name::from("Foo"), Name::from("App\\Foo")),
            (Name::from("Bar"), Name::from("App\\Bar")),
        ]
    );
}

#[test]
fn skips_the_object_and_resource_keywords() {
    let ctx = ctx_for("App");
    for keyword in ["object", "resource"] {
        let ty = types::parse_written_type(keyword);
        let mut out = Vec::new();
        collect_class_name_case_refs(&ty, &ctx, &mut out);
        assert!(
            out.is_empty(),
            "keyword {keyword} unexpectedly collected: {out:?}"
        );
    }
}

#[test]
fn iterable_names_the_global_traversable() {
    let ty = resolve_names_in_type(types::parse_written_type("iterable"), &ctx_for("App"));
    assert_eq!(ty, types::parse_type_string("array|Traversable"));
}

#[test]
fn a_written_traversable_resolves_like_any_class_name() {
    let ctx = ctx_for("App");
    let ty = types::parse_written_type("Traversable<Edge>");
    let mut out = Vec::new();
    collect_class_name_case_refs(&ty, &ctx, &mut out);
    assert_eq!(
        out,
        vec![
            (Name::from("Traversable"), Name::from("App\\Traversable")),
            (Name::from("Edge"), Name::from("App\\Edge")),
        ]
    );
}

#[test]
fn a_closure_signature_names_the_global_closure() {
    let ty = resolve_names_in_type(
        types::parse_written_type("Closure(Foo): void"),
        &ctx_for("App"),
    );
    assert_eq!(ty, types::parse_type_string("Closure(App\\Foo): void"));
}

#[test]
fn recurses_through_a_variance_annotation() {
    let ctx = ctx_for("App");
    let ty = Type::variance_arg(
        CallSiteVariance::Covariant,
        Type::object("Foo".into(), Vec::new()),
    );
    let mut out = Vec::new();
    collect_class_name_case_refs(&ty, &ctx, &mut out);
    assert_eq!(out, vec![(Name::from("Foo"), Name::from("App\\Foo"))]);
}
