//! The batch of a load's project tables (`Project::insert_file_tables`) leaves
//! them as the serial inserts of each file leave them, in the order of the
//! files: the use sites, the duplicate declaration sites and the two reverse
//! indexes of references. Each batch runs on 1, 2 and 8 threads, as one batch
//! and as two. The use sites and the duplicate sites compare by value, and the
//! indexes by a sorted dump, so each key's spelling is compared too.

use std::collections::BTreeMap;

use rayon::ThreadPoolBuilder;

use super::super::*;

/// The files of a load, as `(path, analysed, source)`, the context files
/// first. Two of them declare `Lib\Base` and `Lib\helper`, one names `base` in
/// another case, and one is analysed, so the merge must keep each key's sites
/// and references in the order of the files.
const SOURCES: &[(&str, bool, &str)] = &[
    (
        "vendor/lib/base.php",
        false,
        r#"<?php
namespace Lib;
class Base { public function run(Base $other): Base { return $other; } }
function helper(int $n): int { return $n; }
"#,
    ),
    (
        "vendor/lib/other.php",
        false,
        r#"<?php
namespace Lib;
if (!function_exists('Lib\helper')) {
    function helper(int $n): int { return 0; }
}
class Base { public function run(Base $other): ?Base { return null; } }
function value(): Base { return new base(); }
"#,
    ),
    (
        "vendor/app/service.php",
        false,
        r#"<?php
namespace App;
use Lib\Base;
class Service extends Base {
    public function make(): Base { return new Base(); }
}
function run(): int { return \Lib\helper(2); }
"#,
    ),
    (
        "src/app/main.php",
        true,
        r#"<?php
namespace App;
class Base {}
function helper(): int { return 1; }
$service = new Service();
helper();
"#,
    ),
];

/// A project that indexes the use sites of its files, as a batch run with the
/// dead-code check on does.
fn project_with_use_sites() -> Project {
    let mut project = Project::new();
    project.apply_settings(ProjectSettings {
        rules: crate::rules_settings::RulesSettings {
            check_dead_code: true,
            ..Default::default()
        },
        ..ProjectSettings::default()
    });
    project
}

/// The files of `SOURCES`, parsed and indexed for their use sites, each with
/// its id. Each call numbers them the same way.
fn parsed_files() -> Vec<(FileId, PathBuf, ParsedFile)> {
    let mut project = project_with_use_sites();
    SOURCES
        .iter()
        .map(|&(path, analysed, source)| {
            let path = PathBuf::from(path);
            let id = project.files.id_for(&path);
            let mut parsed =
                parse_file_with_names(&path, None, source, false, BodyText::Parsed, None, analysed)
                    .expect("parses");
            project.index_use_sites(&mut parsed, source);
            (id, path, parsed)
        })
        .collect()
}

/// `files` as the entries one batch takes.
fn entries(
    files: &mut [(FileId, PathBuf, ParsedFile)],
) -> impl Iterator<Item = (FileId, &Path, &mut ParsedFile)> {
    files
        .iter_mut()
        .map(|(id, path, parsed)| (*id, path.as_path(), parsed))
}

/// The project the batch makes of `files`, merged as one batch on `pool`.
fn merged(pool: &rayon::ThreadPool, files: &mut [(FileId, PathBuf, ParsedFile)]) -> Project {
    let mut project = Project::new();
    project.insert_file_tables(pool, entries(files));
    project
}

/// The tables as the serial inserts of each file leave them, in the order of
/// the files.
fn serial(files: &[(FileId, PathBuf, ParsedFile)]) -> Project {
    let mut project = Project::new();
    for (id, path, parsed) in files {
        for (span, target) in parsed.class_fn_refs.clone().unwrap_or_default() {
            project.use_sites.insert(path, target, span);
        }
        project
            .duplicate_fn_sites
            .merge(parsed.duplicate_fn_sites.clone());
        project
            .duplicate_class_sites
            .merge(parsed.duplicate_class_sites.clone());
        register_references(&mut project.dependents, *id, &parsed.references);
        register_references(
            &mut project.signature_dependents,
            *id,
            &parsed.declaration_references,
        );
    }
    project
}

/// A reverse index as a sorted dump: each key with its spelling, and the files
/// that reference it.
fn dump(index: &FxHashMap<deps::RefKey, FxHashSet<FileId>>) -> BTreeMap<String, Vec<FileId>> {
    index
        .iter()
        .map(|(key, files)| {
            let mut files: Vec<FileId> = files.iter().copied().collect();
            files.sort();
            (format!("{key:?}"), files)
        })
        .collect()
}

/// Asserts that the tables of `actual` are those of `expected`.
fn assert_same_tables(expected: &Project, actual: &Project, label: &str) {
    assert_eq!(expected.use_sites, actual.use_sites, "use sites, {label}");
    assert_eq!(
        expected.duplicate_fn_sites, actual.duplicate_fn_sites,
        "duplicate functions, {label}"
    );
    assert_eq!(
        expected.duplicate_class_sites, actual.duplicate_class_sites,
        "duplicate classes, {label}"
    );
    assert_eq!(
        dump(&expected.dependents),
        dump(&actual.dependents),
        "dependents, {label}"
    );
    assert_eq!(
        dump(&expected.signature_dependents),
        dump(&actual.signature_dependents),
        "signature dependents, {label}"
    );
}

#[test]
fn the_batch_leaves_the_tables_as_the_serial_inserts_do() {
    let expected = serial(&parsed_files());
    for threads in [1, 2, 8] {
        let pool = ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .expect("a pool");
        let actual = merged(&pool, &mut parsed_files());
        assert_same_tables(&expected, &actual, &format!("{threads} threads"));
    }
}

#[test]
fn two_batches_leave_the_tables_as_one_batch_does() {
    let expected = serial(&parsed_files());
    for threads in [1, 2, 8] {
        let pool = ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .expect("a pool");
        let mut files = parsed_files();
        let half = files.len() / 2;
        let (first, second) = files.split_at_mut(half);
        let mut actual = Project::new();
        actual.insert_file_tables(&pool, entries(first));
        actual.insert_file_tables(&pool, entries(second));
        assert_same_tables(
            &expected,
            &actual,
            &format!("two batches, {threads} threads"),
        );
    }
}

#[test]
fn an_empty_batch_changes_nothing() {
    let pool = ThreadPoolBuilder::new()
        .num_threads(2)
        .build()
        .expect("a pool");
    let mut actual = merged(&pool, &mut parsed_files());
    actual.insert_file_tables(&pool, std::iter::empty());
    assert_same_tables(&serial(&parsed_files()), &actual, "after an empty batch");
}
