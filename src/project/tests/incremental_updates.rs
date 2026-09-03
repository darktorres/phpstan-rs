//! Incremental `update_file` behavior: registry updates, re-check scoping,
//! and dependency invalidation (including transitive cases) across files.

use super::super::*;

#[test]
fn updating_one_file_does_not_disturb_another_files_registry_entries() {
    let mut project = Project::new();
    project
        .load_file(
            PathBuf::from("a.php"),
            "<?php function helperA(): int { return 1; }",
        )
        .unwrap();
    project
        .load_file(
            PathBuf::from("b.php"),
            "<?php function helperB(): int { return helperA(); }",
        )
        .unwrap();
    project.finish_loading();

    assert!(project.registry().functions.contains_key("helperA"));
    assert!(project.registry().functions.contains_key("helperB"));

    // Change b.php's return type - a.php's own registry entry and
    // parse tree must be untouched by this.
    project
        .update_file(
            &PathBuf::from("b.php"),
            "<?php function helperB(): string { return 'x'; }",
        )
        .unwrap();

    assert!(project.registry().functions.contains_key("helperA"));
    let helper_b = &project.registry().functions["helperB"];
    assert_eq!(helper_b.return_type.to_string(), "string");
}

#[test]
fn removing_a_declaration_on_update_drops_it_from_the_registry() {
    let mut project = Project::new();
    project
        .load_file(
            PathBuf::from("a.php"),
            "<?php function helperA(): int { return 1; } class Foo {}",
        )
        .unwrap();
    project.finish_loading();
    assert!(project.registry().classes.contains_key("Foo"));

    // The new version of the file no longer declares Foo at all.
    project
        .update_file(
            &PathBuf::from("a.php"),
            "<?php function helperA(): int { return 1; }",
        )
        .unwrap();
    assert!(!project.registry().classes.contains_key("Foo"));
    assert!(project.registry().functions.contains_key("helperA"));
}

#[test]
fn incremental_update_still_resolves_cross_file_inheritance() {
    let mut project = Project::new();
    project
        .load_file(
            PathBuf::from("base.php"),
            "<?php class Base { public int $x = 1; }",
        )
        .unwrap();
    project
        .load_file(
            PathBuf::from("child.php"),
            "<?php class Child extends Base {}",
        )
        .unwrap();
    project.finish_loading();
    assert!(project.registry().is_subclass_of("Child", "Base"));

    // Touching an unrelated file shouldn't break that cross-file
    // relationship.
    project
        .update_file(
            &PathBuf::from("child.php"),
            "<?php class Child extends Base {}",
        )
        .unwrap();
    assert!(project.registry().is_subclass_of("Child", "Base"));
}

#[test]
fn changing_a_declaration_re_checks_files_that_reference_it() {
    let mut project = Project::new();
    project
        .load_file(
            PathBuf::from("base.php"),
            "<?php class Base { public int $x = 1; }",
        )
        .unwrap();
    project
        .load_file(
            PathBuf::from("caller.php"),
            "<?php function useIt(Base $b) { return $b->x + 1; }",
        )
        .unwrap();
    project
        .load_file(
            PathBuf::from("unrelated.php"),
            "<?php function unrelated(): int { return 1; }",
        )
        .unwrap();
    project.finish_loading();

    // Base.php's own edit doesn't itself introduce a diagnostic here,
    // but caller.php (which references Base by name in a type hint)
    // must be among the re-checked files - unrelated.php must not be.
    let result = project
        .update_file(
            &PathBuf::from("base.php"),
            "<?php class Base { public int $x = 1; public function extra(): void {} }",
        )
        .unwrap();

    assert!(result.contains_key(&PathBuf::from("base.php")));
    assert!(result.contains_key(&PathBuf::from("caller.php")));
    assert!(!result.contains_key(&PathBuf::from("unrelated.php")));
}

#[test]
fn removing_a_referenced_declaration_still_flags_its_old_dependents() {
    let mut project = Project::new();
    project
        .load_file(
            PathBuf::from("thing.php"),
            "<?php function thing(): int { return 1; }",
        )
        .unwrap();
    project
        .load_file(
            PathBuf::from("caller.php"),
            "<?php function useThing() { return thing(); }",
        )
        .unwrap();
    project.finish_loading();

    // thing.php no longer declares `thing` at all - caller.php, which
    // calls it by name, still needs to be re-checked (it'll now see
    // an unresolvable call), even though nothing in caller.php itself
    // changed.
    let result = project
        .update_file(&PathBuf::from("thing.php"), "<?php // now empty")
        .unwrap();

    assert!(result.contains_key(&PathBuf::from("thing.php")));
    assert!(result.contains_key(&PathBuf::from("caller.php")));
    assert!(!project.registry().functions.contains_key("thing"));
}

#[test]
fn invalidation_is_transitive_across_an_inheritance_chain() {
    let mut project = Project::new();
    // C declares $x. B extends C (so B's *effective* shape includes
    // $x, without B's own source ever mentioning it). A references B
    // by name (a type hint) but never mentions C at all.
    project
        .load_file(
            PathBuf::from("c.php"),
            "<?php class C { public int $x = 1; }",
        )
        .unwrap();
    project
        .load_file(PathBuf::from("b.php"), "<?php class B extends C {}")
        .unwrap();
    project
        .load_file(
            PathBuf::from("a.php"),
            "<?php function useB(B $b) { return $b->x; }",
        )
        .unwrap();
    project.finish_loading();

    // Changing C ripples through B into A, even though a.php's own
    // parse tree never references "C" - a one-hop lookup from C's
    // direct dependents (just b.php) would miss a.php entirely.
    let result = project
        .update_file(
            &PathBuf::from("c.php"),
            "<?php class C { public string $x = 'a'; }",
        )
        .unwrap();

    assert!(result.contains_key(&PathBuf::from("c.php")));
    assert!(result.contains_key(&PathBuf::from("b.php")));
    assert!(result.contains_key(&PathBuf::from("a.php")));
}

