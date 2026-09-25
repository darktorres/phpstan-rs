#![no_main]

use libfuzzer_sys::fuzz_target;

// See fuzz/src/lib.rs's doc comment on guarded_parse_phpdoc for what it
// guards against.
fuzz_target!(|source: String| {
    phpstan_rs_fuzz::guarded_parse_phpdoc(source);
});
