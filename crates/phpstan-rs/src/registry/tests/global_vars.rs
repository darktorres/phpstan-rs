//! `Registry::global_variables` - a `global $x;`-declared variable's own
//! inferred type, folded from every real assignment site to it anywhere
//! in the corpus, replacing the unconditional `Mixed` fallback.

use std::path::Path;

use super::super::*;
use crate::types::Type;
use syntax::ast::Stmt;
use syntax::lexer::Lexer;
use syntax::parser::Parser;

fn parse(src: &str) -> Vec<Stmt> {
    let tokens = Lexer::new(src).tokenize_with_spans();
    Parser::new(tokens).parse_program().unwrap()
}

/// Builds a `Registry` with every function/class across `sources`
/// indexed (via one combined `Registry::build`, so cross-file lookups
/// resolve), then re-runs `infer_global_variables` with `sources` kept as
/// genuinely separate files - real per-file `NameContext` isolation and
/// include-graph resolution, not one concatenated blob.
fn build_with_files(sources: &[(&str, &str)]) -> Registry {
    let parsed: Vec<(String, Vec<Stmt>)> = sources
        .iter()
        .map(|(path, src)| (path.to_string(), parse(src)))
        .collect();
    let combined: Vec<Stmt> = parsed.iter().flat_map(|(_, p)| p.clone()).collect();
    let mut registry = Registry::build(&combined);
    let files: Vec<(&Path, &[Stmt])> = parsed
        .iter()
        .map(|(path, p)| (Path::new(path.as_str()), p.as_slice()))
        .collect();
    registry.infer_global_variables(&files);
    registry
}

fn global_type<'a>(registry: &'a Registry, name: &str) -> Option<&'a Type> {
    registry.global_variables.get(name)
}

#[test]
fn a_single_class_object_global_resolves_to_that_class() {
    let registry = build_with_files(&[(
        "a.php",
        r#"<?php
            class Template {}
            $template = new Template();
        "#,
    )]);
    assert_eq!(
        global_type(&registry, "template"),
        Some(&Type::Object("Template".into(), vec![]))
    );
}

#[test]
fn multiple_distinct_classes_across_sites_resolve_to_a_union() {
    let registry = build_with_files(&[(
        "a.php",
        r#"<?php
            class Foo {}
            class Bar {}
            $template = new Foo();
            $template = new Bar();
        "#,
    )]);
    match global_type(&registry, "template") {
        Some(Type::Union(members)) => {
            assert_eq!(members.len(), 2, "got: {members:?}");
            assert!(members.contains(&Type::Object("Foo".into(), vec![])));
            assert!(members.contains(&Type::Object("Bar".into(), vec![])));
        }
        other => panic!("expected a Union of Foo/Bar, got: {other:?}"),
    }
}

#[test]
fn an_array_literal_assigned_global_accumulates_shape_fields_across_files() {
    let registry = build_with_files(&[
        ("a.php", "<?php $conf = array('a' => 1);"),
        ("b.php", "<?php $conf['b'] = 'x';"),
    ]);
    match global_type(&registry, "conf") {
        Some(Type::ArrayShape(fields)) => {
            let fields = fields.named_fields();
            let names: Vec<&str> = fields.iter().map(|(k, _, _)| k.as_str()).collect();
            assert!(names.contains(&"a"), "got: {names:?}");
            assert!(names.contains(&"b"), "got: {names:?}");
        }
        other => panic!("expected an ArrayShape with a/b fields, got: {other:?}"),
    }
}

#[test]
fn a_local_variable_never_declared_global_does_not_contribute() {
    let registry = build_with_files(&[(
        "a.php",
        r#"<?php
            function f() {
                $template = "not a global";
            }
        "#,
    )]);
    assert_eq!(global_type(&registry, "template"), None);
}

#[test]
fn a_function_body_site_requires_its_own_global_declaration() {
    let registry = build_with_files(&[(
        "a.php",
        r#"<?php
            class Template {}
            function f() {
                global $template;
                $template = new Template();
            }
        "#,
    )]);
    assert_eq!(
        global_type(&registry, "template"),
        Some(&Type::Object("Template".into(), vec![]))
    );
}

#[test]
fn a_closures_own_global_does_not_leak_to_the_outer_scope() {
    let registry = build_with_files(&[(
        "a.php",
        r#"<?php
            class Template {}
            function f() {
                $inner = function () {
                    global $template;
                    $template = new Template();
                };
            }
        "#,
    )]);
    // `$template` is only ever assigned inside the closure's own scope -
    // `f`'s own body never declares `global $template;` itself, but that
    // doesn't matter here since the assignment is inside the closure,
    // which does declare it. This confirms the closure's own site is
    // still found (it's its own valid scope), not that it leaks anywhere.
    assert_eq!(
        global_type(&registry, "template"),
        Some(&Type::Object("Template".into(), vec![]))
    );
}

