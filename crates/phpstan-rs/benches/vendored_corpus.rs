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
//! Source files are read once, up front, into the session's overlay -
//! each `Criterion::bench_function` iteration re-runs only the
//! `Session::load`/`check_all` work, not disk I/O,
//! which would otherwise dominate the measurement and mask the actual
//! parse/index/check throughput this benchmark exists to track.
//!
//! Criterion's baseline comparison - against the last recorded run under
//! `target/criterion/`, flagging a statistically meaningful regression - is
//! not run by CI, which has no quiet machine to time on. CI compiles this
//! bench and runs it once in criterion's test mode; the regression check is
//! the local perf gate (`cargo xtask perf`, and `cargo xtask gates`).

use std::path::{Path, PathBuf};

use config::ConfigFile;
use criterion::{Criterion, criterion_group, criterion_main};
use phpstan_rs::project;
use phpstan_rs::project::session::{Overrides, Policy, Session};

fn bench_vendored_corpus(c: &mut Criterion) {
    // The directories are relative to the workspace root, two levels above
    // this package.
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let session = Session::resolve(
        &ConfigFile::empty(),
        None,
        root,
        Overrides {
            paths: vec![
                PathBuf::from("vendor/phpstorm-stubs"),
                PathBuf::from("vendor/phpstan-src/src"),
            ],
            no_composer: true,
            ..Overrides::default()
        },
        Policy::Fail,
        project::worker_pool(),
    )
    .expect("the corpus paths resolve");
    let files = session.listing().0;
    assert!(
        !files.is_empty(),
        "no .php files found under vendor/phpstorm-stubs or vendor/phpstan-src/src - \
         check vendor/phpstan-src's git submodule is initialized \
         (git submodule update --init)"
    );

    // Real PHP source isn't required to be UTF-8; the bytes are read as
    // they are, once, so an iteration reads no disk.
    let root = session.root().to_path_buf();
    let session = session.with_overlay(files.into_iter().map(|path| {
        let bytes = std::fs::read(root.join(&path)).unwrap_or_default();
        (path, bytes)
    }));
    let pool = project::worker_pool();
    c.bench_function("load_and_check_vendored_corpus", |b| {
        b.iter(|| {
            let project = session.load().expect("the corpus loads").project;
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
