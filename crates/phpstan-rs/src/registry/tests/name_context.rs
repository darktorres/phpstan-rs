//! `collect_class_name_case_refs` - the read-only, pair-collecting
//! sibling of `resolve_names_in_type`.

use super::super::*;
use crate::types::{self, CallSiteVariance, Type};
use syntax::ast::Stmt;
use syntax::intern::intern;
use syntax::name_resolution::NameContext;

fn ctx_for(namespace: &str) -> NameContext {
    let stmts: Vec<Stmt> = Vec::new();
    let mut ctx = NameContext::build(&stmts);
    ctx.set_namespace(namespace);
    ctx
}

#[test]
fn collects_a_representative_nested_type_s_own_class_name_refs() {
    // A `Union`/`ClassString`/`ArrayShape` combination:
    // `class-string<Foo>|array{owner: Bar}`.
    let ctx = ctx_for("App");
    let ty = types::parse_type_string("class-string<Foo>|array{owner: Bar}");
    let mut out = Vec::new();
    collect_class_name_case_refs(&ty, &ctx, &mut out);
    assert_eq!(
        out,
        vec![
            (intern("Foo"), intern("App\\Foo")),
            (intern("Bar"), intern("App\\Bar")),
        ]
    );
}

#[test]
fn skips_the_object_traversable_and_resource_sentinels() {
    let ctx = ctx_for("App");
    for sentinel in ["object", "Traversable", "resource"] {
        let ty = types::parse_type_string(sentinel);
        let mut out = Vec::new();
        collect_class_name_case_refs(&ty, &ctx, &mut out);
        assert!(
            out.is_empty(),
            "sentinel {sentinel} unexpectedly collected: {out:?}"
        );
    }
}

#[test]
fn recurses_into_a_traversables_own_generic_argument_without_collecting_traversable_itself() {
    // Real shape: `bacon/bacon-qr-code`'s own `@return Traversable<Edge>`
    // - a real, programmer-written PHPDoc tag, not this project's own
    // internal `iterable`-desugaring synthetic use of `Traversable` -
    // `Edge`'s own case/namespace-qualification must still be collected.
    let ctx = ctx_for("App");
    let ty = Type::Object(
        "Traversable".into(),
        vec![Type::Object("Edge".into(), Vec::new())],
    );
    let mut out = Vec::new();
    collect_class_name_case_refs(&ty, &ctx, &mut out);
    assert_eq!(out, vec![(intern("Edge"), intern("App\\Edge"))]);
}

#[test]
fn recurses_through_a_variance_annotation() {
    let ctx = ctx_for("App");
    let ty = Type::VarianceArg(
        CallSiteVariance::Covariant,
        Box::new(Type::Object("Foo".into(), Vec::new())),
    );
    let mut out = Vec::new();
    collect_class_name_case_refs(&ty, &ctx, &mut out);
    assert_eq!(out, vec![(intern("Foo"), intern("App\\Foo"))]);
}
