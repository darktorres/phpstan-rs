//! The per-file cache, end to end: a run that reads context files back from
//! it reports what a run that parsed them did, whether the entries are whole,
//! truncated, damaged or stale.

mod common;

use std::path::{Path, PathBuf};

use common::{TempDir, bin};

const CONFIG: &str = "paths = [\"src\"]\nscan_paths = [\"vendor\"]\n";

const BASE: &str = "<?php\nnamespace Lib;\n\nclass Base\n{\n    public function known(): int\n    {\n        return 1;\n    }\n}\n";

const USER: &str = "<?php\nnamespace App;\n\nuse Lib\\Base;\n\nfunction run(Base $b): void\n{\n    $b->known();\n    $b->unknown();\n}\n";

fn project() -> TempDir {
    let dir = TempDir::new("file_cache");
    dir.write("phpstan-rs.toml", CONFIG);
    dir.write("vendor/Base.php", BASE);
    dir.write("src/run.php", USER);
    dir
}

/// A run's output, with the cache in `cache`.
fn run(dir: &TempDir, cache: &Path) -> String {
    let output = bin()
        .current_dir(dir.path())
        .env("PHPSTAN_RS_CACHE_DIR", cache)
        .env_remove("PHPSTAN_RS_NO_FILE_CACHE")
        .env_remove("PHPSTAN_RS_NO_STUB_CACHE")
        .args(["check-project", "--config", "phpstan-rs.toml"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// The entries the per-file cache holds.
fn entries(cache: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let Ok(builds) = std::fs::read_dir(cache.join("files")) else {
        return found;
    };
    for build in builds.flatten() {
        for shard in std::fs::read_dir(build.path())
            .into_iter()
            .flatten()
            .flatten()
        {
            for entry in std::fs::read_dir(shard.path())
                .into_iter()
                .flatten()
                .flatten()
            {
                found.push(entry.path());
            }
        }
    }
    found
}

#[test]
fn a_warm_run_reports_what_a_cold_run_does() {
    let dir = project();
    let cache = TempDir::new("file_cache_dir");
    let cold = run(&dir, cache.path());
    assert!(
        cold.contains("unknown"),
        "the run finds the missing method: {cold}"
    );
    let stored = entries(cache.path());
    assert!(!stored.is_empty(), "the cold run stored entries");
    let written: Vec<_> = stored
        .iter()
        .map(|entry| std::fs::metadata(entry).unwrap().modified().unwrap())
        .collect();
    assert_eq!(run(&dir, cache.path()), cold);
    // The warm run read the entries and wrote none: the same files, as
    // they were.
    assert_eq!(entries(cache.path()).len(), stored.len());
    let rewritten: Vec<_> = stored
        .iter()
        .map(|entry| std::fs::metadata(entry).unwrap().modified().unwrap())
        .collect();
    assert_eq!(written, rewritten);
}

#[test]
fn a_truncated_or_damaged_entry_is_parsed_again() {
    let dir = project();
    let cache = TempDir::new("file_cache_damaged");
    let cold = run(&dir, cache.path());
    let stored = entries(cache.path());
    assert!(!stored.is_empty());
    for entry in &stored {
        let bytes = std::fs::read(entry).unwrap();
        std::fs::write(entry, &bytes[..bytes.len() / 2]).unwrap();
    }
    assert_eq!(run(&dir, cache.path()), cold, "truncated entries");
    for entry in &stored {
        let mut bytes = std::fs::read(entry).unwrap();
        let last = bytes.len() - 1;
        bytes[last] ^= 0xff;
        std::fs::write(entry, bytes).unwrap();
    }
    assert_eq!(run(&dir, cache.path()), cold, "damaged entries");
    // The entries were stored whole again.
    assert_eq!(run(&dir, cache.path()), cold);
}

#[test]
fn an_edited_context_file_is_not_read_from_its_old_entry() {
    let dir = project();
    let cache = TempDir::new("file_cache_edited");
    let before = run(&dir, cache.path());
    assert!(before.contains("unknown"));
    dir.write(
        "vendor/Base.php",
        &BASE.replace(
            "known(): int",
            "unknown(): int\n    {\n        return 2;\n    }\n\n    public function known(): int",
        ),
    );
    let after = run(&dir, cache.path());
    assert!(
        !after.contains("Call to an undefined method"),
        "got: {after}"
    );
    assert_ne!(before, after);
}

#[test]
fn the_cache_is_off_when_asked() {
    let dir = project();
    let cache = TempDir::new("file_cache_off");
    let output = bin()
        .current_dir(dir.path())
        .env("PHPSTAN_RS_CACHE_DIR", cache.path())
        .env("PHPSTAN_RS_NO_FILE_CACHE", "1")
        .args(["check-project", "--config", "phpstan-rs.toml"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(entries(cache.path()).is_empty());
}
