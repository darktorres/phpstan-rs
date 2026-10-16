//! Two jobs of a one-thread pool that each load a project needing the
//! built-in base complete while that base builds. A job waiting for the base
//! must not run the other job on its own stack, which would force the base
//! again while the first job still holds it.

use std::path::PathBuf;
use std::sync::{Arc, mpsc};
use std::time::Duration;

use phpstan_rs::project::session::{OverlayFile, Session};
use phpstan_rs::registry::stub_set::{StubSource, Stubs};
use rayon::prelude::*;

#[test]
fn a_cold_base_forced_by_two_jobs_of_one_pool_completes() {
    let (done, finished) = mpsc::channel();
    std::thread::spawn(move || {
        // The build gets a pool of its own: the one worker under test waits
        // for the build and could not serve it.
        let build_pool = Arc::new(
            rayon::ThreadPoolBuilder::new()
                .num_threads(2)
                .build()
                .unwrap(),
        );
        let stubs = Stubs::new(StubSource::Build, build_pool);
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(1)
            .build()
            .unwrap();
        pool.install(|| {
            (0..2).into_par_iter().for_each(|_| {
                let file = OverlayFile {
                    path: PathBuf::from("cold_base.php"),
                    bytes: b"<?php\nfunction cold_base(): int {\n    return 1;\n}\n".to_vec(),
                    analysed: true,
                    stub: false,
                };
                Session::for_overlay(vec![file], stubs.clone())
                    .load()
                    .expect("the file loads");
            });
        });
        let _ = done.send(());
    });
    finished
        .recv_timeout(Duration::from_secs(300))
        .expect("the two jobs did not both finish: a deadlock, or a panic in one of them");
}
