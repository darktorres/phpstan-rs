//! The batch merge of declarations (`Registry::merge_declarations`) leaves the
//! tables as the sequential inserts (`insert_built_decl`) leave them over the
//! same files in the same order. The checks cover each version case of
//! `version_cases`, with the changes recorded and not recorded, on 1, 2 and 8
//! threads, and as one batch and as two.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use rayon::ThreadPoolBuilder;
use types::{PhpVersion, PhpVersions, Type};

use super::super::*;
use super::helpers::parse;

/// The built-in stubs, indexed into every registry the tests make.
const STUBS: &str = r#"<?php
function strlen(string $s): int { return 0; }
class ArrayObject { public function count(): int { return 0; } }
"#;

/// The files of a load as `(path, analyzed, source)`: the context files first
/// and the analysed ones after, in the order `Session::load` inserts them.
const SOURCES: &[(&str, bool, &str)] = &[
    (
        "vendor/app/a.php",
        false,
        r#"<?php
namespace App;
class Foo { public function run(): int { return 1; } }
function helper(): int { return 1; }
const LIMIT = 10;
define('App\MODE', 'dev');
"#,
    ),
    (
        "vendor/app/b.php",
        false,
        r#"<?php
namespace App;
if (!class_exists('Foo')) {
    class Foo { public function fallback(): int { return 2; } }
}
if (!function_exists('helper')) {
    function helper(): int { return 2; }
}
const LIMIT = 20;
"#,
    ),
    (
        "vendor/app/f.php",
        false,
        r#"<?php
namespace app;
class FOO {}
const limit = 30;
function helper(): int { return 6; }
"#,
    ),
    (
        "vendor/polyfill.php",
        false,
        r#"<?php
if (!class_exists('ArrayObject')) {
    class ArrayObject { public function extra(): int { return 4; } }
}
"#,
    ),
    (
        "vendor/phpstan/d.php",
        false,
        r#"<?php
namespace PHPStan\Type;
class Helper { public function name(): string { return ''; } }
"#,
    ),
    (
        "src/app/c.php",
        true,
        r#"<?php
namespace App;
class Foo { public function run(): int { return 3; } }
function HELPER(): int { return 3; }
/** @phpstan-assert int $value */
function checked(mixed $value): void {}
class Guard {
    /** @phpstan-assert string $value */
    public function check(mixed $value): void {}
}
"#,
    ),
    (
        "src/app/e.php",
        true,
        r#"<?php
namespace App;
if (\PHP_VERSION_ID < 80000) {
    class Legacy {}
}
if (\PHP_VERSION_ID >= 80000) {
    function since80(): int { return 5; }
}
if (\PHP_VERSION_ID >= 80100) {
    class Modern {}
}
"#,
    ),
];

/// A file of the load, with the declarations it builds.
struct File {
    path: &'static str,
    analyzed: bool,
    decls: Vec<BuiltDecl>,
}

fn files() -> Vec<File> {
    SOURCES
        .iter()
        .map(|&(path, analyzed, text)| File {
            path,
            analyzed,
            decls: build_declarations(&parse(text), &Origin::Unknown),
        })
        .collect()
}

/// The PHP versions the declarations are checked on: none configured, and two
/// ranges, one that keeps `Legacy` as a maybe and one that rules it out.
fn version_cases() -> [Option<PhpVersions>; 3] {
    [None, Some(versions(7, 4)), Some(versions(8, 0))]
}

fn versions(major: u8, minor: u8) -> PhpVersions {
    PhpVersions::new(PhpVersion::new(major, minor), PhpVersion::new(8, 5))
}

/// The registry the files are inserted into, as a load starts with: the
/// built-in stubs indexed, and `ArrayObject` a built-in class. With
/// `recording`, the registry keeps its declarations as built, so the changes
/// are recorded (see `Registry::raw`).
fn start(versions: Option<PhpVersions>, recording: bool) -> Registry {
    let mut registry = Registry {
        php_versions: versions,
        ..Registry::default()
    };
    registry.index(&parse(STUBS), &Origin::Unknown, false);
    registry.built_in_stub_classes.insert("ArrayObject", ());
    if recording {
        registry.raw = Some(Box::new(RawDeclarations {
            classes: registry.classes.clone(),
            functions: registry.functions.clone(),
        }));
    }
    registry
}

/// The registry the files give when each declaration is inserted in turn.
fn serial(versions: Option<PhpVersions>, recording: bool, files: &[File]) -> Registry {
    let mut registry = start(versions, recording);
    registry.begin_declaration_changes();
    for file in files {
        for decl in &file.decls {
            if let BuiltDecl::Class { name, .. } = decl {
                let path = Path::new(file.path).to_path_buf();
                registry.record_phpstan_class_file(name, || path);
            }
            registry.insert_built_decl(decl.clone(), file.analyzed);
        }
    }
    registry
}

/// Merges `files` into `registry` as one batch on `pool`.
fn merge(registry: &mut Registry, pool: &rayon::ThreadPool, files: &[File]) {
    let declared: Vec<DeclaredFile<'_>> = files
        .iter()
        .map(|file| DeclaredFile {
            decls: &file.decls,
            analyzed: file.analyzed,
            path: Path::new(file.path),
        })
        .collect();
    registry.merge_declarations(pool, |merge| merge.insert(declared, None));
}

