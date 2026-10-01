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
    assert_eq!(helper_b.return_type().to_string(), "string");
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
    // but every class lives in a namespace: the declared names and the
    // references must reach the same keys.
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
    project.apply_settings(ProjectSettings {
        retain_source: true,
        ..ProjectSettings::default()
    });
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
                "<?php function base(): string { return 'x'; }",
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
        Some(&types::Type::object("Template".into(), vec![]))
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
        Some(&types::Type::object("Other".into(), vec![]))
    );
}

#[test]
fn an_edited_context_file_stays_context() {
    let mut project = Project::new();
    let path = PathBuf::from("lib/helper.php");
    project
        .load_file(path.clone(), "<?php function helper(): int { return 1; }")
        .unwrap();
    project.finish_loading();
    assert!(!project.files[&path].analyzed);

    project
        .update_file(&path, "<?php function helper(): int { return 2; }")
        .unwrap();
    assert!(!project.files[&path].analyzed);
}

#[test]
fn an_edited_analyzed_file_stays_analyzed() {
    let mut project = Project::new();
    let path = PathBuf::from("vendor/acme/helper.php");
    project
        .load_analyzed_file(path.clone(), "<?php function helper(): int { return 1; }")
        .unwrap();
    project.finish_loading();
    assert!(project.files[&path].analyzed);

    project
        .update_file(&path, "<?php function helper(): int { return 2; }")
        .unwrap();
    assert!(project.files[&path].analyzed);
}

fn class_has_method(project: &Project, class: &str, method: &str) -> bool {
    project
        .registry()
        .classes
        .get(class)
        .is_some_and(|c| c.methods().contains_key(method))
}

#[test]
fn editing_a_file_merged_into_a_built_in_class_keeps_the_built_in_members() {
    let sources = [
        "<?php if (!class_exists('ArrayObject')) { class ArrayObject { public function polyOnly(): int { return 1; } } }",
        "<?php class ArrayObject { public function polyOnly(): int { return 1; } }",
    ];
    for source in sources {
        for analyzed in [false, true] {
            let mut project = Project::new();
            let path = PathBuf::from("polyfill.php");
            if analyzed {
                project.load_analyzed_file(path.clone(), source).unwrap();
            } else {
                project.load_file(path.clone(), source).unwrap();
            }
            project.finish_loading();
            assert!(class_has_method(&project, "ArrayObject", "count"));
            assert!(class_has_method(&project, "ArrayObject", "polyOnly"));

            project.update_file(&path, &format!("{source}\n")).unwrap();
            assert!(
                class_has_method(&project, "ArrayObject", "count"),
                "{source} (analyzed: {analyzed})"
            );
            assert!(class_has_method(&project, "ArrayObject", "polyOnly"));
        }
    }
}

#[test]
fn editing_a_polyfill_a_built_in_function_beat_keeps_the_built_in() {
    let source = "<?php if (!function_exists('mb_str_pad')) { function mb_str_pad($string, $length) { return $string; } }";
    let mut project = Project::new();
    let path = PathBuf::from("polyfill.php");
    project.load_file(path.clone(), source).unwrap();
    project.finish_loading();
    let params = |project: &Project| project.registry().functions["mb_str_pad"].params().len();
    assert_eq!(params(&project), 5);

    project.update_file(&path, &format!("{source}\n")).unwrap();
    assert_eq!(params(&project), 5);
}

#[test]
fn removing_a_file_merged_into_a_built_in_class_restores_the_built_in() {
    let mut project = Project::new();
    let path = PathBuf::from("polyfill.php");
    project
        .load_file(
            path.clone(),
            "<?php if (!class_exists('ArrayObject')) { class ArrayObject { public function polyOnly(): int { return 1; } } }",
        )
        .unwrap();
    project.finish_loading();

    project.remove_file(&path);
    assert!(class_has_method(&project, "ArrayObject", "count"));
    assert!(!class_has_method(&project, "ArrayObject", "polyOnly"));
}

