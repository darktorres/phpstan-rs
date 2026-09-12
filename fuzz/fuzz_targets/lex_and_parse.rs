#![no_main]

use libfuzzer_sys::fuzz_target;

// See fuzz/src/lib.rs's own doc comment for why guarded_parse exists and
// what it guards against (the parser's deliberate MAX_EXPR_DEPTH panic,
// and a real stack overflow below it on an ordinary-sized thread stack).
fuzz_target!(|source: String| {
    phpstan_rs_fuzz::guarded_parse(source);
});