/// The registry the files give when they are merged as one batch on `pool`.
fn batch(
    versions: Option<PhpVersions>,
    recording: bool,
    pool: &rayon::ThreadPool,
    files: &[File],
) -> Registry {
    let mut registry = start(versions, recording);
    registry.begin_declaration_changes();
    merge(&mut registry, pool, files);
    registry
}

/// Every table the declarations write, keyed by name, so that two registries
/// compare by value.
#[derive(Debug, PartialEq)]
struct Tables<'a> {
    classes: BTreeMap<&'a str, &'a ClassReflection>,
    functions: BTreeMap<&'a str, &'a FunctionReflection>,
    global_constants: BTreeMap<&'a str, &'a Type>,
    unconditionally_declared_classes: BTreeSet<&'a str>,
    unconditionally_declared_functions: BTreeSet<&'a str>,
    stub_provided_functions: BTreeSet<&'a str>,
    function_decl_sites: BTreeMap<&'a str, &'a Option<u32>>,
    names_with_assert_tags: BTreeSet<&'a str>,
    phpstan_class_files: BTreeMap<&'a str, &'a Path>,
    /// The changed classes, functions and constants, with each constant's type
    /// before its first change.
    changed: (
        BTreeSet<&'a str>,
        BTreeSet<&'a str>,
        BTreeMap<&'a str, &'a Option<Type>>,
    ),
}

fn tables(registry: &Registry) -> Tables<'_> {
    Tables {
        classes: registry
            .classes
            .iter()
            .map(|(name, value)| (name.as_str(), value))
            .collect(),
        functions: registry
            .functions
            .iter()
            .map(|(name, value)| (name.as_str(), value))
            .collect(),
        global_constants: registry
            .global_constants
            .iter()
            .map(|(name, value)| (name.as_str(), value))
            .collect(),
        unconditionally_declared_classes: registry
            .unconditionally_declared_classes
            .iter()
            .map(Name::as_str)
            .collect(),
        unconditionally_declared_functions: registry
            .unconditionally_declared_functions
            .iter()
            .map(Name::as_str)
            .collect(),
        stub_provided_functions: registry
            .stub_provided_functions
            .iter()
            .map(Name::as_str)
            .collect(),
        function_decl_sites: registry
            .function_decl_sites
            .iter()
            .map(|(name, value)| (name.as_str(), value))
            .collect(),
        names_with_assert_tags: registry
            .names_with_assert_tags
            .iter()
            .map(Name::as_str)
            .collect(),
        phpstan_class_files: registry
            .phpstan_class_files
            .iter()
            .map(|(name, path)| (name.as_str(), path.as_path()))
            .collect(),
        changed: (
            registry
                .changed
                .classes
                .iter()
                .map(String::as_str)
                .collect(),
            registry
                .changed
                .functions
                .iter()
                .map(String::as_str)
                .collect(),
            registry
                .changed
                .constants
                .iter()
                .map(|(name, before)| (name.as_str(), before))
                .collect(),
        ),
    }
}

#[test]
fn the_batch_leaves_the_tables_as_the_sequential_inserts_do() {
    let files = files();
    let pools = [1, 2, 8].map(|threads| {
        ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .expect("a pool")
    });
    for versions in version_cases() {
        for recording in [false, true] {
            let sequential = serial(versions, recording, &files);
            for pool in &pools {
                let merged = batch(versions, recording, pool, &files);
                assert_eq!(
                    tables(&sequential),
                    tables(&merged),
                    "versions {versions:?}, recording {recording}, {} threads",
                    pool.current_num_threads()
                );
            }
        }
    }
}

#[test]
fn two_batches_leave_the_tables_as_one_sequential_pass_does() {
    let files = files();
    let (first, second) = files.split_at(files.len() / 2);
    for versions in version_cases() {
        for recording in [false, true] {
            let sequential = serial(versions, recording, &files);
            for threads in [1, 2, 8] {
                let pool = ThreadPoolBuilder::new()
                    .num_threads(threads)
                    .build()
                    .expect("a pool");
                let mut registry = start(versions, recording);
                registry.begin_declaration_changes();
                merge(&mut registry, &pool, first);
                merge(&mut registry, &pool, second);
                assert_eq!(
                    tables(&sequential),
                    tables(&registry),
                    "versions {versions:?}, recording {recording}, {threads} threads"
                );
            }
        }
    }
}

#[test]
fn the_declarations_under_test_take_effect() {
    let files = files();
    let none = serial(None, true, &files);
    let at_80 = serial(Some(versions(8, 0)), true, &files);

    // A guard the versions rule out keeps its declaration out; one they allow
    // keeps it in.
    assert!(none.classes.get("App\\Legacy").is_some());
    assert!(at_80.classes.get("App\\Legacy").is_none());
    assert!(at_80.classes.get("App\\Modern").is_some());
    assert!(at_80.functions.get("App\\since80").is_some());

    // The analysed `Foo` takes its name over the context files' declarations,
    // and the conditional fallback never takes effect.
    let foo = at_80.classes.get("App\\Foo").expect("App\\Foo is declared");
    assert!(foo.methods.get("run").is_some());
    assert!(foo.methods.get("fallback").is_none());

    assert!(
        at_80
            .phpstan_class_files
            .get("PHPStan\\Type\\Helper")
            .is_some()
    );
    assert!(at_80.changed.classes.contains("app\\foo"));
    assert!(serial(None, false, &files).changed.classes.is_empty());
}