#[test]
fn a_closure_that_does_not_redeclare_global_does_not_see_the_outer_scope_site() {
    let registry = build_with_files(&[(
        "a.php",
        r#"<?php
            class Template {}
            class Other {}
            function f() {
                global $template;
                $template = new Template();
                $inner = function () {
                    // No `global $template;` here - this assignment is to
                    // a local variable shadowing the name, not the global.
                    $template = new Other();
                };
            }
        "#,
    )]);
    assert_eq!(
        global_type(&registry, "template"),
        Some(&Type::Object("Template".into(), vec![]))
    );
}

#[test]
fn new_anonymous_class_methods_get_the_same_scope_boundary_treatment() {
    let registry = build_with_files(&[(
        "a.php",
        r#"<?php
            class Template {}
            function f() {
                $obj = new class {
                    public function m() {
                        global $template;
                        $template = new Template();
                    }
                };
            }
        "#,
    )]);
    assert_eq!(
        global_type(&registry, "template"),
        Some(&Type::Object("Template".into(), vec![]))
    );
}

#[test]
fn a_shape_field_picks_up_a_helpers_registry_resolved_return_type() {
    let registry = build_with_files(&[(
        "a.php",
        r#"<?php
            class Foo {}
            function someHelper(): Foo { return new Foo(); }
            $g['key'] = someHelper();
        "#,
    )]);
    match global_type(&registry, "g") {
        Some(Type::ArrayShape(fields)) => {
            let fields = fields.named_fields();
            let (_, ty, _) = fields
                .iter()
                .find(|(k, _, _)| k == "key")
                .expect("key field");
            assert_eq!(ty, &Type::Object("Foo".into(), vec![]));
        }
        other => panic!("expected an ArrayShape, got: {other:?}"),
    }
}

#[test]
fn a_static_factory_call_and_a_class_constant_both_resolve() {
    let registry = build_with_files(&[(
        "a.php",
        r#"<?php
            class Foo {
                const BAR = 1;
                static function factory(): Foo { return new Foo(); }
            }
            $g1 = Foo::factory();
            $g2 = Foo::BAR;
        "#,
    )]);
    assert_eq!(
        global_type(&registry, "g1"),
        Some(&Type::Object("Foo".into(), vec![]))
    );
    assert_eq!(global_type(&registry, "g2"), Some(&Type::IntLiteral(1)));
}

#[test]
fn array_merge_with_self_folds_the_calls_resolved_shape_fields_in() {
    let registry = build_with_files(&[(
        "a.php",
        r#"<?php
            /**
             * @return array{b: int}
             */
            function extra(): array {
                return array('b' => 2);
            }
            $g = array('a' => 1);
            $g = array_merge($g, extra());
        "#,
    )]);
    match global_type(&registry, "g") {
        Some(Type::ArrayShape(fields)) => {
            let fields = fields.named_fields();
            let names: Vec<&str> = fields.iter().map(|(k, _, _)| k.as_str()).collect();
            assert!(names.contains(&"a"), "got: {names:?}");
            assert!(names.contains(&"b"), "got: {names:?}");
        }
        other => panic!("expected an ArrayShape with a/b fields, got: {other:?}"),
    }
}

#[test]
fn coalesce_assign_contributes_the_rhs_type_but_compound_concat_assign_is_mixed() {
    let registry = build_with_files(&[(
        "a.php",
        r#"<?php
            $g['a'] ??= 'default';
            $g['b'] .= 'suffix';
        "#,
    )]);
    match global_type(&registry, "g") {
        Some(Type::ArrayShape(fields)) => {
            let fields = fields.named_fields();
            let a = &fields.iter().find(|(k, _, _)| k == "a").expect("a field").1;
            let b = &fields.iter().find(|(k, _, _)| k == "b").expect("b field").1;
            assert_eq!(a, &Type::StringLiteral("default".to_string()));
            assert_eq!(b, &Type::Mixed(false));
        }
        other => panic!("expected an ArrayShape, got: {other:?}"),
    }
}

#[test]
fn a_genuine_object_and_array_conflict_resolves_to_a_union_not_mixed() {
    let registry = build_with_files(&[(
        "a.php",
        r#"<?php
            class Foo {}
            $g = new Foo();
            $g = array('a' => 1);
        "#,
    )]);
    match global_type(&registry, "g") {
        Some(Type::Union(members)) => {
            assert!(members.contains(&Type::Object("Foo".into(), vec![])));
            assert!(members.iter().any(|m| matches!(m, Type::ArrayShape(_))));
        }
        other => panic!("expected a Union of Object/ArrayShape, got: {other:?}"),
    }
}

#[test]
fn exceeding_the_shape_field_cap_degrades_to_array_mixed_not_mixed() {
    let mut src = String::from("<?php\n");
    for i in 0..100 {
        src.push_str(&format!("$g['k{i}'] = {i};\n"));
    }
    let registry = build_with_files(&[("a.php", &src)]);
    match global_type(&registry, "g") {
        Some(Type::Array(_, value)) => {
            assert_eq!(**value, Type::Mixed(false));
        }
        other => panic!("expected a plain Array(Mixed) after overflow, got: {other:?}"),
    }
}

