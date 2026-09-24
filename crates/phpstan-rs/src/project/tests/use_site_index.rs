//! `Project::use_sites()`'s two-tier index: Tier 1 (class/function
//! reference sites) - eager, syntactic - and Tier 2 (member reference
//! sites) - lazy, checker-driven, built once by `ensure_member_index_
//! built`. Tier 1 is populated whenever `retain_source` is on (the LSP;
//! real, precise per-identifier spans via `use_sites::
//! index_class_fn_refs`) OR `check_dead_code` is on (a batch
//! `check-project` run with no retained source, via `index_class_fn_
//! refs_without_source` - real references still, just scoped to a less
//! narrow AST-node span - see that function's own doc comment); with
//! neither on, Tier 1 stays empty - measured too expensive on a real
//! corpus to populate unconditionally the way `decl_locations` is (see
//! `Project::insert_parsed_file`'s own doc comment). This is what
//! `checker::whole_program::dead_code` (`project::tests::dead_code`)
//! relies on. The LSP wiring itself (`textDocument/references`/
//! `textDocument/rename`) is exercised separately in `lsp::tests::
//! references_and_rename`.

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
fn tier_1_stays_empty_without_retain_source_or_check_dead_code() {
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
        "Tier 1 must stay off when neither retain_source nor check_dead_code \
         is on - a real corpus measurement found it too expensive to \
         populate unconditionally"
    );
}

#[test]
fn tier_1_is_indexed_without_retain_source_when_check_dead_code_is_on() {
    let mut project = Project::new();
    project.set_check_dead_code(true);
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
        1,
        "Tier 1 doesn't need retained source text to exist at all when \
         check_dead_code needs it - only to get a precise per-identifier \
         span, see `use_sites::index_class_fn_refs_without_source`'s own \
         doc comment"
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

#[test]
fn untyped_class_constant_value_reference_is_indexed() {
    // Deliberately UNTYPED - `check_typed_class_constant_values` (the
    // existing infer_expr call this item's own design traced) skips a
    // class whose consts are all untyped entirely, so this reference is
    // only ever found by the new `collect_default_and_const_value_
    // member_refs` enumeration, never by the pre-existing call.
    let mut project = project_with_retained_source();
    project
        .load_file(
            PathBuf::from("a.php"),
            "<?php class LogLevel { const INFO = 1; }\nclass Foo { const LEVEL = LogLevel::INFO; }\n",
        )
        .unwrap();
    project.finish_loading();
    project.ensure_member_index_built();

    let refs = project.use_sites().references(&RefTarget::Member(
        "LogLevel".to_string(),
        "INFO".to_string(),
    ));
    assert_eq!(refs.len(), 1, "got: {refs:?}");
}

#[test]
fn typed_class_constant_value_reference_is_indexed_and_not_duplicated() {
    // TYPED this time - both `check_typed_class_constant_values`'s own
    // pre-existing `infer_expr` call AND the new enumeration pass visit
    // this exact same `LogLevel::INFO` expression - the HashSet-based
    // dedup in `Checker::member_ref_sites` must collapse the two
    // independent visits into exactly one recorded site, not two.
    let mut project = project_with_retained_source();
    project
        .load_file(
            PathBuf::from("a.php"),
            "<?php class LogLevel { const INFO = 1; }\nclass Foo { const int LEVEL = LogLevel::INFO; }\n",
        )
        .unwrap();
    project.finish_loading();
    project.ensure_member_index_built();

    let refs = project.use_sites().references(&RefTarget::Member(
        "LogLevel".to_string(),
        "INFO".to_string(),
    ));
    assert_eq!(
        refs.len(),
        1,
        "expected exactly one entry, not two, got: {refs:?}"
    );
}

#[test]
fn parameter_default_value_member_reference_is_indexed() {
    // No existing Checker path visits a parameter's own default value at
    // all (confirmed by direct trace) - this is a real gap only
    // `collect_default_and_const_value_member_refs` closes.
    let mut project = project_with_retained_source();
    project
        .load_file(
            PathBuf::from("a.php"),
            "<?php class SomeClass { const DEFAULT_VALUE = 1; }\nfunction f($x = SomeClass::DEFAULT_VALUE) {}\n",
        )
        .unwrap();
    project.finish_loading();
    project.ensure_member_index_built();

    let refs = project.use_sites().references(&RefTarget::Member(
        "SomeClass".to_string(),
        "DEFAULT_VALUE".to_string(),
    ));
    assert_eq!(refs.len(), 1, "got: {refs:?}");
}

#[test]
fn property_default_value_member_reference_is_indexed() {
    let mut project = project_with_retained_source();
    project
        .load_file(
            PathBuf::from("a.php"),
            "<?php class Bar { const BAZ = 1; }\nclass Foo { public $x = Bar::BAZ; }\n",
        )
        .unwrap();
    project.finish_loading();
    project.ensure_member_index_built();

    let refs = project
        .use_sites()
        .references(&RefTarget::Member("Bar".to_string(), "BAZ".to_string()));
    assert_eq!(refs.len(), 1, "got: {refs:?}");
}

#[test]
fn member_reference_index_is_not_built_until_first_requested() {
    let mut project = project_with_retained_source();
    project
        .load_file(
            PathBuf::from("a.php"),
            "<?php class Bar { const BAZ = 1; }\nclass Foo { public $x = Bar::BAZ; }\n",
        )
        .unwrap();
    project.finish_loading();

    let target = RefTarget::Member("Bar".to_string(), "BAZ".to_string());
    assert_eq!(
        project.use_sites().references(&target).len(),
        0,
        "Tier 2 must not exist before the first ensure_member_index_built call"
    );
    project.ensure_member_index_built();
    assert_eq!(
        project.use_sites().references(&target).len(),
        1,
        "expected the reference to appear once the index is actually built"
    );
}
