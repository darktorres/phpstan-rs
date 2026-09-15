//! `collect_class_name_case_refs` - the read-only, pair-collecting
//! sibling of `resolve_names_in_type`.

use super::super::*;
use crate::ast::Stmt;
use crate::types::{self, Type};

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
            ("Foo".to_string(), "App\\Foo".to_string()),
            ("Bar".to_string(), "App\\Bar".to_string()),
        ]
    );
}

#[test]
fn skips_the_object_traversable_and_resource_sentinels() {
    let ctx = ctx_for("App");
    for sentinel in ["object", "Traversable", "resource"] {
        let ty = Type::Object(sentinel.to_string(), Vec::new());
        let mut out = Vec::new();
        collect_class_name_case_refs(&ty, &ctx, &mut out);
        assert!(
            out.is_empty(),
            "sentinel {sentinel} unexpectedly collected: {out:?}"
        );
    }
}

#[test]
fn recurses_through_the_dollar_dollar_variance_sentinels_without_collecting_them() {
    let ctx = ctx_for("App");
    let ty = Type::Object(
        "$$covariant".to_string(),
        vec![Type::Object("Foo".to_string(), Vec::new())],
    );
    let mut out = Vec::new();
    collect_class_name_case_refs(&ty, &ctx, &mut out);
    assert_eq!(out, vec![("Foo".to_string(), "App\\Foo".to_string())]);
}
