//! `Checker::check_too_wide_property_types` (`property.unusedType`/
//! `staticProperty.unusedType`) and the two-pass collect-then-consume
//! architecture (`Project::collect_property_assign_facts`/`check_file_
//! with_property_assign_facts`) that lets a trait-donated property write
//! discovered in one file reach it for a host declared in another.

use std::sync::Arc;

use super::super::*;

#[test]
fn a_private_property_never_assigned_one_declared_union_member_is_flagged() {
    let mut project = Project::new();
    project
        .load_file(
            PathBuf::from("a.php"),
            r#"<?php
                class Foo {
                    /** @var int|string */
                    private $x;
                    public function set(): void {
                        $this->x = 1;
                    }
                }
            "#,
        )
        .unwrap();
    project.finish_loading();

    let facts = Arc::new(project.collect_property_assign_facts());
    let diags = project.check_file_with_property_assign_facts(&PathBuf::from("a.php"), &facts);
    assert!(
        diags.iter().any(|d| d.identifier == "property.unusedType"),
        "got: {diags:?}"
    );
}

#[test]
fn a_private_property_assigned_every_declared_union_member_is_never_flagged() {
    let mut project = Project::new();
    project
        .load_file(
            PathBuf::from("a.php"),
            r#"<?php
                class Foo {
                    /** @var int|string */
                    private $x;
                    public function set(): void {
                        $this->x = 1;
                        $this->x = "a";
                    }
                }
            "#,
        )
        .unwrap();
    project.finish_loading();

    let facts = Arc::new(project.collect_property_assign_facts());
    let diags = project.check_file_with_property_assign_facts(&PathBuf::from("a.php"), &facts);
    assert!(
        !diags.iter().any(|d| d.identifier == "property.unusedType"),
        "got: {diags:?}"
    );
}

#[test]
fn a_trait_donated_property_write_discovered_in_a_different_file_reaches_the_host() {
    let mut project = Project::new();
    project
        .load_file(
            PathBuf::from("trait.php"),
            r#"<?php
                trait T {
                    public function set(): void {
                        $this->x = 1;
                        $this->x = "a";
                    }
                }
            "#,
        )
        .unwrap();
    project
        .load_file(
            PathBuf::from("host.php"),
            r#"<?php
                class Foo {
                    use T;
                    /** @var int|string */
                    private $x;
                }
            "#,
        )
        .unwrap();
    project.finish_loading();

    let facts = Arc::new(project.collect_property_assign_facts());
    // The trait's own contribution was discovered while checking
    // trait.php, attributed to Foo's own fqn - host.php's own check must
    // see it too, via the whole-project merge, not just its own file's
    // live accumulation (host.php declares no method of its own that
    // assigns $x at all).
    let diags = project.check_file_with_property_assign_facts(&PathBuf::from("host.php"), &facts);
    assert!(
        !diags.iter().any(|d| d.identifier == "property.unusedType"),
        "got: {diags:?}"
    );
}

/// Test-only: replicates `Project::collect_property_assign_facts`'s own
/// merge loop but with an explicit `facts_only` flag, so the fully-
/// gated collect pass and the ungated real pass can be compared
/// directly on the exact same files - catches a future accidentally-
/// skipped `Checker::check_program`/`check_function_impl` block that
/// turns out to have a hidden side effect beyond pushing diagnostics,
/// which a diagnostic-only assertion could miss.
fn collect_property_assign_facts_with_mode(
    project: &Project,
    facts_only: bool,
) -> PropertyAssignFacts {
    let mut merged: PropertyAssignFacts = FxHashMap::default();
    for path in project.files.keys() {
        let Some(file) = project.files.get(path) else {
            continue;
        };
        let mut checker = Checker::new(&project.registry);
        checker.set_facts_only(facts_only);
        checker.check_program(&file.ast);
        for (fqn, props) in checker.take_property_assign_facts() {
            let entry = merged.entry(fqn).or_default();
            for (prop, mut types) in props {
                entry.entry(prop).or_default().append(&mut types);
            }
        }
    }
    merged
}

#[test]
fn facts_only_mode_agrees_with_the_full_pass_on_a_trait_donated_property_write() {
    let mut project = Project::new();
    project
        .load_file(
            PathBuf::from("trait.php"),
            r#"<?php
                trait T {
                    public function set(): void {
                        $this->x = 1;
                        $this->x = "a";
                    }
                }
            "#,
        )
        .unwrap();
    project
        .load_file(
            PathBuf::from("host.php"),
            r#"<?php
                class Foo {
                    use T;
                    /** @var int|string */
                    private $x;
                }
            "#,
        )
        .unwrap();
    project.finish_loading();

    let facts_only = collect_property_assign_facts_with_mode(&project, true);
    let full = collect_property_assign_facts_with_mode(&project, false);
    assert_eq!(
        facts_only, full,
        "facts_only=true and facts_only=false disagree on a trait-donated write"
    );
    assert!(
        facts_only
            .get("Foo")
            .and_then(|props| props.get("x"))
            .is_some_and(|types| types.len() == 2),
        "expected both $this->x writes to be recorded, got: {facts_only:?}"
    );
}

#[test]
fn facts_only_mode_agrees_with_the_full_pass_on_a_pest_test_closures_property_write() {
    // The file-local `uses(...)` (no `->in(...)`) override is read
    // directly inside `check_program` itself, and `collect_property_
    // assign_facts` constructs its Checker with no `set_pest_this_type`
    // call beforehand (unlike the real pass) - this file-local override
    // is the *only* source of Pest `$this`-type info the collect pass
    // ever gets, so it must stay ungated in facts_only mode or every
    // Pest test closure's own property writes would be silently lost.
    let mut project = Project::new();
    project
        .load_file(
            PathBuf::from("Bound.php"),
            r#"<?php
                class Bound {
                    /** @var int|string */
                    private $x;
                }
            "#,
        )
        .unwrap();
    project
        .load_file(
            PathBuf::from("FooTest.php"),
            r#"<?php
                function it($description, $callback) {}
                uses(Bound::class);
                it('writes a property', function (): void {
                    $this->x = 1;
                    $this->x = "a";
                });
            "#,
        )
        .unwrap();
    project.finish_loading();

    let facts_only = collect_property_assign_facts_with_mode(&project, true);
    let full = collect_property_assign_facts_with_mode(&project, false);
    assert_eq!(
        facts_only, full,
        "facts_only=true and facts_only=false disagree on a Pest test closure's property write"
    );
    assert!(
        facts_only
            .get("Bound")
            .and_then(|props| props.get("x"))
            .is_some_and(|types| types.len() == 2),
        "expected both $this->x writes inside the Pest closure to be recorded, got: {facts_only:?}"
    );
}

#[test]
fn a_trait_donated_property_write_missing_one_union_member_is_flagged_on_the_host() {
    let mut project = Project::new();
    project
        .load_file(
            PathBuf::from("trait.php"),
            r#"<?php
                trait T {
                    public function set(): void {
                        $this->x = 1;
                    }
                }
            "#,
        )
        .unwrap();
    project
        .load_file(
            PathBuf::from("host.php"),
            r#"<?php
                class Foo {
                    use T;
                    /** @var int|string */
                    private $x;
                }
            "#,
        )
        .unwrap();
    project.finish_loading();

    let facts = Arc::new(project.collect_property_assign_facts());
    let diags = project.check_file_with_property_assign_facts(&PathBuf::from("host.php"), &facts);
    assert!(
        diags.iter().any(|d| d.identifier == "property.unusedType"),
        "got: {diags:?}"
    );
}