#[test]
fn editing_one_of_two_merged_declarations_keeps_the_other() {
    let mut project = Project::new();
    let first = PathBuf::from("lib/a.php");
    let second = PathBuf::from("lib/b.php");
    project
        .load_file(
            first.clone(),
            "<?php class Shared { public function fromA(): int { return 1; } }",
        )
        .unwrap();
    project
        .load_file(
            second.clone(),
            "<?php class Shared { public function fromB(): int { return 1; } }",
        )
        .unwrap();
    project.finish_loading();
    assert!(class_has_method(&project, "Shared", "fromA"));
    assert!(class_has_method(&project, "Shared", "fromB"));

    project
        .update_file(
            &first,
            "<?php class Shared { public function fromA2(): int { return 1; } }",
        )
        .unwrap();
    assert!(!class_has_method(&project, "Shared", "fromA"));
    assert!(class_has_method(&project, "Shared", "fromA2"));
    assert!(class_has_method(&project, "Shared", "fromB"));

    project.remove_file(&second);
    assert!(class_has_method(&project, "Shared", "fromA2"));
    assert!(!class_has_method(&project, "Shared", "fromB"));
}

/// Loads `files`, then edits `edited` to `new_source`, returning which
/// files the edit rechecked.
fn rechecked_by_edit(files: &[(&str, &str)], edited: &str, new_source: &str) -> Vec<String> {
    let mut project = Project::new();
    for (path, source) in files {
        project
            .load_analyzed_file(PathBuf::from(path), source)
            .unwrap();
    }
    project.finish_loading();
    let result = project
        .update_file(&PathBuf::from(edited), new_source)
        .unwrap();
    let mut rechecked: Vec<String> = result.keys().map(|p| p.display().to_string()).collect();
    rechecked.sort();
    rechecked
}

#[test]
fn a_fully_qualified_class_reference_is_rechecked() {
    let rechecked = rechecked_by_edit(
        &[
            ("base.php", "<?php class Base { function m() {} }"),
            ("top.php", "<?php (new \\Base())->m();"),
            ("ns.php", "<?php namespace App; (new \\Base())->m();"),
        ],
        "base.php",
        "<?php class Base {}",
    );
    assert_eq!(rechecked, ["base.php", "ns.php", "top.php"]);
}

#[test]
fn a_class_referenced_only_in_phpdoc_is_rechecked() {
    let rechecked = rechecked_by_edit(
        &[
            ("base.php", "<?php class Base { function m() {} }"),
            (
                "doc.php",
                "<?php /** @param Base $b */ function f($b) { $b->m(); }",
            ),
            (
                "mixin.php",
                "<?php /** @mixin Base */ class Host {} function g(Host $h) { $h->m(); }",
            ),
        ],
        "base.php",
        "<?php class Base {}",
    );
    assert_eq!(rechecked, ["base.php", "doc.php", "mixin.php"]);
}

#[test]
fn a_fully_qualified_function_call_is_rechecked() {
    let rechecked = rechecked_by_edit(
        &[
            ("g.php", "<?php function g(int $i) {}"),
            ("call.php", "<?php namespace App; \\g(1);"),
        ],
        "g.php",
        "<?php function g(string $s) {}",
    );
    assert_eq!(rechecked, ["call.php", "g.php"]);
}

#[test]
fn a_same_named_class_in_another_namespace_is_not_rechecked() {
    let rechecked = rechecked_by_edit(
        &[
            ("base.php", "<?php namespace A; class Base {}"),
            ("other.php", "<?php namespace B; class Base {} new Base();"),
        ],
        "base.php",
        "<?php namespace A; class Base { function m() {} }",
    );
    assert_eq!(rechecked, ["base.php"]);
}

#[test]
fn editing_a_constant_takes_its_new_value() {
    let mut project = Project::new();
    project
        .load_analyzed_file(PathBuf::from("c.php"), "<?php const VERSION = 1;")
        .unwrap();
    project.finish_loading();
    project
        .update_file(&PathBuf::from("c.php"), "<?php const VERSION = 2;")
        .unwrap();
    assert_eq!(
        project.registry().global_constants["VERSION"].to_string(),
        "2"
    );
}

#[test]
fn a_constant_deleted_from_its_file_is_undefined() {
    let mut project = Project::new();
    project
        .load_analyzed_file(
            PathBuf::from("c.php"),
            "<?php const GONE = 1; const KEPT = 1;",
        )
        .unwrap();
    project.finish_loading();
    project
        .update_file(&PathBuf::from("c.php"), "<?php const KEPT = 1;")
        .unwrap();
    assert!(!project.registry().global_constants.contains_key("GONE"));
    assert!(project.registry().global_constants.contains_key("KEPT"));
}

