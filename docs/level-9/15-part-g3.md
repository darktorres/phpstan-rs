## G3. The error-node model and parse recovery (Part F 3, 9)

## Tasks this part owns

None. G3 is a design: the error-node model (G3.1 to G3.5), the contract for readers (G3.6), and the oracles (G3.7). The rows its design serves are owned where they are built: SYN-1.9, Q-10 and SYN-6 by Part C's batch 3 (C1), CHECK-12 by Part D's wall step W7, and CHECK-13 by Part H1 (H1.4).

### G3 why now

Decision 4 says every parse yields a tree; Q-10 and decision 15 say what a failing file
keeps and how its version range is read; F77 says the failure is reported. Step 2 fixes the stored
layout and the builder's API, and batch 3 builds the recovery on them, so the model comes first.

### G3 what exists (verified)

- `parse` returns `Result<ParsedFile, ParseError>`; `ParseError { message, span }` is built at 28
  sites (31 lines name `ParseError {`; the type and its two impls are the other three), and the `expect*`
  helpers in `parser/tokens.rs` (`expect`, `expect_semicolon`, `expect_ident`, `expect_variable`) are the
  common path (`expect_variable` takes the offending token before it fails, so it peeks first once
  recovery needs "an `expect` that fails consumes nothing"); the parser uses `?` throughout and has no
  speculative parse-and-retry (a search for rewinds finds only the body skim's token pushback), so
  recovery needs no node rollback.
- The lexer has `LexWarning { span, message, is_error }` for what it skipped or what PHP refuses; only
  three sites set `is_error` today (a numeric literal, an octal digit, a `\u{}` escape). An unterminated
  string, comment or heredoc records nothing (an unterminated heredoc takes the rest of the file as its
  body; an unterminated single-quoted string ends one byte past the source), and a NUL byte in code is
  lexed as `Token::Eof`, which drops the rest of the file silently where PHP 8.5.4 reports "unexpected
  character 0x00". Those are new lexer errors, not existing ones.
- A load that fails to parse is `Read::Unparsed(ParseFailure { line, col, message })` and the file
  contributes nothing (F77). A body error in a context file is invisible because bodies are skimmed.
- `types::php_version` has `PhpVersion`, `PhpVersions { min, max }`, `PhpFeature` (45 variants) with
  `is_available(v)` (including features removed at a version), `KNOWN_PHP_VERSIONS` (5.3 to 8.5) and
  `known_php_versions_where` (which returns the hull `min..max`, not a set); the registry has
  `php_versions` and its methods `php_supports_everywhere`, `php_supports_somewhere` and
  `php_version_consensus` (`registry.rs`), which the checker calls.
- `docker` is available, and `tools/php-comparison-matrix` is the precedent: a PHP script run on
  official images whose JSON is committed as test data (`crates/types/src/tests/data/`).

### G3.1 Contract

`syntax::parse(bytes) -> Parsed` is infallible. `Parsed` holds the `Ast`, whose
`errors() -> &[SyntaxError]` and `marks() -> &[Mark]` are sorted by position, and the file facts
with `has_errors`. For every byte string: it returns within a work bound proportional to the input and
within a memory bound (tested); it does not panic; every span is inside the file and nested in its
parent's; the tokens plus the spans in `errors()` cover the file (coverage is defined over those spans, not
over nodes, since a failed parameter, `use` item, catch type or class member is an entry in `errors()` and
not a node). **The memory bound** is a `NestingTooDeep` error kind at a fixed depth near PHP's (php 8.5.4
evaluates 9,000 nested `[` and fails at 10,000 and 12,000 with "memory exhausted", as at 40,000 parentheses):
the guard (`syntax::stack`) turns a stack overflow into heap growth in 16 MiB segments without limit, so
without a bound a file of tens of megabytes of `(` would cost tens of gigabytes; a bound near PHP's is both
runtime truth and the resource bound, tested at the boundary. A source over 2^28 bytes is refused by the loader
before `parse` (B5), so the id and offset limits cannot fire inside it. `ParseError` becomes `SyntaxError {
span, kind: ErrorKind, detail: Text }`; the message is built from the kind when shown, in PHP's wording
(`syntax error, unexpected token ";", expecting "]"`), so no per-error string lives in the tree.

