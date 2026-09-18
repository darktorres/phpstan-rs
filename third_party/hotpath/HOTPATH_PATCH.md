# Vendored patch: `hotpath` 0.24.0

Source: https://crates.io/crates/hotpath (https://github.com/pawurb/hotpath-rs), MIT.

Only `src/` plus a trimmed `Cargo.toml` (its own `[[bin]]`/`[[test]]`
targets removed, since we don't vendor `bin/`/`tests/`) are kept here.
Everything else - dependency list, feature flags, the crate's own
allocation/timing-tracking logic - is byte-for-byte the upstream 0.24.0
release.

## The patch

Two occurrences of a hardcoded `MAX_DEPTH: usize = 64` (one in
`src/lib_on/functions/alloc/core.rs`, one in `src/lib_on/caller_stack.rs`)
bumped to `2048`.

## Why

`hotpath-alloc`'s per-thread allocation-tracking stack is a fixed-size
array of `MAX_DEPTH` frames; pushing past it panics
(`assert!((stack.depth.get() as usize) < MAX_DEPTH)` in
`functions/alloc/guard.rs`). This project's own checker recurses one
measured (`#[hotpath::measure]`) frame per nested expression/statement
when `Checker::check_expr`/`check_stmt` carry the attribute, and real
corpus code (deeply nested expressions in the vendored Piwigo corpus this
project profiles against) exceeds 64 such frames, aborting the whole
`hotpath-alloc` profiling run before it can report anything.

Bumping to 2048 was confirmed sufficient against that same corpus
(`/home/torres/piwigo17-frozen`) with `check_expr`/`check_stmt`/
`infer_expr`/`infer_binary` all simultaneously measured - a run that
would abort outright at the original limit.

## Re-syncing with a newer upstream release

Re-apply the same two-line change (search each file for
`MAX_DEPTH: usize = `) after copying in the new version's `src/`, and
re-trim the new `Cargo.toml`'s `[[bin]]`/`[[test]]` sections the same way.
