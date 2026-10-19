//! The paths of the generated autoload files, which name `$vendorDir` and
//! `$baseDir` at their head: a project whose vendor directory is not `vendor/`
//! directly under it, or lies beside it, has its paths spelled from the
//! project directory. The files are what `composer dump-autoload` wrote on
//! throwaway projects, checked in beside this module.

use super::helpers::*;
use crate::canonical::canonicalize;
use crate::dir_listings::DirListings;
use crate::manifest::lexically_normalized;
use crate::vendor::*;
use std::path::{Path, PathBuf};

/// A layout's generated files: name and contents, as Composer wrote them.
type Files = &'static [(&'static str, &'static str)];

/// The files of a `vendor/` directly under the project.
const VENDOR_DEFAULT: Files = &[
    (
        "autoload_static.php",
        include_str!("data/generated/vendor/autoload_static.php"),
    ),
    (
        "autoload_classmap.php",
        include_str!("data/generated/vendor/autoload_classmap.php"),
    ),
    (
        "autoload_psr4.php",
        include_str!("data/generated/vendor/autoload_psr4.php"),
    ),
    (
        "autoload_namespaces.php",
        include_str!("data/generated/vendor/autoload_namespaces.php"),
    ),
];

/// The files of a `libs/vendor` inside the project.
const LIBS_VENDOR: Files = &[
    (
        "autoload_static.php",
        include_str!("data/generated/libs-vendor/autoload_static.php"),
    ),
    (
        "autoload_classmap.php",
        include_str!("data/generated/libs-vendor/autoload_classmap.php"),
    ),
    (
        "autoload_psr4.php",
        include_str!("data/generated/libs-vendor/autoload_psr4.php"),
    ),
    (
        "autoload_namespaces.php",
        include_str!("data/generated/libs-vendor/autoload_namespaces.php"),
    ),
];

/// The files of a vendor directory beside the project, named by its
/// `config.vendor-dir` of `../elsewhere-vendor`.
const ELSEWHERE: Files = &[
    (
        "autoload_static.php",
        include_str!("data/generated/elsewhere/autoload_static.php"),
    ),
    (
        "autoload_classmap.php",
        include_str!("data/generated/elsewhere/autoload_classmap.php"),
    ),
    (
        "autoload_psr4.php",
        include_str!("data/generated/elsewhere/autoload_psr4.php"),
    ),
    (
        "autoload_namespaces.php",
        include_str!("data/generated/elsewhere/autoload_namespaces.php"),
    ),
];

/// One layout: where its vendor directory and project lie, relative to a
/// root, and the files Composer wrote in the vendor directory.
struct Layout {
    vendor: &'static str,
    project: &'static str,
    files: Files,
}

const LAYOUTS: [Layout; 3] = [
    Layout {
        vendor: "project/vendor",
        project: "project",
        files: VENDOR_DEFAULT,
    },
    Layout {
        vendor: "project/libs/vendor",
        project: "project",
        files: LIBS_VENDOR,
    },
    Layout {
        vendor: "elsewhere-vendor",
        project: "elsewhere",
        files: ELSEWHERE,
    },
];

/// Keeps the entries of each generated file discovery reads, by its reading.
#[derive(Default)]
struct Recorded(std::sync::Mutex<Vec<(Reading, GeneratedEntries)>>);

impl GeneratedCache for Recorded {
    fn load(&self, _: Reading, _: &[u8]) -> Option<GeneratedEntries> {
        None
    }

    fn store(&self, reading: Reading, _: &[u8], entries: &GeneratedEntries) {
        self.0.lock().unwrap().push((reading, entries.clone()));
    }
}

/// Writes the files of `layout` that `keep` takes into its vendor directory
/// under `root`.
fn write_layout(root: &TempDir, layout: &Layout, keep: impl Fn(&str) -> bool) {
    for (name, contents) in layout.files {
        if keep(name) {
            root.write(&format!("{}/composer/{name}", layout.vendor), contents);
        }
    }
}

/// The entries of the generated files discovery reads from the vendor
/// directory of `layout` under `root`.
fn entries_read(root: &Path, layout: &Layout) -> Vec<(Reading, GeneratedEntries)> {
    let recorded = Recorded::default();
    discover_vendor_context(
        &root.join(layout.vendor),
        &DirListings::default(),
        Some(&recorded),
    );
    recorded.0.into_inner().unwrap()
}

/// The entries of the file read as `reading`.
fn entries_for(kept: &[(Reading, GeneratedEntries)], reading: Reading) -> &GeneratedEntries {
    kept.iter()
        .find(|(read, _)| *read == reading)
        .map(|(_, entries)| entries)
        .unwrap_or_else(|| panic!("no {reading:?} file was read"))
}

