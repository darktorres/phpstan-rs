# Guiding the fuzzer

Four targets now: `lex_parse`/`token_stream_parse` (from the original
`master`-side Phase 19 work) plus `structured_php`/`parse_type_string`
(merged in alongside them - see ROADMAP.md's Phase 19 entry for how the
two efforts relate). This doc covers ways to make mutations land on more
syntactically-interesting input for the two byte/token-level targets, and
how the two structured targets already avoid needing most of them.

## 1. Feed `lex_parse` a bigger real corpus

`fuzz/seed_corpus.sh` already does this (seeds from `vendor/phpstorm-stubs`
+ `vendor/phpstan-src/src`) - run it once per checkout:

    ./fuzz/seed_corpus.sh
    cargo fuzz run lex_parse

`cargo fuzz run <target> [CORPUS]...` also accepts extra corpus/seed
directories as positional args before `--`, if you want to seed from
somewhere else too - list the target's own managed corpus dir
(`fuzz/corpus/<target>`) FIRST when you do, or libFuzzer treats whatever
directory you list first as the writable/primary one and saves new finds
there instead (confirmed the hard way: an early attempt at this pointed
`vendor/phpstorm-stubs` first and it started writing hash-named fuzz
corpus entries directly into that vendored source tree).

## 2. A libFuzzer dictionary

`fuzz/php.dict` - built directly from this project's own token tables
(`Keyword::as_str()` in `src/token.rs`, plus every multi-character
punctuation/operator token the lexer itself recognizes in `src/lexer.rs`),
not guessed. Biases mutation toward inserting whole recognized tokens
instead of random bytes:

    cargo fuzz run lex_parse -- -dict=fuzz/php.dict -max_total_time=120

## 3. `-max_len` / `-only_ascii`

Recommended combined invocation for a longer session:

    cargo fuzz run lex_parse -- -dict=fuzz/php.dict -max_len=500 -max_total_time=300

- `-max_len=500` keeps inputs small and iteration fast.
- `-only_ascii=1` (not in the default recommendation above) stops
  mutation from spending budget on multi-byte UTF-8 - trades away real
  coverage (the lexer's own `is_ident_start`/`is_ident_continue` accept
  bytes `>= 0x80` as identifier characters, so real Unicode identifiers
  are legitimate lexer input), worth adding only for faster iteration on
  ASCII syntax specifically.

## 4. The two structured targets already do this differently

`structured_php` and `parse_type_string` don't need any of the above -
they generate syntactically-plausible input directly via a hand-written
`Arbitrary` impl instead of relying on byte-level mutation to stumble
onto valid syntax. `structured_php`'s own generator
(`fuzz_targets/structured_php.rs`) assembles a small `FuzzProgram` out of
a `Fragment`/`Expr` enum pair - `if`/`while`/`for`/`foreach`/`match`/
`switch` wrapping a nested fragment, `function`/`class`/`interface`/
`trait`/`enum` declarations, assignments, closures/arrow-fns, attribute
groups, named/spread call arguments, heredoc/nowdoc, nullsafe property/
method access, first-class callable syntax (`foo(...)`) - each with
randomized real pieces (names, literals, operator choice), rendered into
a single `"<?php\n"`-prefixed source string. `token_stream_parse`
(pre-existing) is the same idea one level lower: `arbitrary`-derived
`Token`s fed straight into the parser, bypassing the lexer (and
`structured_php`'s own source-text rendering) entirely.

    cargo fuzz run structured_php -- -max_total_time=120
    cargo fuzz run parse_type_string -- -max_total_time=120

Both share `fuzz/src/lib.rs`'s panic-handling/64MiB-worker-stack guard
(`guarded_parse`/`guarded_parse_type`) - see that file's own doc comment
for why `structured_php` needs it (its generator reaches real recursive
depth on purpose, unlike `lex_parse`/`token_stream_parse`, which don't
swallow any panic message at all - see their own doc comments for why
that's the right call for them).
