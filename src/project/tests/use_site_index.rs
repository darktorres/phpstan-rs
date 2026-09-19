//! `Project::use_sites()`'s Tier-1 (class/function reference) index -
//! eager, syntactic, gated on `retain_source` exactly like
//! `decl_locations`. Tier 2 (member references, `ensure_member_index_
//! built`) is exercised alongside the LSP wiring in
//! `lsp::tests::references_and_rename` instead, once the checker
//! plumbing lands.

use super::super::use_sites::RefTarget;
use super::super::*;

fn project_with_retained_source() -> Project {
    let mut project = Project::new();
    project.set_retain_source(true);
    project
}

#[test]
fn class_and_function_reference_sites_are_indexed_after_finish_loading() {
    let mut project = project_with_retained_source();
    project
        .load_file(
            PathBuf::from("a.php"),
            "<?php function greet(): void {}\nclass Foo {}\ngreet();\nnew Foo();\n",
        )
        .unwrap();
    project.finish_loading();

    let refs = project
        .use_sites()
        .references(&RefTarget::Function("greet".to_string()));
    assert_eq!(refs.len(), 1, "got: {refs:?}");
    let refs = project
        .use_sites()
        .references(&RefTarget::Class("Foo".to_string()));
    // The class's OWN declaration is deliberately not indexed here - a
    // "reference" site is a usage, not the declaration itself (the LSP
    // layer's own `includeDeclaration` handling consults
    // `decl_locations` separately for that, already built for hover/
    // definition).
    assert_eq!(
        refs.len(),
        1,
        "expected only the `new Foo()` site, got: {refs:?}"
    );
}

#[test]
fn updating_a_file_drops_its_stale_use_sites() {
    let mut project = project_with_retained_source();
    let path = PathBuf::from("a.php");
    project
        .load_file(path.clone(), "<?php function greet(): void {}\ngreet();\n")
        .unwrap();
    project.finish_loading();
    assert_eq!(
        project
            .use_sites()
            .references(&RefTarget::Function("greet".to_string()))
            .len(),
        1
    );

    project
        .update_file(&path, "<?php function greet(): void {}\n")
        .unwrap();
    assert_eq!(
        project
            .use_sites()
            .references(&RefTarget::Function("greet".to_string()))
            .len(),
        0,
        "the call site was removed by the edit and must no longer be indexed"
    );
}

#[test]
fn use_site_index_is_empty_without_retain_source() {
    let mut project = Project::new();
    project
        .load_file(
            PathBuf::from("a.php"),
            "<?php function greet(): void {}\ngreet();\n",
        )
        .unwrap();
    project.finish_loading();
    assert_eq!(
        project
            .use_sites()
            .references(&RefTarget::Function("greet".to_string()))
            .len(),
        0,
        "Tier 1 must stay off (like decl_locations) when retain_source is off"
    );
}

#[test]
fn a_type_hint_reference_and_a_static_access_class_half_are_both_indexed() {
    let mut project = project_with_retained_source();
    project
        .load_file(
            PathBuf::from("a.php"),
            "<?php class Foo { const BAR = 1; }\nfunction f(Foo $x): void { echo Foo::BAR; }\n",
        )
        .unwrap();
    project.finish_loading();

    let refs = project
        .use_sites()
        .references(&RefTarget::Class("Foo".to_string()));
    // The parameter type hint and the `Foo::` class half of the
    // static-const access - NOT the class's own declaration, and NOT
    // the `BAR` member half, which is Tier 2's job entirely.
    assert_eq!(refs.len(), 2, "got: {refs:?}");
}

#[test]
fn a_self_reference_inside_its_own_class_is_not_indexed_as_a_rename_site() {
    // See `use_sites::resolve_class_ref`'s own doc comment: `self::`
    // resolves to the current class for real PHP semantics, but its own
    // span covers the literal text "self", never the class's own name -
    // recording it here would corrupt a future rename of this class.
    let mut project = project_with_retained_source();
    project
        .load_file(
            PathBuf::from("a.php"),
            "<?php class Foo { const BAR = 1; function f(): int { return self::BAR; } }\n",
        )
        .unwrap();
    project.finish_loading();

    let refs = project
        .use_sites()
        .references(&RefTarget::Class("Foo".to_string()));
    assert_eq!(
        refs.len(),
        0,
        "expected no reference at all - not the declaration (never indexed as a use-site), and not the self:: access, got: {refs:?}"
    );
}

#[test]
fn parameter_default_value_reference_is_indexed() {
    let mut project = project_with_retained_source();
    project
        .load_file(
            PathBuf::from("a.php"),
            "<?php class Def {}\nfunction f($x = Def::class) {}\n",
        )
        .unwrap();
    project.finish_loading();

    let refs = project
        .use_sites()
        .references(&RefTarget::Class("Def".to_string()));
    assert_eq!(
        refs.len(),
        1,
        "expected the parameter default's Def::class reference, got: {refs:?}"
    );
}

#[test]
fn class_constant_value_reference_is_indexed() {
    let mut project = project_with_retained_source();
    project
        .load_file(
            PathBuf::from("a.php"),
            "<?php class Other { const X = 1; }\nclass Foo { const Y = Other::X; }\n",
        )
        .unwrap();
    project.finish_loading();

    let refs = project
        .use_sites()
        .references(&RefTarget::Class("Other".to_string()));
    assert_eq!(
        refs.len(),
        1,
        "expected Foo's own const-value reference to Other, got: {refs:?}"
    );
}

#[test]
fn property_default_value_reference_is_indexed() {
    let mut project = project_with_retained_source();
    project
        .load_file(
            PathBuf::from("a.php"),
            "<?php class Def {}\nclass Foo { public $x = Def::class; }\n",
        )
        .unwrap();
    project.finish_loading();

    let refs = project
        .use_sites()
        .references(&RefTarget::Class("Def".to_string()));
    assert_eq!(
        refs.len(),
        1,
        "expected the property default's Def::class reference, got: {refs:?}"
    );
}
