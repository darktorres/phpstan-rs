use std::path::{Path, PathBuf};

use config::ConfigFile;
use phpstan_rs::project::session::{Overrides, Policy, Session};
use phpstan_rs::{output, project};

/// `phpstan-rs incremental-bench <dir> [<file-to-touch>]` - loads every
/// .php file under `dir` into a `Project` through a `Session` (a full
/// load, timed), then
/// re-parses and re-checks just one file via `Project::update_file` (the
/// incremental path, also timed) and reports both, to make concrete that
/// an edit to one file costs O(that file), not O(the project). Picks the
/// first file in directory-walk order when none is named explicitly.
pub(crate) fn run_incremental_bench(dir: &Path, file_to_touch: Option<&Path>) {
    let root = std::env::current_dir().expect("the working directory exists");
    let session = match Session::resolve(
        &ConfigFile::empty(),
        None,
        root,
        Overrides {
            paths: vec![dir.to_path_buf()],
            no_composer: true,
            ..Overrides::default()
        },
        Policy::Fail,
        project::worker_pool(),
    ) {
        Ok(session) => session.for_editing(),
        Err(e) => {
            eprintln!("{e}");
            return;
        }
    };
    let (files, _) = session.listing();
    if files.is_empty() {
        eprintln!("no .php files found under {}", dir.display());
        return;
    }

    let target = match file_to_touch {
        Some(f) => f.to_path_buf(),
        None => files[0].clone(),
    };
    if !files.contains(&target) {
        eprintln!(
            "{} is not among the files under {}",
            target.display(),
            dir.display()
        );
        return;
    }

    let full_load_start = std::time::Instant::now();
    let mut project = match session.load() {
        Ok(loaded) => loaded.project,
        Err(e) => {
            eprintln!("{e}");
            return;
        }
    };
    let full_load = full_load_start.elapsed();

    let target_source = String::from_utf8_lossy(
        &session
            .read(&target)
            .unwrap_or_else(|e| panic!("failed to read {target:?}: {e}")),
    )
    .into_owned();

    let incremental_start = std::time::Instant::now();
    let results = project.update_file(&target, &target_source).unwrap();
    let incremental = incremental_start.elapsed();

    println!(
        "full load:         {} files in {:?}",
        project.file_count(),
        full_load
    );
    println!(
        "incremental update: {} file(s) re-checked ({}, plus {} dependent file(s)) in {:?}",
        results.len(),
        target.display(),
        results.len().saturating_sub(1),
        incremental
    );
    if incremental.as_nanos() > 0 {
        println!(
            "speedup:            {:.0}x",
            full_load.as_secs_f64() / incremental.as_secs_f64()
        );
    }
    let mut result_paths: Vec<&PathBuf> = results.keys().collect();
    result_paths.sort();
    for path in result_paths {
        let diagnostics = &results[path];
        println!("{}: {} diagnostic(s)", path.display(), diagnostics.len());
        for d in diagnostics {
            println!("  {}", output::diagnostic_line(d));
            if let Some(tip) = d.tip_text() {
                println!("      💡 {tip}");
            }
        }
    }
}
