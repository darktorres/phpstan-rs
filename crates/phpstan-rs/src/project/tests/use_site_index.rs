//! `Project::use_sites()`'s two-tier index: Tier 1 (class/function
//! reference sites) - a product of each file's parse, syntactic, and
//! indexed as files load when `retain_source` (the LSP) or
//! `check_dead_code` is on, which is what `project::whole_program::
//! dead_code` (`project::tests::dead_code`) relies on - and Tier 2 (member
//! reference sites) - lazy, checker-driven, built once by
//! `ensure_member_index_built`. With neither flag on, Tier 1 stays empty:
//! measured too expensive on a real corpus to populate unconditionally.
//! The LSP wiring itself (`textDocument/references`/`textDocument/rename`)
//! is exercised separately in `lsp::tests::references_and_rename`.

use super::super::test_support::Files;
use super::super::use_sites::RefTarget;
use super::super::*;

fn retaining_source() -> Files {
    Files::new().settings(ProjectSettings {
        retain_source: true,
        ..ProjectSettings::default()
    })
}

#[test]
fn class_and_function_reference_sites_are_indexed_after_finish_loading() {
    let mut project = retaining_source()
        .context(
            "a.php",
            "<?php function greet(): void {}\nclass Foo {}\ngreet();\nnew Foo();\n",
        )
        .load();
    project.ensure_member_index_built();

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
    let path = PathBuf::from("a.php");
    let mut project = retaining_source()
        .context(&path, "<?php function greet(): void {}\ngreet();\n")
        .load();
    project.ensure_member_index_built();
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
    let project = Files::new()
        .settings(ProjectSettings::default())
        .context("a.php", "<?php function greet(): void {}\ngreet();\n")
        .load();
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
    let project = Files::new()
        .settings(ProjectSettings {
            rules: crate::rules_settings::RulesSettings {
                check_dead_code: true,
                ..Default::default()
            },
            ..ProjectSettings::default()
        })
        .context("a.php", "<?php function greet(): void {}\ngreet();\n")
        .load();
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
    let mut project = retaining_source()
        .context(
            "a.php",
            "<?php class Foo { const BAR = 1; }\nfunction f(Foo $x): void { echo Foo::BAR; }\n",
        )
        .load();
    project.ensure_member_index_built();

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
    let mut project = retaining_source()
        .context(
            "a.php",
            "<?php class Foo { const BAR = 1; function f(): int { return self::BAR; } }\n",
        )
        .load();
    project.ensure_member_index_built();

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
    let mut project = retaining_source()
        .context(
            "a.php",
            "<?php class Def {}\nfunction f($x = Def::class) {}\n",
        )
        .load();
    project.ensure_member_index_built();

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
    let mut project = retaining_source()
        .context(
            "a.php",
            "<?php class Other { const X = 1; }\nclass Foo { const Y = Other::X; }\n",
        )
        .load();
    project.ensure_member_index_built();

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
    let mut project = retaining_source()
        .context(
            "a.php",
            "<?php class Def {}\nclass Foo { public $x = Def::class; }\n",
        )
        .load();
    project.ensure_member_index_built();

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
    let mut project = retaining_source().context("a.php", "<?php class LogLevel { const INFO = 1; }\nclass Foo { const LEVEL = LogLevel::INFO; }\n").load();
    project.ensure_member_index_built();
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
    let mut project = retaining_source().context("a.php", "<?php class LogLevel { const INFO = 1; }\nclass Foo { const int LEVEL = LogLevel::INFO; }\n").load();
    project.ensure_member_index_built();
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
    let mut project = retaining_source().context("a.php", "<?php class SomeClass { const DEFAULT_VALUE = 1; }\nfunction f($x = SomeClass::DEFAULT_VALUE) {}\n").load();
    project.ensure_member_index_built();
    project.ensure_member_index_built();

    let refs = project.use_sites().references(&RefTarget::Member(
        "SomeClass".to_string(),
        "DEFAULT_VALUE".to_string(),
    ));
    assert_eq!(refs.len(), 1, "got: {refs:?}");
}

#[test]
fn property_default_value_member_reference_is_indexed() {
    let mut project = retaining_source()
        .context(
            "a.php",
            "<?php class Bar { const BAZ = 1; }\nclass Foo { public $x = Bar::BAZ; }\n",
        )
        .load();
    project.ensure_member_index_built();
    project.ensure_member_index_built();

    let refs = project
        .use_sites()
        .references(&RefTarget::Member("Bar".to_string(), "BAZ".to_string()));
    assert_eq!(refs.len(), 1, "got: {refs:?}");
}

#[test]
fn member_reference_index_is_not_built_until_first_requested() {
    let mut project = retaining_source()
        .context(
            "a.php",
            "<?php class Bar { const BAZ = 1; }\nclass Foo { public $x = Bar::BAZ; }\n",
        )
        .load();

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

#[test]
fn class_and_function_sites_are_indexed_as_files_load_with_retained_source() {
    let path = PathBuf::from("a.php");
    let mut session = retaining_source()
        .context(&path, "<?php function greet(): void {}\ngreet();\n")
        .session();
    let mut project = session.load_listed(false, &[path], Vec::new()).project;
    let greet = RefTarget::Function("greet".to_string());
    assert_eq!(project.use_sites().references(&greet).len(), 1);

    session.set_overlay(Path::new("b.php"), b"<?php greet();\n".to_vec());
    session.load_into(&mut project, &[PathBuf::from("b.php")], &[]);
    project.finish_loading();
    assert_eq!(project.use_sites().references(&greet).len(), 2);
}
