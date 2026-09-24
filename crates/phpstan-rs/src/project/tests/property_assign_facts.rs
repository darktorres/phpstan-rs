//! `Project::collect_property_assign_facts`'s facts-only mode agrees with
//! the full pass. The `property.unusedType` check itself is covered by the
//! fixtures under `tests/fixtures/project/property_assign_facts`.

use super::super::*;

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