#[test]
fn invalidation_still_works_for_namespaced_classes() {
    // Same shape as invalidation_is_transitive_across_an_inheritance_chain,
    // but every class lives in a namespace - real code, unlike almost
    // every other test in this file. Phase 9.5 made declared_names
    // return fully-qualified names (to match Registry::index's now
    // namespace-aware keys, for correct incremental *removal*), but
    // deps::referenced_names deliberately still collects bare names
    // (an intentional, documented approximation - see its own doc
    // comment) - changed_symbols (fed from declared_names) and
    // `dependents`'s keys (fed from referenced_names) must actually
    // agree for transitive_affected_files' lookup to find anything at
    // all, and after Phase 9.5 they silently didn't for any namespaced
    // class: `dependents.get("App\\C")` found nothing under a
    // "App\\C" key that was never inserted (only the bare "C" key
    // was), so a.php was never re-checked even though it genuinely
    // depends on the changed class.
    let mut project = Project::new();
    project
        .load_file(
            PathBuf::from("c.php"),
            "<?php namespace App; class C { public int $x = 1; }",
        )
        .unwrap();
    project
        .load_file(
            PathBuf::from("b.php"),
            "<?php namespace App; class B extends C {}",
        )
        .unwrap();
    project
        .load_file(
            PathBuf::from("a.php"),
            "<?php namespace App; function useB(B $b) { return $b->x; }",
        )
        .unwrap();
    project.finish_loading();

    let result = project
        .update_file(
            &PathBuf::from("c.php"),
            "<?php namespace App; class C { public string $x = 'a'; }",
        )
        .unwrap();

    assert!(result.contains_key(&PathBuf::from("c.php")));
    assert!(
        result.contains_key(&PathBuf::from("b.php")),
        "b.php (extends C) should have been re-checked, got: {:?}",
        result.keys().collect::<Vec<_>>()
    );
    assert!(
        result.contains_key(&PathBuf::from("a.php")),
        "a.php (references B) should have been re-checked, got: {:?}",
        result.keys().collect::<Vec<_>>()
    );
}

#[test]
fn a_class_referenced_only_inside_a_property_hook_body_is_a_real_dependency() {
    // Coverage measurement found deps.rs's collect_property_hooks/
    // collect_hook_body had zero test executions - PHP 8.4 property
    // hooks are real, newer syntax, and a class mentioned only inside
    // a get/set hook body (not a plain method) is exactly the kind of
    // reference the earlier Phase 7 regression already proved this
    // codebase can silently miss.
    let mut project = Project::new();
    project
        .load_file(
            PathBuf::from("dep.php"),
            "<?php class Dep { public static function make(): Dep { return new Dep(); } }",
        )
        .unwrap();
    project
        .load_file(
            PathBuf::from("holder.php"),
            r#"<?php class Holder {
            public $prop {
                get => Dep::make();
            }
        }"#,
        )
        .unwrap();
    project.finish_loading();

    let result = project
        .update_file(
            &PathBuf::from("dep.php"),
            "<?php class Dep { public static function make(): int { return 1; } }",
        )
        .unwrap();
    assert!(
        result.contains_key(&PathBuf::from("holder.php")),
        "holder.php (references Dep only inside a property hook body) should have been re-checked, got: {:?}",
        result.keys().collect::<Vec<_>>()
    );
}

#[test]
fn a_class_referenced_only_via_an_enum_is_a_real_dependency() {
    // Same coverage finding for collect_enum: an enum's `implements`
    // and its methods' bodies are real references that must feed the
    // same invalidation graph a class's `implements`/methods already
    // do - untested before this.
    let mut project = Project::new();
    project
        .load_file(PathBuf::from("dep.php"), "<?php interface Dep {}")
        .unwrap();
    project
        .load_file(
            PathBuf::from("status.php"),
            "<?php enum Status implements Dep { case Active; }",
        )
        .unwrap();
    project.finish_loading();

    let result = project
        .update_file(
            &PathBuf::from("dep.php"),
            "<?php interface Dep { public function f(): void; }",
        )
        .unwrap();
    assert!(
        result.contains_key(&PathBuf::from("status.php")),
        "status.php (enum implements Dep) should have been re-checked, got: {:?}",
        result.keys().collect::<Vec<_>>()
    );
}

#[test]
fn unrelated_files_are_not_swept_into_transitive_invalidation() {
    let mut project = Project::new();
    project
        .load_file(
            PathBuf::from("c.php"),
            "<?php class C { public int $x = 1; }",
        )
        .unwrap();
    project
        .load_file(PathBuf::from("b.php"), "<?php class B extends C {}")
        .unwrap();
    project
        .load_file(
            PathBuf::from("a.php"),
            "<?php function useB(B $b) { return $b->x; }",
        )
        .unwrap();
    project
        .load_file(
            PathBuf::from("unrelated.php"),
            "<?php function unrelated(): int { return 1; }",
        )
        .unwrap();
    project.finish_loading();

    let result = project
        .update_file(
            &PathBuf::from("c.php"),
            "<?php class C { public string $x = 'a'; }",
        )
        .unwrap();

    assert!(!result.contains_key(&PathBuf::from("unrelated.php")));
}
