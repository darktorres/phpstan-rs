//! Incremental `update_file` behavior: registry updates, re-check scoping,
//! and dependency invalidation (including transitive cases) across files.

use super::super::test_support::Files;
use super::super::*;

#[test]
fn updating_one_file_does_not_disturb_another_files_registry_entries() {
    let mut project = Files::new()
        .context("a.php", "<?php function helperA(): int { return 1; }")
        .context(
            "b.php",
            "<?php function helperB(): int { return helperA(); }",
        )
        .load();

    assert!(project.registry().functions().contains_key("helperA"));
    assert!(project.registry().functions().contains_key("helperB"));

    // Change b.php's return type - a.php's own registry entry and
    // parse tree must be untouched by this.
    project
        .update_file(
            &PathBuf::from("b.php"),
            "<?php function helperB(): string { return 'x'; }",
        )
        .unwrap();

    assert!(project.registry().functions().contains_key("helperA"));
    let helper_b = &project.registry().functions()["helperB"];
    assert_eq!(helper_b.return_type().to_string(), "string");
}

#[test]
fn updating_an_unrelated_file_keeps_an_unchanged_function_result_overlay() {
    let mut project = Files::new()
        .analysed(
            "a.php",
            "<?php const ANSWER = 42; /** @param ANSWER $value */ function reads_answer($value): void {}",
        )
        .analysed("b.php", "<?php class Other {}")
        .load();

    assert_eq!(
        project
            .registry()
            .lookup_function("reads_answer")
            .unwrap()
            .params()[0]
            .ty(),
        &types::Type::int_literal(42)
    );

    project
        .update_file(
            &PathBuf::from("b.php"),
            "<?php class Other { public int $x; }",
        )
        .unwrap();

    assert_eq!(
        project
            .registry()
            .lookup_function("reads_answer")
            .unwrap()
            .params()[0]
            .ty(),
        &types::Type::int_literal(42)
    );
}

#[test]
fn removing_a_declaration_on_update_drops_it_from_the_registry() {
    let mut project = Files::new()
        .context(
            "a.php",
            "<?php function helperA(): int { return 1; } class Foo {}",
        )
        .load();
    assert!(project.registry().classes().contains_key("Foo"));

    // The new version of the file no longer declares Foo at all.
    project
        .update_file(
            &PathBuf::from("a.php"),
            "<?php function helperA(): int { return 1; }",
        )
        .unwrap();
    assert!(!project.registry().classes().contains_key("Foo"));
    assert!(project.registry().functions().contains_key("helperA"));
}

#[test]
fn incremental_update_still_resolves_cross_file_inheritance() {
    let mut project = Files::new()
        .context("base.php", "<?php class Base { public int $x = 1; }")
        .context("child.php", "<?php class Child extends Base {}")
        .load();
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
    let mut project = Files::new()
        .context("base.php", "<?php class Base { public int $x = 1; }")
        .context(
            "caller.php",
            "<?php function useIt(Base $b) { return $b->x + 1; }",
        )
        .context(
            "unrelated.php",
            "<?php function unrelated(): int { return 1; }",
        )
        .load();

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
    let mut project = Files::new()
        .context("thing.php", "<?php function thing(): int { return 1; }")
        .context(
            "caller.php",
            "<?php function useThing() { return thing(); }",
        )
        .load();

    // thing.php no longer declares `thing` at all - caller.php, which
    // calls it by name, still needs to be re-checked (it'll now see
    // an unresolvable call), even though nothing in caller.php itself
    // changed.
    let result = project
        .update_file(&PathBuf::from("thing.php"), "<?php // now empty")
        .unwrap();

    assert!(result.contains_key(&PathBuf::from("thing.php")));
    assert!(result.contains_key(&PathBuf::from("caller.php")));
    assert!(!project.registry().functions().contains_key("thing"));
}

