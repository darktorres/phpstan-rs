//! Generates `PHPSTORM_STUBS_SOURCES`, a `&[&str]` of every vendored
//! `jetbrains/phpstorm-stubs` file's contents, so `php_stubs.rs` doesn't
//! need 558 hand-written `include_str!` calls. Absolute paths are used in
//! the generated `include_str!` invocations because the generated file
//! lives under `OUT_DIR` (deep in `target/`), and `include_str!`'s path is
//! resolved relative to the file containing the macro call, not the
//! crate root - a relative path here would resolve against the wrong
//! directory once `include!`-ed into `php_stubs.rs`.

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

fn main() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let stubs_dir = Path::new(&manifest_dir).join("vendor/phpstorm-stubs");

    let mut files = Vec::new();
    collect_php_files(&stubs_dir, &mut files);

    let mut generated = String::new();
    generated.push_str("pub static PHPSTORM_STUBS_SOURCES: &[&str] = &[\n");
    for path in &files {
        generated.push_str(&format!("    include_str!({:?}),\n", path));
    }
    generated.push_str("];\n");

    let out_dir = std::env::var("OUT_DIR").unwrap();
    fs::write(
        Path::new(&out_dir).join("phpstorm_stubs_generated.rs"),
        generated,
    )
    .unwrap();

    println!("cargo:rerun-if-changed=vendor/phpstorm-stubs");
    println!("cargo:rerun-if-changed=build.rs");
}
