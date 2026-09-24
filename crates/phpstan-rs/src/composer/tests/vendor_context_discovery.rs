//! Discovering `vendor/` files and directories worth loading for
//! type-resolution context from a project's real, installed Composer
//! autoloader.

use super::super::*;
use super::helpers::*;

#[test]
fn no_composer_json_means_no_context() {
    let dir = TempDir::new("no_composer_json");
    assert_eq!(
        discover_vendor_context(dir.path()),
        ComposerContext::default()
    );
}

#[test]
fn composer_json_without_installed_vendor_means_no_context() {
    let dir = TempDir::new("no_vendor");
    dir.write("composer.json", "{}");
    assert_eq!(
        discover_vendor_context(dir.path()),
        ComposerContext::default()
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

    let ctx = discover_vendor_context(dir.path());
    let foo = std::fs::canonicalize(dir.path().join("vendor/acme/lib/Foo.php")).unwrap();
    let src_dir = std::fs::canonicalize(dir.path().join("vendor/acme/lib/src")).unwrap();
    assert_eq!(ctx.files, vec![foo]);
    assert_eq!(ctx.dirs, vec![src_dir]);
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

    let ctx = discover_vendor_context(dir.path());
    let class_loader =
        std::fs::canonicalize(dir.path().join("vendor/composer/ClassLoader.php")).unwrap();
    assert!(
        ctx.files.contains(&class_loader),
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

    let ctx = discover_vendor_context(dir.path());
    let foo = std::fs::canonicalize(dir.path().join("vendor/acme/lib/Foo.php")).unwrap();
    let acme_dir = std::fs::canonicalize(dir.path().join("vendor/acme/lib")).unwrap();

    // The vendor-rooted entries resolve.
    assert!(ctx.files.contains(&foo), "got: {:?}", ctx.files);
    assert!(ctx.dirs.contains(&acme_dir), "got: {:?}", ctx.dirs);

    // The `__DIR__ . '/../..'`-rooted (first-party, `app/`) entries
    // must be dropped by the vendor-only filter.
    let first_party_file = std::fs::canonicalize(dir.path().join("app/FirstParty.php")).unwrap();
    let first_party_dir = std::fs::canonicalize(dir.path().join("app")).unwrap();
    assert!(
        !ctx.files.contains(&first_party_file),
        "got: {:?}",
        ctx.files
    );
    assert!(!ctx.dirs.contains(&first_party_dir), "got: {:?}", ctx.dirs);
}

#[test]
fn dunder_dir_lexes_and_parses_as_a_bare_name() {
    // Sanity check this project's own parser treats `__DIR__` as an
    // ordinary bareword constant reference, same as any other bare
    // constant - the assumption `eval_concat` depends on entirely.
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

#[cfg(unix)]
#[test]
fn a_path_repository_symlink_out_of_vendor_is_still_vendor_context() {
    let dir = TempDir::new("path_repository");
    dir.write("composer.json", "{}");
    dir.write("packages/b/src/B.php", "<?php class B {}");
    std::fs::create_dir_all(dir.path().join("vendor/acme")).unwrap();
    std::os::unix::fs::symlink(
        dir.path().join("packages/b"),
        dir.path().join("vendor/acme/b"),
    )
    .unwrap();
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

    let ctx = discover_vendor_context(dir.path());
    assert_eq!(ctx.dirs.len(), 1, "{:?}", ctx.dirs);
    assert!(ctx.dirs[0].ends_with("vendor/acme/b/src"), "{:?}", ctx.dirs);
}
