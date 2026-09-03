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

- Test suite: 939 unit tests (`src/`) + 26 CLI integration tests + 3
  `main.rs` unit tests, all passing as of Phase 14.5's closing round.
- Piwigo frozen-corpus diagnostic count - the primary false-positive-hunt
  metric (`phpstan-rs.toml`-scoped, `/home/torres/piwigo17-frozen`) -
  has fallen every round: 2312 (Phase 22 start) -> 1280 -> 1021 -> 792 ->
  732 -> 706 -> 580 -> 532 -> 533 -> 512 -> 508 -> 350 -> 295 -> 270 ->
  249 (Phase 35) -> 240 -> 239 (Phase 12) -> 211 (Phase 13) -> 1115 ->
  875 (Phase 14's closing round) -> 453 -> 434 -> 397 (Phase 14.5's first
  three rounds) -> **296 (current, Phase 14.5's closing round)**. The
  211 -> 1115 jump was a real, confirmed *increase*, not a regression:
  `list<T>`/`non-empty-array<T>` now match real PHPStan's own strictness
  rather than this project's usual leniency default, a deliberate,
  explicitly confirmed choice given 1575 first-party `list<`
  occurrences. Phase 14.5's four rounds then closed most of the
  resulting gap: nearly all of it turned out to be this project's own
  type-inference gaps (spread array literals, `array_merge` including
  its own spread-argument form, PHP 8.4 property hooks,
  `preg_match_all`'s per-group arrays, array-append widening - the
  single most common accumulator idiom in PHP, the `sort()` family's own
  dynamic type-specifying extensions, `$x !== []`/static-property
  narrowing, a `try`/`catch` scope-merge gap, a missing `Union` arm in
  the core array-type helpers, `glob()`/`explode()`'s own
  dynamic-return-type extensions, and - by far the single largest fix of
  any round - a method-override return-type check that compared
  docblock-refined types instead of native ones) rather than genuine
  corpus-side `@return list<T>` overclaims.
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
| PHP 8.5's pipe operator (`BinOp::Pipe`) always infers `Mixed` - needs new infrastructure to resolve an arbitrary callable expression to its own return type, which this project has nowhere yet | 13 |
| Broader rule catalog (`DeadCode`, `Comparison`/exhaustiveness, `Exceptions`, visibility/abstract-method completeness) | 15 |
| `continue N`'s own level argument is parsed and stored but never consulted (`break N`'s own level is, since Phase 29) | 15 |
| `Type::Array` has no key-type parameter - a `foreach`'s own `$key`, `array_keys()`'s precise result, or a `preg_match` by-ref `$matches` element, is always `Mixed`/imprecise regardless of how precisely-keyed the real array is. Surfaced repeatedly: Phase 29 (a loop key), Phase 31 (`array_keys()`, partially closed for the literal-shape case only), Phase 32 (the generic fallback), Phase 35 (a repository call finally being checked at all) | 15 |
| No CI-friendly output formats (JSON/checkstyle/junit/...) | 16 |
| No LSP hover/go-to-definition | 17 |
| No extension/plugin API | 18 |

### Known but not yet scheduled to any phase

- **`callable-string`/`trait-string`/`interface-string`/`enum-string`
  stay aliased to plain `string`** (Phase 14) - each names "a string
  believed to hold a specific kind of identifier," with no literal-
  based or narrowing-based way for this project to ever verify content.
- **`array_merge` only tracks list/non-empty markers and shape-merging
  when its arguments are uniformly known-shaped or uniformly list-like**
  (Phase 14.5) - a *mix* of a known `array{...}` shape and a plain,
  unproven `array<T>` argument still degrades to a generic element-type
  union rather than real PHPStan's own full key-aware merge across all
  arguments together; every other stdlib array function beyond the
  seven Phase 14/14.5 cover (`array_values`/`array_filter`/
  `array_map`/`array_combine`/`array_slice`/`array_reverse`/
  `array_unique`, plus `array_merge` itself) remains untouched.
- **Two narrower narrowing gaps found while building Phase 14's own
  sources, deliberately left unfixed for now**: the reversed comparison
  `0 < count($x)` (only `count($x) > 0`/`count($x) >= 1` narrow); and
  `!empty($x)` doesn't further upgrade an already-`non-empty-string`
  value to the stronger `non-falsy-string` inside a nested check after
  `$x !== ''` - both real, but low-value (a rare reversed idiom; a rare
  redundant double-guard pattern) with no corpus evidence calling for
  them.
- **Doctrine's `EntityManager::getReference()`'s real generic signature
  (`@template T of object`, `@param class-string<T> $entityName`,
  `@return T|null`) lives only on the interface it implements**
  (`EntityManagerInterface`) - the concrete class's own method has no
  docblock at all, so this project's method-signature resolution never
  sees the generic bound, falling back to `mixed|null`. Confirmed real
  (`ActivityRepository.php:123`, `CommentRepository.php:80`, Phase 13).
  Fixing it properly means real interface-to-implementation PHPDoc
  inheritance for generics, a sizable feature on its own.
