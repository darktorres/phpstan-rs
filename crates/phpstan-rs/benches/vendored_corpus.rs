//! An automated regression-guard benchmark - the gap `run_incremental_bench`
//! (main.rs, the existing `incremental-bench` CLI command) doesn't fill
//! on its own, since it's a manual, human-run tool, not something CI or
//! a test suite runs automatically.
//!
//! Benchmarks the same "full load, then check everything" shape
//! `run_incremental_bench`'s own "full load" half already exercises,
//! against this project's in-repo vendored corpus
//! (`vendor/phpstorm-stubs` + `vendor/phpstan-src/src`, 2351 files
//! combined) - real-world-shaped PHP (not synthetic), already embedded
//! in the binary via `build.rs` for the stub-class pipeline, and
//! guaranteed to be present wherever this repo is checked out (modulo
//! the submodule-init guard `build.rs` itself enforces).
//!
//! Source files are read once, up front, outside the benchmarked
//! closure - each `Criterion::bench_function` iteration re-runs only the
//! `Project::load_file`/`finish_loading`/`check_all` work, not disk I/O,
//! which would otherwise dominate the measurement and mask the actual
//! parse/index/check throughput this benchmark exists to track.
//!
//! Criterion's headline feature - comparing a run against its last
//! recorded baseline (stored under `target/criterion/`) and flagging a
//! statistically meaningful regression - is the mechanism that catches
//! a real slowdown via CI rather than by chance.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use criterion::{Criterion, criterion_group, criterion_main};
use phpstan_rs::project::{self, Project};

fn load_sources(dirs: &[&str]) -> Vec<(PathBuf, String)> {
    let mut files = Vec::new();
    // `dirs` are relative to the workspace root, two levels above this
    // package.
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    for dir in dirs {
        project::collect_php_files(&root.join(dir), &mut files);
    }
    files.sort();
    files
        .into_iter()
        .map(|path| {
            // Real PHP source isn't required to be UTF-8 (see main.rs's
            // own try_load_file for the same reasoning) - lossy decoding
            // is fine here too, since this benchmark only cares about
            // parse/check throughput, not correctness.
            let bytes = std::fs::read(&path).unwrap_or_default();
            let source = String::from_utf8_lossy(&bytes).into_owned();
            (path, source)
        })
        .collect()
}

fn bench_vendored_corpus(c: &mut Criterion) {
    let sources = load_sources(&["vendor/phpstorm-stubs", "vendor/phpstan-src/src"]);
    assert!(
        !sources.is_empty(),
        "no .php files found under vendor/phpstorm-stubs or vendor/phpstan-src/src - \
         check vendor/phpstan-src's git submodule is initialized \
         (git submodule update --init)"
    );

    let stubs = phpstan_rs::registry::stub_set::Stubs::new(
        phpstan_rs::registry::stub_set::StubSource::from_environment(),
        project::worker_pool(),
    );
    let pool = Arc::clone(stubs.pool());
    c.bench_function("load_and_check_vendored_corpus", |b| {
        b.iter(|| {
            let mut project = Project::from_stubs(&stubs);
            for (path, source) in &sources {
                let _ = project.load_file(path.clone(), source);
            }
            project.finish_loading_with(&stubs);
            std::hint::black_box(project.check_all(&pool));
        });
    });
}

// One iteration of the full 2351-file corpus takes ~1s - criterion's
// default 100-sample target would take ~2.5 minutes per run, too slow
// for a benchmark meant to actually get run routinely. 20 samples still
// gives a real confidence interval and outlier detection, in well under
// a minute.
fn custom_criterion() -> Criterion {
    Criterion::default().sample_size(20)
}

criterion_group! {
    name = benches;
    config = custom_criterion();
    targets = bench_vendored_corpus
}
criterion_main!(benches);