#[test]
fn removing_a_file_keeps_another_files_definition_of_its_constant() {
    let mut project = Project::new();
    project
        .load_analyzed_file(PathBuf::from("a.php"), "<?php define('SHARED', 1);")
        .unwrap();
    project
        .load_analyzed_file(PathBuf::from("b.php"), "<?php define('SHARED', 2);")
        .unwrap();
    project.finish_loading();
    assert!(project.registry().global_constants["SHARED"].is_mixed());
    project.remove_files(&[PathBuf::from("b.php")]);
    assert_eq!(
        project.registry().global_constants["SHARED"].to_string(),
        "1"
    );
}

#[test]
fn editing_a_file_defining_a_built_in_constant_keeps_the_built_in() {
    let mut project = Project::new();
    let built_in = project.registry().global_constants["PHP_EOL"].clone();
    project
        .load_analyzed_file(PathBuf::from("c.php"), "<?php const UNRELATED = 1;")
        .unwrap();
    project.finish_loading();
    project
        .update_file(
            &PathBuf::from("c.php"),
            "<?php if (!defined('PHP_EOL')) { define('PHP_EOL', \"\\n\"); }",
        )
        .unwrap();
    project
        .update_file(&PathBuf::from("c.php"), "<?php const UNRELATED = 1;")
        .unwrap();
    assert_eq!(project.registry().global_constants["PHP_EOL"], built_in);
}

#[test]
fn a_file_using_a_changed_constant_is_rechecked() {
    let rechecked = rechecked_by_edit(
        &[
            ("c.php", "<?php namespace App; const VERSION = 1;"),
            ("use.php", "<?php namespace App; echo VERSION;"),
            ("global.php", "<?php echo \\App\\VERSION;"),
        ],
        "c.php",
        "<?php namespace App; const VERSION = 2;",
    );
    assert_eq!(rechecked, ["c.php", "global.php", "use.php"]);
}

#[test]
fn an_edited_entity_table_name_is_found_after_the_edit() {
    let entity = |table: &str| {
        format!(
            "<?php use Doctrine\\ORM\\Mapping as ORM; #[ORM\\Entity] #[ORM\\Table(name: '{table}')] class Tag {{ #[ORM\\Id] #[ORM\\Column] public int $id; }}"
        )
    };
    let mut project = Project::new();
    project
        .load_analyzed_file(PathBuf::from("tag.php"), &entity("tags"))
        .unwrap();
    project.finish_loading();
    assert!(
        project
            .registry()
            .doctrine_entity_for_table("tags")
            .is_some()
    );
    project
        .update_file(&PathBuf::from("tag.php"), &entity("labels"))
        .unwrap();
    assert!(
        project
            .registry()
            .doctrine_entity_for_table("tags")
            .is_none()
    );
    assert!(
        project
            .registry()
            .doctrine_entity_for_table("labels")
            .is_some()
    );
}

/// Asserts that editing `edited` to `new_source` leaves every file with the
/// diagnostics a project loading the edited text from the start reports.
fn assert_edit_matches_a_fresh_load(files: &[(&str, &str)], edited: &str, new_source: &str) {
    let load = |files: &[(&str, &str)]| {
        let mut project = Project::new();
        for (path, source) in files {
            project
                .load_analyzed_file(PathBuf::from(path), source)
                .unwrap();
        }
        project.finish_loading();
        project
    };
    let mut edited_project = load(files);
    edited_project
        .update_file(&PathBuf::from(edited), new_source)
        .unwrap();
    let fresh_files: Vec<(&str, &str)> = files
        .iter()
        .map(|&(path, source)| (path, if path == edited { new_source } else { source }))
        .collect();
    let fresh_project = load(&fresh_files);
    let messages = |project: &Project, path: &str| -> Vec<String> {
        project
            .check_file(Path::new(path))
            .iter()
            .map(|d| format!("{}:{} {}", d.span.start.line, d.span.start.col, d.message))
            .collect()
    };
    for (path, _) in files {
        assert_eq!(
            messages(&edited_project, path),
            messages(&fresh_project, path),
            "{path} after editing {edited}"
        );
    }
}

