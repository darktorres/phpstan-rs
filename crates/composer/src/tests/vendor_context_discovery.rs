//! Discovering `vendor/` files and directories worth loading for
//! type-resolution context from a project's real, installed Composer
//! autoloader.

use super::helpers::*;
use crate::canonical::canonicalize;
use crate::dir_listings::DirListings;
use crate::hashed_path::{HashedPath, Resolved};
use crate::test_links::{link_dir, link_file};
use crate::vendor::*;
use std::path::PathBuf;
use syntax::ast::*;
use syntax::lexer::Lexer;
use syntax::parser::Parser;

/// The paths of `listed`, resolved or not.
fn paths(listed: &[VendorPath]) -> Vec<PathBuf> {
    listed
        .iter()
        .map(|path| path.as_path().to_path_buf())
        .collect()
}

#[test]
fn no_composer_json_means_no_context() {
    let dir = TempDir::new("no_composer_json");
    assert_eq!(
        discover_vendor_context(&dir.path().join("vendor"), &DirListings::default(), None),
        VendorContext::default()
    );
}

#[test]
fn composer_json_without_installed_vendor_means_no_context() {
    let dir = TempDir::new("no_vendor");
    dir.write("composer.json", "{}");
    assert_eq!(
        discover_vendor_context(&dir.path().join("vendor"), &DirListings::default(), None),
        VendorContext::default()
    );
}

#[test]
fn resolves_classmap_and_psr4_from_the_separate_files() {
    let dir = TempDir::new("separate_files");
    dir.write("composer.json", "{}");
    dir.write("vendor/acme/lib/Foo.php", "<?php class Foo {}");
    dir.write("vendor/acme/lib/src/Bar.php", "<?php class Bar {}");
    dir.write(
        "vendor/composer/autoload_classmap.php",
        r#"<?php
$vendorDir = dirname(__DIR__);
$baseDir = dirname($vendorDir);
return array(
    'Acme\\Foo' => $vendorDir . '/acme/lib/Foo.php',
);
"#,
    );
    dir.write(
        "vendor/composer/autoload_psr4.php",
        r#"<?php
$vendorDir = dirname(__DIR__);
$baseDir = dirname($vendorDir);
return array(
    'Acme\\' => array($vendorDir . '/acme/lib/src'),
);
"#,
    );

    let ctx = discover_vendor_context(&dir.path().join("vendor"), &DirListings::default(), None);
    let foo = canonicalize(&dir.path().join("vendor/acme/lib/Foo.php")).unwrap();
    let src_dir = canonicalize(&dir.path().join("vendor/acme/lib/src")).unwrap();
    // Both are entries of listed directories that are not links, so the walk
    // takes them as they are.
    assert_eq!(
        ctx.files,
        vec![VendorPath::Resolved(Resolved::resolve(&foo))]
    );
    assert_eq!(
        ctx.dirs,
        vec![VendorPath::Resolved(Resolved::resolve(&src_dir))]
    );
}

// Real code that constructs Composer's own bootstrap loader directly
// (`new \Composer\Autoload\ClassLoader()`, a real plugin-autoloading
// idiom) must not be flagged as an unknown class -
// `vendor/composer/ClassLoader.php` is never listed in any of the
// generated autoload maps at all (it's what *runs* them), so it needs
// its own, separate, unconditional inclusion.
#[test]
fn always_includes_composers_own_class_loader_file() {
    let dir = TempDir::new("class_loader");
    dir.write("composer.json", "{}");
    dir.write(
        "vendor/composer/ClassLoader.php",
        "<?php namespace Composer\\Autoload; class ClassLoader {}",
    );
    dir.write(
        "vendor/composer/autoload_classmap.php",
        r#"<?php
return array();
"#,
    );

    let ctx = discover_vendor_context(&dir.path().join("vendor"), &DirListings::default(), None);
    let class_loader = canonicalize(&dir.path().join("vendor/composer/ClassLoader.php")).unwrap();
    assert!(
        ctx.files
            .contains(&VendorPath::Resolved(Resolved::resolve(&class_loader))),
        "expected ClassLoader.php in {:?}",
        ctx.files
    );
}