### G3.2 Three kinds of failure, and one thing that is not a failure

1. *Missing.* A required token or operand is absent and the next token cannot continue the
   construct. The parser records the error at the position and builds the node as if the token were
   there: a call without `)`, a block without `}`, a statement without `;`, an operator without its
   right operand (a zero-width `ExprKind::Error`). The tree keeps its regular shape.
2. *Unexpected.* Tokens that cannot belong to the construct are consumed into an error node covering
   them: `StmtKind::Error`, `ExprKind::Error`, a `TypeHint::Error`. An error node is a leaf with an
   `ErrorId` (an index into `errors()`); it keeps no children. What the parser finished before failing is
   already a node of the enclosing construct (item 1), so nothing needs salvaging. **A class has no member
   enum** (its members are five per-kind lists, I1.4), so a failed member, and likewise a failed parameter,
   `use` item or catch type (records, not expression slots), is an entry in `errors()` found by span, and
   the owner's `incomplete` bit (in the flags word of `ClassNode`, `FunctionNode`, `ClosureNode`,
   `ArrowFnNode` and `PropertyNode`, which exist) says the declaration lost something; no field is added
   and no asserted size moves.
3. *Lexical.* Unterminated strings, comments and heredocs, `08`, an invalid escape PHP refuses, a heredoc
   body line indented less than its closing marker or mixing tabs and spaces there (PHP's "Invalid body
   indentation level"; `strip_heredoc_indent` cuts silently today), and, from PHP 8.0, a NUL byte or a stray
   control byte (item 4: a validity mark, not a `SyntaxError`). The lexer records a `SyntaxError` and yields a token the parser
   treats as a literal operand, so the parse continues; a NUL becomes an `Error` token like any stray
   control byte (the lexer returns `Eof` only at the real end), and an unterminated string's span is clamped
   to the source. Of these only the numeric literals, the octal digit and the `\u{}` escape are
   `LexWarning`s with `is_error` today; the others are new lexer errors (G3.8). The warnings that are not
   errors (skipped bytes before 8.0) stay warnings.
4. *Not a failure: syntax some version accepts.* The parser accepts every version's syntax and
   records a `Mark` (G3.5). Only syntax that no version accepts is an error (SYN-6): `(void)` as an
   expression and `function exit` in user code. **A stray control byte, a NUL byte included, is a validity mark,**
   not an error node: valid through 7.4 with a warning and a skip (the ledger's SYN-6 and Q-2; the syntax matrix
   confirms NUL itself on the 7.x images, which no binary here can), refused from 8.0 (php 8.5.4: "unexpected
   character 0x00"), which bounds a file from above at 8.0 (decision 15) and keeps the oracle's rule of G3.7.1
   without an exception.

### G3.3 Layout, for step 2

Three enum arms, each with a payload of one `ErrorId` (4 bytes, inside
the 32-byte node budget), added with SYN-1.9: `ExprRepr::Error`, `StmtRepr::Error` and `HintRepr::Error`
(no `MemberRepr`: item 2 of G3.2 says where a class records its errors). `Ast` gains `errors:
Vec<SyntaxError>` and `marks: Vec<Mark>`. Step 2 does not build them: the parse functions keep
`Result` and the first failure ends the parse, so that step 2 is checked by dump equality; batch 3
replaces each `?` with the recovery below. What step 2 must guarantee is only that the arms fit: an enum
is as wide as its largest arm, and the largest arms of `ExprRepr` and `StmtRepr` are 20 and 28 bytes
(I1.2, I1.3), so a 4-byte payload relays out nothing; `HintNode` has room in its 20 bytes.

### G3.4 The recovery (batch 3)

A parse function returns its node, never a `Result`.
- *Progress, as a mechanism.* Every recovery loop records its position when it starts an iteration and,
  if the position has not moved when it comes round again, consumes one token as unexpected; the `expect*`
  helpers peek before they fail (so "an `expect` that fails consumes nothing" is true of all four); an
  error node consumes at least one token unless the token ends an enclosing construct (then it is
  zero-width). A step budget proportional to the token count ends the parse by turning the rest into one
  `Error` statement, so termination does not rest on an argument.
- *Suppression.* After an error at position p, another *missing* error is not recorded until a token
  past p is consumed, so one mistake gives one report; unexpected-token errors always record. At a
  bound (200 to start with) the parser stops recording, takes the rest of the file as one
  `StmtKind::Error` and ends; ordinary damage never reaches it (a test, G3.7.3).
- *Delimiters.* A stack of open delimiters (`(`, `[`, `{`, `${`, `#[`, alternative-syntax openers).
  Skipping never passes the closer of an enclosing construct; **a closer with no opener** (a stray `}` at
  the top level included) **is one unexpected token, consumed as a one-token error statement,** so it can
  never be a statement that consumes nothing; a delimiter left open at EOF is reported once, at its
  opener ("unclosed `{`"), and closed implicitly.
- *Statement level.* On a statement that cannot start or continue: consume tokens into a
  `StmtKind::Error` until a `;` at relative depth 0 (consumed), a `}` that closes an enclosing construct
  (not consumed), `?>` or an inline-HTML token, or a token that can only start a statement at depth 0
  and is not preceded by `->`, `?->`, `::` or `new` and is not inside a `function (...)` header
  (`if`, `while`, `for`, `foreach`, `switch`, `try`, `return`, `echo`, `class`, `interface`, `trait`,
  `const`, `declare`, `global`, `unset`, `goto`, `break`, `continue`, `abstract`, `final`, and `use`
  only at the start of a statement). Not stoppers: `function`, `fn`, `static`, `new`, `clone`, `print`,
  `include`, `yield`, `throw`, `match`, which can begin an expression, and `namespace`, `enum` and `#[`,
  which can too (`namespace\foo();`, `enum();`, `#[A] fn() => 1;`) and stop only when the tokens after them
  complete a declaration, as `stmt.rs` decides. Every keyword above is valid as a member name after `->` or
  `::` (`$o->use`, `$o->if()`, `Foo::while()`) and `class` follows `::` and `new`, hence the exclusions; one
  golden dump per excluded token.
- *Expression level.* A primary that cannot start: if the token is one that ends an expression
  (`)` `]` `}` `;` `,` `=>` `:` `?>` EOF) the error node is zero-width and nothing is consumed;
  otherwise it consumes that token. A binary operator with no right operand gets a zero-width error
  operand. (The rule is by the token's class alone; the statement and delimiter rules above are what keep a
  zero-width node from being repeated.)
- *Lists* (arguments, array items, parameters, `use` lists, `catch` types, match arms): after an
  item expect `,` or the closer. If neither: a token that can only begin a statement ends the list with one
  missing-closer error, and in an argument, parameter or `use` list so does a token that begins a new line
  after a complete item (`foo(1, 2` followed by `$y = 3;` is one error, as in PHP, and `$y = 3` is the next
  statement, not a third argument); in an array or `match`, whose items often sit one to a line, a token that
  can start an item is a missing `,` and the item is parsed; anything else is skipped as unexpected until
  `,`, the closer, or a statement-level stopper at depth 0. The golden dumps and the quality oracle of
  G3.7.3 pin the choice.
- *Class bodies.* A member that fails is an entry in `errors()` covering the tokens to the next member
  start (`public`, `protected`, `private`, `static`, `abstract`, `final`, `readonly`, `const`,
  `function`, `use`, `case`, `var`, `#[`), a `;` (consumed) or the closing `}` (not consumed), and sets the
  class's `incomplete` bit (G3.2).
- *Headers.* A function or class whose name is missing keeps the declaration with an empty
  zero-width name and the error, then skips to its `(`, `{` or `;`; every reader treats such a name as
  unknown (G3.6). A declaration that parsed its name is always a declaration, so references to it bind.
- *Everything else* (strings with interpolation, heredocs, alternative syntax, `match`, closures)
  follows the same four rules; each construct's sync set is a table next to its parse function, and
  the tests of G3.7 cover each row.

### G3.5 Marks: what a file used that a version lacks (decision 15)

`Mark { feature:
SyntaxFeature, span: Span, detail: MarkDetail }`, recorded by the parser where the grammar uses a
feature, deduplicated by span, sorted. `SyntaxFeature` is the grammar-only subset of `types::PhpFeature`'s
45 variants plus those below, defined in a leaf crate both `syntax` and `types` depend on (`php-version`:
`PhpVersion`, `PhpVersions`, `VersionSet`, `Availability`, `SyntaxFeature` with its one threshold table,
`KNOWN_PHP_VERSIONS`; Part E 32), because the parser is below `types` in the graph; `PhpFeature` in `types`
delegates to it for those variants and keeps the library gates that are not the parser's
(`PrintfHhSpecifier`, `FilterThrowOnFailureConstant`, `TentativeReturnTypes`, …). A file's validity is the
`VersionSet` of the known versions every mark accepts (`known_php_versions_where` returns a hull, not a set),
intersected with the declared range; an analysed file is reported at each mark a version of its range
refuses, a context file is not (decision 15), **and neither is a file that every load reaches only through
an include a guard covers when no autoloader maps it**, which also does not narrow the range (decisions 15
and 18). The guards are evaluated over the range being computed, so `effective_range` (J1.2) computes the
range with such files left out and then checks the guards against the result; when the analysed files share
no version the whole range is read and the conflict is reported. The mark count (every use of a feature
from 7.1 on is a mark in every `Ast`) is added to step 0's census, and marks are stored per feature with
a first span and a count if the census says the volume matters.

A mark is not only a feature with a threshold, because some syntax changes meaning instead of
becoming invalid, and some is deprecated and still valid. `MarkDetail` carries what the answer needs, and
`accepts(&self, v)` takes it. **There are two kinds of mark:** *validity marks* bound a file's range, and
*deprecation marks* never do (they are read by the existing deprecation rules).
- *Attributes* (`#[`): before 8.0 `#` starts a comment to the end of the line. A group is
  `comment_safe` when it lies on one line and nothing but whitespace follows it; only an unsafe group
  bounds validity from below. (`symfony/polyfill-*` and others rely on single-line groups.)
- *Reserved words as names* (`match`, `enum` in some positions, `readonly`, `fn`, `mixed`, `never`,
  `static`…): valid as an identifier below the version that reserves them, invalid from it, so such a
  use bounds validity from above.
- *Removed syntax* (`(real)`, `(unset)`, `$s{0}`, a stray control byte from 8.0): bounds from
  above; `(void)` bounds a statement use to 8.5 and an expression use to nothing (an error).
- *Deprecated, still valid* (the backtick operator from 8.5, which php 8.5.4 compiles and runs with a
  notice; `"${var}"` from 8.2): deprecation marks, read by `PhpFeature::DeprecatedBacktickOperator` and the
  like. Decision 15 bounds a file from above only by syntax a version removed, so these never bound it.

The inventory starts from `PhpFeature`'s syntax variants (non-capturing catch, throw expression,
promoted properties, the `(unset)` cast, final constants, readonly
classes and properties, constants in traits, typed class constants, named arguments, union types,
pure intersection types, first-class callables, property hooks, asymmetric visibility, dynamic class
constant fetch, standalone `null`/`false`/`true`, DNF types, `(void)`, attributes on global
constants, final promoted properties; array unpacking with string keys is not here, since its validity
depends on the unpacked array's keys and not on syntax) and adds the 7.x set: nullable types and `void` (7.1),
`iterable` (7.1), class constant visibility (7.1), short `list` syntax with keys and references
(7.1, 7.3), multi-catch (7.1), `object` (7.2), trailing comma in calls (7.3), flexible heredoc
(7.3), typed properties (7.4), arrow functions (7.4), `??=` (7.4), array spread (7.4), numeric
separators (7.4), `static`/`mixed` types and `?->` (8.0), `match` (8.0), trailing comma in
parameters (8.0), enums and `never` and `0o` octals (8.1), the pipe operator (8.5). The list is provisional:
**the matrix of G3.7 is its authority**, and a row is not final until the
images agree with it.

### G3.6 What readers do with an error (the contract)

- *Checker.* `ExprKind::Error` infers the error type (F21's `Mixed(Error)`, which lands in batch 0: every "is this unknown" guard already covers it, and it records that a report was made), narrows nothing, reports nothing. **What a statement error sets:** `any_variable_may_exist`
  (set by `extract()`, `eval` and `include`) is read today only at `builtin_calls.rs:826`, and the diagnostics
  `extract()` and `eval` discard are dropped by `saw_dynamic_var_construct`, which drops every diagnostic of
  the enclosing body (a loss for an editor). So a statement error sets `any_variable_may_exist`, and
  `variable.undefined` honours it: a read of a variable the swallowed region may have assigned reports
  nothing, which also changes `include` (it sets the flag and still reports such a read today), a delta for
  VAL-2.10 with its fixture; completion falls through. This is PHPStan's own rule (`canAnyVariableExist()` is true
  at global scope and after an include or `extract()`: `afterExtractCall`), and the stored Piwigo baselines hold no
  `variable.undefined`, so the oracle should not move. **`extract()` is made precise in the same commit:** every variable in scope becomes `mixed` and any may exist, and nothing else is dropped, where
  `saw_dynamic_var_construct` truncates every diagnostic of the body (`function_decl.rs`), the ones before the call
  and the ones unrelated to variables included; the two detections also disagree (`\extract(...)` and
  `EXTRACT(...)` set the flag but not the truncation), so one test on the resolved name serves both. `eval` keeps
  the truncation, since evaluated code can also declare functions and classes. **Exact include modelling is
  CHECK-12 (Stage D, on the scope model of D1 and D2):** the included file's top-level assignments in the
  includer's scope, and its return value as the type of the include (`mixed` today); `any_variable_may_exist` is
  its fallback for an include that cannot be resolved, so the flag is not interim. **A missing name** (the commonest missing token in an
  editor is a member name) is a zero-width name that every reader treats as unknown: no `property.notFound`,
  `method.notFound` or `class.notFound` for it, and the probe hit is still recorded, with a test per access
  kind. **The `Error` arms are added to the view enums and to a reader audit:** an `xtask arch` pass lists
  the matches that name `ExprKind::` or `StmtKind::` and end in a wildcard (`infer_expr_unguarded` ends
  `_ => Type::mixed()`, which would give implicit mixed, not `Mixed(Error)`), each classified once in the
  commit that adds the variants. An error node is a leaf, so nothing is checked inside it; a node built around
  a missing token is checked as written. For a file with errors the opt-in dead-code, unused and
  whole-program rules are not run (they would read a hole as absence).
- *Declarations.* A class whose `incomplete` bit is set (a failed member, G3.2) or whose header failed keeps
  the members that parsed: a lookup of a member it does not have answers *unknown*, as for a class with an
  unresolved ancestor, so `method.notFound` and its family stay silent. A function whose parameter or
  return type failed keeps the rest, and the failed part is `mixed`; a body with an error is a
  skimmed body (impure, throws anything). After a `use` that failed, a name in that file that nothing
  else binds is *unknown*, not `class.notFound`, since the import it might have been is lost.
- *Registry and cache.* The file's declarations enter the registry as for any file, with the
  `incomplete` flags; the per-file cache stores them with the flags and the file's error list (`CachedFile`
  stores `lex_warnings` today and gains `errors()`, so a warm run reports as a cold run does: B8's equality
  of cache off, written and read) and is keyed by the build, so a parser change never reads an old entry.
- *Fixes.* `--fix` and code actions skip a file that has errors.
- *Reporting.* An analysed file's errors are `phpstanRs.parseError` (level 0, non-ignorable, at the
  error's span, PHP's message) and fail the run. A context file's errors are recorded in the dump and the
  per-file cache and reported as a run-level notice, `phpstanRs.contextParseError`: its
  path, line and column and the first error, printed where today's "N file(s) skipped: parse error" count is, with
  the exit status unchanged and an opt-in switch that makes it fail the run. A failing run would fail on a vendor file the parser refuses. Decisions 9 and 15 ("nothing in a context file is reported,
  since its author is not the user") are read with this notice, which is not a finding
  about the file's code but the report that part of the analysis context is missing (a class it declares may show
  as `class.notFound` elsewhere). A parser panic stays `phpstanRs.internalError`. **The version
  marks are the data the existing rules read:** `check_void_cast` (`cast.voidNotSupported`, `cast.void`) and the
  about 25 other syntax features reported through the registry's `php_supports_*` (property hooks, promoted
  properties, union types, named arguments, first-class callables, asymmetric visibility, typed class
  constants, …) each iterate `ast.marks()` and keep their identifier, message and level, so no construct is
  reported twice and no identifier that users have in `ignoreErrors` or a baseline changes; the commit that
  adds the marks lists which `php_supports_*` sites become mark readers, with a test that one `(void)`
  expression gives exactly one diagnostic and that a baseline keyed by those identifiers still matches.
