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

#[test]
fn a_class_respelled_by_an_edit_leaves_the_subtype_index_once() {
    let mut project = Project::new();
    let path = PathBuf::from("a.php");
    project
        .load_analyzed_file(
            path.clone(),
            "<?php class Base {} class Child extends Base {} trait T {} class User { use T; }",
        )
        .unwrap();
    project.finish_loading();
    let subclasses = |project: &Project| -> Vec<String> {
        project
            .registry()
            .subclasses("Base")
            .iter()
            .map(|name| name.to_string())
            .collect()
    };
    let users = |project: &Project| -> Vec<String> {
        project
            .registry()
            .inheritors("T")
            .iter()
            .map(|name| name.to_string())
            .collect()
    };
    assert_eq!(subclasses(&project), ["Child"]);
    assert_eq!(users(&project), ["User"]);
    project
        .update_file(
            &path,
            "<?php class Base {} class CHILD extends Base {} trait T {} class USER { use T; }",
        )
        .unwrap();
    assert_eq!(subclasses(&project), ["CHILD"]);
    assert_eq!(users(&project), ["USER"]);
    project
        .update_file(&path, "<?php class Base {} trait T {}")
        .unwrap();
    assert!(subclasses(&project).is_empty());
    assert!(users(&project).is_empty());
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
        .map(|(i, source)| (format!("f{i:03}.php"), source.clone()))
        .collect();
    // Each fresh load parses every text again, though all but the edited
    // one are the ones the load before parsed: the parse of a file is a
    // function of its path and text, and a fresh load takes a copy of it. A
    // text that does not parse is ignored, as `load_analyzed_file`'s error
    // is.
    let parsed: std::cell::RefCell<
        rustc_hash::FxHashMap<(String, String), Option<crate::project::ParsedFile>>,
    > = Default::default();
    let load = |texts: &[(String, String)]| {
        let mut project = Project::new();
        let files: Vec<Option<crate::project::ParsedFile>> = texts
            .iter()
            .map(|(path, source)| {
                parsed
                    .borrow_mut()
                    .entry((path.clone(), source.clone()))
                    .or_insert_with(|| {
                        crate::project::parse_file(Path::new(path), source.as_bytes(), false).ok()
                    })
                    .clone()
            })
            .collect();
        // As `load_files` loads a batch: a project that is to have PHPStan's
        // source in it starts from the registry that has it, and does not
        // switch to it when it finishes.
        project.expect_files(files.iter().flatten());
        for ((path, source), file) in texts.iter().zip(files) {
            if let Some(file) = file {
                project.insert_parsed_file(PathBuf::from(path), file, source.to_string(), true);
            }
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

/// The checker fixtures' texts, read once for every test of the process.
fn checker_sources() -> &'static [String] {
    static SOURCES: std::sync::LazyLock<Vec<String>> = std::sync::LazyLock::new(|| {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/checker");
        php_files_under(&root)
            .iter()
            .filter_map(|path| std::fs::read_to_string(path).ok())
            .collect()
    });
    &SOURCES
}

#[test]
fn edits_across_many_fixtures_match_a_fresh_load() {
    let sources: Vec<String> = checker_sources().iter().step_by(40).cloned().collect();
    assert_edits_match_fresh_loads(&sources, 8, |step, len| step * 7 % len);
}

/// Every fixture takes part in some project: consecutive groups of 50,
/// four edits each.
#[test]
fn edits_across_every_fixture_match_a_fresh_load() {
    use rayon::prelude::*;
    let sources = checker_sources();
    // The groups share nothing, so they run beside each other, on a pool of
    // this test's own. A group that is first to need a registry built
    // lazily (`Project::phpstan_base`) makes the others wait for it, and
    // its build runs on `worker_pool()`: were the groups that pool's
    // workers, all of them would be waiting and none could serve the build.
    let pool = rayon::ThreadPoolBuilder::new()
        .stack_size(crate::project::WORKER_STACK_SIZE)
        .build()
        .expect("the test pool starts");
    pool.install(|| {
        sources
            .par_chunks(50)
            .enumerate()
            .for_each(|(group, chunk)| {
                assert_edits_match_fresh_loads(chunk, 4, |step, len| (step * 5 + group) % len);
            });
    });
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
        let sources: Vec<String> = checker_sources()
            .iter()
            .skip(seed % stride)
            .step_by(stride)
            .cloned()
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

#[test]
fn a_rebuild_follows_the_order_files_were_loaded_not_their_paths() {
    // `z.php` loads before `a.php`; both declare `Dup`, the second replacing
    // what the first gave, so the result depends on the order.
    let z = "<?php class Dup { function fromZ() {} }";
    let a_before = "<?php class Dup { function fromA() {} }";
    let a_after = "<?php class Dup { function fromA() {} function edited() {} }";
    let load = |first: &str, second: &str| {
        let mut project = Project::new();
        project
            .load_analyzed_file(PathBuf::from("z.php"), first)
            .unwrap();
        project
            .load_analyzed_file(PathBuf::from("a.php"), second)
            .unwrap();
        project.finish_loading();
        project
    };
    let methods = |project: &Project| -> Vec<String> {
        let mut names: Vec<String> = project.registry().classes["Dup"]
            .methods()
            .keys()
            .map(|name| name.to_string())
            .collect();
        names.sort();
        names
    };
    let mut edited = load(z, a_before);
    edited
        .update_file(Path::new("a.php"), a_after)
        .expect("parses");
    let fresh = load(z, a_after);
    assert_eq!(methods(&edited), methods(&fresh));
}

/// The route the edit tests' fresh loads take, a batch, gives what a load
/// file by file does, which switches to the registry with PHPStan's source
/// when the project finishes.
#[test]
fn a_batch_load_equals_a_load_file_by_file() {
    let sources = [
        "<?php function helper(int $a): int { return $a; }",
        "<?php use PHPStan\\Type\\Type; function takes(Type $t): void { $t->describe(); }",
        "<?php class Plain { public function run(): string { return helper(1); } }",
    ];
    let paths = ["a.php", "b.php", "c.php"];
    let diagnostics = |project: &Project| -> Vec<String> {
        let mut all: Vec<String> = project
            .analyzed_paths()
            .iter()
            .flat_map(|path| {
                project
                    .check_file(path)
                    .into_iter()
                    .map(move |d| format!("{} {}", path.display(), d.message))
            })
            .collect();
        all.sort();
        all
    };
    let mut one_by_one = Project::new();
    for (path, source) in paths.iter().zip(sources) {
        let _ = one_by_one.load_analyzed_file(PathBuf::from(path), source);
    }
    one_by_one.finish_loading();
    let mut batch = Project::new();
    let files: Vec<crate::project::ParsedFile> = paths
        .iter()
        .zip(sources)
        .map(|(path, source)| {
            crate::project::parse_file(Path::new(path), source.as_bytes(), false).expect("parses")
        })
        .collect();
    batch.expect_files(files.iter());
    for ((path, source), file) in paths.iter().zip(sources).zip(files) {
        batch.insert_parsed_file(PathBuf::from(path), file, source.to_string(), true);
    }
    batch.finish_loading();
    assert_eq!(diagnostics(&one_by_one), diagnostics(&batch));
    assert_eq!(
        one_by_one.registry().classes.len(),
        batch.registry().classes.len()
    );
}

/// One edit as the language server makes it.
enum Edit<'a> {
    Update(&'a str, &'a str),
    Remove(&'a str),
}

/// Asserts that after `edits`, the diagnostics the language server holds
/// for each file - its last check, until an edit rechecks it - are those a
/// project loading the edited files from the start reports, with `settings`
/// applied to both.
fn assert_held_diagnostics_match_a_fresh_load(
    settings: impl Fn() -> ProjectSettings,
    files: &[(&str, &str)],
    edits: &[Edit],
) {
    let load = |files: &[(String, String)]| {
        let mut project = Project::new();
        project.apply_settings(settings());
        for (path, source) in files {
            project
                .load_analyzed_file(PathBuf::from(path), source)
                .unwrap();
        }
        project.finish_loading();
        project
    };
    let print = |diagnostics: &[Diagnostic]| -> Vec<String> {
        diagnostics
            .iter()
            .map(|d| format!("{}:{} {}", d.span.start.line, d.span.start.col, d.message))
            .collect()
    };
    let mut current: Vec<(String, String)> = files
        .iter()
        .map(|&(path, source)| (path.to_string(), source.to_string()))
        .collect();
    let mut project = load(&current);
    let mut held: std::collections::BTreeMap<String, Vec<String>> = current
        .iter()
        .map(|(path, _)| (path.clone(), print(&project.check_file(Path::new(path)))))
        .collect();
    for edit in edits {
        let rechecked = match *edit {
            Edit::Update(path, source) => {
                current.retain(|(p, _)| p != path);
                current.push((path.to_string(), source.to_string()));
                project.update_file(Path::new(path), source).unwrap()
            }
            Edit::Remove(path) => {
                current.retain(|(p, _)| p != path);
                held.remove(path);
                project.remove_file(Path::new(path))
            }
        };
        for (path, diagnostics) in rechecked {
            let path = path.display().to_string();
            if current.iter().any(|(p, _)| *p == path) {
                held.insert(path, print(&diagnostics));
            }
        }
    }
    let fresh = load(&current);
    for (path, _) in &current {
        assert_eq!(
            held.get(path).cloned().unwrap_or_default(),
            print(&fresh.check_file(Path::new(path))),
            "{path}"
        );
    }
}

#[test]
fn a_trait_is_rechecked_when_a_class_using_it_changes() {
    let files = [
        (
            "t.php",
            "<?php trait T { public function m(): int { return $this->x; } }",
        ),
        ("c.php", "<?php class C { use T; public int $x = 1; }"),
    ];
    assert_held_diagnostics_match_a_fresh_load(
        ProjectSettings::default,
        &files,
        &[Edit::Update("c.php", "<?php class C { use T; }")],
    );
    assert_held_diagnostics_match_a_fresh_load(
        ProjectSettings::default,
        &files,
        &[Edit::Remove("c.php")],
    );
}

#[test]
fn the_other_declarations_of_a_duplicate_are_rechecked() {
    let settings = || ProjectSettings {
        rules: crate::rules_settings::RulesSettings {
            check_duplicate_function_declarations: true,
            check_duplicate_class_declarations: true,
            ..Default::default()
        },
        ..ProjectSettings::default()
    };
    let files = [
        ("a.php", "<?php function dup(): void {} class Dup {}"),
        ("b.php", "<?php function dup(): void {} class Dup {}"),
    ];
    for edits in [
        &[Edit::Update(
            "a.php",
            "<?php function other(): void {} class Other {}",
        )][..],
        &[Edit::Update(
            "a.php",
            "<?php\n\nfunction dup(): void {}\n\nclass Dup {}",
        )],
        &[Edit::Remove("a.php")],
        &[Edit::Update(
            "c.php",
            "<?php function dup(): void {} class Dup {}",
        )],
    ] {
        assert_held_diagnostics_match_a_fresh_load(settings, &files, edits);
    }
}
