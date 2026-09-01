//! Phase 20 Finding D: an automated regression-guard benchmark - the gap
//! `run_incremental_bench` (main.rs, the existing `incremental-bench` CLI
//! command) doesn't fill on its own, since it's a manual, human-run tool,
//! not something CI or a test suite runs automatically. Exactly the kind
//! of tooling absence that let a real quadratic blowup (Phase 11) ship
//! unnoticed until it happened to hang on a real file.
//!
//! Benchmarks the same "full load, then check everything" shape
//! `run_incremental_bench`'s own "full load" half already exercises,
//! against this project's own in-repo vendored corpus
//! (`vendor/phpstorm-stubs` + `vendor/phpstan-src/src`, 2351 files
//! combined) rather than the real Piwigo corpus this project's own
//! false-positive-hunting sessions use - Piwigo lives outside this repo
//! (`~/piwigo17-rewrite`, machine-specific), so it isn't available to CI
//! or any other machine a benchmark might run on. The vendored corpus is
//! real-world-shaped PHP (not synthetic), already embedded in the binary
//! via `build.rs` for an unrelated reason (Finding 4's stub-class
//! pipeline), and guaranteed to be present wherever this repo is checked
//! out (module the submodule-init guard `build.rs` itself now enforces).
//!
//! Source files are read once, up front, outside the benchmarked
//! closure - each `Criterion::bench_function` iteration re-runs only the
//! `Project::load_file`/`finish_loading`/`check_all` work, not disk I/O,
//! which would otherwise dominate the measurement and mask the actual
//! parse/index/check throughput this benchmark exists to track.
//!
//! Criterion's own headline feature - comparing a run against its last
//! recorded baseline (stored under `target/criterion/`) and flagging a
//! statistically meaningful regression - is the actual "caught by CI
//! rather than by chance" mechanism this finding calls for.

use std::path::{Path, PathBuf};

use criterion::{Criterion, criterion_group, criterion_main};
use phpstan_rs::project::{self, Project};

fn load_sources(dirs: &[&str]) -> Vec<(PathBuf, String)> {
    let mut files = Vec::new();
    for dir in dirs {
        project::collect_php_files(Path::new(dir), &mut files);
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
         run from the crate root, and check vendor/phpstan-src's git submodule is \
         initialized (git submodule update --init)"
    );

    c.bench_function("load_and_check_vendored_corpus", |b| {
        b.iter(|| {
            let mut project = Project::new();
            for (path, source) in &sources {
                let _ = project.load_file(path.clone(), source);
            }
            project.finish_loading();
            std::hint::black_box(project.check_all());
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
