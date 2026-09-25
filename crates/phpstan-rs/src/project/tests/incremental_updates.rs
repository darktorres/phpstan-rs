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
    // every other test in this file. `declared_names` returns
    // fully-qualified names (to match Registry::index's namespace-aware
    // keys, for correct incremental removal), but
    // `deps::referenced_names` deliberately still collects bare names
    // (an intentional, documented approximation - see its own doc
    // comment) - `changed_symbols` (fed from `declared_names`) and
    // `dependents`'s keys (fed from `referenced_names`) must actually
    // agree for `transitive_affected_files`' lookup to find anything at
    // all: `dependents.get("App\\C")` must find something under an
    // "App\\C" key, not only a bare "C" key, or a.php is never
    // re-checked even though it genuinely depends on the changed class.
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
    // `deps.rs`'s `collect_property_hooks`/`collect_hook_body` must
    // collect references inside a get/set hook body (not just a plain
    // method) - PHP 8.4 property hooks are newer syntax, and a class
    // mentioned only there is exactly the kind of reference this
    // codebase can otherwise silently miss.
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

#[test]
fn a_failed_reparse_keeps_the_files_last_good_ast_and_registry_entries() {
    // `update_file` used to unindex a file's own last-good `FileState`
    // (removing it from `self.files`, which `Project::ast`/`Project::
    // source` read straight from) BEFORE attempting the new parse -
    // meaning any edit that failed to parse permanently discarded the
    // last-good AST the moment this call returned `Err`, not just for
    // that one call. This is the exact state an LSP `didChange` full-
    // sync update sends on essentially every keystroke while a
    // statement is mid-edit (e.g. typing `$x->` has no valid member
    // name yet), so every feature reading `Project::ast` for that path
    // (hover, definition, documentSymbol, `lsp::completion`'s own
    // member-completion probe) would see nothing at all until the file
    // parsed cleanly again.
    let mut project = Project::new();
    project.set_retain_source(true);
    let path = PathBuf::from("a.php");
    project
        .load_file(path.clone(), "<?php function helperA(): int { return 1; }")
        .unwrap();
    project.finish_loading();

    let err = project.update_file(&path, "<?php function helperA(): int {");
    assert!(
        err.is_err(),
        "expected the truncated source to fail to parse"
    );

    assert!(
        project.ast(&path).is_some(),
        "the last-good AST must survive a failed reparse"
    );
    assert!(
        project.registry().functions.contains_key("helperA"),
        "the last-good registry entry must survive a failed reparse too"
    );
}

/// VSCode plan item 2.2's own load-bearing regression test: proves
/// `update_file_cancellable`'s `should_abort` closure can observe a
/// generation bump made by a genuinely different, concurrently-running
/// thread strictly between two per-file checks of the SAME in-flight
/// cascade - the property a single-threaded caller (and therefore every
/// other test in this file) can never exercise, since nothing else could
/// possibly run between two of its own sequential closure calls.
#[test]
fn update_file_cancellable_can_be_aborted_mid_cascade_by_a_concurrent_generation_bump() {
    use std::sync::mpsc;
    use std::sync::{Arc, Mutex};
    use std::thread;

    let mut project = Project::new();
    project
        .load_file(
            PathBuf::from("base.php"),
            "<?php function base(): int { return 1; }",
        )
        .unwrap();
    for i in 1..=3 {
        project
            .load_file(
                PathBuf::from(format!("dep{i}.php")),
                &format!("<?php function dep{i}(): int {{ return base(); }}"),
            )
            .unwrap();
    }
    project.finish_loading();

    let project = Arc::new(Mutex::new(project));
    let generation = Arc::new(Mutex::new(1u64));
    let (checked_tx, checked_rx) = mpsc::channel::<()>();
    let (bump_tx, bump_rx) = mpsc::channel::<()>();

    let worker_project = Arc::clone(&project);
    let worker_generation = Arc::clone(&generation);
    let handle = thread::spawn(move || {
        let mut call_count = 0;
        let should_abort = move || {
            call_count += 1;
            // Rendezvous right before the SECOND per-file check - proves
            // at least one file was already processed before the
            // concurrent bump takes effect, not just "aborts before
            // doing any work at all."
            if call_count == 2 {
                checked_tx.send(()).unwrap();
                bump_rx.recv().unwrap();
            }
            *worker_generation.lock().unwrap() != 1
        };
        worker_project
            .lock()
            .unwrap()
            .update_file_cancellable(
                &PathBuf::from("base.php"),
                "<?php function base(): int { return 2; }",
                should_abort,
            )
            .unwrap()
    });

    // Block until the worker has made its second should_abort check (i.e.
    // has already processed exactly one file), then bump the generation
    // from THIS thread - never touched by the worker itself - and release
    // it.
    checked_rx.recv().unwrap();
    *generation.lock().unwrap() = 2;
    bump_tx.send(()).unwrap();

    let result = handle.join().unwrap();
    assert_eq!(
        result.len(),
        1,
        "expected the cascade to abort after exactly one file, got: {result:?}"
    );
}

#[test]
fn update_file_refreshes_inferred_global_variable_types() {
    let mut project = Project::new();
    project
        .load_file(PathBuf::from("a.php"), "<?php class Template {}")
        .unwrap();
    project
        .load_file(
            PathBuf::from("b.php"),
            "<?php function f() { global $template; $template = new Template(); }",
        )
        .unwrap();
    project.finish_loading();

    assert_eq!(
        project.registry().global_variables.get("template"),
        Some(&types::Type::Object("Template".into(), vec![]))
    );

    // Editing the assigning file to point at a different class must
    // update the registry's own inferred type on the next settle, not
    // just at the initial `finish_loading()`.
    project
        .update_file(
            &PathBuf::from("a.php"),
            "<?php class Template {} class Other {}",
        )
        .unwrap();
    project
        .update_file(
            &PathBuf::from("b.php"),
            "<?php function f() { global $template; $template = new Other(); }",
        )
        .unwrap();

    assert_eq!(
        project.registry().global_variables.get("template"),
        Some(&types::Type::Object("Other".into(), vec![]))
    );
}