#[test]
fn resolves_the_real_autoload_static_shape_with_chained_dir_concat() {
    // Modeled on a real, installed `autoload_static.php` to exercise
    // the left-nested, multi-segment `__DIR__ . '/..' . '/...'` concat
    // shape real Composer generates, which a flat
    // `$vendorDir . '/...'`-only fixture would never catch.
    let dir = TempDir::new("static_shape");
    dir.write("composer.json", "{}");
    dir.write("vendor/acme/lib/Foo.php", "<?php class Foo {}");
    dir.write("app/FirstParty.php", "<?php class FirstParty {}");
    dir.write("vendor/acme/other/Other.php", "<?php class Other {}");
    dir.write(
        "vendor/composer/autoload_static.php",
        r#"<?php

namespace Composer\Autoload;

class ComposerStaticInitTest
{
    public static $files = array (
    );

    public static $prefixLengthsPsr4 = array (
    );

    public static $prefixDirsPsr4 = array (
        'Acme\\' =>
        array (
            0 => __DIR__ . '/..' . '/acme/lib',
        ),
        'App\\' =>
        array (
            0 => __DIR__ . '/../..' . '/app',
        ),
    );

    public static $classMap = array (
        'Acme\\Foo' => __DIR__ . '/..' . '/acme/lib/Foo.php',
        'Acme\\Other' => __DIR__ . '/..' . '/acme/other/Other.php',
        'App\\FirstParty' => __DIR__ . '/../..' . '/app/FirstParty.php',
    );

    public static function getInitializer(ClassLoader $loader)
    {
        return \Closure::bind(static function () use ($loader) {
        }, null, ClassLoader::class);
    }
}
"#,
    );

    let ctx = discover_vendor_context(&dir.path().join("vendor"), &DirListings::default(), None);
    let foo = canonicalize(&dir.path().join("vendor/acme/lib/Foo.php")).unwrap();
    let other = canonicalize(&dir.path().join("vendor/acme/other/Other.php")).unwrap();
    let acme_dir = canonicalize(&dir.path().join("vendor/acme/lib")).unwrap();

    // The vendor-rooted entries resolve: the PSR-4 directory, and a classmap
    // file beside it. The classmap file under the directory is left to the
    // directory's walk.
    assert!(paths(&ctx.dirs).contains(&acme_dir), "got: {:?}", ctx.dirs);
    assert!(paths(&ctx.files).contains(&other), "got: {:?}", ctx.files);
    assert!(!paths(&ctx.files).contains(&foo), "got: {:?}", ctx.files);

    // The `__DIR__ . '/../..'`-rooted (first-party, `app/`) entries
    // must be dropped by the vendor-only filter.
    let first_party_file = canonicalize(&dir.path().join("app/FirstParty.php")).unwrap();
    let first_party_dir = canonicalize(&dir.path().join("app")).unwrap();
    assert!(
        !paths(&ctx.files).contains(&first_party_file),
        "got: {:?}",
        ctx.files
    );
    assert!(
        !paths(&ctx.dirs).contains(&first_party_dir),
        "got: {:?}",
        ctx.dirs
    );
}

#[test]
fn dunder_dir_lexes_and_parses_as_a_bare_name() {
    // Sanity check this project's own parser treats `__DIR__` as an
    // ordinary bareword constant reference, same as any other bare
    // constant - the assumption the spelling of a generated file's paths
    // depends on entirely.
    let tokens = Lexer::new("<?php return __DIR__;").tokenize_with_spans();
    let program = Parser::new(tokens).parse_program().unwrap();
    assert_eq!(program.len(), 1);
    match &program[0].kind {
        StmtKind::Return(Some(e)) => {
            assert!(matches!(&e.kind, ExprKind::Name(n) if n == "__DIR__"));
        }
        other => panic!("expected a Return statement, got {other:?}"),
    }
}

