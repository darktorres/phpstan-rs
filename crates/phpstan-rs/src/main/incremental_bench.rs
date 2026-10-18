use std::fs;
use std::path::{Path, PathBuf};

use phpstan_rs::registry::stub_set::{StubSource, Stubs};
use phpstan_rs::{output, project};

/// `phpstan-rs incremental-bench <dir> [<file-to-touch>]` - loads every
/// .php file under `dir` into a `Project` (a full load, timed), then
/// re-parses and re-checks just one file via `Project::update_file` (the
/// incremental path, also timed) and reports both, to make concrete that
/// an edit to one file costs O(that file), not O(the project). Picks the
/// first file in directory-walk order when none is named explicitly.
pub(crate) fn run_incremental_bench(dir: &Path, file_to_touch: Option<&Path>) {
    let stubs = Stubs::new(StubSource::from_environment(), project::worker_pool());
    let mut files = Vec::new();
    project::collect_php_files(dir, &mut files);
    if files.is_empty() {
        eprintln!("no .php files found under {}", dir.display());
        return;
    }
    files.sort();

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

    let sources: Vec<(PathBuf, Vec<u8>)> = files
        .iter()
        .map(|p| {
            let bytes = fs::read(p).unwrap_or_else(|e| panic!("failed to read {p:?}: {e}"));
            (p.clone(), bytes)
        })
        .collect();

    let full_load_start = std::time::Instant::now();
    let overlay = sources
        .iter()
        .map(|(path, bytes)| project::session::OverlayFile {
            path: path.clone(),
            bytes: bytes.clone(),
            analysed: false,
            stub: false,
        })
        .collect();
    // Not built once: the edit below changes the project.
    let session = project::session::Session::for_overlay(overlay, stubs.clone());
    let mut project = project::Project::from_stubs(&stubs);
    session
        .load_into(&mut project)
        .unwrap_or_else(|e| panic!("failed to load the files: {e}"));
    let full_load = full_load_start.elapsed();

    let target_source = sources
        .iter()
        .find(|(p, _)| p == &target)
        .map(|(_, s)| s.clone())
        .unwrap();

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
    let mut result_paths: Vec<_> = results
        .iter()
        .map(|(&id, diagnostics)| (project.path_of(id), diagnostics))
        .collect();
    result_paths.sort_by_key(|&(path, _)| path);
    for (path, diagnostics) in result_paths {
        let shown = project::shown_path(project.absolute_root(), path);
        println!("{}: {} diagnostic(s)", shown.display(), diagnostics.len());
        for d in diagnostics {
            println!("  {}", output::diagnostic_line(d));
            if let Some(tip) = d.tip_text() {
                println!("      💡 {tip}");
            }
        }
    }
}