/// The directories discovery gives a generated file of `layout` under `root`:
/// the vendor directory the caller names, its parent as the base, and the
/// `composer/` directory the file lies in.
fn default_dirs(root: &Path, layout: &Layout) -> (PathBuf, PathBuf, PathBuf) {
    let vendor = root.join(layout.vendor);
    let base = vendor.parent().unwrap().to_path_buf();
    (vendor.join("composer"), vendor, base)
}

/// The entries with their paths folded and made relative to `root`, so that
/// two projects written in different temporary directories compare equal.
fn relative(root: &Path, entries: Vec<(String, Vec<PathBuf>)>) -> Vec<(String, Vec<PathBuf>)> {
    entries
        .into_iter()
        .map(|(key, paths)| {
            let paths = paths
                .iter()
                .map(|path| {
                    lexically_normalized(path)
                        .strip_prefix(root)
                        .expect("a path of the project lies under it")
                        .to_path_buf()
                })
                .collect();
            (key, paths)
        })
        .collect()
}

#[test]
fn the_separate_files_spell_their_paths_from_the_project_directory_their_heads_name() {
    for layout in &LAYOUTS {
        let dir = TempDir::new("generated_heads_separate");
        let root = canonicalize(dir.path()).unwrap();
        write_layout(&dir, layout, |name| name != "autoload_static.php");
        let kept = entries_read(&root, layout);
        let (composer, vendor, base) = default_dirs(&root, layout);
        let project = PathBuf::from(layout.project);
        let vendor_rel = PathBuf::from(layout.vendor);

        let classes = entries_for(&kept, Reading::Classmap).classes(&composer, &vendor, &base);
        assert_eq!(
            relative(&root, classes),
            vec![
                ("App\\Foo".to_owned(), vec![project.join("src/Foo.php")]),
                (
                    "App\\Lib\\Bar".to_owned(),
                    vec![project.join("src/Lib/Bar.php")]
                ),
                (
                    "Composer\\InstalledVersions".to_owned(),
                    vec![vendor_rel.join("composer/InstalledVersions.php")]
                ),
            ],
            "classmap of the layout at {}",
            layout.vendor
        );
        let psr4 = entries_for(&kept, Reading::Psr4).psr4(&composer, &vendor, &base);
        assert_eq!(
            relative(&root, psr4),
            vec![("App\\".to_owned(), vec![project.join("src")])],
            "PSR-4 of the layout at {}",
            layout.vendor
        );
    }
}

#[test]
fn the_static_file_and_the_separate_files_name_the_same_paths_under_each_layout() {
    for layout in &LAYOUTS {
        let separate_dir = TempDir::new("generated_heads_compare_separate");
        let separate_root = canonicalize(separate_dir.path()).unwrap();
        write_layout(&separate_dir, layout, |name| name != "autoload_static.php");
        let separate = entries_read(&separate_root, layout);
        let (composer, vendor, base) = default_dirs(&separate_root, layout);

        let static_dir = TempDir::new("generated_heads_compare_static");
        let static_root = canonicalize(static_dir.path()).unwrap();
        write_layout(&static_dir, layout, |name| name == "autoload_static.php");
        let statics = entries_read(&static_root, layout);
        let (static_composer, static_vendor, static_base) = default_dirs(&static_root, layout);

        let from_static = entries_for(&statics, Reading::StaticClass);
        let from_files = entries_for(&separate, Reading::Classmap);
        assert_eq!(
            relative(
                &static_root,
                from_static.classes(&static_composer, &static_vendor, &static_base)
            ),
            relative(
                &separate_root,
                from_files.classes(&composer, &vendor, &base)
            ),
            "classes of the layout at {}",
            layout.vendor
        );
        assert_eq!(
            relative(
                &static_root,
                from_static.psr4(&static_composer, &static_vendor, &static_base)
            ),
            relative(
                &separate_root,
                entries_for(&separate, Reading::Psr4).psr4(&composer, &vendor, &base)
            ),
            "PSR-4 of the layout at {}",
            layout.vendor
        );
    }
}

#[test]
fn an_absolute_head_names_the_path_it_spells() {
    let dir = TempDir::new("generated_heads_absolute");
    let root = canonicalize(dir.path()).unwrap();
    dir.write(
        "vendor/composer/autoload_classmap.php",
        r#"<?php

// autoload_classmap.php @generated by Composer

$vendorDir = '/abs/vendor';
$baseDir = '/abs/project';

return array(
    'App\\Foo' => $baseDir . '/src/Foo.php',
);
"#,
    );
    let recorded = Recorded::default();
    discover_vendor_context(
        &root.join("vendor"),
        &DirListings::default(),
        Some(&recorded),
    );
    let kept = recorded.0.into_inner().unwrap();
    let (composer, vendor, base) = (root.join("vendor/composer"), root.join("vendor"), root);
    assert_eq!(
        entries_for(&kept, Reading::Classmap).classes(&composer, &vendor, &base),
        vec![(
            "App\\Foo".to_owned(),
            vec![PathBuf::from("/abs/project/src/Foo.php")]
        )]
    );
}
