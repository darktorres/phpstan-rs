#!/usr/bin/env bash
# Phase 19 Part A: seeds `lex_parse`'s own corpus from this project's
# already-vendored real-world PHP (`vendor/phpstorm-stubs` +
# `vendor/phpstan-src/src`, the identical two directories `benches/
# vendored_corpus.rs` already reuses for the same reason - real,
# representative PHP source is a far better fuzzing seed than an empty
# corpus). `fuzz/corpus/` is gitignored (cargo-fuzz's own default) - run
# this once before `cargo fuzz run lex_parse`, not committed to
# git.
set -euo pipefail
cd "$(dirname "$0")/.."

mkdir -p fuzz/corpus/lex_parse
find vendor/phpstorm-stubs vendor/phpstan-src/src -name '*.php' | while IFS= read -r f; do
    cp "$f" "fuzz/corpus/lex_parse/$(echo "$f" | tr '/' '_')"
done

echo "seeded $(find fuzz/corpus/lex_parse -type f | wc -l) file(s) into fuzz/corpus/lex_parse"