#[test]
fn a_top_level_assignment_only_included_from_inside_a_function_is_excluded() {
    let registry = build_with_files(&[
        (
            "a.php",
            r#"<?php
                class Template {}
                $template = new Template();
            "#,
        ),
        (
            "b.php",
            r#"<?php
                function includer() {
                    include('a.php');
                }
            "#,
        ),
    ]);
    // `a.php` is never reached from a genuine top-level include site (its
    // only resolved incoming edge is from inside `includer()`), so its
    // own top level doesn't count as a real global-scope candidate.
    assert_eq!(global_type(&registry, "template"), None);
}

#[test]
fn a_top_level_assignment_reachable_via_at_least_one_genuine_path_is_included() {
    let registry = build_with_files(&[
        (
            "a.php",
            r#"<?php
                class Template {}
                $template = new Template();
            "#,
        ),
        (
            "b.php",
            r#"<?php
                function includer() {
                    include('a.php');
                }
            "#,
        ),
        (
            "c.php",
            r#"<?php
                include('a.php');
            "#,
        ),
    ]);
    // `c.php` is a real entry point (nothing includes it) with a genuine
    // top-level include of `a.php` - that one real path is enough.
    assert_eq!(
        global_type(&registry, "template"),
        Some(&Type::Object("Template".into(), vec![]))
    );
}

#[test]
fn a_file_with_no_incoming_include_edges_defaults_to_included() {
    let registry = build_with_files(&[(
        "a.php",
        r#"<?php
            class Template {}
            $template = new Template();
        "#,
    )]);
    assert_eq!(
        global_type(&registry, "template"),
        Some(&Type::Object("Template".into(), vec![]))
    );
}

#[test]
fn a_dynamic_include_target_is_a_dead_end_not_a_false_resolution() {
    let registry = build_with_files(&[
        (
            "a.php",
            r#"<?php
                class Template {}
                $template = new Template();
            "#,
        ),
        (
            "b.php",
            r#"<?php
                include($dynamic_path);
            "#,
        ),
    ]);
    // `b.php`'s include target is a bare variable - unresolvable, so it
    // contributes no edge at all. `a.php` has no incoming edges either
    // way, so it still defaults to included (unaffected by `b.php`).
    assert_eq!(
        global_type(&registry, "template"),
        Some(&Type::Object("Template".into(), vec![]))
    );
}

#[test]
fn a_constant_then_literal_suffix_include_target_resolves_by_suffix_match() {
    let registry = build_with_files(&[
        (
            "include/a.php",
            r#"<?php
                class Template {}
                $template = new Template();
            "#,
        ),
        (
            "b.php",
            r#"<?php
                define('ROOT_PATH', '/var/www/');
                include(ROOT_PATH . 'include/a.php');
            "#,
        ),
    ]);
    assert_eq!(
        global_type(&registry, "template"),
        Some(&Type::Object("Template".into(), vec![]))
    );
}

#[test]
fn a_multi_level_nested_index_write_still_records_its_outermost_key() {
    // Real Piwigo shape (`functions_mail.inc.php`'s `switch_lang_to`):
    // `$switch_lang['language'][$user['language']]['lang_info'] = ...;`
    // doesn't match the single-level `$name['key'] = value` shape at
    // all, but the outermost key's own existence is still real and must
    // be recorded - missing it produced a genuine false
    // `offsetAccess.notFound` on `$switch_lang['language']` reads
    // elsewhere, confirmed via a piwigo16 corpus diff before this test
    // was written.
    let registry = build_with_files(&[(
        "a.php",
        r#"<?php
            $switch_lang['language'][$dynamic]['lang_info'] = 'x';
        "#,
    )]);
    match global_type(&registry, "switch_lang") {
        Some(Type::ArrayShape(fields)) => {
            let fields = fields.named_fields();
            let names: Vec<&str> = fields.iter().map(|(k, _, _)| k.as_str()).collect();
            assert_eq!(names, vec!["language"], "got: {names:?}");
        }
        other => panic!("expected an ArrayShape with a 'language' field, got: {other:?}"),
    }
}

#[test]
fn a_nested_append_write_still_records_its_outermost_key() {
    // Real Piwigo shape (`functions_mail.inc.php`): `$switch_lang['stack'][]
    // = $value;` - an append (`Index(base, None)`) nested one level under
    // a real key. The outermost operation has no key of its own, but
    // `'stack'` still needs to be recorded.
    let registry = build_with_files(&[(
        "a.php",
        r#"<?php
            $switch_lang['stack'][] = 'x';
        "#,
    )]);
    match global_type(&registry, "switch_lang") {
        Some(Type::ArrayShape(fields)) => {
            let fields = fields.named_fields();
            let names: Vec<&str> = fields.iter().map(|(k, _, _)| k.as_str()).collect();
            assert_eq!(names, vec!["stack"], "got: {names:?}");
        }
        other => panic!("expected an ArrayShape with a 'stack' field, got: {other:?}"),
    }
}

#[test]
fn a_bare_top_level_append_contributes_nothing() {
    let registry = build_with_files(&[("a.php", "<?php $g[] = 'x';")]);
    match global_type(&registry, "g") {
        None | Some(Type::Mixed(false)) => {}
        other => panic!("expected no contribution from a bare append, got: {other:?}"),
    }
}
