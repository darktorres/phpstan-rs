# Guiding the fuzzer

The `lex_and_parse` target feeds a raw, `arbitrary`-derived `String`
straight into `project::parse_file`. Most byte-level mutations are just
garbage, which is why a fuzzing session prints a flood of the lexer's own
`"warning: unexpected byte 0x.. in PHP source, skipping"`
(`src/lexer.rs:855`) - expected, harmless, not a crash, see that file's
own match arm. The 4 items below are all done now; each says how to use
it.

## 1. Feed it a bigger real corpus

`cargo fuzz run <target> [CORPUS]...` accepts extra corpus/seed
directories as positional args before `--` (confirmed via
`cargo +nightly fuzz run --help`). Import a much larger pile of real PHP
into `corpus/lex_and_parse/` beyond the 4 originally-committed seeds:

    cargo +nightly fuzz run lex_and_parse vendor/phpstorm-stubs -- -max_total_time=120

Every file under the given directory gets imported into the corpus on
that run (a one-time seeding step, not something that needs repeating -
`corpus/lex_and_parse/` keeps whatever it's grown to across runs).

## 2. A libFuzzer dictionary

`fuzz/php.dict` - built directly from this project's own token tables
(`Keyword::as_str()` in `src/token.rs`, plus every multi-character
punctuation/operator token the lexer itself recognizes in `src/lexer.rs`),
not guessed. Confirmed loading cleanly (`Dictionary: 106 entries`, no
parse errors). Biases mutation toward inserting whole recognized tokens
instead of random bytes:

    cargo +nightly fuzz run lex_and_parse -- -dict=fuzz/php.dict -max_total_time=120

## 3. `-max_len` / `-only_ascii`

Recommended combined invocation for a longer session:

    cargo +nightly fuzz run lex_and_parse -- -dict=fuzz/php.dict -max_len=500 -max_total_time=300

- `-max_len=500` keeps inputs small and iteration fast.
- `-only_ascii=1` (not in the default recommendation above) stops
  mutation from spending budget on multi-byte UTF-8 - trades away real
  coverage (the lexer's own `is_ident_start`/`is_ident_continue` accept
  bytes `>= 0x80` as identifier characters, so real Unicode identifiers
  are legitimate lexer input), worth adding only for faster iteration on
  ASCII syntax specifically.

## 4. A structured `Arbitrary`-driven fuzz target

A second target, `structured_php`, complementing `lex_and_parse` (which
stays as-is - pure-random-bytes "must never crash on garbage" coverage is
still worth keeping on its own). Instead of raw bytes coerced into a
`String`, it assembles a small `FuzzProgram` (`fuzz_targets/structured_php.rs`)
out of a hand-written `Fragment`/`Expr` enum pair - `if`/`while`/`for`/
`foreach`/`match`/`switch` wrapping a nested fragment, `function`/`class`/
`interface`/`trait`/`enum` declarations, assignments, closures/arrow-fns,
attribute groups, named/spread call arguments, heredoc/nowdoc, nullsafe
property/method access, PHP 8.1's first-class callable syntax
(`foo(...)`), literal/identifier leaves - each with `arbitrary`-driven
randomized pieces (names, literals, operator choice), rendered into a
single `"<?php\n"`-prefixed source string. Reaches real recursive nesting
depth (exercising `Parser::enter_expr`'s `MAX_EXPR_DEPTH` guard through
genuine structure) and always actually enters PHP mode, instead of
relying on byte mutation to stumble onto a `<?php` prefix by chance.

Both targets share the same panic-handling/64MiB-worker-stack guard,
factored into `fuzz/src/lib.rs`'s `guarded_parse` (see `lex_and_parse.rs`'s
own doc comment history for why that guard exists at all - the
`MAX_EXPR_DEPTH` panic and the stack-overflow-below-it risk).

    cargo +nightly fuzz run structured_php -- -max_total_time=120

## A third target: `parse_type_string`

`types::parse_type_string` (`src/types/parse.rs`) is a separate
recursive-descent parser over raw PHPDoc type strings - same
untrusted-input risk shape as the lexer/parser, previously unfuzzed. It
has no `MAX_EXPR_DEPTH`-style guard of its own (confirmed: no
`panic!`/depth guard anywhere in `parse.rs`/`conditional.rs`/
`templates.rs`), so a deeply nested type string
(`array<array<array<...>>>`) is a real, legitimate stack-overflow risk
worth fuzzing for - `fuzz/src/lib.rs::guarded_parse_type` still runs it on
the same 64MiB worker stack `guarded_parse` uses (so a crash means real
unbounded recursion, not just an ordinary small default stack), but
swallows no panic message: unlike `guarded_parse`, there's no known,
deliberate panic here to allowlist, so any panic is a genuine bug.

    cargo +nightly fuzz run parse_type_string -- -max_total_time=120
