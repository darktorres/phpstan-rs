//! A run that loads the vendor classes its code names reports what a run
//! that loads every vendor file reports.

use super::helpers::*;
use crate::app::config_schema;
use crate::project::session::{Overrides, Session};

/// A composer project: the app's one file, and a vendor package whose
/// `Pkg\A` extends `Pkg\B`, whose body makes a `Pkg\D`; `Pkg\Unused` and
/// `Pkg\Other` nothing names; and a `files` entry with a function.
fn project(dir: &TempDir) {
    dir.write("composer.json", "{}");
    dir.write(
        "vendor/pkg/src/A.php",
        "<?php namespace Pkg; class A extends B { public function foo(int $x): int { return $x; } }",
    );
    dir.write(
        "vendor/pkg/src/B.php",
        "<?php namespace Pkg; class B { public function make() { return new D(); } public function by() { return 'Pkg\\Hidden'; } }",
    );
    dir.write(
        "vendor/pkg/src/Hidden.php",
        "<?php namespace Pkg; class Hidden { public function h(): int { return 1; } }",
    );
    dir.write(
        "vendor/pkg/src/ByString.php",
        "<?php namespace Pkg; class ByString { public function s(): int { return 1; } }",
    );
    dir.write(
        "vendor/pkg/src/D.php",
        "<?php namespace Pkg; class D { public function dd(): string { return ''; } }",
    );
    dir.write(
        "vendor/pkg/src/Unused.php",
        "<?php namespace Pkg; class Unused { public function u(): int { return 1; } }",
    );
    dir.write(
        "vendor/pkg/src/Sub/Other.php",
        "<?php namespace Pkg\\Sub; class Other { public function o(): int { return 1; } }",
    );
    dir.write(
        "vendor/pkg/helpers.php",
        "<?php namespace Pkg; function helper(): int { return 1; }",
    );
    dir.write(
        "vendor/composer/autoload_static.php",
        r#"<?php
namespace Composer\Autoload;
class ComposerStaticInitTest
{
    public static $files = array (
        'abc' => __DIR__ . '/..' . '/pkg/helpers.php',
    );
    public static $prefixDirsPsr4 = array (
        'Pkg\\' => array (0 => __DIR__ . '/..' . '/pkg/src'),
    );
    public static $classMap = array (
        'Pkg\\A' => __DIR__ . '/..' . '/pkg/src/A.php',
        'Pkg\\B' => __DIR__ . '/..' . '/pkg/src/B.php',
        'Pkg\\D' => __DIR__ . '/..' . '/pkg/src/D.php',
        'Pkg\\Unused' => __DIR__ . '/..' . '/pkg/src/Unused.php',
    );
}
"#,
    );
    dir.write(
        "app/main.php",
        r#"<?php
use Pkg\A;
$a = new A();
$a->foo();
$a->make()->dd(1);
\Pkg\helper(1);
$o = new \Pkg\Sub\Other();
$o->o(1);
is_a($a, 'Pkg\\ByString');
"#,
    );
}

/// The diagnostics of the run and the number of context files it loaded.
fn run(dir: &TempDir, lazy: bool) -> (Vec<(String, u32, String)>, usize) {
    let config =
        config::ConfigFile::parse(&format!("lazy_vendor_context = {lazy}\n"), &config_schema())
            .unwrap();
    let overrides = Overrides {
        paths: vec!["app".into()],
        report_level: Some(u8::MAX),
        ..Overrides::default()
    };
    let session = Session::resolve(
        &config,
        None,
        dir.0.clone(),
        overrides,
        crate::project::worker_pool(),
    )
    .unwrap();
    let analysis = session.run(false).unwrap();
    let report = analysis.finalize(&session.finalize_options());
    let mut found: Vec<(String, u32, String)> = report
        .results
        .into_iter()
        .flat_map(|(path, diagnostics)| {
            diagnostics.into_iter().map(move |d| {
                (
                    path.display().to_string(),
                    d.span.start.line,
                    format!("{} {}", d.identifier, d.message),
                )
            })
        })
        .collect();
    found.sort();
    (found, analysis.context_loaded)
}

#[test]
fn loading_by_name_reports_what_loading_every_file_reports() {
    let dir = TempDir::new("lazy_vendor");
    project(&dir);
    let (eager, eager_loaded) = run(&dir, false);
    let (lazy, lazy_loaded) = run(&dir, true);
    assert!(
        eager.len() >= 4,
        "the fixture reports what it is there to compare: {eager:?}"
    );
    assert_eq!(lazy, eager);
    // Every vendor file but `Pkg\Unused`, which nothing names, and
    // `Pkg\Hidden`, which only the body of a vendor method holds as a
    // string. `Pkg\ByString` is named by a string of the analysed file, and
    // `Pkg\Sub\Other` is found under its PSR-4 directory.
    assert_eq!(
        eager_loaded, 8,
        "A, B, D, Unused, Other, helpers, Hidden, ByString"
    );
    assert_eq!(lazy_loaded, 6, "A, B, D, Other, helpers, ByString");
}
