# phpstan-rs roadmap

A from-scratch Rust PHP parser + type checker, aimed at growing toward
PHPStan feature parity. Phases 8+ are grounded in direct inspection of
`phpstan/phpstan-src` and `jetbrains/phpstorm-stubs` (not recalled
knowledge) and verified at real scale against Piwigo's own codebase
(`src/`, `tools/`) plus its `vendor/` tree, and - from Phase 22 onward -
against a frozen local copy (`/home/torres/piwigo17-frozen`,
`phpstan-rs.toml`-scoped) so before/after diffs aren't disturbed by the
live corpus's own file churn.

Status key:

- **DONE** - shipped and verified.
- **PARTIAL** - some of the phase's own scope shipped, some didn't.
- **PLANNED** - a real, detailed, source-grounded design exists; not
  started.
- **NOT STARTED** - a name and a rough idea only, no detailed plan yet.

This document has two halves. **Current status** is what's true right
now - read this before starting new work. **Completed phase history** is
how it got here, kept for lineage (a limitation or an open item is often
easier to judge with the phase that found it in view). Phase numbers are
cited in both halves so either can be cross-referenced from the other.

---

# Current status

## Headline numbers

- Test suite: 773 unit tests (`src/`) + 26 CLI integration tests + 3
  `main.rs` unit tests, all passing as of Phase 35.
- Piwigo frozen-corpus diagnostic count - the primary false-positive-hunt
  metric (`phpstan-rs.toml`-scoped, `/home/torres/piwigo17-frozen`) -
  has fallen every round: 2312 (Phase 22 start) -> 1280 -> 1021 -> 792 ->
  732 -> 706 -> 580 -> 532 -> 533 -> 512 -> 508 -> 350 -> 295 -> 270 ->
  **249 (current, Phase 35)**.
- Full Piwigo+`vendor/` corpus (~20,300 files, unscoped): last measured
  1,021 diagnostics as of Phase 24. Not re-measured every round since -
  the scoped frozen corpus above is the round-over-round regression
  signal now; re-run the unscoped check before trusting that number
  today.
- Test coverage: last measured 90.53% line / 97.46% function (`cargo
  llvm-cov`), during the Phase 6/9 audit. Stale - re-run before citing.

## Open phases

Every phase not listed here is DONE (see history below). These are the
phases with real, concrete open work:

**Phase 12 - Bespoke per-function/method type extensions (PARTIAL).**
Only remaining item: **`preg_match`/`preg_match_all`** by-ref `$matches`
shape inference from the pattern's own capture groups. Blocked on real
PCRE-aware parsing - no suitable Rust PCRE crate exists without a C
dependency, and Rust's own `regex` crate is a different-enough regex
flavor (no backreferences, different edge cases) to silently produce
wrong answers on real PCRE patterns. Everything else originally scoped
to this phase (`array_map`/`filter`/`reduce`, `sprintf`/`printf`, the
`array_column` family, `is_*` narrowing predicates) shipped.

**Phase 13 - Narrowing precision and remaining false-positive causes
(PLANNED).** Open items:
- Ternary conditions don't narrow their own branches (`infer_expr`'s
  `Ternary` arm checks both branches against the same un-narrowed
  `locals` an `if`/`while` would fork).
- A batch of `src/`-level findings audited only at the pattern level,
  never individually traced: `explode`/`substr`/`strtotime`/
  `strip_tags`/`strrpos`-style `string, string|false`/`string,
  string|null` mismatches, `count()` argument mismatches against
  `array|Countable`, and several `__construct()`/setter argument
  mismatches. **Caveat added during this rewrite**: some or all of the
  `explode`/`substr`-shaped ones may already be resolved by Phase 24's
  `#[LanguageLevelTypeAware]` fix - not re-verified, so don't assume
  this batch is still fully live without re-bucketing first.
- Traits checked standalone, with no knowledge of the eventual host
  class's own members.
- `@property`/`@property-read`/`@property-write` PHPDoc tags not parsed
  at all.
- `always_terminates` doesn't recognize `switch`/`try`/`match` as
  terminating even when every branch does.
- `for`/`foreach`/`switch`/`try` bodies share one unforked, unnarrowed
  scope (only `if`/`while` fork `locals` per branch today).
- Array-literal element types aren't tracked (`[1, 2, 3]` always infers
  `Array(Mixed)`).
- `BinOp::Div` and PHP 8.5's `|>` pipe operator both always infer
  `Mixed`.