#[test]
fn a_path_repository_symlink_out_of_vendor_is_still_vendor_context() {
    let dir = TempDir::new("path_repository");
    dir.write("composer.json", "{}");
    dir.write("packages/b/src/B.php", "<?php class B {}");
    std::fs::create_dir_all(dir.path().join("vendor/acme")).unwrap();
    link_dir(
        dir.path().join("packages/b"),
        dir.path().join("vendor/acme/b"),
    );
    dir.write(
        "vendor/composer/autoload_psr4.php",
        r#"<?php
$vendorDir = dirname(__DIR__);
$baseDir = dirname($vendorDir);
return array(
    'Acme\\B\\' => array($vendorDir . '/acme/b/src'),
    'App\\' => array($baseDir . '/src'),
);
"#,
    );
    dir.write("src/App.php", "<?php class App {}");

    let ctx = discover_vendor_context(&dir.path().join("vendor"), &DirListings::default(), None);
    assert_eq!(ctx.dirs.len(), 1, "{:?}", ctx.dirs);
    // Kept as written, so the walk resolves it.
    assert!(
        matches!(&ctx.dirs[0], VendorPath::Unresolved(dir) if dir.ends_with("vendor/acme/b/src")),
        "{:?}",
        ctx.dirs
    );
}

#[test]
fn a_php_file_under_a_psr4_directory_is_left_to_the_directory() {
    let dir = TempDir::new("left_to_directory");
    dir.write("composer.json", "{}");
    dir.write("vendor/acme/lib/Foo.php", "<?php class Foo {}");
    dir.write("vendor/acme/lib/src/Bar.php", "<?php class Bar {}");
    dir.write("vendor/acme/lib/src/helpers.inc", "<?php const HELPER = 1;");
    dir.write(
        "vendor/composer/autoload_classmap.php",
        r#"<?php
$vendorDir = dirname(__DIR__);
return array(
    'Acme\\Bar' => $vendorDir . '/acme/lib/src/Bar.php',
    'Acme\\Foo' => $vendorDir . '/acme/lib/Foo.php',
);
"#,
    );
    dir.write(
        "vendor/composer/autoload_files.php",
        r#"<?php
$vendorDir = dirname(__DIR__);
return array(
    $vendorDir . '/acme/lib/src/helpers.inc',
);
"#,
    );
    dir.write(
        "vendor/composer/autoload_psr4.php",
        r#"<?php
$vendorDir = dirname(__DIR__);
return array(
    'Acme\\' => array($vendorDir . '/acme/lib/src'),
);
"#,
    );

    let ctx = discover_vendor_context(&dir.path().join("vendor"), &DirListings::default(), None);
    let lib = canonicalize(&dir.path().join("vendor/acme/lib")).unwrap();
    // The directory loads `Bar.php`. `Foo.php` lies beside the directory, and
    // `helpers.inc` is not PHP, so the walk does not take it: both stay.
    assert_eq!(paths(&ctx.dirs), vec![lib.join("src")]);
    assert_eq!(ctx.files.len(), 2, "got: {:?}", ctx.files);
    assert!(
        paths(&ctx.files).contains(&lib.join("Foo.php")),
        "got: {:?}",
        ctx.files
    );
    assert!(
        paths(&ctx.files).contains(&lib.join("src/helpers.inc")),
        "got: {:?}",
        ctx.files
    );
}

#[cfg(unix)]
#[test]
fn a_php_file_its_directory_cannot_list_is_kept() {
    use std::os::unix::fs::PermissionsExt;

    let dir = TempDir::new("unlistable_dir");
    dir.write("composer.json", "{}");
    dir.write("vendor/acme/lib/src/Bar.php", "<?php class Bar {}");
    dir.write("vendor/acme/lib/src/hidden/Baz.php", "<?php class Baz {}");
    dir.write(
        "vendor/composer/autoload_classmap.php",
        r#"<?php
$vendorDir = dirname(__DIR__);
return array(
    'Acme\\Baz' => $vendorDir . '/acme/lib/src/hidden/Baz.php',
);
"#,
    );
    dir.write(
        "vendor/composer/autoload_psr4.php",
        r#"<?php
$vendorDir = dirname(__DIR__);
return array(
    'Acme\\' => array($vendorDir . '/acme/lib/src'),
);
"#,
    );
    // Passable but not readable: the walk of `src` cannot list `hidden`.
    let hidden = dir.path().join("vendor/acme/lib/src/hidden");
    std::fs::set_permissions(&hidden, std::fs::Permissions::from_mode(0o111)).unwrap();

    let ctx = discover_vendor_context(&dir.path().join("vendor"), &DirListings::default(), None);
    // A root reads any directory, and then the walk lists the file, so the
    // file is left out exactly when its directory can be read. The mode is
    // restored before any assertion can fail, so the directory is removed.
    let listable = std::fs::read_dir(&hidden).is_ok();
    std::fs::set_permissions(&hidden, std::fs::Permissions::from_mode(0o755)).unwrap();
    let baz = canonicalize(&dir.path().join("vendor/acme/lib/src"))
        .unwrap()
        .join("hidden/Baz.php");
    assert_eq!(
        paths(&ctx.files).contains(&baz),
        !listable,
        "got: {:?}",
        ctx.files
    );
}

