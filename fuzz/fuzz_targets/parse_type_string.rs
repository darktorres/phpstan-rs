#![no_main]

use libfuzzer_sys::fuzz_target;

// See fuzz/src/lib.rs's own doc comment on guarded_parse_type for what it
// guards against (a large worker stack, since a PHPDoc type string can
// nest arbitrarily deep and this parser has no MAX_EXPR_DEPTH-style
// guard the way the statement parser does).
fuzz_target!(|source: String| {
    phpstan_rs_fuzz::guarded_parse_type(source);
});