**Phase 14 - Refinement/accessory types (PLANNED).** `non-empty-string`/
`non-empty-array`/etc. alias straight to their unrefined base type
instead of being tracked precisely. Design: a new `Type::Refined(Box
<Type>, RefinementKind)` (or an `Intersection`-with-marker, mirroring
real PHPStan's own `Accessory*Type` design), threaded through
`is_subtype`/`is_assignable` so a refined type stays assignable to its
own base type one-way, plus a few real narrowing sources (a non-empty
string literal, `trim()`'s known-safe cases, a `!empty($x)` guard).

**Phase 15 - Broader rule catalog (PLANNED).** No rules yet in:
`DeadCode` (unreachable code past an always-terminating statement,
always-true/false conditions, unused private methods/constants),
`Comparison` (`match`/`switch` exhaustiveness, strict-comparison-of-
incompatible-types, always-true/false `in_array`/`array_key_exists`
checks), `Exceptions` (needs a real throw-type model this project
doesn't have at all - not even a `FunctionSig`-level "what can this
throw" field), and visibility/abstract-method-completeness enforcement.
Also carries three narrower, still-open gaps: `continue N`'s own level
argument is parsed but never consulted; `Type::Array` has no key-type
parameter (`foreach`'s own `$key`, or a `preg_match` by-ref `$matches`
element, is always `Mixed` - this is the same root cause Phase 35 just
ran into again, seven phases after it was first tracked here); `goto`/
`label` form no control-flow-graph edge, so a backward `goto` could make
an `always_terminates`/narrowing conclusion unsound.

**Phase 16 - CI-friendly output formats (PLANNED).** No JSON/checkstyle/
junit/etc. output. Realistic starting subset: `json` (most universally
consumable) plus `checkstyle` or `junit`. Needs real source positions to
be useful beyond plain text - Phase 11 already shipped those, so this is
otherwise unblocked.

**Phase 17 - LSP hover/go-to-definition (PLANNED).** Needs a
position-keyed lookup index - today's `Registry`/`Project` lookups are
all name-keyed. `src/lsp.rs`'s existing hand-rolled JSON-RPC loop
(`run_loop`) is otherwise ready to extend with `textDocument/hover`/
`textDocument/definition`.

**Phase 18 - Extension/plugin API (PLANNED).** Deliberately last and
least concrete - premature before Phases 12-15 give this project enough
built-in rules/type-extensions that a third party would actually want to
add to, rather than just use.

**Phase 19 - Fuzz testing (NOT STARTED).** Name and rough intent only:
`arbitrary` + `cargo-fuzz` for the lexer/parser (a malformed-input crash
is a real, if narrow, risk the corpus-testing approach can't rule out),
`proptest` for `types.rs`'s subtyping logic (reflexivity, transitivity).
Both dev-only. Deliberately deferred behind Phases 11-15.

## Known limitations

Every deliberate "this checker doesn't model X" boundary, gathered in
one place so none of them are only discoverable by reading in-code
comments.

### Tracked under a future phase

| Limitation | Phase |
| --- | --- |
| Bespoke per-function/method type extensions still missing: `preg_match`'s by-ref `$matches` shape | 12 |
| Traits checked standalone, without their eventual host class's shape | 13 |
| `@property`/`@property-read`/`@property-write` PHPDoc tags not parsed | 13 |
| `always_terminates` doesn't recognize a terminating `switch`/`try`/`match` | 13 |
| `for`/`foreach`/`switch`/`try` share one unforked, unnarrowed scope | 13 |
| Array-literal element types not tracked (`Array(Mixed)` always) | 13 |
| Integer division and PHP 8.5's pipe operator (`BinOp::Div`/`Pipe`) always infer `Mixed` | 13 |
| Refinement/accessory types (`non-empty-string`, `non-empty-array`, `int<min, max>` ranges, ...) aliased to their base type, not tracked precisely | 14 |
| No `Type::FloatLiteral` variant - a real float literal always widens straight to plain `Float` (unlike `IntLiteral`/`StringLiteral`), so a PHPDoc float-literal-union type (`1.5\|2.5`) rejects every value, valid or not | 14 |
| Broader rule catalog (`DeadCode`, `Comparison`/exhaustiveness, `Exceptions`, visibility/abstract-method completeness) | 15 |
| `continue N`'s own level argument is parsed and stored but never consulted (`break N`'s own level is, since Phase 29) | 15 |
| `Type::Array` has no key-type parameter - a `foreach`'s own `$key`, `array_keys()`'s precise result, or a `preg_match` by-ref `$matches` element, is always `Mixed`/imprecise regardless of how precisely-keyed the real array is. Surfaced repeatedly: Phase 29 (a loop key), Phase 31 (`array_keys()`, partially closed for the literal-shape case only), Phase 32 (the generic fallback), Phase 35 (a repository call finally being checked at all) | 15 |
| No CI-friendly output formats (JSON/checkstyle/junit/...) | 16 |
| No LSP hover/go-to-definition | 17 |
| No extension/plugin API | 18 |

### Known but not yet scheduled to any phase

- **phpstorm-stubs' interleaved-multiple-parameter-list shape isn't
  recognized.** Some stubs (`pg_query_params`, `pg_query`, several
  others) declare *two full parameter lists* under the same parameter
  names across PHP-version eras (e.g. two separate `$connection`
  parameters, one closed-range-with-default, one open-range-with-no-
  default, side by side) rather than the clean "one superseded param +
  one modern variadic" shape `array_map` has, which Phase 8.5's fix
  already handles. Left an estimated ~384 residual "expects at least N
  arguments" diagnostics as of Phase 8.5 (re-confirmed still present,
  not yet fixed, as of Phase 24). No phase has been scheduled for this -
  a candidate for a future round if it turns out to matter in practice.

### Permanent design boundaries (not phase-tracked - not planned, by choice)

- **Conditional PHPDoc return types** (`@return ($x is 'a' ? A : B)`) -
  documented in `types.rs`'s own module comment since Phase 2; these
  need real per-call-site type-parameter binding this project's PHPDoc
  parser deliberately doesn't attempt.
- **A dynamic class target** (`$obj::method()`, `(new Foo())::CONST`) is
  never checked, by the same "don't guess" precedent `Expr::New` already
  set for a dynamic class-name expression - there's no static name to
  resolve at all, not a missing feature to build.
- **A generic bound checked against a class this project has no real
  data for** (`checker::is_type_fully_known`, Phase 10.5) silently skips
  that check rather than guessing - a PHAR-bundled dependency with no
  loose source available anywhere this project vendors (Phase 21 closed
  this for real PHPStan itself by reusing `vendor/phpstan-src` as a
  second stub-class source; `vendor/phpstan-src`'s own transitive
  `nikic/php-doc-parser` dependency still has this problem one level
  down, not vendored here), or a local `@phpstan-type` alias this
  project doesn't parse.
- **Variance checking (Phase 10.5) doesn't cover properties, or flip
  variance through a nested generic argument.** Properties: this
  project doesn't track `readonly` at all, and a *readonly* property
  typed `T` is actually a legitimate covariant-safe position - checking
  properties without that information would misflag every real one.
  Nested arguments: a `T` found inside another class's own
  `Collection<T>` type argument is checked the same as a direct
  occurrence, not re-derived from `Collection`'s own variance.
- **Integer overflow/precision, floating-point rounding, and other
  runtime-value-dependent behavior** - this checker only ever reasons
  about static types, never concrete values (Phase 1's own founding
  choice), so anything whose result depends on the actual runtime value
  rather than its type is out of scope by construction.
- **`array_flip`/`compact`/`get_defined_vars`** (Phase 12) - would need
  tracking array *key* types (see the `Type::Array` row above) or
  building a result from a string-named/whole-scope variable lookup, a
  different and much more dynamic mechanism than anything this project
  attempts. Deliberately not attempted, not a "not yet" gap.
- **A `Closure(...)`/`callable(...)` PHPDoc shape's own per-parameter/
  return signature** (Phase 30) - degrades to plain `Type::Callable`,
  matching this project's existing bare-`callable` leniency, rather than
  modeling the shape precisely.
- **A `foreach` header's own inline `@var` annotation** (Phase 35) - the
  loop body is checked inside the same match arm that binds the loop
  variable, so only a tag on a statement *inside* the body (the
  evidenced real-world shape) is supported; a tag directly on the header
  would need to affect the loop body's own checking and was deliberately
  left unsupported rather than shipped half-working.

---

# Completed phase history

## Phases 1-7: foundations

1. **PHPDoc doc comments.** Capture and parse `/** ... */` (`@param`,
   `@return`, `@var`).
2. **PHPDoc types.** Wire tags into the registry; array shapes and
   generic type arguments; `@template` generics with call-site
   substitution.
3. **Flow-sensitive narrowing.** Narrow variable types across `if`/
   `while` branches (`instanceof`, equality, common predicate
   functions).
4. **Properties and class context.** Property types, constructor
   promotion, trait `use`, `self`/`static`/`parent` resolution.
5. **Built-in stub generation.** Generated from real php-src source -
   superseded by Phase 8's broader phpstorm-stubs corpus.
6. **Undefined-variable detection.** By-ref tracking; `foreach`/`catch`/
   destructuring target binding; array/shape index access. **Re-audited
   2026-08-30** via `cargo llvm-cov`: found the largest gap in this
   project's history to that point - `Expr::Closure`/`ArrowFn`/`Match`
   had **no handling anywhere** in the checker, so every diagnostic was
   invisible inside any closure, arrow function, or `match` body
   (2221 -> 2321 first-party diagnostics once fixed, all spot-checked
   genuine). Fixed with real PHP capture semantics (`use (...)`-only for
   closures, implicit whole-scope-by-value for arrow fns, `$this`
   auto-bound for non-static closures). Found 4 more independent bugs
   chasing this: a static method's `$this->prop` wasn't flagged; closure/
   arrow-fn type hints bypassed namespace resolution; multi-line `@param`/
   `@return` tags got truncated mid-parse; an unsealed-shape `...<K, V>`
   marker aborted an entire array-shape parse instead of just that field.
   *(Unplanned) Real-world hardening pass* - a detour fixing dozens of
   parser/lexer gaps (PHP 8.1 enums, DNF types, `declare()`, goto/labels,
   asymmetric visibility, PHP 8.5 clone-with) against Piwigo + php-src's
   own test corpus + Piwigo's `vendor/` (17387 files).
7. **LSP / incremental / tooling.** Incremental multi-file `Project`
   (re-check one changed file), a transitive cross-file dependency graph,
   a real Content-Length-framed JSON-RPC LSP server. **Re-audited
   2026-08-30**, found and fixed a Phase 9.5 regression: `declared_names`
   started returning FQNs, but the invalidation BFS looks names up
   bare-keyed - silently broke cascading invalidation for every
   namespaced class.

## Phase 8 - Ingest `jetbrains/phpstorm-stubs` - DONE

Vendors the real 558 stub files (of 1052 total) via `build.rs`, resolving
1000+ real function signatures instead of two hand-picked extensions.

## Phase 8.5 (unplanned) - Stub arity precision - DONE

Two independent bugs in how a stub's minimum-argument count gets
generated, both real false positives on common functions.

- **Version-gated parameters counted as permanently required.**
  phpstorm-stubs marks "this parameter only existed in versions X-Y" via
  a `#[PhpStormStubsElementAvailable(from, to)]` attribute on individual
  parameters rather than separate per-version signatures - neither the
  parser nor `php_stubs.rs` recognized it, so `array_map($cb, $array)`
  (2 args) was flagged as needing 3. Fixed at the **lexer** level (not
  just stub generation, since real projects vendor phpstorm-stubs
  themselves): a closed-range parameter with no default is dropped from
  the token stream entirely.
- **A stub modeling only its highest-arity overload.** `array_keys(['a'
  => 1])` and `mt_rand()` (both real, valid lower-arity forms) were
  flagged too - fixed via `DocBlock`'s new `optional_params` (a
  `[optional]` `@param` marker treated as an implicit default). Found a
  second bug verifying this: the `@param` parser never matched a
  **by-ref** parameter's name at all (`&$matches`, since it looked for a
  bare `$name`) - this was the actual live cause of false positives on
  `preg_match`/`preg_replace`/`str_replace` against Piwigo's own
  vendored older phpstorm-stubs release.