#[test]
fn invalidation_is_transitive_across_an_inheritance_chain() {
    let mut project = Files::new()
        .context("c.php", "<?php class C { public int $x = 1; }")
        .context("b.php", "<?php class B extends C {}")
        .context("a.php", "<?php function useB(B $b) { return $b->x; }")
        .load();

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
    let mut project = Files::new()
        .context(
            "c.php",
            "<?php namespace App; class C { public int $x = 1; }",
        )
        .context("b.php", "<?php namespace App; class B extends C {}")
        .context(
            "a.php",
            "<?php namespace App; function useB(B $b) { return $b->x; }",
        )
        .load();

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
    let mut project = Files::new()
        .context(
            "dep.php",
            "<?php class Dep { public static function make(): Dep { return new Dep(); } }",
        )
        .context(
            "holder.php",
            r#"<?php class Holder {
            public $prop {
                get => Dep::make();
            }
        }"#,
        )
        .load();

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
    let mut project = Files::new()
        .context("dep.php", "<?php interface Dep {}")
        .context(
            "status.php",
            "<?php enum Status implements Dep { case Active; }",
        )
        .load();

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
    let mut project = Files::new()
        .context("c.php", "<?php class C { public int $x = 1; }")
        .context("b.php", "<?php class B extends C {}")
        .context("a.php", "<?php function useB(B $b) { return $b->x; }")
        .context(
            "unrelated.php",
            "<?php function unrelated(): int { return 1; }",
        )
        .load();

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
    let path = PathBuf::from("a.php");
    let mut project = Files::new()
        .settings(ProjectSettings {
            retain_source: true,
            ..ProjectSettings::default()
        })
        .context(&path, "<?php function helperA(): int { return 1; }")
        .load();

    let err = project.update_file(&path, "<?php function helperA(): int {");
    assert!(
        err.is_err(),
        "expected the truncated source to fail to parse"
    );

    assert!(
        project.ast_of(&path).is_some(),
        "the last-good AST must survive a failed reparse"
    );
    assert!(
        project.registry().functions().contains_key("helperA"),
        "the last-good registry entry must survive a failed reparse too"
    );
}