// The discovery checks each file it leaves to a directory against the check
// made per file from the paths, so this fixture holds each case of the drop:
// the walk of the nearest directory lists a file only through directories
// that are not links, down to a file that is not one.
#[test]
fn a_file_is_left_to_a_directory_only_when_the_directorys_walk_lists_it() {
    let dir = TempDir::new("left_by_listing");
    let dir_link =
        |target: &str, link: &str| link_dir(dir.path().join(target), dir.path().join(link));
    let file_link =
        |target: &str, link: &str| link_file(dir.path().join(target), dir.path().join(link));
    dir.write("composer.json", "{}");
    dir.write("vendor/acme/a/src/A1.php", "<?php class A1 {}");
    dir.write("vendor/acme/a/src/deep/er/A2.php", "<?php class A2 {}");
    dir.write("vendor/acme/a/src/notes.txt", "");
    dir.write("vendor/acme/a/Beside.php", "<?php class Beside {}");
    dir.write("vendor/acme/n/O.php", "<?php class O {}");
    dir.write("vendor/acme/n/inner/sub/I.php", "<?php class I {}");
    dir.write("vendor/acme/lonely/L.php", "<?php class L {}");
    // Path-repository packages, linked into `vendor/` and kept as written.
    dir.write("packages/b/src/B.php", "<?php class B {}");
    dir.write("packages/b/Real.php", "<?php class Real {}");
    file_link("packages/b/Real.php", "packages/b/src/Link.php");
    dir.write("packages/c/C.php", "<?php class C {}");
    std::fs::create_dir_all(dir.path().join("vendor/acme/d/src")).unwrap();
    dir.write("packages/e/Z.php", "<?php class Z {}");
    dir_link("packages/b", "vendor/acme/b");
    dir_link("packages/c", "vendor/acme/c");
    dir_link("packages/e", "vendor/acme/d/src/lnk");
    dir.write(
        "vendor/composer/autoload_classmap.php",
        r#"<?php
$vendorDir = dirname(__DIR__);
return array(
    'A1' => $vendorDir . '/acme/a/src/A1.php',
    'A2' => $vendorDir . '/acme/a/src/deep/er/A2.php',
    'Beside' => $vendorDir . '/acme/a/Beside.php',
    'O' => $vendorDir . '/acme/n/O.php',
    'I' => $vendorDir . '/acme/n/inner/sub/I.php',
    'L' => $vendorDir . '/acme/lonely/L.php',
    'B' => $vendorDir . '/acme/b/src/B.php',
    'Link' => $vendorDir . '/acme/b/src/Link.php',
    'C' => $vendorDir . '/acme/c/C.php',
    'Z' => $vendorDir . '/acme/d/src/lnk/Z.php',
);
"#,
    );
    dir.write(
        "vendor/composer/autoload_files.php",
        r#"<?php
$vendorDir = dirname(__DIR__);
return array(
    $vendorDir . '/acme/a/src/notes.txt',
);
"#,
    );
    dir.write(
        "vendor/composer/autoload_psr4.php",
        r#"<?php
$vendorDir = dirname(__DIR__);
return array(
    'A\\' => array($vendorDir . '/acme/a/src'),
    'N\\' => array($vendorDir . '/acme/n', $vendorDir . '/acme/n/inner'),
    'B\\' => array($vendorDir . '/acme/b/src'),
    'C\\' => array($vendorDir . '/acme/c'),
    'D\\' => array($vendorDir . '/acme/d/src'),
);
"#,
    );

    let ctx = discover_vendor_context(&dir.path().join("vendor"), &DirListings::default(), None);
    let mut kept: Vec<&str> = ctx
        .files
        .iter()
        .map(|file| file.as_path().file_name().unwrap().to_str().unwrap())
        .collect();
    kept.sort_unstable();
    // `A1`, `A2`, `O` and `I` lie under directories the walks list, and so does
    // `B`, through the link that is its package's directory. `Link` is a link,
    // `C`'s directory is one, and `Z` lies behind one; `Beside` and `L` lie
    // under no directory, and `notes.txt` is not PHP.
    assert_eq!(
        kept,
        [
            "Beside.php",
            "C.php",
            "L.php",
            "Link.php",
            "Z.php",
            "notes.txt"
        ]
    );
    assert_eq!(ctx.dirs.len(), 6, "{:?}", ctx.dirs);
}