Verified at real scale (18923 files): 46791 -> 42192 diagnostics, zero
new false positives, 293/293 tests passing. A residual ~384 "expects at
least" diagnostics from a structurally different, harder pattern
(interleaved multi-version parameter lists) were left - see Known
Limitations above.

## Phase 9 - Undefined-symbol rule family - DONE

Undefined-class/function/method/property(-read)/static-access detection,
with real `__call`/`__get`/`__callStatic` exemptions and self/parent/
static-aware resolution. Found and fixed several bugs along the way
(leading-backslash normalization, `use`/`use function` imports parsed
but never read, `Registry::index` never recursing into `if`-guarded
polyfill declarations). Finishing the last two rules surfaced 3 more real
gaps: backed enums' `->value`/`->name` and `cases()`/`from()`/`tryFrom()`
weren't registered at all (~2800 false positives); `&&`/`||`
short-circuit narrowing wasn't modeled; `instanceof` narrowing only ever
matched a bare variable, never a property chain (fixed via a new
`narrowing_key` helper).

## Phase 9.5 (unplanned) - Namespace-aware registry - DONE

Undefined-method detection was reverted after real-scale testing found
it unsafe: zero namespace modeling meant a cross-package class-name
collision (1595 distinct colliding names found across Piwigo+vendor)
could silently overwrite one class's method table with an unrelated
one's. Fixed properly with `registry::NameContext` (namespace + `use`-
import map, tracked positionally per file), re-verified safe at scale
(22,900 false positives -> 3, all genuine). Found 4 more bugs verifying
this (multi-parent interface walking, an `!`-vs-`instanceof` operator-
precedence bug, two pseudo-type name collisions). **Re-audited
2026-08-30** with fresh namespaced tests, found 4 more variations on
"something that used to be a harmless bare name became visibly wrong
once namespace-qualification existed": `lookup_own_signature`'s
top-level-function lookup used raw names not FQNs; `instanceof self`
wasn't resolved against the enclosing class; `@return $this`/
`self::RFC_*`-style pseudo-types weren't recognized; `catch (SomeException
$e)` built its type from the raw AST name with zero resolution.

## Phase 10 - Class-level generics - DONE

`Type::Object` now carries real type arguments (`Collection<User>` ->
`Object("Collection", [Object("User", [])])`). A class's own `@template
T` is recognized across its methods/properties. Deliberately scoped to
call-site substitution only - variance, bound enforcement, and
`@extends`/`@implements` chain substitution moved to Phase 10.5.

## Phase 10.5 (unplanned) - Generic variance and inheritance chains - DONE

- **Inheritance-chain substitution.** `@extends Parent<Arg>`/`@implements
  Iface<Arg>` now parsed and resolved. Found and fixed a real bug: the
  first implementation flat-merged the *entire* ancestor chain's
  template bindings into one map, letting an unrelated ancestor's
  same-named `T` silently overwrite another's (`T` is by far the most
  common single-letter generic name) - fixed by scoping every lookup to
  a specific target ancestor via new `_with_owner` lookup variants.
- **Template bound enforcement.** `@template T of Bound` now actually
  checked (`check_generic_bounds`), not just parsed past. Found 3 real
  bugs verifying this: `@template-covariant`/`@template-contravariant`
  tags weren't recognized as templates at all (15 false positives);
  `is_assignable` never treated native `object` as a universal
  supertype (~25 false positives, real trigger: Doctrine's
  `EntityRepository<T of object>`); an enum was never registered as
  implementing `UnitEnum`/`BackedEnum` in its own hierarchy data. Also
  added a "don't know, don't guess" leniency skipping a bound check
  entirely when either side mentions a class with no real registered
  data (a PHAR-bundled dependency, or a local `@phpstan-type` alias).
  Net corpus effect: 42192 -> 41255 (bug 1 alone fixed real pre-existing
  false positives unrelated to bounds).
- **Variance enforcement.** `check_variance` flags a covariant `T` in
  parameter position or contravariant `T` in return position. Found a
  real bug immediately: every one of an initial 27 diagnostics was on a
  `__construct` (the extremely common `readonly` value-holder pattern) -
  real PHPStan exempts constructors for exactly this reason (never
  invoked polymorphically); fixed by exempting `__construct` entirely,
  restoring the pre-check baseline (a genuinely variance-clean corpus,
  confirmed via 18 new targeted unit tests, not a broken check).

## Phase 11 - Source positions, levels, config - DONE

Six ordered stages, each its own commit:

1. **Lexer emits real spans** (`token::Position`/`Span`, UTF-8-character
   columns, precomputed line-start table for O(log n) offset lookup).
   Found and fixed a severe performance bug: naive byte-to-line/column
   conversion recounted from a line's own start every call - O(line
   length x tokens) blowup, hanging for minutes on a real 1.25M-character
   generated line already in this project's corpus. Fixed with an
   incremental cache exploiting strictly-increasing tokenization offsets.
2. **Spans threaded through the AST** (`Stmt`/`Expr` wrapped as
   `{kind, span}`, ~450 call sites migrated, Rust's exhaustiveness
   checking making every miss a compile error).
3. **Diagnostics carry real spans.**
4. **Real CLI/LSP output positions** - `into_diagnostics()` now sorts by
   `(line, col)` (internal checks don't run in source order, so this is
   load-bearing, not defensive); LSP output converts to 0-indexed ranges.
5. **Discrete rule-gating levels + strictness parameters.** Each of 16
   diagnostic push sites tagged with a level mirroring real PHPStan's own
   groupings (verified against a fresh `phpstan-src` clone's
   `conf/config.levelN.neon`); `--level N` flag. `strict_mixed_arrays`
   opts into treating an unknown array literal as *not* freely
   compatible with a specific shape (real PHPStan's own level-9
   philosophy) - surfaces 1814 additional real findings on Piwigo,
   default output unchanged.
6. **Baseline mechanism, `clap` CLI migration, TOML config file.**
   `--generate-baseline`/`--baseline` (keyed by path+location+message, so
   a baselined finding survives incidental code shifts); derived `Cli`
   enum replacing hand-rolled flag parsing (a deliberate breaking CLI
   syntax change, acceptable pre-1.0); `phpstan-rs.toml` config with CLI
   flags taking precedence.

## Phase 12 - Bespoke per-function/method type extensions - PARTIAL

See **Open phases** above for what's left (`preg_match`). Shipped:
`array_map`/`array_filter`/`array_reduce` (a two-pass mechanism binding
the *other* argument's element type into a closure's own body-inference
before computing its return type - generic template unification alone
can't do this for an untyped closure param); `sprintf`/`printf` format-
string validation against literal arguments (found and fixed a real
unguarded panic in the `array_filter` special case, and a missing
`non-negative-int` pseudo-type along the way: fixing the latter alone
dropped the full corpus 48496 -> 48243); `array_column`/`array_combine`/
`array_fill`/`array_fill_keys`/`array_chunk`; `is_array`/`is_string`/
`is_int`/`is_callable` narrowing predicates (`class_exists`/`in_array`/
`method_exists`-style existence checks deliberately excluded - they
check "does this exist," not "what type is this value now").

## Phase 13 - Narrowing precision and remaining false-positive causes - PLANNED

Three real, independent namespace/pseudo-type resolution bugs found and
fixed via a direct audit, 2026-08-30 (each had been silently live for an
unknown stretch): a native type hint's leading `\` was stripped before
namespace resolution ever saw it (48243 -> 44716); a quoted PHPDoc
literal-string type (`'ASC'|'DESC'`) wasn't recognized, the quotes
becoming part of a mangled "class name" (44716 -> 44525);
`resolve_pseudo` never resolved `self`/`static`/`parent` wrapped in a
`Union` (`?self` factory-method returns) (44525 -> 44353). Real
first-party `src/` diagnostics dropped 2327 -> 2270 across these three.
Also shipped, folded into this phase: **Composer integration for
`vendor/` type context** (`src/composer.rs` - auto-loads a project's
installed autoloader for symbol resolution without reporting on vendor
code itself; `--no-composer` 2269 diagnostics -> auto-detected 1181,
with the 5 residual "not found" cases traced to two understood, accepted
edge cases: first-party code under a non-`src/` PSR-4 root, and
Composer's own bootstrap `ClassLoader` which is structurally invisible
to any of the four autoload mechanisms this feature reads).
Everything else scoped to this phase is still open - see above.

## Phase 20 - Performance & robustness - DONE

Started from a real incident: Phase 11's lexer work shipped an O(n²)
blowup that hung for minutes on a real corpus file, caught only by this
project's own real-corpus verification step (no dedicated performance
tooling existed yet). Profiled a real `check-project` run
(`perf record --call-graph dwarf`) to prioritize:

1. **A real stack-overflow crash on deeply-nested expressions.** The
   parser's `MAX_EXPR_DEPTH=1000` guard was undermined by worker threads
   spawning with Rust's default ~2MiB stack - confirmed a hard SIGABRT
   crash at only ~200-300 levels of nesting. Fixed with an explicit 8MiB
   worker stack.
2. **`Locals`'s cloning cost** (~28% of allocator-churn time, cloned at
   ~22 call sites per branch). Swapped to `imbl::GenericHashMap`
   (persistent, structurally-shared, O(1) clone). A real methodology
   lesson: system-wide `perf` sampling first reported this as
   essentially unchanged (~32% -> ~31.5%) - too coarse to see the real
   effect. A new in-process instrumentation feature, `hotpath` (item 6
   below), measured the true, internally-consistent win: `check_stmt`
   -13.9%, `check_function` -10.1%, `check_program` -8.3%, `check_expr`
   -7.5%, total runtime -5.3%.
3. **`FxHashMap` for `Registry`/`Project`'s other hot, build-once
   HashMaps.**
4. **An automated regression-guard benchmark** (`benches/
   vendored_corpus.rs`, criterion, against this project's own in-repo
   vendored corpus so it works in CI without an external Piwigo copy).
5. **Pathological-input regression tests** codifying both findings above.
6. **`hotpath`, an opt-in in-process instrumentation profiler** (zero-
   cost when disabled) - the tool that caught item 2's real effect after
   system-wide `perf` sampling missed it.

## Phases 21-35 - Piwigo false-positive hunts

Fifteen rounds of the same repeatable method, refined as it went: run
`check-project` against a real, PHPStan-clean Piwigo corpus; bucket the
output by normalized message shape (strip argument indices, variable
names, string literals, class-name prefixes - see the categorization
snippet below); root-cause each cluster by reading the *actual*
triggering source (Piwigo's own code, this project's own checker, and
the real vendored `phpstorm-stubs`/`phpstan-src`) rather than guessing
from message text; fix; re-verify with a location-based diff (a changed
line landing at an already-diagnosed location is a safe reshape, a
genuinely new location gets individually traced to ground truth before
being accepted). Phase 22 switched from the live, file-count-drifting
`~/piwigo17-rewrite` to a frozen local copy with a proper scoped config,
the methodology used ever since:

```bash
cd /home/torres/piwigo17-frozen
/path/to/phpstan-rs check-project --config phpstan-rs.toml 2>&1 \
  | grep -oP '(?<=: ).*$' \
  | sed -E "s/\\\$[A-Za-z_][A-Za-z0-9_]*/\$VAR/g; s/[0-9]+/N/g; s/'[^']*'/'STR'/g" \
  | sort | uniq -c | sort -rn | head -50