#[test]
fn a_function_that_became_impure_makes_its_callers_impure() {
    assert_edit_matches_a_fresh_load(
        &[
            ("g.php", "<?php function g(): int { return 1; }"),
            ("f.php", "<?php function f(): int { return g(); } f();"),
        ],
        "g.php",
        "<?php function g(): int { echo 'x'; return 1; }",
    );
}

#[test]
fn an_inherited_return_tag_follows_the_parents_edit() {
    assert_edit_matches_a_fresh_load(
        &[
            (
                "parent.php",
                "<?php class P { /** @return int */ public function m() { return 1; } }",
            ),
            (
                "child.php",
                "<?php class C extends P { public function m() { return 2; } } function f(C $c): int { return $c->m(); }",
            ),
        ],
        "parent.php",
        "<?php class P { /** @return string */ public function m() { return 'a'; } }",
    );
}

#[test]
fn a_method_removed_from_a_trait_is_gone_from_its_users() {
    assert_edit_matches_a_fresh_load(
        &[
            ("t.php", "<?php trait T { public function m(): void {} }"),
            ("c.php", "<?php class C { use T; } (new C())->m();"),
        ],
        "t.php",
        "<?php trait T {}",
    );
}

#[test]
fn value_of_an_edited_enum_takes_its_new_cases() {
    assert_edit_matches_a_fresh_load(
        &[
            ("e.php", "<?php enum E: string { case A = 'a'; }"),
            (
                "f.php",
                "<?php /** @param value-of<E> $v */ function f(string $v): void {} f('b');",
            ),
        ],
        "e.php",
        "<?php enum E: string { case A = 'a'; case B = 'b'; }",
    );
}

#[test]
fn removing_a_trait_file_drops_its_members_from_its_users() {
    let mut project = Project::new();
    project
        .load_analyzed_file(
            PathBuf::from("t.php"),
            "<?php trait T { public function m(): void {} }",
        )
        .unwrap();
    project
        .load_analyzed_file(PathBuf::from("c.php"), "<?php class C { use T; }")
        .unwrap();
    project.finish_loading();
    assert!(project.registry().classes["C"].methods().contains_key("m"));
    project.remove_files(&[PathBuf::from("t.php")]);
    assert!(!project.registry().classes["C"].methods().contains_key("m"));
}

#[test]
fn phpstans_own_classes_are_loaded_only_while_a_file_names_them() {
    let rule = "PHPStan\\Rules\\Rule";
    let mut project = Project::new();
    project
        .load_analyzed_file(PathBuf::from("a.php"), "<?php class A {}")
        .unwrap();
    project.finish_loading();
    assert!(!project.registry().classes.contains_key(rule));
    project
        .update_file(
            &PathBuf::from("a.php"),
            "<?php class A { public function f(\\PHPStan\\Rules\\Rule $r): void {} }",
        )
        .unwrap();
    assert!(project.registry().classes.contains_key(rule));
    project
        .update_file(&PathBuf::from("a.php"), "<?php class A {}")
        .unwrap();
    assert!(!project.registry().classes.contains_key(rule));
    assert!(project.registry().classes.contains_key("Exception"));
}

#[test]
fn a_project_declaring_a_phpstan_class_merges_its_declaration_into_the_stub() {
    let mut project = Project::new();
    project
        .load_analyzed_file(
            PathBuf::from("rule.php"),
            "<?php namespace PHPStan\\Rules; interface Rule { public function ownMethod(): void; }",
        )
        .unwrap();
    project.finish_loading();
    let rule = &project.registry().classes["PHPStan\\Rules\\Rule"];
    assert!(rule.methods().contains_key("ownMethod"));
    assert!(rule.methods().contains_key("processNode"));
}

/// Every `.php` file under `dir`, sorted.
fn php_files_under(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_php_files(dir, &mut files);
    files.sort();
    files
}

