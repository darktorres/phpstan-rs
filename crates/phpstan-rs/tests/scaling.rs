//! Scaling tests of the `phpstan-rs` binary. They time real runs, so
//! they're a test target of their own: cargo runs test binaries one after
//! another, and nothing else competes with these for the machine.

mod common;

use common::{TempDir, bin};

#[test]
fn check_scales_roughly_linearly_with_a_single_very_long_line() {
    // Guards against an O(line length x tokens on it) blowup: a single
    // very long line must scale roughly linearly with size, not
    // quadratically. Quadratic scaling would need ~16x the time for 4x
    // the size; a generous 8x ceiling comfortably clears linear-ish
    // scaling while still catching a real quadratic regression.
    fn single_long_line(statements: usize) -> String {
        let mut src = String::from("<?php ");
        for i in 0..statements {
            src.push_str(&format!("$a{i}=1;"));
        }
        src
    }

    let small_dir = TempDir::new("scale_small");
    small_dir.write("a.php", &single_long_line(20_000));
    let large_dir = TempDir::new("scale_large");
    large_dir.write("a.php", &single_long_line(80_000));

    let small_start = std::time::Instant::now();
    let small_output = bin().arg("check").arg(small_dir.path()).output().unwrap();
    let small_elapsed = small_start.elapsed();
    assert!(small_output.status.success());

    let large_start = std::time::Instant::now();
    let large_output = bin().arg("check").arg(large_dir.path()).output().unwrap();
    let large_elapsed = large_start.elapsed();
    assert!(large_output.status.success());

    assert!(
        large_elapsed.as_secs_f64() < small_elapsed.as_secs_f64() * 8.0 + 0.5,
        "4x input size took {:?} vs {:?} for the smaller input - looks quadratic, not linear \
         (the +0.5s slack absorbs fixed process-startup overhead at these small sizes)",
        large_elapsed,
        small_elapsed
    );
}