#[test]
fn an_entry_lies_in_a_base_as_its_directory_is_placed() {
    use std::ffi::OsStr;
    use std::path::Path;

    let dirs = [
        "/",
        "/a",
        "/a/vendor",
        "/a/vendor/x",
        "/a/vendorx",
        "/b",
        "/a/b/c",
        "a",
        "a/vendor",
        "",
    ];
    let bases = [
        "/a/vendor",
        "/vendor",
        "/",
        "/a/vendor/x",
        "a/vendor",
        "vendor",
    ];
    let names = ["vendor", "x", "vendorx", "a", "b"];
    for dir in dirs.map(Path::new) {
        for base in bases.map(Path::new) {
            for name in names.map(OsStr::new) {
                assert_eq!(
                    Place::of(dir, base).holds(name, base),
                    dir.join(name).starts_with(base),
                    "{} joined with {} against {}",
                    dir.display(),
                    name.display(),
                    base.display()
                );
            }
        }
    }
}

/// A cache in memory, which counts what it finds and what it keeps.
#[derive(Default)]
struct Kept {
    entries: std::sync::Mutex<Vec<(Reading, Vec<u8>, GeneratedEntries)>>,
    found: std::sync::atomic::AtomicUsize,
    stored: std::sync::atomic::AtomicUsize,
}

impl Kept {
    /// How many entries it found and how many it kept.
    fn counts(&self) -> (usize, usize) {
        use std::sync::atomic::Ordering::Relaxed;
        (self.found.load(Relaxed), self.stored.load(Relaxed))
    }
}

impl GeneratedCache for Kept {
    fn load(&self, reading: Reading, bytes: &[u8]) -> Option<GeneratedEntries> {
        let entries = self.entries.lock().unwrap();
        let found = entries
            .iter()
            .find(|(kept, kept_bytes, _)| *kept == reading && kept_bytes == bytes)
            .map(|(.., entries)| entries.clone());
        if found.is_some() {
            self.found
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        found
    }

    fn store(&self, reading: Reading, bytes: &[u8], entries: &GeneratedEntries) {
        self.stored
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.entries
            .lock()
            .unwrap()
            .push((reading, bytes.to_vec(), entries.clone()));
    }
}

const STATIC_FILE: &str = r#"<?php

namespace Composer\Autoload;

class ComposerStaticInitKept
{
    public static $files = array (
        'f0' => __DIR__ . '/..' . '/acme/lib/functions.php',
    );

    public static $prefixDirsPsr4 = array (
        'Acme\\' =>
        array (
            0 => __DIR__ . '/..' . '/acme/lib/src',
        ),
    );