- **No mechanism for multi-variable compound-boolean narrowing** - a
  single `||`-of-`&&`-of-negations condition narrowing several different
  variables at once (`if ((!is_int($a) && !is_string($a)) || (!is_int
  ($b) && !is_string($b))) { continue; }`) isn't recognized at all; the
  existing `Or`-tree handling only unions branches narrowing the *same*
  key. Confirmed real (`CommentRepository.php`'s own
  `findAllWithConditions`/`findList`, 7 sites, Phase 13) - its own
  sizable narrowing-engine feature.
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

## Phase 12 - Bespoke per-function/method type extensions - DONE

See **Open phases** above for what's left (`preg_match`'s remaining
literal-enumeration/tagged-union precision). Shipped: `array_map`/
`array_filter`/`array_reduce` (a two-pass mechanism binding the *other*
argument's element type into a closure's own body-inference before
computing its return type - generic template unification alone can't do
this for an untyped closure param); `sprintf`/`printf` format-string
validation against literal arguments (found and fixed a real unguarded
panic in the `array_filter` special case, and a missing
`non-negative-int` pseudo-type along the way: fixing the latter alone
dropped the full corpus 48496 -> 48243); `array_column`/`array_combine`/
`array_fill`/`array_fill_keys`/`array_chunk`; `is_array`/`is_string`/
`is_int`/`is_callable` narrowing predicates (`class_exists`/`in_array`/
`method_exists`-style existence checks deliberately excluded - they
check "does this exist," not "what type is this value now"); and
(unplanned follow-on) `preg_match`/`preg_match_all`'s by-ref `$matches`
shape, via a new structure-only PCRE parser
(`src/regex_groups.rs`) - see **Open phases** above for the full
breakdown and the one item still deferred.

This same follow-on found and fixed a real, pre-existing gap in
`write_index_into` (`checker/assignment.rs`): it had no `Type::Union`
arm at all, silently leaving a `Union`-typed container's write untracked
(traced live to `PiwigoInfosSender::send`). Fixing that - combined with
`preg_match`'s own new `Union([matched, empty])` unconditional shape
(Finding E) making `Type::Union` values "stick" through reads and writes
instead of collapsing to `Mixed` at the first opportunity - surfaced a
second, more serious real hang: `merge_scopes` (`checker/control_flow.rs`,
run once per `if`/`else`) and `write_index_into`'s new `Union` arm both
call `union_types`, which had no cap on how many distinct members it
would accumulate. A real corpus function with many sequential/nested
branches assigning genuinely distinct array shapes to the same variable
grew that variable's own union member count combinatorially, each
merge's `contains`-based dedup scan (and each member's own recursive
`PartialEq`) growing right along with it - confirmed by attaching `gdb`
to the hung process and reading the actual stack (`Type::eq` called from
`union_types` called from `merge_scopes`/`write_index_into`), not
guessed. Fixed with a defensive `MAX_UNION_MEMBERS` cap in `union_types`
(`checker/expr.rs`) collapsing to `Mixed` past 12 members - the same
`MAX_GROUP_DEPTH`/`MAX_EXPR_DEPTH` widening-for-termination precedent
Phase 20 already established, and a stable fixed point (`union_types`
already treats `Mixed` as absorbing, so a variable that trips the cap
once stays collapsed rather than re-paying the cost every subsequent
merge).

**Known follow-up, deliberately not attempted here**: this cap is the
*minimal correct* fix for the termination risk, not the *most precise*
one. Real PHPStan instead **generalizes** same-shaped array types -
several `ArrayShape`s that share the same key set get merged into one
shape with unioned field types, rather than kept as separate union
members - which would preserve real precision for exactly the
many-branches-same-variable pattern that trips this cap today, instead
of falling back to `Mixed`. Building that generalization is a
meaningfully-sized feature in its own right (deciding when two shapes
are "the same shape family," merging their fields correctly) - the same
scope class as the already-deferred Phase 14 (refinement/accessory
types), not a small tweak to land alongside an unrelated hang fix. Left
as a real, named future item: replace/augment the hard `MAX_UNION_MEMBERS`
cap with shape generalization by shared key-set before falling back to
`Mixed`.

The same corpus re-check (frozen-corpus count 249 -> 241) found and fixed
two more real, pre-existing gaps along the way, both surfaced by Finding
E's `Union`-awareness the same way `write_index_into`'s own gap was:

- **`check_assignment_target`'s `Index` arm only exempted the outermost
  key of a write target from `check_shape_key_access`'s "does this key
  exist" check, not any index nested *inside* that target's own base.**
  `$piwigoInfos['updates'][] = [...]` (`PiwigoInfosSender::send`, a real,
  first-time nested append with no prior flat `$piwigoInfos['updates'] =
  [];`) routed its own inner `$piwigoInfos['updates']` sub-expression
  through `check_expr` as a genuine read, wrongly flagging "no key
  'updates'" the moment `$piwigoInfos`'s `Union`-typed shape started being
  inspected for real. Fixed by recursing through
  `check_assignment_target` itself (not `check_expr`) whenever a write
  target's own base is *another* `Index`, generalizing the existing
  single-level exemption over any nesting depth - the same way
  `bind_assignment_target`'s own `Index` arm already does for the write
  side. Also fixed a second, identical-shape false positive this same bug
  caused (`StatsPageRenderer::getMonthStats`, "no key 'month'").
