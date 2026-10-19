# Part I. The designs the batches wait for (Part F 5, 7, 8, 11, 12)

Each is a section, written from the code at `8a1ca1e0`. I1 is item 9, step 2's layout; I2 to I5 belong to
batches 1, 2, 6 and 7.

## I2. Token payloads under decision 4 (Part F 7)

**The problem.** 20 G item 10 gives `TokenStream` a `payloads: Vec<Payload<'a>>` holding `&'a Name`, `&'a [u8]`
and `&'a str` into an arena. Decision 4 dropped the arena (a `bumpalo` arena is not `Sync`), and the tree's name
table and side buffers (B3) grow while the parse runs, so no reference into them can be held across a push. The
stream cannot hold ids either without the lexer calling the builder for every token.

**The design: a token is a kind and a span, and its text is read at the span when the parser takes it.** The
parser holds the source it parses (it does, the worker's read buffer); the builder interns a name, takes a span of
the source or appends decoded bytes to a side buffer at the moment a node needs it. Nothing is materialized at lex
time that a node may not want.

```rust
pub struct TokenStream {
    kinds: Vec<TokenKind>,     // one byte: the 64 punctuation and structural kinds, the 62 keywords, and the new classes below (Cast, QualifiedName, Number, the string kinds, Error)
    spans: Vec<Span>,
    docs: Vec<DocMark>,        // each doc comment: the index of the token it precedes, and its span
}
```

A template or heredoc carries only its kind and its span: its parts are built when the literal is taken, so a skimmed
body pays for none (the first draft kept an `extras` and a `parts` array, filled at lex time for every template in
the file, with three cursors that had to move in lockstep through a skim that does not take tokens).

| class | the lexer records | the parser's `take` does |
|---|---|---|
| identifier, keyword used as a name | kind (`Identifier` or the keyword's); the `WatchList` bits are set in the file facts from the spelling, by the perfect-hash table of SYN-4.1 | `b.name(&src[span])`: hash once, probe the tree's name table, intern into the global interner on a miss |
| variable | `Variable`, span including the `$` | the name is `&src[span][1..]` |
| qualified name written without spaces | one `QualifiedName` token (SYN-4.8) | `b.name` of the slice, split at the last `\` by the header |
| number | `Number` (the lexer validates the lexeme and records an error for `08`, `1__0`) | parse the digits skipping `_` (SYN-4.9); an integer past `i64` becomes a float, as now |
| single-quoted string | `StringPlain` or `StringEscaped` (does it hold a `\\` or `\'`) | a span of the source for `StringPlain`, with no copy; decoded into `side_bytes` for `StringEscaped` |
| double-quoted string without interpolation | the same two kinds | a span when it holds no escape; otherwise decoded into `side_bytes` (PHP's escapes) |
| double-quoted string, backtick string, heredoc, nowdoc | `Template`, `ShellTemplate`, `Heredoc` or `Nowdoc`, the span of the whole literal, and a flag per token of the string family: has-escape, interpolates, **has-braced-part** (the skim reads this bit) | scan the span with the code that is `scan_interpolated` now and build the parts: a part is `Literal(span, needs_decode)`, `SimpleVar(span)`, `Property { var, prop, nullsafe }`, `Index { var, key }`, `NameIndex { name, key }` or `Complex(span)`, all spans of the source |
| heredoc indentation | nothing | the closing indent is read back from the closing line at the end of the span, and after each LF inside a literal part up to that many space or tab bytes are dropped before the part is decoded by flavour (double-quoted and heredoc decode `\"` differently, backtick decodes `` \` ``, nowdoc decodes nothing); a part whose first literal starts after the first line's indent keeps its span there. Today `strip_heredoc_indent` runs on the whole body before `scan_interpolated`, and a pinned test has `Literal "!\n  "` over `(3,17)-(4,7)`; PHP refuses an under-indented body line ("Invalid body indentation level"), which today is silently cut |
| inline HTML | `InlineHtml` (text only: before `<?=` or the end of the file) or `InlineHtmlOpen` (the text and the `<?php` tag that ends it: the span includes the five bytes of the tag, as `lex_inline_html` advances past them before it returns, which `checker/whitespace.rs` and the statement's span depend on) | a span of the source (less the tag for `InlineHtmlOpen`) |
| `<?=` | the `Echo` keyword with the span of `<?=`, as now | nothing |
| `?>` | `CloseTag`, the span of `?>` and the optional newline it absorbs (today `Token::Semicolon` with that span); the parser treats it as a statement end, and G3.4's stoppers can tell it from `;` | nothing |
| a cast `(int)` | `Cast`, span including the parentheses. **The lexer rule, exactly:** the parentheses hold a case-insensitive cast name between optional spaces or tabs and nothing else (php 8.5.4: `( int )`, `(INT)` and tab-padded forms are casts; a newline or a comment inside is not) | the spelling is `&src[span]` between them, spaces and tabs trimmed (`CastSpelling::from_name`) |
| everything else | the kind | nothing |

Two deltas follow from the cast rule, each with a fixture and approved on 2026-10-18 (M5b) with the others at batch 1: today
the parser recognises a cast by a three-token lookahead that allows comments and newlines between the tokens, which PHP
does not; and `foo(string);` lexes as a cast token and is a parse error, as PHP reports it, where it is a call with a
constant argument today. The oracle excludes `Cast` from its "exact" claims.

- **No payload array.** The 20 G design's `payloads` is replaced by the span every token already has.
  `Payload`, `Token`'s `Arc`, `String` and `Name` fields, `Tok`, `DocGroup`, `Supply`, `next_raw`, `next_tok`,
  `fill`, `pop`, the `VecDeque` and `raw_keyword` go; a keyword's spelling is the source at its span.
- **Fragments.** An interpolation fragment (`{$a['k']}`) is lexed, when the literal is taken, into a scratch stream
  taken from a small stack of them, over the same source with absolute spans, because the file's own stream is
  still in use. The parser's sub-parser reads that stream, as `build_interpolated` does now; the
  `ComplexExpr(String, u32)` text copy goes, being a slice of the source, **except where it was never a slice**:
  `${name[expr]}` is rewritten today to `$name[expr]` (the braces dropped), `${expr}` is re-spelled, and a bare
  `$a[key]` whose key fits none of the three simple shapes is rebuilt with quotes added (a pinned test has
  `"$arr[1a]"` as an index with the key `1a`, which php 8.5.4 refuses, as it refuses `"$arr[foo bar]"` and
  `"$arr[ 0 ]"`). So: `${name[expr]}` is a part of its own (`NameIndex`: the name span and the bracketed expression
  span, lexed as a fragment and built as `Index(Variable, expr)`); `${expr}` is the slice from the dollar to the
  closing brace; and the bare fallback follows PHP: a lexical error (G3.2 item 3), with the pinned test replaced by
  an error test, an approved delta with its `gates/test-renames` entry.
- **Doc comments** are `DocMark { before: u32, span: Span }`. The doc before the current token is the run of
  marks at its index (a cursor beside the token cursor); a single doc is a span of the source, and several are
  folded when a declaration or statement asks, the folded text written once into `side_text` for the group,
  however many statements share it. The rules
  `DocGroup` carries today are kept: a declaration takes the last mark, a statement the folded text (or the last);
  a doc before an attribute list wins and carries no `first_start` (`parse_leading_attributes` re-keys it into a
  fresh group), so the parser holds it as pending state across the attributes.
- **Lookahead at any distance** is `kinds[pos + n]`. **Skimming a body is the three-bit state machine it is today,
  over kinds:** brace depth opens on both `{` and `${`; the bits are *after a member operator* (`::`, `->`, `?->`),
  *after `function`* and *after `PHPStan\`*; and the triggers (the file reads the body at load) are `Eof`, inline
  HTML, a template with the has-braced-part bit, `function` followed by a name, `class`, `interface`, `trait`,
  `enum`, `global`, `include`, `include_once`, `require`, `require_once`, `yield` and `goto` (none of them after
  a member operator), `define`, and the `PHPStan\` prefix, the last two read from the identifier's source only
  for identifiers that begin with `d` or `p`. The oracle is `parse.rs`'s six skim tests, with `${$x}` and
  `QualifiedName` cases added. A skimmed region costs no interning and no string decoding, where today every
  token of it is lexed to a value and dropped.
- **Buffers** are per worker thread, cleared between files and emptied when the load ends; `reserve` starts from
  the source length divided by a constant the census sets (about 4 bytes per token at the project's average).
  **A retained-capacity cap** applies to the token arrays and the builder's vectors, as `KEPT_READ_CAPACITY`
  (1 MiB) does for the read buffer: capacity above it is released after each file, so one 100 MB file does not
  hold nine bytes a token on a worker until the load ends, and a 100 MB file is in the peak-RSS checks of step 3
  and S-7.
- **Errors** (G3): the lexer records a `SyntaxError` with the token's span and kind `Error` where PHP refuses the
  lexeme, and the parser treats that token as a literal operand. A NUL byte is an `Error` token and not `Eof` (the
  stream ends at the real end only), and an unterminated string's span is clamped to the source (G3.2).
- **The stream ends at `__halt_compiler`.** The lexer ends the stream at `__halt_compiler`, `(`, `)`, then `;` or
  `?>` in statement position (not after `->`, `?->`, `::`, `function`, `const` or a backslash), gives the final
  `Eof` the file-end span (the `Block` statement's span reaches the end today), records the data offset (the
  checker already knows `__COMPILER_HALT_OFFSET__`, `checker/global_constants.rs`) and lexes nothing after it:
  today the parser calls `skip_to_eof` (`lexer.skip_to_end()` sets `pos` to the end of the source without
  lexing), and a whole-file lex would otherwise lex binary PHAR data and report errors in it. A test whose payload
  holds `08`, `1__0`, a lone quote, `<<<EOT`, NUL and control bytes gives no warning, no error and the same
  tree.
- **The lexer's result** is `Lexed { stream: TokenStream, ignore_marks, warnings and errors, facts, line_index,
  comments (optional) }`, which replaces `Lexed`, `LexOutput` and `LexEnd`; the entry points that survive are the
  whole file, a fragment and in-PHP (the `tokenize_with_spans` and `with_comments` of `lsp/completion.rs`,
  `stub_attributes.rs` and `output/codeframe.rs`), and the shared name cache `parse_with_names` takes today
  (`NameCache`) is the builder's `b.name` path. **Ignore marks and file facts** are recorded in the lexing loop as
  now.

**What it changes in Part B.** B5's "until SYN-2 the lexer still makes a `PhpString` per literal and the builder
copies it" ends: after SYN-2 the lexer never touches the builder, and the builder's `name` and `text_span` are
called only by the parser. B3's side buffers are filled at take time and a span costs nothing, so a skimmed body
and an unclaimed doc add nothing to them. The token dump of B8 prints (kind, span, the text at the span, and for a string kind the decoded bytes through
the builder's own decode). **It is not "by construction" the same dump as the old tokens':** the new stream
differs from the old in an explicit list, and each difference is a delta with its fixture, or is mapped by the dump's
kind-name function to the old name: `Cast` replaces three tokens (and the two deltas above), `QualifiedName`
several, `Number` merges `Integer` and `Float`, six string kinds replace three, `CloseTag` was `Semicolon`,
`InlineHtml` and `InlineHtmlOpen` keep the old span, a NUL is an `Error` token and not `Eof`, and the stream ends at
`__halt_compiler`. The B8 baseline and this dump also cover a CRLF, a lone-CR and a BOM-prefixed copy of every
corpus file, made in memory (the corpora are LF-only, and the lexer's newline-sensitive sites, a `//` or `#`
comment's end, the newline `?>` absorbs and a heredoc's closing label, take the one terminator function of I3).

**Readers of token lists outside `syntax`:** `stub_attributes.rs` (`parse_native_type`), `output/codeframe.rs`,
`lsp/completion.rs`, `xtask/src/front_end.rs`, the `lex_parse` bench and the test helpers, and the fuzz targets
`lex_parse` and `token_stream_parse`, read `kinds`, `spans` and the source they hold; `Parser::new(&TokenStream,
src)` replaces the test constructors. **`token_stream_parse` is rebuilt,** because it feeds `Parser::new` an
arbitrary `Vec<Token>` with every span default today: it generates arbitrary kinds, prints each as a canonical
lexeme into a source and lexes that, and a `TokenStream` validator (spans inside the source and ordered, a flag
byte on every string-family kind) runs in `Parser::new` in debug builds and is satisfied by the generator.

**Oracle.** O1, O2 and the token dump; the property test that lexing a file and parsing it equals the old
streaming parse on every corpus file (the tree dump); a fuzz target that compares the old and new lexer's
(kind, span) pairs until the old one is deleted (one of the policy's listed exceptions, Part E 13). **Saving:**
the 20 G figure, about 1.7 G, from the plumbing alone (257 Ir per token, 1.77 G), and what a skimmed region no
longer interns or decodes, which no one has measured. **Measure, with a threshold:** callgrind of
`parse_with_ignores` and the existing criterion bench "lex phpstorm-stubs", off and warm, against the base
commit; whole-file lexing adds work the streaming parse did not do (a failed parse lexes the rest, a skim lexes
everything), so the accept is that lex plus parse Ir together fall by at least half of the 1.77 G plumbing
estimate (on the stubs and Piwigo), and if that work eats the saving the design is reconsidered (a bounded-batch
lexer that the parser can stop is the alternative); a measurement that does not show the saving is a finding, not
a claim.

## I3. SYN-3.2's kernel (Part F 8)

Read from `syntax/src/span.rs` (`LineIndex::new`: `memchr_iter` for `\n`, then `Utf8Chunks` for the columns of a
non-ASCII file) and `wide` 0.7.33's source (`~/.cargo/registry/src/*/wide-0.7.33`), which Part E 10 chose on merit
(it is also already in the build through `imbl`).

**What `wide` gives, and what it does not.** `u8x16` has `cmp_eq`, `max`, `min`, saturating add and subtract,
`move_mask` (the high bit of each byte, one bit per lane) and `any`/`all`/`none`. It compiles to SSE2 where the
target has it (baseline x86-64), to NEON on aarch64 and to a scalar `[u8; 16]` elsewhere, chosen at compile time
(`pick!` over `target_feature`). It has **no unsigned greater-than** on `u8x16`, and `u8x32` is real AVX2 only if
the build enables it, which the baseline build does not. So the kernel is built from equality masks and the sign
bit, in 16-byte blocks, with no run-time dispatch (Part E 10's answer).

**The kernel.** One pass over `src` produces the line starts and whether the file is ASCII:

```rust
pub(crate) fn scan_lines(src: &[u8], rule: Terminators) -> (Vec<u32>, bool)   // starts, ascii
```
per 16-byte block: `v = u8x16::new(block)`; `lf = v.cmp_eq(LF).move_mask()`; the vectors are OR-ed (`BitOr` exists
on `u8x16`) and one `move_mask` after the loop decides `ascii` (a `move_mask` per block for the high bits would
cost two or three per block on NEON); and, for the rule that counts `\r`, `cr = v.cmp_eq(CR).move_mask()`. The
block's line ends are `lf` for `Terminators::Lf`
and, for `Terminators::Php`, `lf | (cr & !(lf >> 1))` (a `\r` not followed by `\n` ends a line; `\r\n` ends it at
the `\n`), with one bit of carry across blocks: a `\r` in a block's last lane is a line end only if the next
block's first byte is not `\n`. Each set bit `i` gives the start `base + i + 1`, taken by `trailing_zeros` and
`mask &= mask - 1`. The tail (under 16 bytes) is copied into a zero-padded block and its masks cut to its length.
A start is a `u32` that cannot wrap, since the loader refuses a source of 2^28 bytes or more (B5).

- **One rule, applied everywhere a newline matters.** PHP ends a line at `\n`, `\r\n` or a lone `\r`, and the
  lexer applies it in more places than the line index (php 8.5.4 was run: a `//` or `#` comment ends at a lone CR,
  `?>` absorbs a lone CR, and a heredoc's closing label is found after a lone CR). Today the lexer ends a line
  comment at `\n` or `?>`, absorbs only `\r\n` or `\n` after `?>`, searches heredoc lines by `\n`, and the splitters
  (`codeframe.rs`, the source-line helpers) split on `\n`. So `Terminators` is one function used by the line
  index, the lexer's comment, close-tag and heredoc/nowdoc sites, the shebang skip and the splitters, and SYN-3.2
  ships `Php` in batch 2 with the lone-CR fixtures and their approved delta (a lone-CR file's comment no longer
  swallows the code after it, and its heredocs close): shipping `Lf` and leaving `Php` to F1's text, which
  changes "the lexer's line starts and the line index" only, would leave line numbers following CR while the
  comment swallowed the code after it, and would carry an unused enum arm that `clippy -D warnings` must be told
  about. `CRATE_SPLIT_PLAN.md`'s F1 text is edited to say so. The oracle is a scalar loop; for the old rule also
  the `memchr` version it replaces.
- **Types (SYN-3.1).** `LineStarts { starts: Box<[u32]>, len, ascii }` for every file, from the scan, one
  allocation (`into_boxed_slice` after the exact fill). `LineIndex` is `LineStarts` plus the column tables, built
  for analysed files and the server's open documents only; it is built from the starts and the source, and the
  tables are skipped when `ascii` is true.
- **The column tables** are built from `first_non_ascii(bytes, from)` positions (`move_mask` and
  `trailing_zeros` over the same blocks) with a decoder that looks only at a window of at most four bytes at
  each non-ASCII byte (the maximal ill-formed subpart is one character: `Utf8Chunks`' rule) and, for a source the
  load already knows is valid UTF-8 (`simdutf8` in `source::decode_text`; the lexer holds `text: Option<&str>`),
  takes the width from the lead byte alone. **Keeping `Utf8Chunks` and replacing only its position search would not
  deliver the saving:** `Utf8Chunks::next` is itself a byte-at-a-time scalar decoder over every byte, ASCII
  included (a copy of std's loop is in `bumpalo`'s `lossy.rs`; std's own current source was not readable here),
  so for a valid file one `next()` walks the whole file and the position search then walks it again, and the
  validation walk is most of the 83.4 M the column tables cost. The current `Utf8Chunks` version stays as the
  oracle on random invalid UTF-8, and what the column tables save is restated after that.
- **SYN-4.3's blank runs** use the same blocks: a mask of the bytes in the lexer's blank set (`cmp_eq` for each
  member, or-ed), and the run's length is the number of trailing ones (`(!mask).trailing_zeros()`), crossing to the
  next block while the whole block is blank.
- **Dependency:** `wide` becomes a direct dependency of `syntax`, pinned as the workspace pins its others
  (`=0.7.33`; Zlib or Apache-2.0 or MIT; it uses `safe_arch` and `bytemuck`, and holds `unsafe` of its own too:
  the `transmute` in `u8x16::new`, the NEON intrinsics, `unsafe impl Pod`). No `unsafe` of ours.

**Tests.** The scan against the scalar oracle on: every string of length at most 6 over `{a, LF, CR, 0x80}`
placed at each offset 0 to 33 inside filler of `a`, CR and LF (so the content, not the memory alignment, moves
across the 16-byte block grid: blocks are cut from the slice start); lengths 15, 16, 17, 31, 32 and 33 ending in
CR; a CR in lane 15 followed by a non-LF, by LF and by CR LF; a block of only CR LF pairs; CR at the end of the file
with a length that is a multiple of 16; the ascii flag with 0x80 in lane 0 and in lane 15; 100,000 strings of
length 0 to 200 over `{a, LF, CR, 0x80, 0xff, 0x00}` from a fixed-seed generator; files that end in `\r`, `\r\n` or
`\n`; and the corpus. The suite runs on the aarch64 and Windows legs and says so (`wide`'s scalar branch is never
forced on a leg; only the leg's own target is covered). `LineIndex::line_col` and `offset` on the multi-byte and
astral lines the existing tests use, built through the new path, equal the old. The blank-run and
`first_non_ascii` helpers against their scalar definitions the same way.

**Measure.** Off callgrind of `LineIndex::new` before and after, and the main thread's share: 270.4 M over 19,211
builds in the 20 G profile (63.5 M for 1.68 M `memchr` calls, 83.4 M for the column tables, 116.3 M of its own
and 7.2 M unattributed); the ledger's later 363.4 M is over 25,753 builds, stub files included, with no
breakdown; the baseline is re-measured before the change. The estimate that the scan costs about one
instruction per byte and under ten per line is a model made for SSE2; no saving is claimed before the run.
**Accept and kill:** the kernel beats the `memchr` plus `is_ascii` version by at least 20% (proposed) in Ir and in a
criterion bench of `scan_lines` on x86-64, and is not slower on the aarch64 runners (macOS and Linux) or on
Windows (R16's method: a criterion bench on each leg, since instruction counts are not time), where `move_mask` is four
NEON instructions and a horizontal add and `memchr` itself avoids it. Otherwise `memchr` stays and `wide` is
dropped, or the same API is written over `u64` words (eight bytes at a time, the classic zero-byte test). There is
no scalar-fallback trigger: `wide` takes its scalar branch only where neither `sse2` nor `neon` is a target
feature, which x86-64 and aarch64 never are.

## I4. The single grammar's lowering (Part F 11)

F41 (`STAGE_B_FIX_REVIEW.md`), DOC-2.1 (the ledger) and C7's grammar item are one piece of work. (Finding numbers
are ambiguous across the two review files: F40, F41, F51, F61 and F63 exist in both with different content; this part
cites `STAGE_B_REVIEW` F40, the PHPDoc parsing one, and `STAGE_B_FIX_REVIEW` F41.) Read from `types/src/parse.rs`
(1,429 lines, outline and the keyword table), `phpdoc/src/grammar/ast.rs` (`TypeNode`, 19 variants),
`phpdoc/src/doc_type.rs` and `registry/type_node.rs`, and the call sites (58 mentions of `parse_written_type`
in 3 crates, 219 of `parse_type_string`, 156 of them in `types`' own tests).

**Today.** A PHPDoc type is parsed twice. `phpdoc::grammar` lexes and parses it into a `TypeNode` (and reports
`phpDoc.parseError`); then `DocType::ty` renders the node back to text (`to_source_string`) and
`types::parse_written_type` parses that text again with a second, string-splitting parser, memoized by the text
(2,048 entries in each of 64 shards: 131,072 entries and 64 MiB of text at most). Each of F41's bullets is a place
where one of the two gets the language wrong. `types::parse_display`
is a third reader, of a different language (a type's `Display`, which `assertType()` and the type dumps are written
in); it stays, being `Display`'s inverse.

### I4.1 Where it lives

- **The lowering is `phpdoc::lower`.** `phpdoc` depends on `types` and `syntax`; `types` depends on `syntax` only.
  F41 considered moving the grammar into a crate below `types` so `types` could keep a text entry point. It does
  not need one: outside its own tests `types` calls its parser only from `parse.rs` itself. So `types` loses its
  parser, as the crate split's §3.3 says ("`types::parse_type_string` deleted"), and the grammar stays where
  DOC-2.1 put it. No new crate.
- **The type knowledge stays in `types`:** the keyword table (`parse_type_keyword_or_class`, about 380 lines of
  `int-mask`, `class-string`, `non-empty-lowercase-string`, `key-of`, ...) becomes `types::phpdoc_keyword(word, args:
  &[Type]) -> Option<Type>`, taking lowered arguments where it takes argument text now, and the builders
  the string parser's `try_parse_array_shape`, `try_parse_object_shape` and `try_parse_callable_shape` use become
  public constructors (`Shape::from_fields`, `CallableSig::from_parts` and the like) that `lower` calls. The string
  splitters (`split_top_level`, `top_level_find`, `matching_close`, `try_parse_conditional_type`,
  `parse_type_atom`) are deleted. **The extraction also completes the table** against `TypeNodeResolver`'s
  identifier and generic case lists, with a fixture per keyword: the string parser lacks `number`,
  `empty-scalar`, `non-empty-scalar`, `non-empty-literal-string`, `associative-array`, `static-closure`,
  `static-pure-closure`, `non-empty-mixed`, `callable-object`, `callable-array`, `empty`, the `__string*` pair,
  and the generics `__benevolent`, `new` and `template-type` (checked in the vendored `phpstan-src`, which is
  past 2.2.16; a newer upstream was not fetched), so each reads as a class today, is namespace-qualified and
  reaches `class.notFound`. `number`, like `scalar` and `numeric`, yields to a class or a `use` alias of that
  name (PHPStan's `tryResolvePseudoTypeClassType`), the one scope-dependent part, which C7 does; the rest of
  C7's "PHPDoc keyword types resolve before namespace qualification" is done here, and leaves C7's and RES-1's
  lists.
- **Entry points** in `phpdoc` (one name across Part E 14, F41 and here): `lower(&TypeNode) -> Type` (names as
  written, a leading `\` kept: what `parse_written_type` returns), `parse_phpdoc_type(text) -> (Type,
  Vec<ParseError>)` (lex, parse, lower) and `parse_global_type(text)` (the old `parse_type_string`: leading `\`
  stripped). The text entry points return the errors, because a string a user supplied must not turn into the
  error type, which accepts and is accepted by everything, with no report: `checker/constant_resolver.rs`,
  `restricted_symbol_usage.rs` and `registry/assertion_table.rs` parse user-supplied texts today with no error
  channel, and those callers report. Fixed texts (`predefined_constants.rs` parses nine per call) are statics
  (a `LazyLock` table, as 25 G says), and the text memo goes unless a dynamic caller measures a need for it. An
  error node lowers to the error type; a tag with an error contributes nothing when PHPStan parses that tag
  family (a `phpstan-` prefixed tag, or one in `PLAIN_TAGS_PHPSTAN_PARSES`), and the others apply as they are
  (`docblock.rs`). A *missing-token* error whose recovery is unambiguous (a `callable` with no return type, I4.2)
  is not an error node: the grammar reports it and the lowering keeps what the author wrote, so the tag keeps the
  recovered type.
- **`lower` is a walk generic over a scope** (the identity scope gives `lower`), living in `phpdoc`, so that C7
  adds an implementation (names, templates, aliases and `self` resolved in the walk) and not a second walk. It is
  written against the node accessors that DOC-2.7 keeps (children by iterator), so DOC-2.5 and 2.7 change the
  node's storage and not `lower`; after C7 `DocType` keeps the node, and the resolved type is DOC-2.2's memo value.

### I4.2 The lowering, by `TypeNode` variant

| node | lowers to |
|---|---|
| `Identifier(w)` | `phpdoc_keyword(w, [])`, else the class of that name as written; `$this`, `self`, `static`, `parent` as now |
| `Nullable(t)`, `Union`, `Intersection`, `Paren` | the normalizing constructors (`Type::union_of`, `intersect_types`); `?T` is the normalizing union of `T` and `null` (the string parser builds a raw nested union, a delta: flattened), and `?mixed` is `mixed` (tested by `is_mixed()`); **the written width is bounded:** `union_types_many` flattens every operand with `push_member`, a linear scan per member, and runs its passes before `MAX_UNION_MEMBERS` applies, so a written union of n distinct members costs at least n²/2 comparisons, and `eager` `DocType` runs the lowering for every docblock read; the lowering caps the written width at a multiple of the cap and generalizes as the cap does (or dedupes through a hash set before the passes), with a 50,000-member union, a 100,000-deep nest and colliding-name inputs (`FxHasher` does not resist collisions) in I4.5 step 3's inputs and S-1's |
| `Generic(w, args)` | `phpdoc_keyword(w, lowered args)`, else a generic class reference; `int<a, b>` is a range only for the exact word `int` (the string parser tests `ends_with("int")`, so `Point<1, 2>` becomes a range today, a delta); an argument of a keyword generic is lowered as the type it is, and a wildcard or a variance marker there is accepted as PHPStan does (ruled 2026-10-18): phpdoc-parser turns `*` into `mixed` with bivariant variance and PHPStan's `array`, `list` and the other keyword generics ignore variances, so `array<*, int>` is `array<mixed, int>`; only the class arm used them (`parse_generic_arg`), and today a keyword argument that holds one fails, a delta |
| `Wildcard`, `VarianceArg` | for a class generic as `parse_generic_arg` does: any type; call-site variance |
| `ArrayOf(t)`, `OffsetAccess(b, o)` | `array<t>` (list-ness as the string parser gives it today), the offset-access type with any type as the offset (the string parser read it as a shape key by name, a delta) |
| `ArrayShape { keyword, fields, sealed, unsealed_type }` | the shape constructors with `list`, `non-empty` and `object` refinements kept (`STAGE_B_REVIEW` F40) |
| `Callable { keyword, templates, params, return_type }` | `CallableSig` with its own templates marked; **without a return type** (`callable(int)`) the grammar records a PHPDoc syntax error for the missing return type, as phpdoc-parser and PHPStan do (the colon is mandatory there, and the port's `None` branch carries no note that its acceptance is deliberate), and the node is lowered as written, `callable(int): mixed`, so the tag keeps the parameters the author wrote, where the string parser's fallback, a bare `callable`, discarded them (ruled 2026-10-18; the missing-token recovery of G3.2 item 1, applied to PHPDoc); `Closure(...)` is the intersection of `\Closure` and the callable (`parse.rs`), not the bare class |
| `IntRange`, `IntLiteral`, `FloatLiteral`, `StringLiteral` | the literal types; a float only from a `FloatLiteral` token |
| `ConstFetch { class, const_name }` | the constant-fetch type, a trailing `*` kept; `Foo::class` is `class-string<Foo>` (`Type::class_string`, as the string parser has it) |
| `Conditional { subject, negated, target, if_true, if_false }` | `ConditionalSubject` and the three lowered branches (each branch is the node the grammar built) |
| `Error` | the error type |

The explicit `mixed` of a node is the kind F21 gives (`Mixed(Explicit)`), set at the keyword by `phpdoc_keyword`, as
the string parser already does at `Type::explicit_mixed()`; the text search for explicit `mixed` that C7 mentions is
gone (a70a5b42). Template and alias references are marked by the same later passes as now, until C7's resolver
takes the name scope.

### I4.3 What changes in behaviour (F41, with F19 and F40's grammar items)

`lex_number` follows phpdoc-parser's lexemes (`Lexer.php`, read at 2.3.5; the latest release was not checked): an
optional sign, `0b`, `0o` and `0x` integers and plain decimal digit runs, all with `_` separators and
case-insensitive (`0X1F`, `1E5`, `0B1`), and floats with a fraction or exponent, including a trailing dot (`1.`).
There is no leading-`0` octal in phpdoc-parser (`017` is a plain digit run). The values follow PHP's source
reading and deliberately differ from PHPStan's `(int)` and `(float)` casts of the lexeme (php 8.5.4: `0x1F` reads
31, `017` reads 15 and `0b11` reads 3, where `(int)"0x1F"` is 0 and `(int)"017"` is 17); an integer past `i64` is a
float, as PHP reads such a literal, but `-9223372036854775808` is the int `PHP_INT_MIN` (ruled 2026-10-18): the
sign is part of the PHPDoc lexeme, where in PHP source the literal overflows before the minus applies (php 8.5.4:
the source literal is `float(-9.2233720368547758E+18)` and `(int)"-9223372036854775808"` is
`int(-9223372036854775808)`), and a float would make `@return -9223372036854775808` reject `PHP_INT_MIN`.
Consequences, each with a fixture: a class named `Infinity`, `Nan` or `inf` is a class
(it was `INF` or `NAN`); `@param 1_000 $u` and `0x1F` parse (they gave `phpDoc.parseError`); `1e25` is `1.0E+25`; a
conditional's branches may hold `\Closure(): int`, `self::A` and `?T` (the first-`:` split gave `class.notFound`).
The string parser's other known errors: `value-of` and `key-of` of a list (parsing saw only a bare array or
shape; the lowering lowers the argument as a type, so `value-of<list<int>>` is `int`), and the differences listed
in I4.2 (each a delta with a fixture, approved on 2026-10-18 with M5b's PHPDoc group; the consequences above are
plain bug fixes). `numeric` and `non-decimal-int-string` already have their right types in
`parse.rs` at this commit, and a bare `class-string` stays a plain `string` by design (the comment there says it
has nothing to preserve); what needs the name scope is C7's. `TYPE-5` is not part of this: it is its own commit
after step 5 (below).

### I4.4 The consumers

- **`DocType`** holds the node and the lowered `Type` made when the docblock is read (eager, DOC-2.1): the `OnceLock`
  and the text memo go, and `WrittenType` for a bare `TypeNode` lowers it.
- **DOC-2.2's memo key** needs no node identity. Written types are hash-consed, so the lowered `Type`'s node is the
  identity of the written type (two docblocks that write `array<int, Foo>` share it), and the resolution context is
  interned (PROD-5.2's `ImportScope`, the template names, the aliases, the `self` names). The key is the pair
  (the interned `Type` itself, which the entry holds so its address cannot be reused, and the interned context's
  id): equality is pointer identity on exact forms and costs 8 bytes, and the value is the whole resolved type.
  The first draft keyed a 128-bit fingerprint of the node's `exact` hash; that is 64 bits of `FxHasher` from the
  node, so a collision would hand back another type's resolution with no equality check, and `Node.exact` is
  `pub(crate)`. If DOC-2.8's fingerprint is wanted it is a 128-bit hash over the node's structure behind a public
  `Type` API. A test: `\StdClass` and `\stdClass`, `a|b` and `b|a`, and explicit and implicit `mixed` get
  different keys.
- **The 58 `parse_written_type` and the Rust-side `parse_type_string` sites** (about 20 files of `phpstan-rs`) become
  `phpdoc::parse_phpdoc_type` / `parse_global_type` where the type is written as text for a reason (a builtin's
  return rule, stub synthesis; `predefined_constants.rs` takes statics), and constructors where it is a literal
  `Type::int()` in disguise.
- **`types`' 156 test mentions:** a test that reads a type from text moves to `phpdoc/tests`, where it can use the
  grammar and `Type`'s equality; a test of the relation, a union or a shape that only needs a type uses constructors
  (the crate split's "about 150"). The relation tests keep their text form in the crate that has a grammar.

### I4.5 Order and checks

1. The lexemes: `lex_number` and the quoted-literal lexeme, with their grammar tests, one for each lexeme. The
   string lexeme is a step-1 change and not C7's, since it needs no name scope: a backslash escapes any following
   character inside the token (phpdoc-parser's lexer pairs a backslash with any non-newline character; ours
   unescapes only backslash-plus-the-quote, so a literal ending in a backslash pair swallows its closing quote),
   the unescape rules are phpdoc-parser's (`\\` and `\'` single-quoted; `\\ n r t f v e`, `\xHH`, octal and
   `\u{..}` double-quoted), and `TypeNode::StringLiteral` and the shape key carry bytes (`PhpString`), not `String`.
   Fixtures: the one-backslash literal in a union, an escaped quote, a double-quoted escape, a shape key with an
   escape.
2. `types::phpdoc_keyword` and the public shape and callable constructors, extracted with the string parser still
   calling them (and the keyword table completed: I4.1).
3. `phpdoc::lower`. Its baseline is VAL-1.7's PHPDoc record (batch 0), compared through VAL-1.4's canonical
   printer, so the comparison is exact and the string parser need not stay alive; the per-`TypeNode` comparison
   with `parse_written_type(node.to_source_string())` over the stubs, Piwigo, the vendor projects and the fixtures
   runs for one stage only if that record cannot see what it finds (one of the policy's listed exceptions, Part E
   13). Either way the differences of I4.2 and I4.3 are the list the comparison starts from, and each new
   difference is read and added to it with a fixture.
4. `DocType` eager; the callers converted module by module; DOC-2.2 on the memo key.
5. The string parser, its memo and `to_source_string`'s round-trip use deleted; `types::parse` is gone but for
   `parse_display`. `registry/type_node.rs` (`parse_type_node`) becomes `phpdoc::lower`.
6. TYPE-5, **its own commit after step 5** (batch 6), once template and alias references are marked: it is the
   ledger's rule (the array member takes `K` as `array<K, V>` does, and a float or object key leaves only the empty
   array), with fixtures for a template key, an alias key and a class key, because `K` is a template, an alias or
   a class name when it is lowered and only the resolution step knows. F14's design text, which restricts `K` in
   the iterable arm and drops the array member when the intersection with `int` or `string` is never, is
   superseded by that rule; never inside `phpdoc_keyword`, which stays behaviour-exact.

Oracle: O2 and `--dump-types` over the corpus, with I4.2's and I4.3's differences read. Exact otherwise.

**C7 later** adds the name scope: `TypeNodeResolver` is an implementation of the scope that `lower` is generic over
(I4.1), calling the same `phpdoc_keyword` and shape builders with the resolution of names, templates, aliases and
`self` done in the walk, and `phpdoc::lower` is its context-free case; no second walk is written. C7 also keeps
the pieces of the crate split's C7 text that no row names: `TypeNodeResolverExtension` (the extension point,
registered through the `Extensions` input of J2) and the assert targets (`$param->prop`, `$param->method()` and
`$this->method()` are kept), `@throws` and assert types getting alias substitution, and a written `Traversable`
resolving like any name (J2's RES rows).

## I5. The rule message type (Part F 12)

CHECK-6.10 (messages formatted only when reported, 37 rules), CHECK-4.1 (a message kept unformatted past the
report) and the crate split's D6 (`RuleError`, `RuleErrorBuilder`) need one type. Read from `Checker::report`
(`checker.rs:1507`), `report_builder`, `rewind_to` (`checker.rs:1295`), `reports_are_dropped` and
`diagnostics::DiagnosticBuilder`.

**Today.** `report(message: String, span, level, identifier)` has about 440 call sites in `checker/` (and 8 in
`project/mock_hygiene.rs`) and `report_builder(DiagnosticBuilder)` 113; a message is `format!`ed at the call, so a
type is displayed even when the report is dropped by `rewind_to` (a
loop's second pass, 30 of the 55 `offsetAccess.mightNotExist` messages, about 409 K Ir each), by
`reports_are_dropped()` (after the string is built), or by the level filter (22 more, which E5 removes by not
running a rule above the level). Several rules format a subject for every element they check, reported or not
(`phpdoc_type_compat.rs:359`, 21,919 of them). `report` builds the `Diagnostic` at once, converting the span to a
line and column with the file's `LineIndex`, and pushes it; `rewind_to` truncates the vector. **Not everything
pushed is a `report` call, and not every drop is before the check ends:** `check_trait_methods` checks a trait
once per host, then sorts its own slice of the diagnostics by (span, message), dedups by span and message and
retains by identifier and span (`function_decl.rs`, all before the check ends); the rules' `check_stmt` and
`check_expr` return `Vec<Diagnostic>` that the checker extends (`stmt.rs`, `expr.rs`);
`check_too_wide_property_types` and eleven `self.diagnostics.push(DiagnosticBuilder::new(..).build(self.line_index))`
in `phpstan_api.rs` push ready-made `Diagnostic`s; and `sort_diagnostics` (in `diagnostics`) breaks ties by the
rendered message.

**The type.** One `Message`, with `Own` in `syntax` (the impl for `Name`), the impl for `Type` in `types`, and
`Message`, `message!` and `RuleError` in `diagnostics`, which depends on `syntax` alone and so can name `Own` and
`Name`; a `Message` is a boxed closure and names neither `Type` nor `Name`, only the `Own` impls do. That keeps
`RuleError` where the crate split's 3.7 puts it (`diagnostics`).

```rust
pub struct Message(Repr);
enum Repr { Text(String), Deferred(Box<dyn FnOnce() -> String>) }
impl Message {
    pub fn from_static(s: &'static str) -> Message;  pub fn text(s: String) -> Message;   // the only constructors
    pub fn render(self) -> String;  pub fn rendered(&mut self) -> &str                   // rendered: caches into Text, for the dedup and the sort's tie-break
}
// interim, steps (1) to (3) only: impl From<String> for Message {}   (deleted in step 4)
trait Own { type Owned: Display + Debug + 'static; fn own(self) -> Self::Owned }       // Type and Name by clone, &str by Arc<str>, numbers by copy, a Display newtype
message!("Parameter #{} ${} of {} expects {}, {} given.", i, &name, &func, &expected, &given)   // deferred; each argument owned once
message!("Offset {dim} might not exist on {container}.")                              // captures too
```

- `message!` is a proc-macro in a small `phpstan-rs-macros` crate (a new crate: Part E 32's file set), because a
  `macro_rules!` cannot number its arguments and 464 of the checker's 849 `format!` lines use inline captures
  (`{dim}`, `{key:?}`). It parses the literal, owns every positional and captured argument through `Own` (once,
  at the call: `Type` and `Name` clones are refcount increments), accepts `{x:?}` and width specs (so
  `Own::Owned: Debug`), and defers the `format!`; its dependencies (`syn`, `quote`, `proc-macro2`) are judged on
  merit like the others, and the build-time gate is checked. A message built from text stays `Text`: an
  external rule's `RuleErrorBuilder::message("...")` costs what it does now.
- **Pending reports.** The checker's `diagnostics` vector holds `Entry::Pending(Pending { message, tips:
  Vec<Message>, span, level, identifier, non_ignorable, fix })` or `Entry::Done(Diagnostic)` (the ready-made
  ones above, until D6 retires the external splices); `report` pushes a `Pending`; `rewind_to` truncates, so a
  dropped report is never formatted and never converted to a line and column; `into_diagnostics()` renders the
  survivors and builds their ranges, then sorts. So "displayed" means "the file's check ended", which is after
  every drop the checker makes **except the trait path**: `check_trait_methods` renders its slice's messages
  (`rendered`) before it dedups, because the dedup needs message equality, so a trait method costs hosts ×
  findings renders. `Message` has no `PartialEq`; `rendered` is the only way to compare.
- **Tips and fixes** are `Message`s too (a tip is a message; a fix's description is). A fix's replacement text,
  which is code and not prose, stays a `String`.
- **Identifiers** are `&'static str` (CHECK-6.10's "a closed set"): the sites that build one with `format!` are
  about 52 (43 `report(` calls and 11 `DiagnosticBuilder::new(` calls, 21 files, e.g. `duplicates.rs`'s
  `"{kind}.duplicateEnumCase"`), and they become members of the families the batch builds: a const table, kind ×
  suffix, debug-asserted against the closed set, in step (3), so the sites are static now and D6 adopts it;
  `Identifier` keeps its `Cow` for rules outside the package.
- **`RuleError`** (D6) is a `Pending` without the file: `RuleErrorBuilder::message(impl Into<RuleMessage>)`,
  `.identifier(...)`, `.tip(...)`, `.non_ignorable()`, `.fix(...)`, where `RuleMessage: From<String> +
  From<Message>`, so an extension's string still works while the checker's own methods take `Message` only. The
  dispatcher turns each into a `Pending` of the checker it runs in (or straight into a `Diagnostic` where no
  checker is), attaching the file and the trait file.
- **The engine boundary.** `FileCheck.diagnostics` holds rendered `Diagnostic`s, exact values that compare and
  persist (H1.5b, H4.6); a `Message` never crosses it. A warm run that reads a persisted check renders nothing.

**Order.** (1) `Message`, `Own`, `message!`, `Entry`, `report` taking `impl Into<Message>`, `into_diagnostics`
rendering; the interim is `report`'s `impl Into<Message>` signature and `impl From<String> for Message`, so every
existing call site compiles; the trait path's `rendered` and the `Entry::Done` pushes come here too. (2)
`report_missing_offset` (CHECK-4.1's measured site). (3) CHECK-6.10's 37 rules, one commit each family, in two
kinds: a site whose element is rarely reported carries borrowed parts or `Name`/`Type` newtypes (a lazy `Display`
subject, as 20 G designs it) to the report and formats there, because `message!` allocates a `Box` and an
`Arc<str>` on the kept path, and only a site that reports and then drops uses `message!`; kept-path allocations
are measured before and after in the check phase's callgrind, as the plan already promises for the scalar-only
sites. The identifier families (a const table) land here too. (4) Then every other site, about 440 of `report`
and 113 of `report_builder`: the checker's own methods stop accepting a `String` (they take `Message`, and the
interim `From<String>` is deleted), so the compiler names each remaining site and none is left to be converted
someday. A literal becomes `Message::from_static`, anything with arguments `message!`, and `Message::text` is the
explicit eager form: it is in `disallowed-methods` (a per-package list in `crates/phpstan-rs/clippy.toml`, so
package-wide until the split makes `checker/` a crate, and the 20 or more existing
`allow(clippy::disallowed_methods)` sites show how far that list already reaches), and each use carries
`#[expect(clippy::disallowed_methods, reason = "...")]` that an `xtask` counts, so the count ignores the older
allows. The end state is measured (callgrind of the check phase, O2): a net regression on the reports that are
kept would send the scalar-only sites back to `Message::text` by that explicit, counted `expect`. D6 builds
`RuleError` on the same `Message` when it is reached; the rule API takes `RuleMessage`, so an extension's string
costs what it does now.

**Checks.** `message!` renders as `format!` for each `Own` impl (integers, `&str`, `String`, `Type`, `Name`, a
`Display` newtype, a Debug capture); a dropped deferred message is never rendered (a test-only counter in the
closure); `rewind_to` then `into_diagnostics` gives the diagnostics of the kept passes only; a test of the trait
path (hosts × findings, the dedup by `rendered`); a targeted test per family between conversion commits, and O2
(every message and tip byte-identical) once, at the end of batch 7 (the user's cadence: validation at the end of
the item, never per commit). **Estimate, unmeasured:** CHECK-4.1's 22.5 M is for all 55 `offsetAccess` formats; the
22 the level filter drops are dropped in the finalizer, after the checker returned, and E5 removes them by not
running the rule, so this step saves about 13.5 M (33 messages, labelled an estimate) and the other 9 M waits for
E5; the 37 rules' temporaries (21,919 and 46,005 allocations are two of them); nothing is claimed before a run.

---