    public static $classMap = array (
        'Acme\\Foo' => __DIR__ . '/..' . '/acme/lib/src/Foo.php',
        'Acme\\Other' => __DIR__ . '/..' . '/acme/other/Other.php',
    );
}
"#;

#[test]
fn a_generated_files_entries_are_kept_by_its_bytes_and_resolved_each_time() {
    let dir = TempDir::new("kept_entries");
    dir.write("composer.json", "{}");
    dir.write("vendor/acme/lib/functions.php", "<?php function f() {}");
    dir.write("vendor/acme/lib/src/Foo.php", "<?php class Foo {}");
    dir.write("vendor/acme/other/Other.php", "<?php class Other {}");
    dir.write("vendor/composer/autoload_static.php", STATIC_FILE);
    let discover = |cache: Option<&dyn GeneratedCache>| {
        discover_vendor_context(&dir.path().join("vendor"), &DirListings::default(), cache)
    };
    let uncached = discover(None);
    assert_eq!(paths(&uncached.files).len(), 2, "{:?}", uncached.files);
    assert_eq!(paths(&uncached.dirs).len(), 1, "{:?}", uncached.dirs);

    let kept = Kept::default();
    assert_eq!(discover(Some(&kept)), uncached);
    assert_eq!(
        kept.counts(),
        (0, 1),
        "the file is read and its entries kept"
    );
    assert_eq!(discover(Some(&kept)), uncached);
    assert_eq!(kept.counts(), (1, 1), "the kept entries are found");

    // The entries kept are what is resolved: the file is not read again.
    kept.entries.lock().unwrap()[0].2 = GeneratedEntries::default();
    let emptied = discover(Some(&kept));
    assert!(
        emptied.files.is_empty() && emptied.dirs.is_empty(),
        "{emptied:?}"
    );

    // The paths are resolved each time: a file that is gone is not listed.
    kept.entries.lock().unwrap().clear();
    let _ = discover(Some(&kept));
    std::fs::remove_file(dir.path().join("vendor/acme/other/Other.php")).unwrap();
    let found = discover(Some(&kept));
    assert_eq!(kept.counts(), (3, 2));
    assert_eq!(paths(&found.files).len(), 1, "{:?}", found.files);
    assert_eq!(found, discover(None));

    // A file of other bytes is read again.
    dir.write(
        "vendor/composer/autoload_static.php",
        &STATIC_FILE.replace("'f0' => __DIR__ . '/..' . '/acme/lib/functions.php',", ""),
    );
    let edited = discover(Some(&kept));
    assert_eq!(kept.counts(), (3, 3));
    assert!(edited.files.is_empty(), "{:?}", edited.files);
}

#[test]
fn the_separate_generated_files_are_kept_each_by_its_own_bytes() {
    let dir = TempDir::new("kept_separate");
    dir.write("composer.json", "{}");
    dir.write("vendor/acme/lib/src/Foo.php", "<?php class Foo {}");
    dir.write("vendor/acme/lib/Beside.php", "<?php class Beside {}");
    dir.write(
        "vendor/composer/autoload_classmap.php",
        r#"<?php
$vendorDir = dirname(__DIR__);
return array(
    'Beside' => $vendorDir . '/acme/lib/Beside.php',
);
"#,
    );
    dir.write(
        "vendor/composer/autoload_psr4.php",
        r#"<?php
$vendorDir = dirname(__DIR__);
return array(
    'Acme\\' => array($vendorDir . '/acme/lib/src'),
);
"#,
    );
    let uncached =
        discover_vendor_context(&dir.path().join("vendor"), &DirListings::default(), None);
    assert_eq!(paths(&uncached.files).len(), 1, "{:?}", uncached.files);
    assert_eq!(paths(&uncached.dirs).len(), 1, "{:?}", uncached.dirs);
    let kept = Kept::default();
    for counts in [(0, 2), (2, 2)] {
        let found = discover_vendor_context(
            &dir.path().join("vendor"),
            &DirListings::default(),
            Some(&kept),
        );
        assert_eq!(found, uncached);
        assert_eq!(kept.counts(), counts);
    }
    let readings: Vec<Reading> = kept
        .entries
        .lock()
        .unwrap()
        .iter()
        .map(|(reading, ..)| *reading)
        .collect();
    assert_eq!(readings, [Reading::Classmap, Reading::Psr4]);
}

/// A project whose classmap lists `entries`, each a path below
/// `vendor/acme/lib`, over a vendor directory that holds `lib/Foo.php`.
fn project_listing(label: &str, entries: &[&str]) -> TempDir {
    let dir = TempDir::new(label);
    dir.write("composer.json", "{}");
    dir.write("vendor/acme/lib/Foo.php", "<?php class Foo {}");
    let listed: String = entries
        .iter()
        .enumerate()
        .map(|(n, entry)| format!("    'Acme\\\\C{n}' => $vendorDir . '{entry}',\n"))
        .collect();
    dir.write(
        "vendor/composer/autoload_classmap.php",
        &format!("<?php\n$vendorDir = dirname(__DIR__);\nreturn array(\n{listed});\n"),
    );
    dir
}

/// Whether the project's directories fold case: the system finds
/// `vendor/acme/lib/Foo.php` under a spelling of its letters that the
/// listing of its directory does not hold.
fn folds_case(dir: &TempDir) -> bool {
    std::fs::symlink_metadata(dir.path().join("vendor/acme/lib/foo.php")).is_ok()
}

#[test]
fn a_file_the_listing_lacks_is_found_when_the_system_has_it() {
    let dir = project_listing(
        "listing_older_than_the_file",
        &[
            "/acme/lib/Foo.php",
            "/acme/lib/Late.php",
            "/acme/lib/Gone.php",
        ],
    );
    let lib = canonicalize(&dir.path().join("vendor/acme/lib")).unwrap();
    // The directory is listed before `Late.php` is written, so its listing
    // has `Foo.php` alone.
    let table = DirListings::default();
    let listing = table.listing(&HashedPath::new(lib.clone())).unwrap();
    assert_eq!(listing.entries.len(), 1);
    dir.write("vendor/acme/lib/Late.php", "<?php class Late {}");

    // `Late.php` is a miss of the listing that the system settles, and
    // `Gone.php` is a miss that it settles the other way.
    let ctx = discover_vendor_context(&dir.path().join("vendor"), &table, None);
    assert_eq!(
        paths(&ctx.files),
        [lib.join("Foo.php"), lib.join("Late.php")]
    );
}

#[test]
fn a_spelling_whose_case_differs_names_the_file_where_the_directory_folds_case() {
    let dir = project_listing("case_of_the_name", &["/acme/lib/foo.php"]);
    let lib = canonicalize(&dir.path().join("vendor/acme/lib")).unwrap();
    let ctx = discover_vendor_context(&dir.path().join("vendor"), &DirListings::default(), None);
    // The file is named in the spelling the disk has, where the disk takes
    // the other spelling for it.
    let expected: Vec<PathBuf> = if folds_case(&dir) {
        vec![lib.join("Foo.php")]
    } else {
        Vec::new()
    };
    assert_eq!(paths(&ctx.files), expected);
}

#[test]
fn a_directory_whose_case_differs_names_the_file_where_the_directory_folds_case() {
    let dir = project_listing("case_of_the_directory", &["/ACME/Lib/Foo.php"]);
    let lib = canonicalize(&dir.path().join("vendor/acme/lib")).unwrap();
    let ctx = discover_vendor_context(&dir.path().join("vendor"), &DirListings::default(), None);
    let folds = std::fs::symlink_metadata(dir.path().join("vendor/ACME/Lib/Foo.php")).is_ok();
    let expected: Vec<PathBuf> = if folds {
        vec![lib.join("Foo.php")]
    } else {
        Vec::new()
    };
    assert_eq!(paths(&ctx.files), expected);
}

#[test]
fn bytes_that_are_not_utf8_name_no_file_where_paths_are_text() {
    use std::path::Path;

    assert_eq!(
        path_of_text(b"vendor/acme/a.php"),
        Some(Path::new("vendor/acme/a.php"))
    );
    assert_eq!(
        path_of_text("vendor/acme/é.php".as_bytes()),
        Some(Path::new("vendor/acme/é.php"))
    );
    assert_eq!(path_of_text(b"vendor/acme/\xff.php"), None);
    assert_eq!(path_of_text(b"\xc3"), None);
}
