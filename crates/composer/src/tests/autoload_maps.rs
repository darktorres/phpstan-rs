//! The keys of the generated autoload maps: a class name of the classmap and
//! a namespace prefix of PSR-4 and PSR-0, each with the paths it names. They
//! are kept beside the flat lists discovery loads from, which do not change.

use super::helpers::*;
use crate::canonical::canonicalize;
use crate::dir_listings::DirListings;
use crate::vendor::*;
use std::path::PathBuf;

/// Keeps the entries of each generated file it is given, by its reading.
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

/// What discovery reads from the generated file of `dir`'s project that is
/// read as `reading` says.
fn read_as(dir: &TempDir, reading: Reading) -> GeneratedEntries {
    let recorded = Recorded::default();
    discover_vendor_context(
        &dir.path().join("vendor"),
        &DirListings::default(),
        Some(&recorded),
    );
    let kept = recorded.0.into_inner().unwrap();
    kept.into_iter()
        .find(|(kept, _)| *kept == reading)
        .map(|(_, entries)| entries)
        .unwrap_or_else(|| panic!("no {reading:?} file was read"))
}

/// The directories the generated files of `dir`'s project resolve their paths
/// from: `vendor/composer`, `vendor` and the project directory.
fn dirs_of(dir: &TempDir) -> (PathBuf, PathBuf, PathBuf) {
    let base = canonicalize(dir.path()).unwrap();
    let vendor = base.join("vendor");
    (vendor.join("composer"), vendor, base)
}

#[test]
fn the_classmap_keys_are_its_class_names_with_the_file_each_maps_to() {
    let dir = TempDir::new("autoload_maps_classmap");
    dir.write(
        "vendor/composer/autoload_classmap.php",
        r#"<?php
$vendorDir = dirname(__DIR__);
$baseDir = dirname($vendorDir);
return array(
    'Acme\\Foo' => $vendorDir . '/acme/src/Foo.php',
    'Root\\Own' => $baseDir . '/src/Own.php',
    'Acme\\Bar' => realpath($vendorDir . '/acme/src/Bar.php'),
    0 => $vendorDir . '/acme/src/Zero.php',
);
"#,
    );
    let (composer, vendor, base) = dirs_of(&dir);
    let classes = read_as(&dir, Reading::Classmap).classes(&composer, &vendor, &base);
    assert_eq!(
        classes,
        vec![
            (
                "Acme\\Foo".to_string(),
                vec![vendor.join("acme/src/Foo.php")]
            ),
            ("Root\\Own".to_string(), vec![base.join("src/Own.php")]),
            // A file the reading cannot spell keeps its class, with no path.
            ("Acme\\Bar".to_string(), Vec::new()),
        ]
    );
}

#[test]
fn the_psr4_keys_are_the_namespace_prefixes_with_their_directories() {
    let dir = TempDir::new("autoload_maps_psr4");
    dir.write(
        "vendor/composer/autoload_psr4.php",
        r#"<?php
$vendorDir = dirname(__DIR__);
$baseDir = dirname($vendorDir);
return array(
    'Acme\\' => array($vendorDir . '/acme/src', $baseDir . '/lib'),
    'Acme\\Ext\\' => array($vendorDir . '/acme/ext'),
);
"#,
    );
    let (composer, vendor, base) = dirs_of(&dir);
    let prefixes = read_as(&dir, Reading::Psr4).psr4(&composer, &vendor, &base);
    assert_eq!(
        prefixes,
        vec![
            (
                "Acme\\".to_string(),
                vec![vendor.join("acme/src"), base.join("lib")]
            ),
            ("Acme\\Ext\\".to_string(), vec![vendor.join("acme/ext")]),
        ]
    );
}

#[test]
fn the_psr0_keys_are_the_class_name_prefixes_with_their_directories() {
    let dir = TempDir::new("autoload_maps_psr0");
    dir.write(
        "vendor/composer/autoload_namespaces.php",
        r#"<?php
$vendorDir = dirname(__DIR__);
$baseDir = dirname($vendorDir);
return array(
    'Vendor_Pkg_' => array($vendorDir . '/pkg/src'),
    'Acme\\Sub_' => array($baseDir . '/lib', $vendorDir . '/acme/sub'),
);
"#,
    );
    let (composer, vendor, base) = dirs_of(&dir);
    let prefixes = read_as(&dir, Reading::Psr0).psr0(&composer, &vendor, &base);
    assert_eq!(
        prefixes,
        vec![
            ("Vendor_Pkg_".to_string(), vec![vendor.join("pkg/src")]),
            (
                "Acme\\Sub_".to_string(),
                vec![base.join("lib"), vendor.join("acme/sub")]
            ),
        ]
    );
}

#[test]
fn the_static_file_keys_its_classes_its_psr4_prefixes_and_its_psr0_prefixes() {
    let dir = TempDir::new("autoload_maps_static");
    dir.write(
        "vendor/composer/autoload_static.php",
        r#"<?php
namespace Composer\Autoload;

class ComposerStaticInitMaps
{
    public static $classMap = array (
        'Acme\\Foo' => __DIR__ . '/..' . '/acme/src/Foo.php',
    );

    public static $prefixDirsPsr4 = array (
        'Acme\\' => array (
            0 => __DIR__ . '/..' . '/acme/src',
        ),
    );

    public static $prefixesPsr0 = array (
        'V' => array (
            'Vendor_' => array (
                0 => __DIR__ . '/..' . '/pkg/src',
            ),
        ),
    );
}
"#,
    );
    let (composer, vendor, base) = dirs_of(&dir);
    let entries = read_as(&dir, Reading::StaticClass);
    assert_eq!(
        entries.classes(&composer, &vendor, &base),
        vec![(
            "Acme\\Foo".to_string(),
            vec![composer.join("../acme/src/Foo.php")]
        )]
    );
    assert_eq!(
        entries.psr4(&composer, &vendor, &base),
        vec![("Acme\\".to_string(), vec![composer.join("../acme/src")])]
    );
    assert_eq!(
        entries.psr0(&composer, &vendor, &base),
        vec![("Vendor_".to_string(), vec![composer.join("../pkg/src")])]
    );
}

#[test]
fn a_psr0_prefix_of_the_static_file_does_not_add_a_directory_to_load() {
    let dir = TempDir::new("autoload_maps_flat");
    dir.write("vendor/acme/src/Foo.php", "<?php class Foo {}");
    dir.write(
        "vendor/pkg/src/Vendor/Pkg/Thing.php",
        "<?php class Thing {}",
    );
    dir.write(
        "vendor/composer/autoload_static.php",
        r#"<?php
namespace Composer\Autoload;

class ComposerStaticInitFlat
{
    public static $prefixDirsPsr4 = array (
        'Acme\\' => array (
            0 => __DIR__ . '/..' . '/acme/src',
        ),
    );

    public static $prefixesPsr0 = array (
        'V' => array (
            'Vendor_' => array (
                0 => __DIR__ . '/..' . '/pkg/src',
            ),
        ),
    );
}
"#,
    );
    let found = discover_vendor_context(&dir.path().join("vendor"), &DirListings::default(), None);
    let dirs: Vec<PathBuf> = found
        .dirs
        .iter()
        .map(|listed| listed.as_path().to_path_buf())
        .collect();
    assert!(
        dirs.iter().any(|path| path.ends_with("acme/src")),
        "{dirs:?}"
    );
    assert!(
        !dirs.iter().any(|path| path.ends_with("pkg/src")),
        "{dirs:?}"
    );
}