/// Proves `update_file_cancellable`'s `should_abort` closure can observe
/// a generation bump made by a genuinely different, concurrently-running
/// thread strictly between two per-file checks of the SAME in-flight
/// cascade - the property a single-threaded caller (and therefore every
/// other test in this file) can never exercise, since nothing else could
/// possibly run between two of its own sequential closure calls. The
/// pool's threads ask it one at a time, so exactly one file's check
/// starts before the bump however many threads the pool has.
#[test]
fn update_file_cancellable_can_be_aborted_mid_cascade_by_a_concurrent_generation_bump() {
    use std::sync::mpsc;
    use std::sync::{Arc, Mutex};
    use std::thread;

    let mut files = Files::new().context("base.php", "<?php function base(): int { return 1; }");
    for i in 1..=3 {
        files = files.context(
            format!("dep{i}.php"),
            &format!("<?php function dep{i}(): int {{ return base(); }}"),
        );
    }
    let project = files.load();

    let project = Arc::new(Mutex::new(project));
    let generation = Arc::new(Mutex::new(1u64));
    let (checked_tx, checked_rx) = mpsc::channel::<()>();
    let (bump_tx, bump_rx) = mpsc::channel::<()>();

    let worker_project = Arc::clone(&project);
    let worker_generation = Arc::clone(&generation);
    let handle = thread::spawn(move || {
        let calls = Mutex::new((0usize, checked_tx, bump_rx));
        let should_abort = move || {
            let mut calls = calls.lock().unwrap();
            calls.0 += 1;
            // Rendezvous right before the SECOND per-file check, holding
            // `calls` so no other thread asks meanwhile - proves at least
            // one file was already processed before the concurrent bump
            // takes effect, not just "aborts before doing any work at
            // all."
            if calls.0 == 2 {
                calls.1.send(()).unwrap();
                calls.2.recv().unwrap();
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
    let mut project = Files::new()
        .context("a.php", "<?php class Template {}")
        .context(
            "b.php",
            "<?php function f() { global $template; $template = new Template(); }",
        )
        .load();

    assert_eq!(
        project.registry().global_variables().get("template"),
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
        project.registry().global_variables().get("template"),
        Some(&types::Type::object("Other".into(), vec![]))
    );
}

#[test]
fn an_edited_context_file_stays_context() {
    let path = PathBuf::from("lib/helper.php");
    let mut project = Files::new()
        .context(&path, "<?php function helper(): int { return 1; }")
        .load();
    assert!(!project.files[&path].analyzed);

    project
        .update_file(&path, "<?php function helper(): int { return 2; }")
        .unwrap();
    assert!(!project.files[&path].analyzed);
}

#[test]
fn an_edited_analyzed_file_stays_analyzed() {
    let path = PathBuf::from("vendor/acme/helper.php");
    let mut project = Files::new()
        .analysed(&path, "<?php function helper(): int { return 1; }")
        .load();
    assert!(project.files[&path].analyzed);

    project
        .update_file(&path, "<?php function helper(): int { return 2; }")
        .unwrap();
    assert!(project.files[&path].analyzed);
}

fn class_has_method(project: &Project, class: &str, method: &str) -> bool {
    project
        .registry()
        .lookup_own_method(class, method)
        .is_some()
}

#[test]
fn editing_a_file_merged_into_a_built_in_class_keeps_the_built_in_members() {
    let sources = [
        "<?php if (!class_exists('ArrayObject')) { class ArrayObject { public function polyOnly(): int { return 1; } } }",
        "<?php class ArrayObject { public function polyOnly(): int { return 1; } }",
    ];
    for source in sources {
        for analyzed in [false, true] {
            let path = PathBuf::from("polyfill.php");
            let files = Files::new();
            let mut project = if analyzed {
                files.analysed(&path, source)
            } else {
                files.context(&path, source)
            }
            .load();
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
    let path = PathBuf::from("polyfill.php");
    let mut project = Files::new().context(&path, source).load();
    let params = |project: &Project| project.registry().functions()["mb_str_pad"].params().len();
    assert_eq!(params(&project), 5);

    project.update_file(&path, &format!("{source}\n")).unwrap();
    assert_eq!(params(&project), 5);
}

#[test]
fn removing_a_file_merged_into_a_built_in_class_restores_the_built_in() {
    let path = PathBuf::from("polyfill.php");
    let mut project = Files::new()
        .context(
            &path,
            "<?php if (!class_exists('ArrayObject')) { class ArrayObject { public function polyOnly(): int { return 1; } } }",
        )
        .load();

    project.remove_file(&path);
    assert!(class_has_method(&project, "ArrayObject", "count"));
    assert!(!class_has_method(&project, "ArrayObject", "polyOnly"));
}

#[test]
fn editing_one_of_two_merged_declarations_keeps_the_other() {
    let first = PathBuf::from("lib/a.php");
    let second = PathBuf::from("lib/b.php");
    let mut project = Files::new()
        .context(
            &first,
            "<?php class Shared { public function fromA(): int { return 1; } }",
        )
        .context(
            &second,
            "<?php class Shared { public function fromB(): int { return 1; } }",
        )
        .load();
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
    let mut loaded = Files::new();
    for (path, source) in files {
        loaded = loaded.analysed(path, source);
    }
    let mut project = loaded.load();
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
    let mut project = Files::new()
        .analysed("c.php", "<?php const VERSION = 1;")
        .load();
    project
        .update_file(&PathBuf::from("c.php"), "<?php const VERSION = 2;")
        .unwrap();
    assert_eq!(
        project.registry().global_constants()["VERSION"].to_string(),
        "2"
    );
}

#[test]
fn a_constant_deleted_from_its_file_is_undefined() {
    let mut project = Files::new()
        .analysed("c.php", "<?php const GONE = 1; const KEPT = 1;")
        .load();
    project
        .update_file(&PathBuf::from("c.php"), "<?php const KEPT = 1;")
        .unwrap();
    assert!(!project.registry().global_constants().contains_key("GONE"));
    assert!(project.registry().global_constants().contains_key("KEPT"));
}

#[test]
fn removing_a_file_keeps_another_files_definition_of_its_constant() {
    let mut project = Files::new()
        .analysed("a.php", "<?php define('SHARED', 1);")
        .analysed("b.php", "<?php define('SHARED', 2);")
        .load();
    assert!(project.registry().global_constants()["SHARED"].is_mixed());
    project.remove_files(&[PathBuf::from("b.php")]);
    assert_eq!(
        project.registry().global_constants()["SHARED"].to_string(),
        "1"
    );
}

#[test]
fn editing_a_file_defining_a_built_in_constant_keeps_the_built_in() {
    let mut project = Files::new()
        .analysed("c.php", "<?php const UNRELATED = 1;")
        .load();
    let built_in = project.registry().global_constants()["PHP_EOL"].clone();
    project
        .update_file(
            &PathBuf::from("c.php"),
            "<?php if (!defined('PHP_EOL')) { define('PHP_EOL', \"\\n\"); }",
        )
        .unwrap();
    project
        .update_file(&PathBuf::from("c.php"), "<?php const UNRELATED = 1;")
        .unwrap();
    assert_eq!(project.registry().global_constants()["PHP_EOL"], built_in);
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
    let mut project = Files::new().analysed("tag.php", &entity("tags")).load();
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

#[test]
fn of_two_entities_on_one_table_the_first_by_name_maps_it_whatever_the_order() {
    let entity = |class: &str| {
        format!(
            "<?php use Doctrine\\ORM\\Mapping as ORM; #[ORM\\Entity(repositoryClass: {class}Repository::class)] #[ORM\\Table(name: 'shared')] class {class} {{ #[ORM\\Id] #[ORM\\Column] public int $id; }}"
        )
    };
    let mapped = |project: &Project| {
        project
            .registry()
            .doctrine_entity_for_table("shared")
            .and_then(|entity| entity.repository_class.clone())
            .map(|class| class.to_string())
    };
    let mut project = Files::new()
        .analysed("beta.php", &entity("Beta"))
        .analysed("alpha.php", &entity("Alpha"))
        .load();
    assert_eq!(mapped(&project).as_deref(), Some("AlphaRepository"));
    project
        .update_file(&PathBuf::from("alpha.php"), &entity("Alpha"))
        .unwrap();
    assert_eq!(mapped(&project).as_deref(), Some("AlphaRepository"));
}

#[test]
fn effects_read_a_property_by_the_type_php_enforces_before_and_after_an_edit() {
    let files = [
        (
            "store.php",
            "<?php interface Store { public function get(): int; } final class Counter implements Store { public static int $n = 0; public function get(): int { return ++self::$n; } }",
        ),
        (
            "holder.php",
            "<?php final class Holder { /** @param Store $store */ public function __construct(private readonly int $store) {} public function read(): int { return $this->store->get(); } }",
        ),
        ("other.php", "<?php echo 1;"),
    ];
    let mut loaded = Files::new();
    for (path, source) in files {
        loaded = loaded.analysed(path, source);
    }
    let mut project = loaded.load();
    assert!(
        project
            .registry()
            .virtual_method_effect("Holder", "read")
            .is_nothing()
    );
    project.remove_file(&PathBuf::from("other.php"));
    assert!(
        project
            .registry()
            .virtual_method_effect("Holder", "read")
            .is_nothing()
    );
}

#[test]
fn a_class_in_scope_copies_a_trait_member_as_a_load_does() {
    let files = [
        (
            "position.php",
            "<?php interface Positionable { /** @return $this */ public function at(): Positionable; } trait Position { /** @return $this */ public function at(): Positionable { return $this; } }",
        ),
        ("format.php", "<?php class Format {}"),
        (
            "comment.php",
            "<?php class Comment implements Positionable { use Position; public function render(Format $f): string { return ''; } }",
        ),
    ];
    let mut loaded = Files::new();
    for (path, source) in files {
        loaded = loaded.analysed(path, source);
    }
    let mut project = loaded.load();
    let return_type = |project: &Project| {
        project
            .registry()
            .lookup_method("Comment", "at")
            .map(|sig| sig.return_type().to_string())
    };
    let loaded = return_type(&project);
    assert_eq!(loaded.as_deref(), Some("$this"));
    project
        .update_file(&PathBuf::from("format.php"), files[1].1)
        .unwrap();
    assert_eq!(return_type(&project), loaded);
}

/// Asserts that editing `edited` to `new_source` leaves every file with the
/// diagnostics a project loading the edited text from the start reports.
fn assert_edit_matches_a_fresh_load(files: &[(&str, &str)], edited: &str, new_source: &str) {
    let load = |files: &[(&str, &str)]| {
        let mut loaded = Files::new();
        for (path, source) in files {
            loaded = loaded.analysed(path, source);
        }
        loaded.load()
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
fn a_trait_method_result_change_rechecks_its_unchanged_host_and_consumers() {
    let mut project = Files::new()
        .analysed(
            "trait.php",
            "<?php trait T { /** @return int */ public function m() { return 1; } }",
        )
        .analysed("host.php", "<?php class Host { use T; }")
        .analysed(
            "consumer.php",
            "<?php function useHost(Host $host): int { return $host->m(); }",
        )
        .load();
    let original_slot = project.registry().member_slot("Host", "m");
    let original_declaration = project.registry().classes()["Host"].clone();

    let checked = project
        .update_file(
            &PathBuf::from("trait.php"),
            "<?php trait T { /** @return string */ public function m() { return 'x'; } }",
        )
        .unwrap();
    assert!(checked.contains_key(&PathBuf::from("trait.php")));
    assert!(checked.contains_key(&PathBuf::from("host.php")));
    assert!(checked.contains_key(&PathBuf::from("consumer.php")));
    assert_eq!(project.registry().member_slot("Host", "m"), original_slot);
    assert_eq!(project.registry().classes()["Host"], original_declaration);
    assert!(
        project
            .registry()
            .lookup_own_method("Host", "m")
            .unwrap()
            .return_type()
            .is_string()
            .yes()
    );

    let unchanged = project
        .update_file(
            &PathBuf::from("trait.php"),
            "<?php trait T { /** @return string */ public function m() { return 'x'; } }",
        )
        .unwrap();
    let mut unchanged_paths: Vec<_> = unchanged
        .keys()
        .map(|path| path.display().to_string())
        .collect();
    unchanged_paths.sort();
    assert_eq!(unchanged_paths, ["trait.php"]);
}

#[test]
fn a_parent_constant_result_change_rechecks_a_child_signature_consumer() {
    let mut project = Files::new()
        .analysed("parent.php", "<?php class ParentType { public const MODES = ['a']; }")
        .analysed(
            "child.php",
            "<?php class ChildType extends ParentType { /** @param value-of<ParentType::MODES> $mode */ public function setMode($mode): void {} }",
        )
        .analysed(
            "consumer.php",
            "<?php function useChild(ChildType $child): void { $child->setMode('a'); }",
        )
        .load();
    let original_slot = project.registry().member_slot("ChildType", "setMode");
    let original_declaration = project.registry().classes()["ChildType"].clone();

    let checked = project
        .update_file(
            &PathBuf::from("parent.php"),
            "<?php class ParentType { public const MODES = ['a', 'b']; }",
        )
        .unwrap();
    assert!(checked.contains_key(&PathBuf::from("parent.php")));
    assert!(checked.contains_key(&PathBuf::from("child.php")));
    assert!(checked.contains_key(&PathBuf::from("consumer.php")));
    assert_eq!(
        project.registry().member_slot("ChildType", "setMode"),
        original_slot
    );
    assert_eq!(
        project.registry().classes()["ChildType"],
        original_declaration
    );
    let expected =
        types::Type::string_literal("a".into()).union(types::Type::string_literal("b".into()));
    assert_eq!(
        project
            .registry()
            .lookup_own_method("ChildType", "setMode")
            .unwrap()
            .params()[0]
            .ty(),
        &expected
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
    let mut project = Files::new()
        .analysed("t.php", "<?php trait T { public function m(): void {} }")
        .analysed("c.php", "<?php class C { use T; }")
        .load();
    assert!(project.registry().lookup_own_method("C", "m").is_some());
    project.remove_files(&[PathBuf::from("t.php")]);
    assert!(project.registry().lookup_own_method("C", "m").is_none());
}

#[test]
fn phpstans_own_classes_are_loaded_only_while_a_file_names_them() {
    let rule = "PHPStan\\Rules\\Rule";
    let mut project = Files::new().analysed("a.php", "<?php class A {}").load();
    assert!(!project.registry().classes().contains_key(rule));
    project
        .update_file(
            &PathBuf::from("a.php"),
            "<?php class A { public function f(\\PHPStan\\Rules\\Rule $r): void {} }",
        )
        .unwrap();
    assert!(project.registry().classes().contains_key(rule));
    assert!(project.registry().built_in_stub_classes.contains_key(rule));
    project
        .update_file(&PathBuf::from("a.php"), "<?php class A {}")
        .unwrap();
    assert!(!project.registry().classes().contains_key(rule));
    assert!(!project.registry().built_in_stub_classes.contains_key(rule));
    assert!(project.registry().classes().contains_key("Exception"));
}

#[test]
fn a_project_declaring_a_phpstan_class_merges_its_declaration_into_the_stub() {
    let project = Files::new()
        .analysed(
            "rule.php",
            "<?php namespace PHPStan\\Rules; interface Rule { public function ownMethod(): void; }",
        )
        .load();
    let rule = &project.registry().classes()["PHPStan\\Rules\\Rule"];
    assert!(rule.methods().contains_key("ownMethod"));
    assert!(rule.methods().contains_key("processNode"));
}

#[test]
fn a_class_respelled_by_an_edit_leaves_the_subtype_index_once() {
    let path = PathBuf::from("a.php");
    let mut project = Files::new()
        .analysed(
            &path,
            "<?php class Base {} class Child extends Base {} trait T {} class User { use T; }",
        )
        .load();
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
    let load = |texts: &[(String, String)]| {
        let mut loaded = Files::new();
        for (path, source) in texts {
            loaded = loaded.analysed(path, source);
        }
        loaded.load()
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
    let mut project = Files::new()
        .context(
            "classes.php",
            "<?php class A { function m() {} } class B {}",
        )
        .context(
            "writer.php",
            "<?php function w() { global $t; $t = new A(); }",
        )
        .context("reader.php", "<?php function r() { global $t; $t->m(); }")
        .load();

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
    // The context file `z.php` loads before the analysed `a.php`; both
    // declare `Dup`, the second replacing what the first gave, so the result
    // depends on the order.
    let z = "<?php class Dup { function fromZ() {} }";
    let a_before = "<?php class Dup { function fromA() {} }";
    let a_after = "<?php class Dup { function fromA() {} function edited() {} }";
    let load = |first: &str, second: &str| {
        Files::new()
            .context("z.php", first)
            .analysed("a.php", second)
            .load()
    };
    let methods = |project: &Project| -> Vec<String> {
        let mut names: Vec<String> = project.registry().classes()["Dup"]
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
        let mut loaded = Files::new().settings(settings());
        for (path, source) in files {
            loaded = loaded.analysed(path, source);
        }
        loaded.load()
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
fn a_file_naming_a_class_only_in_a_see_tag_is_rechecked() {
    let files = [
        ("see.php", "<?php /** @see Target */ function f(): void {}"),
        ("target.php", "<?php class Target {}"),
    ];
    assert_held_diagnostics_match_a_fresh_load(
        ProjectSettings::default,
        &files,
        &[Edit::Update("target.php", "<?php class Renamed {}")],
    );
    assert_held_diagnostics_match_a_fresh_load(
        ProjectSettings::default,
        &files,
        &[
            Edit::Remove("target.php"),
            Edit::Update("added.php", "<?php class Target {}"),
        ],
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

#[test]
fn an_include_is_rechecked_when_the_file_it_names_appears_or_vanishes() {
    let dir = super::helpers::TempDir::new("include_target");
    let includer = dir.write(
        "includer.php",
        "<?php require __DIR__ . '/target.php'; require __DIR__ . '/data.inc'; require __DIR__ . '/sub/inner.inc';",
    );
    let target = dir.write("target.php", "<?php function target(): void {}");
    let data = dir.write("data.inc", "<?php");
    dir.write("sub/inner.inc", "<?php");
    let mut loaded = Files::new();
    for path in [&includer, &target] {
        loaded = loaded.analysed(path, &fs::read_to_string(path).unwrap());
    }
    let mut project = loaded.load();
    let not_found = |diagnostics: &[Diagnostic]| {
        diagnostics
            .iter()
            .filter(|d| d.identifier == "require.fileNotFound")
            .count()
    };
    assert_eq!(not_found(&project.check_file(&includer)), 0);

    fs::remove_file(&target).unwrap();
    let rechecked = project.remove_file(&target);
    assert_eq!(not_found(&rechecked[&includer]), 1);
    fs::write(&target, "<?php").unwrap();
    let rechecked = project.update_file(&target, "<?php").unwrap();
    assert_eq!(not_found(&rechecked[&includer]), 0);

    fs::remove_file(&data).unwrap();
    let rechecked = project.recheck_including(std::slice::from_ref(&data));
    assert_eq!(not_found(&rechecked[&includer]), 1);
    fs::write(&data, "<?php").unwrap();
    let rechecked = project.recheck_including(std::slice::from_ref(&data));
    assert_eq!(not_found(&rechecked[&includer]), 0);

    fs::remove_dir_all(dir.0.join("sub")).unwrap();
    let rechecked = project.recheck_including(&[dir.0.join("sub")]);
    assert_eq!(not_found(&rechecked[&includer]), 1);

    let rechecked = project.recheck_including(&[dir.0.join("unrelated.inc")]);
    assert!(rechecked.is_empty(), "{rechecked:?}");
}

#[test]
fn a_name_leaves_the_assert_tagged_names_with_its_last_assert_tag() {
    let tagged =
        "<?php class A { /** @phpstan-assert int $x */ public function check($x): void {} }";
    let untagged = "<?php class A { public function check($x): void {} }";
    let mut project = Files::new().analysed("a.php", tagged).load();
    assert!(project.registry().names_with_assert_tags.contains("check"));
    project
        .update_file(&PathBuf::from("a.php"), untagged)
        .unwrap();
    assert!(!project.registry().names_with_assert_tags.contains("check"));
    project
        .update_file(&PathBuf::from("a.php"), tagged)
        .unwrap();
    assert!(project.registry().names_with_assert_tags.contains("check"));
}

#[test]
fn an_edit_keeps_the_use_sites_of_the_files_it_rechecks() {
    let mut project = Files::new()
        .settings(ProjectSettings {
            rules: crate::rules_settings::RulesSettings {
                check_dead_code: true,
                ..Default::default()
            },
            ..ProjectSettings::default()
        })
        .analysed("base.php", "<?php class Base {}")
        .analysed("user.php", "<?php new Base();")
        .load();
    let target = use_sites::RefTarget::Class("Base".to_string());
    assert_eq!(project.use_sites().references(&target).len(), 1);
    let rechecked = project
        .update_file(
            &PathBuf::from("base.php"),
            "<?php class Base { public int $x = 1; }",
        )
        .unwrap();
    assert!(rechecked.contains_key(&PathBuf::from("user.php")));
    assert_eq!(project.use_sites().references(&target).len(), 1);
}

/// What an edit takes from the whole-program checks' inputs goes with it:
/// an enum whose only escape an edit removes has dead cases again, and a
/// named-argument call an edit removes is no longer reported, as in a
/// fresh analysis of the files as they are.
#[test]
fn an_edit_takes_away_the_escapes_and_named_argument_calls_it_removes() {
    let declarations = "<?php enum Suit { case Hearts; case Spades; }
        class Base { public function m(int $old): void {} }
        class Sub extends Base { public function m(int $new): void {} }";
    let before = "<?php function f($name, Base $b) { $b->m(old: 1); return Suit::{$name}; }";
    let after = "<?php function f($name, Base $b) { $b->m(1); return $name; }";
    let analysed = |user: &str| -> Project {
        let mut project = Files::new()
            .settings(ProjectSettings {
                rules: crate::rules_settings::RulesSettings {
                    check_dead_enum_cases: true,
                    check_renamed_named_arguments: true,
                    ..Default::default()
                },
                ..ProjectSettings::default()
            })
            .analysed("declarations.php", declarations)
            .analysed("user.php", user)
            .load();
        let mut paths = project.analyzed_paths();
        paths.sort();
        for (path, outcome) in project.check_files(&paths, None, true, Records::default()) {
            project.keep_analysed_check(&path, &mut outcome.expect("checks"), true);
        }
        project.mark_member_index_built();
        project
    };
    let whole_program = |project: &mut Project| -> Vec<String> {
        let mut messages: Vec<String> = project
            .check_whole_program()
            .into_values()
            .flatten()
            .map(|d| d.message)
            .collect();
        messages.sort();
        messages
    };
    let mut edited = analysed(before);
    assert!(
        whole_program(&mut edited)
            .iter()
            .any(|message| message.contains("named argument")),
        "{:?}",
        whole_program(&mut edited)
    );
    edited.update_file(Path::new("user.php"), after).unwrap();
    let mut fresh = analysed(after);
    let expected = whole_program(&mut fresh);
    assert!(
        expected
            .iter()
            .any(|message| message.contains("Suit::Hearts")),
        "{expected:?}"
    );
    assert_eq!(whole_program(&mut edited), expected);
}
