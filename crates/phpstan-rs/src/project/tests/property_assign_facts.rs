//! Judging private property types against the writes of every file that
//! makes them. The `property.unusedType` check itself is covered by the
//! fixtures under `tests/fixtures/project/property_assign_facts`.

use super::super::*;

fn load(files: &[(&str, &str)]) -> Project {
    let mut project = Project::new();
    for (path, source) in files {
        project.load_file(PathBuf::from(path), source).unwrap();
    }
    project.finish_loading();
    project
}

/// The `property.unusedType` messages among `diagnostics`.
fn messages(diagnostics: &[Diagnostic]) -> Vec<&str> {
    diagnostics
        .iter()
        .filter(|d| *d.identifier == *"property.unusedType")
        .map(|d| d.message.as_str())
        .collect()
}
const TRAIT_WRITING_AN_INT: &str = r#"<?php
    trait T {
        public function set(): void {
            $this->x = 1;
        }
    }
"#;

const HOST_OF_T: &str = r#"<?php
    class Foo {
        use T;
        /** @var int|string|null */
        private $x;
        public function clear(): void {
            $this->x = null;
        }
    }
"#;

#[test]
fn a_trait_file_left_out_of_the_checked_paths_still_contributes_its_writes() {
    let project = load(&[("trait.php", TRAIT_WRITING_AN_INT), ("host.php", HOST_OF_T)]);
    let host = PathBuf::from("host.php");
    let results = project.check_paths(std::slice::from_ref(&host), &worker_pool());
    assert_eq!(
        messages(&results[&host]),
        ["Foo::$x is never assigned string so it can be removed from the property type."]
    );
}

#[test]
fn a_single_file_check_reads_the_writes_of_its_classes_traits_in_other_files() {
    let project = load(&[("trait.php", TRAIT_WRITING_AN_INT), ("host.php", HOST_OF_T)]);
    assert_eq!(
        messages(&project.check_file(Path::new("host.php"))),
        ["Foo::$x is never assigned string so it can be removed from the property type."]
    );
}

#[test]
fn a_property_a_trait_declares_is_judged_against_every_hosts_writes() {
    let project = load(&[
        (
            "trait.php",
            r#"<?php
                trait T {
                    /** @var int|string|bool */
                    private $x;
                }
            "#,
        ),
        (
            "a.php",
            r#"<?php
                class A {
                    use T;
                    public function set(): void { $this->x = 1; }
                }
            "#,
        ),
        (
            "b.php",
            r#"<?php
                class B {
                    use T;
                    public function set(): void { $this->x = "b"; }
                }
            "#,
        ),
    ]);
    let path = PathBuf::from("trait.php");
    let results = project.check_paths(std::slice::from_ref(&path), &worker_pool());
    assert_eq!(
        messages(&results[&path]),
        ["T::$x is never assigned bool so it can be removed from the property type."]
    );
}

#[test]
fn a_write_the_scope_may_not_make_assigns_nothing() {
    let project = load(&[(
        "a.php",
        r#"<?php
            class A {
                /** @var int|string */
                private $x;
                public function set(): void { $this->x = 1; }
            }
            function outside(A $a): void {
                $a->x = "a";
            }
        "#,
    )]);
    let found = project.check_file(Path::new("a.php"));
    assert!(
        messages(&found).contains(
            &"A::$x is never assigned string so it can be removed from the property type."
        ),
        "{found:?}"
    );
}

#[test]
fn a_standalone_check_judges_a_host_declared_before_its_trait() {
    let project = load(&[(
        "a.php",
        r#"<?php
            class Foo {
                use T;
                /** @var int|string */
                private $x;
            }
            trait T {
                public function set(): void {
                    $this->x = 1;
                    $this->x = "a";
                }
            }
        "#,
    )]);
    let file = project.files.get(Path::new("a.php")).unwrap();
    let mut checker = Checker::new(&project.registry).with_settings(&project.settings);
    checker.check_program(file.ast());
    let found = checker.into_diagnostics();
    assert!(
        !found
            .iter()
            .any(|d| *d.identifier == *"property.unusedType"),
        "{found:?}"
    );
}

#[test]
fn a_file_whose_check_panics_costs_only_its_own_results() {
    let panicking = PathBuf::from("facts_panic_injected_a.php");
    let survivor = PathBuf::from("facts_panic_survivor_b.php");
    let project = load(&[
        (
            "facts_panic_injected_a.php",
            r#"<?php
                class A {
                    /** @var int|string */
                    private $x;
                    public function set(): void { $this->x = 1; }
                }
            "#,
        ),
        (
            "facts_panic_survivor_b.php",
            r#"<?php
                class B {
                    /** @var int|string */
                    private $y;
                    public function set(): void { $this->y = 1; }
                }
            "#,
        ),
    ]);
    super::panic_injection::inject_panic(&panicking);

    let (results, panics) = project
        .check_paths_isolating_panics(&[panicking.clone(), survivor.clone()], &worker_pool());
    assert_eq!(panics.len(), 1, "{panics:?}");
    assert_eq!(panics[0].0, panicking);
    assert_eq!(
        messages(&results[&survivor]),
        ["B::$y is never assigned string so it can be removed from the property type."]
    );
}
