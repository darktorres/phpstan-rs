//! `include`/`require` paths that don't name an existing file.

use super::helpers::*;

struct Fixture {
    dir: std::path::PathBuf,
}

impl Fixture {
    fn new(name: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("phpstan-rs-includes-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("lib")).unwrap();
        std::fs::write(dir.join("lib/present.php"), "<?php\n").unwrap();
        std::fs::write(dir.join("sibling.php"), "<?php\n").unwrap();
        Fixture { dir }
    }

    fn check(&self, src: &str) -> Vec<String> {
        let mut out: Vec<String> = diagnostics_for_file(src, &self.dir.join("main.php"))
            .into_iter()
            .filter(|d| d.identifier.ends_with(".fileNotFound"))
            .map(|d| {
                format!(
                    "{}: {}",
                    d.identifier,
                    d.message
                        .replace(&*self.dir.to_string_lossy(), "<dir>")
                        .replace(&*std::env::temp_dir().to_string_lossy(), "<tmp>")
                )
            })
            .collect();
        out.sort();
        out
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[test]
fn existing_paths_are_accepted() {
    let f = Fixture::new("existing");
    let d = f.check(
        r#"<?php
require __DIR__ . '/lib/present.php';
require_once __DIR__ . '/sibling.php';
include 'sibling.php';
include_once dirname(__FILE__) . '/lib/present.php';
require dirname(__DIR__ . '/lib/x') . '/present.php';
"#,
    );
    assert!(d.is_empty(), "{d:?}");
}

#[test]
fn missing_paths_are_reported_per_construct() {
    let f = Fixture::new("missing");
    let d = f.check(
        r#"<?php
require __DIR__ . '/lib/absent.php';
require_once __DIR__ . '/nope.php';
include 'missing.php';
include_once dirname(__DIR__, 1) . '/elsewhere.php';
"#,
    );
    assert_eq!(
        d,
        vec![
            "include.fileNotFound: Path in include() \"missing.php\" is not a file or it does not exist.",
            "includeOnce.fileNotFound: Path in include_once() \"<tmp>/elsewhere.php\" is not a file or it does not exist.",
            "require.fileNotFound: Path in require() \"<dir>/lib/absent.php\" is not a file or it does not exist.",
            "requireOnce.fileNotFound: Path in require_once() \"<dir>/nope.php\" is not a file or it does not exist.",
        ]
    );
}

#[test]
fn directories_are_not_files() {
    let f = Fixture::new("directory");
    let d = f.check("<?php\nrequire __DIR__ . '/lib';\n");
    assert_eq!(
        d,
        vec![
            "require.fileNotFound: Path in require() \"<dir>/lib\" is not a file or it does not exist."
        ]
    );
}

#[test]
fn every_member_of_a_constant_union_is_checked() {
    let f = Fixture::new("union");
    let d = f.check(
        r#"<?php
function load(bool $b): void {
    $file = $b ? 'present.php' : 'gone.php';
    require __DIR__ . '/lib/' . $file;
}
"#,
    );
    assert_eq!(
        d,
        vec![
            "require.fileNotFound: Path in require() \"<dir>/lib/gone.php\" is not a file or it does not exist."
        ]
    );
}

#[test]
fn constants_resolve_to_their_values() {
    let f = Fixture::new("constants");
    let d = f.check(
        r#"<?php
const LIB = __DIR__ . '/lib';
class Paths { const FILE = 'absent.php'; }
require LIB . '/present.php';
require LIB . '/' . Paths::FILE;
"#,
    );
    assert_eq!(
        d,
        vec![
            "require.fileNotFound: Path in require() \"<dir>/lib/absent.php\" is not a file or it does not exist."
        ]
    );
}

#[test]
fn define_and_self_constants_resolve_through_their_initializers() {
    let f = Fixture::new("define");
    let d = f.check(
        r#"<?php
define('ROOT', dirname(__FILE__));
class Loader {
    const LIB = ROOT . '/lib/';
    public function load(): void {
        require self::LIB . 'present.php';
        require static::LIB . 'absent.php';
    }
}
"#,
    );
    assert_eq!(
        d,
        vec![
            "require.fileNotFound: Path in require() \"<dir>/lib/absent.php\" is not a file or it does not exist."
        ]
    );
}

#[test]
fn a_file_redefining_a_magic_constant_does_not_shadow_it() {
    let f = Fixture::new("magic");
    let d = f.check("<?php\ndefine('__DIR__', '/nowhere');\nrequire __DIR__ . '/sibling.php';\n");
    assert!(d.is_empty(), "{d:?}");
}

#[test]
fn unresolvable_paths_are_skipped() {
    let f = Fixture::new("unresolvable");
    let d = f.check(
        r#"<?php
function load(string $name): void {
    require __DIR__ . '/' . $name . '.php';
    include getenv('HOME') . '/x.php';
}
"#,
    );
    assert!(d.is_empty(), "{d:?}");
}

#[test]
fn paths_the_file_checks_for_are_skipped() {
    let f = Fixture::new("guarded");
    let d = f.check(
        r#"<?php
if (file_exists(__DIR__ . '/optional.php')) {
    require __DIR__ . '/optional.php';
}
function f(): void {
    if (is_readable(__DIR__ . '/config.local.php')) {
        include __DIR__ . '/config.local.php';
    }
}
$cb = function () {
    if (is_file('local.php')) {
        include 'local.php';
    }
};
"#,
    );
    assert!(d.is_empty(), "{d:?}");
}

#[test]
fn keyword_casing_does_not_matter() {
    let f = Fixture::new("casing");
    let d = f.check("<?php\nREQUIRE_ONCE __DIR__ . '/nope.php';\n");
    assert_eq!(d.len(), 1, "{d:?}");
}