/// Loads `sources` as one project, replaces `edits` of its files in turn
/// with the other sources, and asserts after each that the project's
/// diagnostics are those of a project loading the files as they now are.
/// `pick` chooses which file each edit replaces, from the edit's number.
fn assert_edits_match_fresh_loads(
    sources: &[String],
    edits: usize,
    pick: impl Fn(usize, usize) -> usize,
) {
    let (loaded, replacements) = sources.split_at(sources.len() / 2);
    let mut texts: Vec<(String, String)> = loaded
        .iter()
        .enumerate()
        .map(|(i, source)| (format!("f{i}.php"), source.clone()))
        .collect();
    let load = |texts: &[(String, String)]| {
        let mut project = Project::new();
        for (path, source) in texts {
            let _ = project.load_analyzed_file(PathBuf::from(path), source);
        }
        project.finish_loading();
        project
    };
    let diagnostics = |project: &Project| -> Vec<String> {
        let mut all: Vec<String> = project
            .analyzed_paths()
            .iter()
            .flat_map(|path| {
                project.check_file(path).into_iter().map(move |d| {
                    format!(
                        "{} {}:{} {}",
                        path.display(),
                        d.span.start.line,
                        d.span.start.col,
                        d.message
                    )
                })
            })
            .collect();
        all.sort();
        all
    };
    let mut project = load(&texts);
    for (step, replacement) in replacements.iter().enumerate().take(edits) {
        let edited = pick(step, texts.len());
        if project
            .update_file(Path::new(&texts[edited].0), replacement)
            .is_err()
        {
            continue;
        }
        texts[edited].1 = replacement.clone();
        assert_eq!(
            diagnostics(&project),
            diagnostics(&load(&texts)),
            "after replacing {} (step {step})",
            texts[edited].0
        );
    }
}

fn fixture_sources(subdirectory: &str) -> Vec<String> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(subdirectory);
    php_files_under(&root)
        .iter()
        .filter_map(|path| std::fs::read_to_string(path).ok())
        .collect()
}

#[test]
fn edits_across_many_fixtures_match_a_fresh_load() {
    let sources: Vec<String> = fixture_sources("checker").into_iter().step_by(40).collect();
    assert_edits_match_fresh_loads(&sources, 8, |step, len| step * 7 % len);
}

/// Every fixture takes part in some project: consecutive groups of 50,
/// four edits each.
#[test]
#[ignore = "slow: every fixture; run with --ignored"]
fn edits_across_every_fixture_match_a_fresh_load() {
    let sources = fixture_sources("checker");
    for (group, chunk) in sources.chunks(50).enumerate() {
        assert_edits_match_fresh_loads(chunk, 4, |step, len| (step * 5 + group) % len);
    }
}

proptest::proptest! {
    #![proptest_config(proptest::prelude::ProptestConfig::with_cases(6))]

    /// Random files in random projects, edited in random order.
    #[test]
    fn random_edits_match_a_fresh_load(
        seed in 0usize..10_000,
        stride in 25usize..90,
        edits in 3usize..7,
    ) {
        let sources: Vec<String> = fixture_sources("checker")
            .into_iter()
            .skip(seed % stride)
            .step_by(stride)
            .collect();
        assert_edits_match_fresh_loads(&sources, edits, |step, len| {
            (seed.wrapping_mul(31).wrapping_add(step * 17)) % len
        });
    }
}

#[test]
fn a_changed_global_variable_type_rechecks_the_files_declaring_it_global() {
    let mut project = Project::new();
    let load = |project: &mut Project, path: &str, source: &str| {
        project
            .load_file(PathBuf::from(path), source)
            .expect("parses");
    };
    load(
        &mut project,
        "classes.php",
        "<?php class A { function m() {} } class B {}",
    );
    load(
        &mut project,
        "writer.php",
        "<?php function w() { global $t; $t = new A(); }",
    );
    load(
        &mut project,
        "reader.php",
        "<?php function r() { global $t; $t->m(); }",
    );
    project.finish_loading();

    let results = project
        .update_file(
            &PathBuf::from("writer.php"),
            "<?php function w() { global $t; $t = new B(); }",
        )
        .unwrap();
    let reader = results
        .get(&PathBuf::from("reader.php"))
        .expect("the reader is rechecked");
    assert!(
        reader.iter().any(|d| d.message.contains("m()")),
        "{reader:?}"
    );
}
