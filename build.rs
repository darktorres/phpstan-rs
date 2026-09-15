//! Generates `PHPSTORM_STUBS_SOURCES` (every vendored
//! `jetbrains/phpstorm-stubs` file's contents) and `PHPSTAN_SRC_SOURCES`
//! (every vendored `phpstan/phpstan-src` runtime source file's contents -
//! PHPStan classes like `PHPStan\Type\StringType`, referenced by any
//! vendor package authoring a PHPStan rule/extension, that
//! `phpstan/phpstan`'s own phar distribution can't provide since it
//! ships no loose source at all), each a `&[&str]` so `php_stubs.rs`
//! doesn't need hundreds of hand-written `include_str!` calls. Absolute
//! paths are used in the generated `include_str!` invocations because the
//! generated file lives under `OUT_DIR` (deep in `target/`), and
//! `include_str!`'s path is resolved relative to the file containing the
//! macro call, not the crate root - a relative path here would resolve
//! against the wrong directory once `include!`-ed into `php_stubs.rs`.

use std::fs;
use std::path::{Path, PathBuf};

fn collect_php_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(|e| e.path());
    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            collect_php_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "php") {
            out.push(path);
        }
    }
}

fn write_source_array(out: &mut String, name: &str, files: &[PathBuf]) {
    out.push_str(&format!("pub static {name}: &[&str] = &[\n"));
    for path in files {
        out.push_str(&format!("    include_str!({:?}),\n", path));
    }
    out.push_str("];\n");
}

fn main() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();

    let mut phpstorm_files = Vec::new();
    collect_php_files(
        &Path::new(&manifest_dir).join("vendor/phpstorm-stubs"),
        &mut phpstorm_files,
    );

    // Deliberately scoped to `src/` only, not the whole `vendor/
    // phpstan-src` root - mirrors phpstorm-stubs' own curation (real
    // runtime source only), excluding `tests/`, `apigen/`, `compiler/`,
    // `e2e/`, `stubs/` (PHPStan's *own* stubs for third-party libraries
    // it itself analyzes - a different meaning of "stubs" than this
    // pipeline's own), `conf/`, `bin/`, `resources/`, `patches/`,
    // `turbo-ext/` - none of which declare the PHPStan\Type\*/
    // PHPStan\Rules\Rule-style classes actually referenced in the wild.
    let mut phpstan_src_files = Vec::new();
    collect_php_files(
        &Path::new(&manifest_dir).join("vendor/phpstan-src/src"),
        &mut phpstan_src_files,
    );

    // `vendor/phpstan-src` is a git submodule (see `.gitmodules`) - unlike
    // a plain vendored directory, `git clone`/`git worktree add` don't
    // populate it on their own, so it silently sits empty until someone
    // remembers `git submodule update --init`. Without this check, that
    // produces a build that compiles and "succeeds" with an empty
    // PHPSTAN_SRC_SOURCES - no error anywhere, just a stub-class pipeline
    // quietly missing ~1800 files' worth of real PHPStan classes (found
    // the hard way: chasing a phantom diagnostic difference between two
    // otherwise-identical builds that turned out to be exactly this).
    // Failing loudly here, rather than trying to run `git submodule
    // update --init` automatically from inside the build script, is the
    // safer choice - a build script invoking git can break in a
    // downloaded-tarball, offline, or sandboxed-CI context that has no
    // reason to expect it.
    assert!(
        !phpstan_src_files.is_empty(),
        "vendor/phpstan-src/src has no .php files - the git submodule          looks uninitialized. Run: git submodule update --init"
    );
    assert!(
        !phpstorm_files.is_empty(),
        "vendor/phpstorm-stubs has no .php files - expected a fully          checked-out repo, not a shallow or partial clone"
    );

    let mut generated = String::new();
    write_source_array(&mut generated, "PHPSTORM_STUBS_SOURCES", &phpstorm_files);
    write_source_array(&mut generated, "PHPSTAN_SRC_SOURCES", &phpstan_src_files);

    let out_dir = std::env::var("OUT_DIR").unwrap();
    fs::write(
        Path::new(&out_dir).join("phpstorm_stubs_generated.rs"),
        generated,
    )
    .unwrap();

    println!("cargo:rerun-if-changed=vendor/phpstorm-stubs");
    println!("cargo:rerun-if-changed=vendor/phpstan-src/src");
    println!("cargo:rerun-if-changed=build.rs");
}
