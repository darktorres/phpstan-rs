//! Times `discover_vendor_context` alone, with a fresh listing table per run,
//! as one load would. Nothing else runs in this process.
//! usage: probe-vendor <project root> [runs]

use std::path::Path;
use std::time::Instant;

fn main() {
    let mut args = std::env::args().skip(1);
    let root = args.next().expect("usage: probe-vendor <project root> [runs]");
    let runs: usize = args.next().map_or(5, |n| n.parse().expect("runs"));
    for run in 0..runs {
        let listings = composer::dir_listings::DirListings::default();
        let start = Instant::now();
        let found = composer::vendor::discover_vendor_context(Path::new(&root), &listings);
        let elapsed = start.elapsed();
        println!(
            "run {run}: {:.1} ms, {} files, {} dirs",
            elapsed.as_secs_f64() * 1e3,
            found.files.len(),
            found.dirs.len()
        );
    }
}