```

Diagnostic count per round (scoped corpus from Phase 22 on): 43,800
(Phase 21 start, unscoped) -> 32,596 -> **2,312** (Phase 22, first scoped
count) -> 1,280 -> 1,021 -> 792 -> 732 -> 706 -> 580 -> 532 -> 533 (a
disclosed, accepted tiny uptick - see Phase 29) -> 512 -> 508 -> 350 ->
295 -> 270 -> **249**.

**Phase 21 (unscoped corpus).** Pest's `it`/`test`/`describe`/hook
closures binding `$this` at runtime (read directly from the real
vendored `pestphp/pest-plugin-phpstan` extension's resolution algorithm,
1498 -> 13); `assert($x instanceof T)` narrowing; a generator function/
closure never confused for returning null; `phpstan/phpstan`'s own
PHAR-bundled classes resolved by chaining `vendor/phpstan-src` into the
stub pipeline; a conditionally-declared class stub (`if
(!class_exists())`) no longer silently overwriting a real unconditional
class of the same name.

**Phase 22 (2,312 -> 1,280).** Pest's `beforeEach`-assigned dynamic
`$this` properties (571 -> 0); array append/dynamic-key writes now widen
a tracked shape instead of leaving it stale, including recursing through
nested writes; a by-ref argument on an *already-defined* variable now
widens too, not just an undefined one; `?? []` no longer discards a
wider left-side array type; anonymous-class method signatures now
resolve names/pseudo-types correctly instead of falling back to an
unresolved branch; a right-hand `&&`/`||` operand's new variable
definitions are no longer silently dropped. The array-write fix's own
verification surfaced a deeper gap - `narrowing_key` only supported
`Variable`/`PropertyAccess`, never a literal-key array index - fixed by
extending it everywhere, which is what let this phase close out the
"property-chain narrowing" limitation tracked since Phase 13 (extended
to array-index chains too).

**Phase 23 (1,280 -> 1,021).** `isset($x)` never narrowed away `Null`
(the single most common PHP nullability idiom, 1,073 occurrences,
previously a complete no-op); an assignment nested inside a narrowing
predicate's own argument (`is_string($result = expr)`) now narrows;
`str_replace`/`preg_replace`-family return types now depend on
`$subject`'s own type; a ternary's two branches now flatten through the
shared `union_types` helper instead of building an unflattened nested
`Union` narrowing couldn't see into; `exit`/`die` now infer `Type::Never`
so a guard clause built on them is recognized as terminating; a negated
literal class-constant value (`const OFF = -1;`) no longer widens to
`Mixed`; a stale legacy `@return resource|...` doc comment no longer
overrides an already-modernized native hint (with a careful exception
for `mixed`/`object` placeholder hints, where the doc genuinely is the
only source of truth) - this last one also corrected a Phase 22 "not
pursued" note that had misdiagnosed the symptom as an unknown narrowing
bug. **Not pursued**: `#[LanguageLevelTypeAware]` attribute parsing
(2,033 occurrences across 84 stub files) - real, non-trivial new parsing
machinery, not a small addition alongside seven otherwise surgical
fixes; flagged as the likely highest-value remaining investigation.

