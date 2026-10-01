//! Sets `PHPSTAN_RS_SOURCE_HASH`: a hash of every workspace crate's
//! sources, build script and manifest, the lockfile and the vendored stub
//! sources - the
//! key the stub registry cache (`registry::stub_cache`) is stored under,
//! so a build whose code or stubs differ in any way never reads another's.

use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

fn collect_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_files(&path, out);
        } else {
            out.push(path);
        }
    }
}

fn main() {
    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let workspace = manifest_dir.join("../..");
    let mut inputs = vec![
        workspace.join("vendor/phpstorm-stubs"),
        workspace.join("vendor/phpstan-src/src"),
        workspace.join("Cargo.lock"),
        workspace.join("Cargo.toml"),
    ];
    let mut crates: Vec<PathBuf> = fs::read_dir(workspace.join("crates"))
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .collect();
    crates.sort();
    for krate in crates {
        for part in ["src", "Cargo.toml", "build.rs", "index.tsv"] {
            let path = krate.join(part);
            if path.exists() {
                inputs.push(path);
            }
        }
    }
    let mut files = Vec::new();
    for input in &inputs {
        println!("cargo::rerun-if-changed={}", input.display());
        if input.is_dir() {
            collect_files(input, &mut files);
        } else {
            files.push(input.clone());
        }
    }
    files.sort();
    let mut hasher = DefaultHasher::new();
    for path in &files {
        path.strip_prefix(&workspace)
            .unwrap_or(path)
            .hash(&mut hasher);
        if let Ok(bytes) = fs::read(path) {
            bytes.hash(&mut hasher);
        }
    }
    std::env::var("TARGET")
        .unwrap_or_default()
        .hash(&mut hasher);
    println!(
        "cargo::rustc-env=PHPSTAN_RS_SOURCE_HASH={:016x}",
        hasher.finish()
    );
}
