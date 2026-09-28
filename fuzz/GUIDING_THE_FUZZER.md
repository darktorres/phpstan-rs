# Guiding the fuzzer

Four targets: `lex_parse` and `token_stream_parse` (source bytes and
token streams into the PHP parser), `structured_php` (generated PHP
programs through the whole per-file project pipeline) and `phpdoc_type`
(strings through the PHPDoc grammar, as a type and as a whole doc
comment). This doc covers ways to make mutations land on more
syntactically-interesting input for the byte/token-level targets, and how
`structured_php` avoids needing most of them.

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

+ `-max_len=500` keeps inputs small and iteration fast.
+ `-only_ascii=1` (not in the default recommendation above) stops
  mutation from spending budget on multi-byte UTF-8 - trades away real
  coverage (the lexer's own `is_ident_start`/`is_ident_continue` accept
  bytes `>= 0x80` as identifier characters, so real Unicode identifiers
  are legitimate lexer input), worth adding only for faster iteration on
  ASCII syntax specifically.

## 4. `structured_php` already does this differently

`structured_php` doesn't need any of the above - it generates
syntactically-plausible input directly via a hand-written `Arbitrary` impl
instead of relying on byte-level mutation to stumble onto valid syntax.
Its generator
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

`phpdoc_type` has no generator: it mutates raw strings, starting from the
type expressions checked in under `fuzz/corpus/phpdoc_type/`. More seeds
there (real `@param`/`@return` types from `vendor/phpstorm-stubs`, say)
steer it the way section 1 steers `lex_parse`.

    cargo fuzz run phpdoc_type -- -max_total_time=120

`structured_php` and `phpdoc_type` share `fuzz/src/lib.rs`'s 64MiB worker
stack (`guarded_parse`/`guarded_parse_phpdoc`) - see that file's doc
comment for why: both reach real recursive depth, unlike
`lex_parse`/`token_stream_parse`, which don't swallow any panic message
at all - see their own doc comments for why that's the right call for
them.