**Phase 24 (1,021 -> 792) - `#[LanguageLevelTypeAware]`.** Closed the
deferral above: this attribute says "this function/param's type differs
by PHP version" (`substr()`'s real stub has *no* native return hint at
all, so a stale pre-8.0 doc comment was the only source used). Shipped
both halves - return-type gating (splices a fresh `@return` line into
the preceding doc comment, extracting the highest-versioned type,
consistent with this project's existing `#[PhpStormStubsElementAvailable]`
precedent) and parameter-type gating (injects synthetic native type-hint
tokens before the parameter, since a parameter has no guaranteed
docblock tag to overwrite the way a return does) - accounted for roughly
a quarter of everything left at the time.

**Phase 25 (792 -> 732) - class-declaration merging.** Piwigo ships a
project-local `Imagick.stub.php` overriding one stale method (real
PHPStan's own documented `stubFiles` merge mechanism) - this project
treated two unconditional declarations of the same class as plain
last-write-wins, so the one-method override silently discarded the
entire real class (60 diagnostics, ~7.6% of everything left).
`ClassInfo::merge` now unions members across unconditional declarations
(second wins on conflict, `extends`/`implements`/generics only ever
filled in). Given this is a *global* registration-semantics change,
verification swept every file in both vendored stub corpora, not just
the usual before/after diff.

**Phase 26 (732 -> 706) - narrowing through wrapper expressions.**
`narrowing_key` never unwrapped a parenthesized `Grouping` node, so the
overwhelmingly common `while (($file = readdir($dir)) !== false)` shape
(parens are *required* here by operator precedence, not optional style)
never narrowed. Also: `apply_narrowing` had no `Cast` arm at all, so an
explicit `if (! (bool) $fp)` truthiness guard didn't narrow either - a
`(bool)` cast doesn't change which values are truthy, so it now
delegates straight through.

**Phase 27 (706 -> 580) - PHPUnit assertion narrowing.** A systemic idiom
(1,200+ combined uses across the test suite) of
`assertIsString`/`assertNotNull`/`assertInstanceOf`/etc. as a narrowing
proof - if execution continues, the assertion held. New
`apply_phpunit_assertion_narrowing`, matched purely by method name (both
`self::`/`$this->` forms), the largest single-round drop since Phase 24.

**Phase 28 (580 -> 532) - `is_resource`/`is_scalar` narrowing.**
`is_resource` was simply absent from the narrowing-predicate table
despite being the idiomatic guard for `proc_open()`/`fsockopen()`'s real
`resource|false` returns; `is_scalar` added too (exactly
`int|float|string|bool`, unlike `is_numeric`, deliberately still
unnarrowed since this project has no `numeric-string` representation).

**Phase 29 (532 -> 533, a disclosed uptick) - a `break`'s own state
reaching past its loop.** A real control-flow-modeling gap: nothing
captured a `break`-reached branch's state for a loop's own post-loop
state, so a variable only ever assigned on the path that breaks stayed
typed as its pre-loop value forever (masking every real type of the
value it could actually hold). New `loop_break_states` stack, with a
`Break` statement pushing the current scope onto the innermost (or `break
N`'s) open frame, merged back after the loop. A real design mistake was
found and reverted mid-fix: also giving `Foreach` a `While`/`For`-style
"may run zero times" merge with its pre-loop state broke a real,
common idiom (a loop mutating the very array it iterates via its own
loop variable). Correctly modeling `break` surfaced 6 further, previously-
masked diagnostics - all traced to the already-known `Type::Array`-has-
no-key-type gap, a legitimate disclosed limitation, not a new defect.

**Phase 30 (533 -> 512) - `Closure(...)`/`callable(...)` shape syntax.**
A PHPDoc callable-shape type (`@param Closure(self):T $func`, real and
documented, e.g. Doctrine DBAL's own `transactional()`) had no parser
case at all, falling through to the generic class-name branch and
producing a garbled, namespace-mangled "class name" as the diagnostic's
own expected-type text. Now degrades to plain `Type::Callable`, matching
this project's existing bare-`callable` leniency.

**Phase 31 (512 -> 508) - `array_keys()`'s real key type.**
`array_keys()`'s stub declares a whole-array either/or (`int[]|string[]`)
rather than a precise per-key result. Scoped fix: when the argument is a
known `ArrayShape`, compute the real per-key union directly (PHP casts a
canonical non-negative-integer string key to int at build time) instead
of always defaulting to the stub's own wider union. Deliberately scoped
no further - most of this bucket's remaining diagnostics pass a variable
with no shape information at all, the general `Type::Array`-has-no-key-
type gap this fix doesn't (and can't, without that gap closing first)
address.

**Phase 32 (508 -> 350) - a full-corpus bucket sweep.** Twelve
independent bugs in one round instead of chasing a single bucket: (A)
duplicate stub parameters from phpstorm-stubs' own version-era
convention weren't deduplicated, shifting later argument positions; (B)
a `LanguageLevelTypeAware` parameter override lost to a competing legacy
`@param` tag; (I) the same docblock-vs-native reconciliation generalized
past `resource` specifically, then *corrected* after a regression:
`is_subtype`'s `Object` comparison is name-equality only, so a doc type
naming a real subclass of a more generic native hint was wrongly
discarded - scoped back to the one provable case; (C) two unconditional
function declarations of the same name overwrote instead of merging
(the function-side sibling of Phase 25's class-merge fix); (D)
`microtime(true)`'s bool-literal-dependent return type; (E) three more
functions added to the subject-dependent-return-type family; (F)
`narrowing_key` now unwraps `?? null` (a literal fallback specifically -
sound; a non-null fallback isn't); (G) `is_numeric()` narrowing, as its
own dedicated arm with asymmetric per-branch exclusion sets (a shared-
target implementation would have introduced a new false positive); (H)
a docblock union containing `mixed` now collapses to plain `mixed`;
(J) `json_encode(..., JSON_THROW_ON_ERROR)` drops `false` from the
return, including through an arbitrary-depth flag chain; (K) a bare
`$cond || neverReturningCall();` guard now narrows `$cond`; (L) exact
type equality was the wrong tool for union-member exclusion (a
docblock-refined `array<string>` survived an `is_array()` exclusion
untouched) - switched to `is_subtype`-based membership, which also fixed
a previously-silent bug in Phase 28's own `is_scalar` false branch, and
a matching blind spot in `union_types`'s own construction-time
absorption. Left four new Known Limitations rows, all closed the very
next phase.

**Phase 33 (350 -> 295) - closing Phase 32's own disclosed rows.**
`instanceof`'s false branch now drops the checked class from a `Union`
(reusing the existing `narrow_type_excluding`, since the false branch
only needs to *drop* a proven-impossible member, not represent "not
this class" on its own); `in_array($needle, $literalArray, true)` and
`array_key_exists($literalKey, $arr)` are now recognized narrowing
predicates, mirroring real PHPStan's own extensions for both; a real
variable-subscript narrowing key (`is_string($this->mirror[$key])`)
falls back to a `$`-prefixed marker - and, checking whether *any*
invalidation-on-reassignment mechanism existed before trusting this new
key format, found there wasn't one for *any* composite key at all (a
stale `"foo->bar"` narrowed value survived a plain `$foo = ...`
reassignment forever) - fixed with a shared `purge_stale_narrowing_for`,
wired into all seven real call sites that can give a variable a new
value; the disclosed `int<min, max>` gap turned out to be misdiagnosed -
the real bug was `array{...}`'s own unkeyed/positional tuple fields
(`array{int, string}`) failing to parse at all, aborting the whole
shape. A correction found by the mandatory full-corpus re-check:
the new positional-field detection first rejected a real, pre-existing,
hyphenated bare key (`plural-forms`, a gettext convention) - fixed
transparently, with its own regression test.

**Phase 34 (295 -> 270) - a fourth full-corpus sweep.** `curl_getinfo($ch,
CONSTANT)` given a real dynamic return type (a curated ~35-entry subset
of real PHPStan's own extension table, scoped to the 2-arg literal-
constant form); `||`'s true branch can now narrow when every leaf is a
same-key predicate check on the identical expression (fixes `is_int($y)
|| is_string($y) ? $y : 0`); a backed enum's `->value` now types as the
union of its own case literals instead of widening to the generic
backing type; `stdClass` is exempt from "access to undefined property"
(a real, registered, zero-property class - real PHP treats it as the
canonical arbitrary-property bag) - scoped to property reads only, an
undefined *method* call on `stdClass` is still a real fatal error and
stays flagged. Deliberately deferred: `class-string<T>` template
binding and inline `@var` annotations - both needed genuine new type-
system/parser machinery, not a narrow fix; closed properly the very next
phase.

**Phase 35 (270 -> 249) - closing Phase 34's own deferred items,
properly.**
- **`class-string<T>` now participates in `@template` binding.** A new
  `Type::ClassString(Box<Type>)` survives from parsing through
  `Foo::class`/`get_class($obj)` inference, template binding/
  substitution, and every recursive `Type`-tree walker, with
  `is_subtype`/`is_assignable` kept freely interchangeable with plain
  `string` in both directions for backward compatibility (`is_assignable`
  additionally getting its own hierarchy-aware override, the same way it
  already does for plain `Object`-vs-`Object`). Fixed the targeted
  `*TestRepoWithEm`/`*TestConnAndRepo` family (6 diagnostics) - and, it
  turned out, far more: every real `TypedRepository::narrow(...)`-shaped
  repository construction in the corpus had been `Mixed` end to end (so
  every call through it was entirely unchecked), and PHPUnit's own
  `self::createStub(Foo::class)` (a `@template T`/`class-string<T>`
  method in its own stubs) now resolves to `Foo&Stub` instead of `Mixed`.
  One new, isolated false positive surfaced as a direct result of a
  repository finally being checked at all - traced to the already-
  tracked `Type::Array`-has-no-key-type gap (Phase 15), not a new
  regression.
- **Inline `@var` annotations are now recognized on any statement**, not
  just an actual property/param/return declaration. `Stmt` carries its
  own leading doc comment now; `check_stmt` applies a recognized
  assertion before dispatch (so later reads in the same block see it)
  and, only when the statement itself is a plain assignment to the same
  variable, again after dispatch - found via adversarial validation of
  the design *before* writing any code that `bind_assignment_target`
  would otherwise silently clobber the pre-applied assertion in the same
  call. A tag directly on a `foreach` header was deliberately scoped out
  after tracing that the loop body is checked *inside* the same match arm
  that binds the loop variable - an after-dispatch-only reapplication
  would produce a half-working result (correct after the loop, wrong
  inside it), worse than not supporting the shape at all (see Known
  Limitations above). Fixed the evidenced `StructuralTest`/`tools/p58/
  trace.php` family, plus several unrelated real `@var` comments already
  written elsewhere in the corpus that this checker had simply never
  read before.

---

# Verification approach (used for every phase above)

- Unit tests for each new rule/type-system feature.
- Re-run against real corpora after every phase: Piwigo's `src/`, its
  `vendor/`, php-src's own test corpus, phpstorm-stubs' real stub files,
  and (from Phase 22 on) a frozen local Piwigo copy with a
  `phpstan-rs.toml`-scoped config for stable before/after diffing.
- For new diagnostic rules: spot-check a sample of newly-surfaced
  diagnostics by hand to confirm they're genuine findings, not false
  positives.
- Location-based classification for every phase's corpus diff (Phase 25
  on): a changed line landing at an already-diagnosed location (a
  reworded/reordered message) is a safe reshape; every genuinely new
  location is traced individually to ground truth before being accepted
  as expected, not just assumed safe from the raw count alone.
- **Test-coverage measurement (`cargo llvm-cov`)**, added while auditing
  Phase 6/9 - manual review kept finding real bugs the test suite should
  have caught but didn't exercise. Close a file's gaps by writing a test
  for what coverage flags, only after first manually verifying
  correctness.
- **A fresh clone of `phpstan/phpstan-src`**, used to ground every
  numeric/behavioral claim about real PHPStan in Phases 8.5 and 12-18 -
  re-verify against a fresh clone before trusting any of those numbers
  after a long gap, since real PHPStan keeps evolving.

Full detail, adversarial fact-checking history, and a dependency
shortlist for Phases 8-10 live in the planning doc this roadmap was
originally distilled from (kept outside this repo, in the assistant
session that built them).