- *Editor.* The live buffer's tree is a request-local parse of the request's text (one parse per completion,
  with its own `LineIndex`): the project's tree lags the buffer by the debounce window and the last-good tree
  stops being a notion (G4b).

### G3.7 Oracles and tests

1. *The syntax matrix.* `tools/php-syntax-matrix/`: a probe set (one file per feature and per edge,
   including the contexts above) run under the official images 7.0 to 8.5 with `php -l` on each probe
   file, writing `{probe: {version: ok | parse error: message | fatal: message}}` into
   `crates/syntax/src/tests/data/syntax-<version>.json` as the comparison matrix does. **`php -l` only
   says accepted or refused, and the marks of G3.5 exist for syntax that an older version accepts and
   reads differently** (a one-line attribute group is a comment before 8.0; `object`, `mixed`, `never`,
   `void` and `iterable` are class names before the version that reserves them; the flexible heredoc
   marker of 7.3), so each probe also records a *meaning* column per version: the token-name sequence
   from `token_get_all` (an identical API on 7.0 to 8.5) and, for type-name probes, a short reflection
   script (declare, then `isBuiltin()` and `getName()`). "Accepted" means accepted with the same meaning;
   the probes expected to differ are listed, and are the data `MarkDetail`'s `comment_safe` and keyword
   cases are checked against. Deprecated-but-valid probes (the backtick operator, `"${var}"`) are in
   the matrix with the result "ok with a notice", and the probe files that hold a CR or CR LF are marked
   `-text` in `.gitattributes` (R2's `* text=auto eol=lf` would store them as LF) or built from Rust
   strings. The test parses each probe and checks: no error nodes if and only if some version accepts
   it with a meaning (a known gap is listed, never silent), and the validity range from the marks equals
   the set of versions that accepted it. This is the authority for G3.5.
2. *Corpus agreement.* Over Piwigo, the vendor projects, phpstorm-stubs and the fixtures: no error
   nodes in a file PHP 8.5 accepts (Q-1's refused list stays the known exceptions); an error in each
   file PHP refuses.
3. *Mutation properties* (`proptest` and the `fuzz` targets `lex_parse` and `token_stream_parse`,
   extended): from corpus files, delete, duplicate or swap a token, truncate at a random byte, insert
   a keyword, a NUL byte or a lone control byte. Invariants: it returns without panic within a work
   and a memory bound; spans nested and in range; tokens plus the spans in `errors()` cover the
   file; the declarations that end before the first error equal those of the undamaged file. **A
   quality oracle, because those invariants are met by a parser that gives up at the first error:** over
   the corpus, delete or insert one closing delimiter or token and require at most the number of errors
   PHP reports (1 for a missing closer) and at least a stated share of the declarations after the damage
   recovered, with per-rule thresholds in the golden dumps; ordinary damage never reaches the 200-error
   stop. The fuzz targets also run 10^6 openers, stray closers at every level, and an unterminated
   everything. **The bar for checking an analysed file with parse errors (Part E 31) is stated
   here:** over the same corpus, for damage to a single token, the diagnostics that are
   new, meaning absent from the undamaged file's own and not parse errors, are none at the 95th percentile of
   damaged files, measured per damage class; a class that misses falls back to parse errors only for the files it
   damages.
4. *Golden dumps* of about 150 hand-written broken snippets, one row per recovery rule of G3.4, in the
   canonical dump of Part B8.
5. *Editor tests*: completion, hover and symbols on buffers broken in each way (G4b).

### G3.8 Staging

(a) Step 2 as G3.3 says. (b) Batch 3, in this order: the lexer's errors and tokens
(the new ones: unterminated strings, comments and heredocs, the NUL byte as an `Error` token, the clamped
span); `SyntaxError`, `errors()` and the infallible signatures with the first-error behaviour unchanged for
valid input (dump equality); the statement, expression, list, class-body and header rules, each with
its golden dumps; the `php-version` crate and the marks; the matrix; the readers (G3.6); reporting
and the editor. (c) SYN-6's rows are marks and error kinds in that table, not a separate pass.