- **`foreach`'s own element-type inference only ever handled a plain
  `Type::Array` subject, falling to `Mixed` for a `Type::ArrayShape`
  one**, discarding real, known per-field types. Real corpus impact
  (`MaintenanceActionDispatcher::dispatch`): `foreach ($versions as $key
  => $value) { $versions[$key] = preg_replace(..., $value); }` over a
  `$versions` shape whose fields are all plainly `string` - with `$value`
  inferred as `Mixed`, `preg_replace`'s own subject-dependent stub
  returned a broader type that leaked an `array<string>` possibility into
  `$versions`'s own dynamic-key-write-widened type, which then stuck
  around (rather than silently collapsing) once `Union` values stopped
  being bailed on immediately. Fixed by joining every field's own type
  (`union_types`-folded) when the `foreach` subject is a `Type::ArrayShape`
  - exact when every field shares one type (the common case both this bug
  and the fix target), still sound but coarser when they don't.

One further diagnostic traced to an **already-tracked** limitation, not a
new bug: `DerivativeImage::build`'s `getByType()` argument mismatch
(`int|string` where `string` is expected) comes from `array_keys()`'s own
lack of a real key-type parameter, the existing `Type::Array` key-type row
in the Known limitations table above (tracked under Phase 15).

**Follow-on investigation, 2026-09-02**: a further corpus re-check (using
`--config phpstan-rs.toml <explicit files>` for bisection) surfaced a
`CategoryUpdateController::__invoke` false positive ("no key 'visible'")
that looked, at first, like a deep, alarming bug: checking that file
alone (or its whole directory) inferred `$update`'s type correctly, but
adding `src/Piwigo/Category/Projection/Category.php` (the `instanceof`
target it narrows `$category` against) to the *same* invocation changed
the result and silently dropped every union member that included
`'visible'`. That framing turned out to be a real but misleading
symptom, not the actual mechanism - direct tracing (via `gdb`-style
targeted `eprintln!` instrumentation on `always_terminates`, then a
from-scratch minimal repro) found two independent, genuinely
general, always-reproducible bugs, neither specific to cross-file
context at all:

- **`apply_equality_narrowing`'s exclusion branch (`checker/
  narrowing.rs`) treated "the other side of a `!==` comparison has this
  *general* static type" the same as "the other side is one specific,
  already-known value"** - sound for `Null`/`True`/`False`/a literal (its
  only real callers before this), unsound for a general type like `Bool`
  (which covers *two* distinct values): `?bool $a` narrowed to `bool` by
  `$a !== null`, then compared `$a !== $b` (`bool $b`, itself not a
  literal) wrongly collapsed `$a` to `Type::Never` the moment
  `$category->visible`'s real type resolved precisely to `Bool` (which is
  why the two-file case differed - `Category.php` not being checked at
  all left `$category->visible` as `Mixed`, which the existing `target_ty
  == Type::Mixed` bail already skips). A `Never`-typed last statement
  then made `always_terminates` (`checker/control_flow.rs`) wrongly treat
  the whole `if`-branch as dead code, discarding its `$update['visible']
  = ...` write entirely from the merged result. Fixed with a new
  `is_known_single_value` gate: only `Null`/`True`/`False`/a literal
  authorize the exclusion; anything else leaves the narrowed type
  unchanged, this project's usual "don't guess" default. Minimized,
  single-file repro: `function test(?bool $a, bool $b) { if ($a !== null
  && $a !== $b) { $update['flag'] = $a; } echo $update['flag']; }`.
  `narrow_type_to` (the `===`-true-branch counterpart) doesn't share this
  bug - for a non-`Union` original it always returns the target
  outright, sound regardless of whether the target is a literal or a
  general type, since `===` between statically-incompatible types is
  dead code either way.
- **`check_expr`'s own `Ternary` arm (`checker/expr.rs`) wrote
  `merge_scopes(then_locals, else_locals)` back into the *real* `locals`
  afterward, copying `StmtKind::If`'s own pattern for a construct that
  isn't actually a control-flow statement.** A ternary's condition
  narrowing a variable within evaluating its own `then`/`else`
  sub-expressions is correct and already achieved by the existing
  per-branch clones; writing that back into the surrounding scope isn't -
  once the ternary as a whole finishes, only its *value* matters to
  whatever contains it, not which hypothetical branch's narrowing might
  have applied. Real, confirmed impact: `$collected === [] ? null :
  preg_replace('/.../', '', $collected[0])` narrowed `$collected`
  (declared `list<string>`) down to the literal empty shape on the
  condition's own true branch, then permanently unioned that into
  `$collected`'s real tracked type - corrupting a *second*, independent
  evaluation of the same sub-expression (`check_property_assignment`'s
  own redundant `infer_expr` call on the same right-hand side) into
  seeing `$collected[0]` as `Mixed` instead of `string`, defeating
  `preg_replace`'s own subject-type-dependent return-type precision.
  `infer_expr`'s own parallel `Ternary` handling never had this bug - it
  only ever receives `&Locals`, not `&mut Locals`, so it structurally
  can't write back at all; the fix removes the write-back from
  `check_expr`'s side to match. Confirmed dead code, not just
  theoretically unnecessary: removing it left all 816 then-existing
  tests passing unchanged.

Both fixed and verified via the disable-and-confirm-failure technique;
frozen-corpus count fell 249 -> 241 (the equality-narrowing fix, plus
incidentally correcting a `never`-typed leak in
`CategoryService::updateUppercats`'s own diagnostic message) -> 240 (the
ternary fix, closing the false positive it had briefly introduced along
the way). No diagnostic regressed at any step.

**Closing round, 2026-09-02**: the two remaining named follow-ups -
`preg_match`'s own literal-enumeration/tagged-union precision, and the
`union_types`/`MAX_UNION_MEMBERS` array-shape-generalization follow-up -
both closed, plus a third gap found while designing the work (the
already-shipped flat shape treated every optional group uniformly as
"maybe entirely absent," when real `$matches` only ever truncates from
its own trailing end). Grounded directly in real PHPStan source
(`RegexGroupParser.php`, `RegexAlternation.php`, `RegexCapturingGroup.php`,
`RegexArrayShapeMatcher.php`) and verified against real PHPStan 2.2.8's
own `\PHPStan\dumpType()` output and live `php`, not recalled from a
single reading - this caught four real design bugs a first read of the
source missed (a sibling alternation branch's earlier group is
force-filled with `''`, not removed; the single-optional-group "without"
variant is a bare `{0: subject}` shape, not "every group but the target";
`in_optional_quantification` resets at every non-quantification AST node
including a non-capturing wrapper, unlike the broader `optional`
propagation; a lookaround's own interior must never leak into its
enclosing literal computation). Full design in
`plans/linked-sniffing-twilight.md`.

Shipped, as four separately committed and verified pieces
(`src/regex_groups.rs`, `checker/expr.rs`, `checker/narrowing.rs`):

- **Literal enumeration** for a constant-only capture group - `(cat|dog)`
  now infers `'cat'|'dog'`, not generic `string` - via a parallel
  `literal_values: Option<Vec<String>>` computed alongside the existing
  structure scan (concatenation as cross-product, alternation as union,
  bounded quantifiers as repetition, all capped at `LITERAL_ENUM_LIMIT`
  (32)), with `^`/`$`/lookarounds correctly contributing a known empty
  string rather than bailing, and a group repeated more than once
  externally correctly losing its own constant typing.
- **Tagged unions** for the two shapes real PHPStan itself special-cases:
  exactly one optional top-level group, and exactly one top-level
  alternation - full array-shape variants (the group/branch present, or
  not) instead of one shape with a bare `optional` field, including the
  `alternation_wrapped_in_optional_quantifier` signal needed for
  `(?:(a)|(b))?`-shaped patterns real PHPStan still tags.
- **Trailing-vs-interior optional-group fix**: an optional group with
  something *required* after it is always present in `$matches` (`''`
  when it didn't itself capture), not "maybe entirely absent" - only a
  *trailing* optional group can really vanish. `''` is also now a real
  possible value for any optional group except the literal last one,
  independent of trailing/interior.
- **`union_types` array-shape generalization**: `generalize_by_key_set`
  merges `ArrayShape`s sharing an identical field-name set (unioning
  their field types) before the `MAX_UNION_MEMBERS` (12) collapse to
  `Mixed` fires, rather than collapsing unconditionally - closing the
  known follow-up left by this phase's own earlier hang fix (see above).
  Confirmed against the live corpus: removed a false positive in
  `SiteUpdateSubController::handle` where `$counts`'s own fields (written
  across many sequential conditional branches) had been collapsing to
  `Mixed` past the cap, wrongly failing `SiteUpdateView`'s own
  constructor-argument check. Frozen-corpus count 240 -> 239, zero
  diagnostics regressed.

**Deliberately still out of scope**: real PHPStan's own literal-value
refinement into `non-empty-string`/`non-falsy-string`/`numeric-string`
accessory intersection types for a regex group. This conflicts with an
existing, deliberate, evidence-backed project-wide policy of aliasing
every string-refinement pseudo-type to plain `Type::String`
everywhere (`types.rs`'s `parse_type_string`) - building it just for
regex groups while every `@param non-empty-string $x` elsewhere keeps
resolving to plain `string` would be an inconsistent special case.
Sequenced as Phase 14's own project-wide effort instead (new refinement
`Type` variant(s), `is_subtype`/`union_types` rules, PHPDoc's own
pseudo-type parsing), with a short follow-on afterward to have this
phase's own literal-vs-plain dispatch use those types instead of falling
back to `string` in the non-literal case.

## Phase 13 - Narrowing precision and remaining false-positive causes - DONE

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
**Closing round, 2026-09-02**: every remaining item on this phase's own
list was checked directly against current code and the live frozen
corpus (239 diagnostics) before any fix was designed - most turned out
to already be fixed in earlier phases, with this list simply never
updated: ternary branch narrowing (`check_expr`/`infer_expr`'s own
`Ternary` arms, correct since Phase 8), `always_terminates` recognizing
`switch`/`try`/`match` (fully implemented since Phase 8), `for`/
`foreach`/`switch` sharing one unforked scope (false - all three already
fork/narrow per-branch, Phase 29), and array-literal element types not
tracked (false - `infer_array_literal_type` already builds a full
`ArrayShape`, since Phase 8).

Five real findings shipped:

- **Finding A - `getenv()`'s dynamic return type.** The real stub
  declares `getenv(): array|string|false` unconditionally; real PHP only
  returns `array` for a truly zero-argument call, and only `string|
  false` when `$name` is a real, non-null string (verified against real
  PHPStan's own `dumpType()`, including the easy-to-get-wrong
  `getenv(null)`/`getenv(local_only: true)` cases, both of which stay
  broad). Traced every `getenv(` call site in the corpus by its actual
  data flow: 26 of 239 diagnostics shared this one root cause (21
  named-argument sites, 5 zero-argument `array_merge(getenv(), ...)`
  sites) - by far the largest single fix of this round, and the same
  root cause behind both `strtotime` sites and both `count()`-adjacent
  sites this phase's own "batch" item had named.
- **Finding B - `is_countable()`** missing from the narrowing-predicate
  table, closing a real `count()`-argument false positive
  (`SectionPopulator::computeMetaRobots`, the only real corpus call
  site).
- **Finding C - `BinOp::Div`** hardcoded `Type::Mixed`; real PHPStan
  gives `int|float` unconditionally unless `float` is provably involved
  (verified via `dumpType()`, including the fully-unknown-operand case).
  Not diagnostic-count-neutral by design (today's `Mixed` was silently
  compatible with everything, so no real division site had ever been
  type-checked before) - one site dropped as a bonus
  (`IntroSubController.php:598`'s own `filesize: mixed` field, fed by a
  division result that used to be silently `Mixed`).
- **Finding D - `@property`/`@property-read`/`@property-write`**
  class-level PHPDoc tags, entirely unparsed until now. Structurally
  identical to the already-parsed `@var Type $name`; wired into the same
  property map real fields use, so existing inheritance-aware lookups
  pick it up for free. `vendor/php-openapi/openapi`'s own `OpenApi`
  class declares `@property Paths $paths` - confirmed via a minimal
  repro that this specific corpus diagnostic (`tools/
  openapi-generate-input-dto.php:95`) still doesn't clear even with this
  fix, traced to a separate, pre-existing gap: the receiving variable's
  own type is a `SpecObjectInterface|OpenApi` union (from a library
  author's plain `@return` fallback for tools that don't parse
  `@phpstan-return T` generics, which this project doesn't), and the
  interface half of that union genuinely has no `$paths` of its own - a
  real "every union member needs the member" correctness rule, not a
  bug this fix could have closed. The feature itself is real and
  verified via four new tests.
- **Finding E - trait method bodies checked once per using class**, not
  standalone. `flatten_traits` already merges a trait's own members into
  every using class before checking starts, so no lookup logic needed to
  change - only *which context* a trait's methods are checked under. A
  new `Registry::classes_using_trait` walks past intermediate trait
  users too (a trait used only by another trait still resolves against
  the real, final class). A trait used by 2+ classes now gets checked
  2+ times, so a host-independent bug in its own body is deduped by
  `(span, message)` rather than reported once per host; an orphaned/
  library trait with no real user still gets checked standalone, exactly
  as before. **Lowest confidence of the five**: zero traits exist
  anywhere in this corpus, so verified only via six new tests, never
  against real corpus behavior.

Confirmed real but deliberately left open, each named precisely rather
than left in a vague "batch": `BinOp::Pipe` (zero corpus usage, needs new
callable-resolution infrastructure); `StmtKind::Try`'s own catch-merge
gap (a false negative, not detectable by corpus diffing); a cluster of
`__construct()` mismatches individually traced to three already-tracked/
documented boundaries (`array_keys()`'s key-type imprecision, Phase 15;
a local `@phpstan-type` alias, a permanent design boundary) plus two
genuinely new gaps (Doctrine's `EntityManager::getReference()` generics
living only on its interface; `CommentRepository`'s own multi-variable
compound-boolean narrowing) - see the Known limitations table and
"Known but not yet scheduled" list above for all of these.

Corpus count: 239 -> 213 (Finding A) -> 212 (Finding B) -> 211 (Finding
C) -> 211 (Findings D and E, unchanged as expected). Zero regressions at
any step.

## Phase 14 - Refinement/accessory types - DONE

Real PHPStan's own representation (confirmed by reading `vendor/
phpstan-src/src/Type/Accessory/*.php` directly): a *string* refinement
is `IntersectionType([StringType, AccessoryXType])`; an *array*
refinement (`list`/`non-empty-array`) is `IntersectionType([ArrayType,
AccessoryArrayListType/NonEmptyArrayType])`; an *int* range
(`positive-int`, `int<min, max>`, ...) is its own standalone
`IntegerRangeType`, not an accessory. Built by reusing this project's
existing `Type::Intersection` variant (PHP 8.1 intersection types)
rather than inventing a new mechanism - its `is_subtype`/`is_assignable`
rules already generalized correctly for a marker-plus-base shape.

New `Type` variants: `IntRange(Option<i64>, Option<i64>)` (fully
replaces `Int`'s own range, the same relationship `IntLiteral` has to
`Int`); `FloatLiteral(f64)` (mirrors `IntLiteral`/`StringLiteral` for a
PHPDoc float-literal union); `Accessory(AccessoryKind)` (`NonEmptyString`/
`NonFalsyString`/`LiteralString`/`NumericString`/`ArrayList`/
`NonEmptyArray`, only ever meaningful inside an `Intersection`).
`parse_type_keyword_or_class` now constructs these instead of aliasing
every refinement pseudo-type to its plain base; `callable-string`/
`trait-string`/`interface-string`/`enum-string` stay aliased (no
verifiable content to attach a real type to).

Four narrowing sources added, each verified against real PHPStan's own
`dumpType()`: `$x !== ''` narrows a `string` to `non-empty-string`;
`!empty($x)` narrows a `string` to `non-falsy-string` (not merely
non-empty - `empty()` is also false for `'0'`) and an `array` to
`non-empty-array`; `count($x) > 0`/`count($x) >= 1` also narrow an
`array` to `non-empty-array`; `is_numeric($x)` narrows a `string` to
`numeric-string`, unblocking its own already-shipped narrowing code
(Phase 32 Finding G), which had documented itself as blocked on this
exact type not existing yet. ROADMAP's own prior suggestion that
`trim()` had a "known-safe case" worth narrowing on was checked
directly and found empirically false - trimming can strip an already-
non-empty string down to `""` regardless of its input.

**The largest real risk turned out to be structural, not conceptual**:
grepping every direct `Type::Array`/`Type::ArrayShape` pattern match in
the checker found 25 real call sites that stopped recognizing a
`list<T>`/`non-empty-array<T>` value as an array at all the moment it
became a real `Intersection` instead of plain `Array(T)` - confirmed
live (`foreach` over a `list<T>`-typed variable silently inferred
`Mixed`). Fixed via two shared helpers (`array_element_type`,
`is_array_like`) adopted across `foreach`, the array-union `+`
operator, `??`'s own array-widening, dynamic-key array writes and
destructuring assignment, `sprintf`'s own format-string check (an
active regression in already-shipped bug-catching, not just missed
precision), and by-ref argument widening - most of the remaining 25
sites already recursed into `Intersection` generically via an existing
`Union` sibling arm and needed no change at all, confirmed by reading
rather than assumed. A second, more severe version of the same class of
bug was caught by the corpus re-check itself: `registry::is_assignable`
was missing the same dedicated Intersection-vs-Intersection rule
`types::is_subtype` got, so its own generic dispatch intercepted a
same-shaped pair (even `list<string>` vs `list<string>`) before ever
reaching the correct logic - unfixed, this alone inflated the corpus
count to 1861 before the real, intended ~875 was reached.

Six bespoke array-function special cases, each verified via
`dumpType()`: `array_values` (new - always `list<V>`, any input shape),
`array_unique` (new - never preserves list-ness), `array_filter`/
`array_combine` (existing - now explicitly strip any accessory marker
instead of passing it through unchanged), `array_map`/`array_slice`/
`array_reverse` (the latter two new - preserve the input's own markers
with default arguments only, confirmed to break with `preserve_keys:
true`). `array_merge` and every other stdlib array function beyond
these six are left untracked - a real, open-ended surface with no
bounded stopping point.

**A deliberate, explicitly confirmed decision, not a default**: a plain
`Type::Array(_)` - even with a known element type - never satisfies
`list<T>`/`non-empty-array<T>`, matching real PHPStan's own confirmed
strictness (verified live: even a known-element `int[]` fails a
`list<int>` sink) rather than this project's usual leniency-for-
uncertainty default. Confirmed corpus-scale, not theoretical, before
the decision was made: `list<` appears 1575 times across 463 first-
party files. The corpus re-check after this decision landed showed
exactly the scale of impact predicted - 211 -> 1115 diagnostics, the
large majority real `@return list<T>` declarations whose method body
only provably returns `array<T>` - explicitly reviewed and confirmed as
the accepted, correct outcome of the decision, not a regression,
before continuing to build the rest of the phase on top of it. Task 7's
own array-destructuring fixes then resolved a large share of that
(1115 -> 875), the rest left as real, uncorrected signal about the
corpus's own type-precision gaps rather than individually triaged here.

## Phase 14.5 (unplanned) - list<T> false-positive cleanup - DONE

A round of bucketing the Piwigo corpus by normalized message shape (the
same Phase 22 methodology) against Phase 14's own 875-diagnostic
closing figure, on the premise that "real, uncorrected signal about the
corpus's own type-precision gaps" deserved actual triage rather than
staying an assumption. It mostly wasn't the corpus - five real gaps in
this project's own type inference, each verified against real
PHPStan's own `dumpType()` before being fixed:

- **A spread array literal** (`[...findA(), ...findB()]`) gave up
  entirely, always inferring bare `Array(Mixed)`. Now tracks list-ness/
  shape precision through: two `list<T>`-typed spread sources stay a
  `list<T>`; a plain `array<T>` source degrades list-ness for the whole
  result; a spread source with fully-known fields (an `ArrayShape`)
  merges field-by-field (canonical int keys renumbered, others kept and
  overwritten) rather than degrading - real code building one value
  from another's own `toArray()` shape plus an extra field
  (`UserInfoWithThemeName::toArray()`).
- **`array_merge()`** fell through to its bare `array` native stub,
  discarding all precision. Now merges two known `array{...}` shapes
  field-by-field via the same rule the spread literal above uses
  (shared via a new `merge_shape_field` helper - `Logger::__construct`
  merging a property's own default options shape with a caller-supplied
  one); merges list-like arguments into a `list<T>`; degrades to a
  plain `array<T>` the moment one argument is an unproven plain array.
  General key-aware shape merging when arguments are a *mix* of known
  shapes and plain arrays remains unhandled, along with every other
  stdlib array function beyond the six Phase 14 already covers.
- **A PHP 8.4 property hook's own `set(Type $value)`** was never
  consulted at all - a property write was always checked against the
  property's own declared type, even when an explicit `set` hook
  deliberately accepts a wider native type and coerces internally
  before the real assignment (`CurrentConfig`'s own
  `apiKeyDuration`/`chmodValue`/`defaultFiltersViews`/`filterPages`/
  `randomIndexRedirect`). `PropertySig` gained `set_ty: Option<Type>`,
  populated from the hook's own native parameter type hint (its doc
  comment isn't captured by the parser at all, so no `@param` is
  available - not needed here, since every corpus case already
  declares a wide-enough native hint on its own).
- **`tempnam()`** carries its own real PHPStan dynamic-return-type
  extension beyond its plain `string|false` native stub - a successful
  call always returns a real `non-falsy-string`, confirmed via
  `dumpType()`. The common `$jar = tempnam(...); if ($jar === false) {
  throw ...; }` idiom feeding a `non-empty-string`-typed sink afterward
  was the single largest non-array-related false-positive bucket.
- **`preg_match_all`'s own per-group match array** (`$matches[N]`) was
  built as a plain `Array`, never marked list-like, despite always
  being real, sequentially int-keyed from 0 (one slot per match) -
  confirmed via `dumpType()`.
- **Array-append (`$arr[] = value`) onto an already list-like
  container** (including a freshly-empty `[]` literal) unconditionally
  dropped list-ness, treating a real append exactly like an arbitrary
  dynamic-key write. Real PHP's own append semantics always assign the
  next sequential int key, so the result stays a real, now-provably-
  non-empty list - confirmed via `dumpType()`. This is the single most
  common accumulator idiom in PHP (`$result = []; foreach (...) {
  $result[] = ...; } return $result;`), and by far the largest single
  fix of this round: 771 -> 453 diagnostics on its own.

Every fix was verified via the same location-based diff this project
always uses (a removed diagnostic's location sampled and traced back to
its real cause; a new location individually investigated before being
accepted) - across all five fixes, only two genuinely new locations
ever appeared (both immediately investigated and fixed as part of the
same round: the spread-literal fix's own field-merge gap, and the
array_merge fix's own shape-merge gap), and zero remained unexplained
at the end.

The "array_merge's own key-renumbering isn't tracked" item in **Known
limitations** below is updated to reflect what this round actually
closed versus what's still open.

**A second round** (453 -> 434) found three more gaps, two of them
surfaced only *because* the first round's own increased precision made
them reachable for the first time:

- **The `sort()` family's own real dynamic type-specifying extensions**
  weren't modeled - the generic by-ref-widening rule (Phase 22 Finding
  3) always overwrote a variable's tracked type with the callee's own
  declared parameter type, correct for a real mutation but wrong here,
  since every one of these natively declares a plain `array &$array`
  regardless of what it actually does. Verified via `dumpType()`:
  `sort`/`rsort`/`usort`/`shuffle` always reindex to a real list,
  preserving the input's own element type; `asort`/`arsort`/`ksort`/
  `krsort`/`uasort`/`uksort`/`natsort`/`natcasesort` preserve the exact
  key structure - and so the exact type - unchanged. An extremely
  common idiom (117 first-party files); this fix alone was 453 -> 442.
- **`$x !== []` never excluded the empty shape at all** - the sort()
  fix's own re-verification surfaced it: `is_known_single_value` didn't
  recognize an empty array literal as exhaustively single-valued the
  way it already does a literal string/int. Fixed by extending it, plus
  a `narrow_type_excluding` arm sharing `upgrade_array_to_non_empty`
  with `!empty($x)`/`count($x) > 0`.
- **`array_merge(...$listOfLists)`** - a spread *argument*, not a
  spread array-literal item - was treated as if the whole array-of-
  arrays were one merge argument, giving `list<list<int>>` instead of
  the real `list<int>` (`SearchService::searchAllwords` flattening a
  `word=>list<int>` map's own values). Fixed by unwrapping one level for
  a spread argument specifically.
- **A `catch` clause's own ending state was discarded entirely** after
  the whole `try`/`catch` - `locals` only ever kept the try body's own
  final state (`$body = null; try { ...; } catch (...) { $body = "x";
  }` then reading `$body` afterward, real `RedirectServiceTest` code).
  Fixed the same way every other control-flow join in this checker
  works: merge the try body's own full-completion state with each catch
  clause's, excluding a branch that always terminates - the same
  if/else guard-clause treatment. This ROADMAP had tracked the missing
  merge as a real gap but assumed it only produced a false *negative*,
  undetectable via corpus diffing; found instead to be a real,
  corpus-confirmed false-positive cause too, and removed from **Known
  limitations** below.

Three more new locations surfaced along the way, each individually
traced rather than assumed: two are real, latent true positives the
round's own improved precision newly surfaced (one already covered by
the "no multi-variable compound-boolean narrowing" limitation below;
one confirmed matching real PHPStan's own behavior directly) - both
accepted, neither a phpstan-rs bug; the third (the malformed-looking
`array_merge(...$listOfLists)` output) was the spread-argument bug
above, fixed rather than accepted.

**A third round** (434 -> 397) found one foundational gap plus three
more bespoke stdlib functions, again mostly surfaced by fixing the
foundational one:

- **`array_element_type`/`is_array_like`/`is_list_like`/
  `is_known_non_empty_array` had no `Type::Union` arm at all** - the
  single most common way a real variable's type actually arrives: a
  control-flow merge (`$x = []; if (...) { foreach (...) { $x[] = ...;
  } }` leaves `$x` a `Union` of the "0 iterations" `array{}` branch and
  the "≥1" real list branch). Every one of `sort()`/`array_merge()`/
  `foreach`/etc. silently stopped recognizing such a value as an array
  at all - the same class of regression Phase 14's own Task 7 audit
  exists to prevent, just for `Union` instead of `Intersection`. An
  empty `ArrayShape` branch contributes no real element-type constraint
  and is skipped in the merge rather than folded in as its own
  degenerate `Mixed` (which would otherwise collapse the *whole* merged
  element type down to `Mixed` the moment any branch was ever empty).
  Found via `LatteTemplateFiles::discover`'s own real if+foreach+append+
  `sort()` pattern.
- This fix's own re-verification found the identical pattern it fixes
  elsewhere, twice: **`array_keys()`'s native stub declares `@return
  int[]|string[]`** - real but useless without this project's own
  key-type tracking (`Type::Array` has no key-type parameter at all, a
  known limitation, Phase 15). Previously harmless (`array_element_type`
  gave up entirely on the resulting `Union`), the fix above made it
  start resolving to a real `int|string`, surfacing as a regression
  everywhere a caller read one of the keys (`opcache-preload.php`'s own
  `foreach (array_keys($classMap) as $symbol) { str_starts_with($symbol,
  ...); }`, real corpus code). Falls back to this project's own usual
  `Mixed` key-type default for a non-literal array argument instead of
  the stub's own misleadingly precise answer. **`array_filter()`'s own
  explicit null-check callback** (`fn ($v) => $v !== null`) is a real,
  separately-recognized PHPStan extension that strips `Null` from the
  retained element type, not just the accessory markers every
  `array_filter` call already loses - verified via `dumpType()`. Real
  Piwigo code (`GalleryController`) builds its own page-items list
  exactly this way.
- **`glob()`** carries its own real dynamic-return-type extension beyond
  its plain `array|false` native stub - a successful call always
  returns a real `list<string>`, confirmed via `dumpType()`
  (`StructuralTest`'s own `findCallSitesInRootPhpFiles`/
  `findCallSitesInBinFiles`, narrowing away `false` via `$paths !==
  false ? $paths : []`).
- **`explode()`** never actually returns `false` at all on a modern PHP
  target - the vendored stub's own `@return string[]|false` reflects PHP
  7's behavior for an empty `$separator`; PHP 8+ throws a `ValueError`
  there instead. Real PHPStan's own dynamic-return-type extension drops
  `|false` entirely and always answers `non-empty-list<string>`,
  verified via `dumpType()` (even for a non-literal `$separator` - there
  is no other way it can return fewer than 1 element). Real Piwigo code
  (`GeneratedFileErrorMapper::lines`) relies on exactly this.

**A fourth round** (397 -> 296) found two gaps unrelated to `list<T>`
itself - both pre-existing, surfaced only because `list<T>` becoming a
real, distinguishable type made them consequential for the first time:

- **`check_method_overrides`'s own return-type check compared the
  *docblock-refined* return type on both sides**, wrongly flagging any
  override that faithfully keeps the ancestor's own native type while
  the ancestor's docblock separately narrowed it further - real PHP
  itself only enforces override-compatibility (Liskov) on the native
  signature, and by default so does real PHPStan
  (`checkPhpDocMethodSignatures` is off unless a project opts in,
  confirmed neither Piwigo's own `phpstan.neon` nor this project's own
  corpus config does, and confirmed directly against real PHPStan with
  the exact shape found). `FunctionSig` gained `return_type_native:
  Type`, computed alongside the existing docblock-merged `return_type`
  and threaded through `merge`/`substitute_class_templates`; the check
  now compares this native-only view on both sides instead. By far the
  single largest fix of any round so far: 397 -> 302 diagnostics on its
  own, all of them real - dozens of Piwigo `*View` classes each
  implementing a shared rendering interface's own
  `pageAssets()`/`exposedStrings()`, correctly matching the interface's
  native `: array` while the interface's docblock alone said `list<T>`.
  A companion test now confirms a genuine native-level incompatibility
  is still caught - this check had no positive-case coverage at all
  before.
- **A static property (`self::$prop`) had no narrowing key at all** -
  only an instance property's `->` chain was handled, so `if
  (self::$prop !== null) { return self::$prop; }` never narrowed away
  `null`. Neither `infer_expr`'s nor `check_expr`'s own `StaticAccess`
  handling consulted `locals` for a narrowed override the way
  `PropertyAccess` already does, even once `narrowing_key` recognized
  the shape - both fixed the same way `PropertyAccess` already works;
  the write side needed no separate wiring, since the generic
  purge-and-update in `bind_assignment_target` already keys off
  `narrowing_key` uniformly. Real Piwigo test code
  (`BrowserTestHelpers::adminSessionCookies`) relies on exactly this.

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
