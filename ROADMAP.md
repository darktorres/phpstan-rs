# phpstan-rs roadmap

A from-scratch Rust PHP parser + type checker, aimed at growing toward
PHPStan feature parity. Phases 8+ are grounded in direct inspection of
`phpstan/phpstan-src` and `jetbrains/phpstorm-stubs` (not recalled
knowledge) and verified at real scale against Piwigo's own codebase
(`src/`, `tools/`) plus its `vendor/` tree (18923 files combined).

Status key: **DONE** / **PARTIAL** (some of the phase's own scope
shipped, some didn't) / **NOT STARTED** / **SKETCHED** (direction only,
no detailed plan yet).

## Phase 1: PHPDoc doc comments - DONE

Capture and parse `/** ... */` doc comments (`@param`, `@return`,
`@var`).

## Phase 2: PHPDoc types - DONE

Wire `@param`/`@return`/`@var` into the registry; array shapes and
generic type arguments; `@template` generics with call-site
substitution (function/method level).

## Phase 3: Flow-sensitive narrowing - DONE

Narrow variable types across `if`/`while` branches (`instanceof`,
equality checks, common predicate functions).

## Phase 4: Properties and class context - DONE

Track property types and constructor-promoted properties; trait `use`
support; resolve `self`/`static`/`parent` against real class context;
wire property types into the checker.

## Phase 5: Built-in stub generation - DONE

Generate stub signatures from real php-src source (superseded by
Phase 8's broader phpstorm-stubs corpus).

## Phase 6: Undefined-variable detection - DONE

Undefined-variable detection with real by-ref tracking; bind
`foreach`/`catch`/destructuring targets to real types; array/shape
index access (infer real types, catch key typos).

**Re-audited 2026-08-30** via test-coverage measurement (`cargo
llvm-cov`, newly set up this session): found and fixed the single
largest gap of this whole project's history so far. `Expr::Closure`,
`Expr::ArrowFn`, and `Expr::Match` had **no handling anywhere** in
`checker.rs` - not `check_expr`, not `infer_expr`. Every diagnostic
this checker has was completely invisible inside any closure, arrow
function, or `match` expression body - confirmed on a minimal repro
(two genuinely undefined variables inside a plain closure and an arrow
function produced zero diagnostics) and on real code (first-party
diagnostics in Piwigo + tools/ rose from 2221 to 2321, all spot-checked
genuine, once closures/arrow fns were finally checked).

Fixed with real PHP capture semantics modeled correctly: a closure
only sees `use (...)`-listed variables (not the enclosing scope) plus
`$this` auto-bound for a non-static closure inside a method; an arrow
function sees the whole enclosing scope by value (real PHP's implicit
capture, no `use` needed). `match` now infers a real union of its arm
result types instead of always `Mixed`.

Found two more real, independent bugs while fixing this:
- `check_function` bound `$this` for *any* method with a class context,
  never checking if the method was actually `static` - a static
  method's own `$this->prop` reference was never flagged, when real
  PHP fatal-errors on it.
- Closure/arrow-fn param and return type hints were parsed with a bare
  `types::parse_type_string`, bypassing namespace resolution entirely -
  a `static fn(array $row): ActionCount => new ActionCount(...)` (a
  real, `use`-imported class) produced a false-positive mismatch
  comparing its correctly-resolved body type against an unresolved
  bare name. Fixed by routing every closure/arrow-fn type hint through
  the same `NameContext`-aware resolution real declarations already
  get.

Chasing the resulting diagnostic-count changes on real code surfaced
two more independent, pre-existing bugs, unrelated to closures but
newly visible once closure bodies started getting checked at all:
- `docblock::parse` worked strictly one physical line at a time, so a
  `@return`/`@param` type wrapped across multiple lines - a real,
  common style for documenting a large array-shape DTO - got truncated
  mid-shape with an unclosed brace, then namespace-mangled into
  nonsense. Fixed by joining a tag's continuation lines before parsing.
- Array-shape parsing aborted the *entire* shape (discarding every real
  field) on hitting PHPStan/Psalm's `...<K, V>` "unsealed shape" marker,
  which has no `:` in it. Fixed by skipping just that one unmodeled
  field instead of failing the whole parse.

All five fixes verified against the real 18923-file Piwigo+vendor
corpus after every change; the corpus's stable 3 first-party
undefined-symbol findings were unchanged throughout, and the specific
false-positive pattern from each bug was confirmed gone by the final
check.

## (Unplanned) Real-world hardening pass - DONE

Not a numbered phase - a detour to run the parser against real PHP:
Piwigo's own codebase (1490+ files), php-src's own `Zend/tests`+
`tests/lang` corpus, and Piwigo's composer `vendor/` tree (17387
files), fixing dozens of parser/lexer gaps (PHP 8.1 enums, DNF types,
`declare()` blocks, goto/labels, asymmetric visibility, PHP 8.5
clone-with syntax, and more) until only a handful of deliberately
obscure edge cases remained. Added the `--check` batch-mode CLI flag
to support this.

## Phase 7: LSP / incremental / tooling - DONE

Incremental multi-file `Project` type (re-check one changed file
without a full reload); a transitive cross-file dependency/invalidation
graph (BFS fixed-point over "declares -> is referenced by -> declares
-> ..."); a real Content-Length-framed JSON-RPC LSP server
(`phpstan-rs --lsp`), verified end-to-end against the real wire
protocol. Added the project's first dependency (`serde`/`serde_json`).

Known limitation carried forward: `Diagnostic` has no real source
position - every diagnostic is anchored at line 1 (Phase 11's job).

**Re-audited 2026-08-30, found and fixed a real regression** introduced
by Phase 9.5 (not caught by the unit test suite, since its own
invalidation tests happen to use non-namespaced PHP snippets where the
bug is invisible): `declared_names` started returning fully-qualified
names to fix registry removal, but the invalidation BFS looks names up
in `dependents`, which stays keyed by bare names on purpose. Feeding an
FQN into a bare-keyed lookup found nothing for any namespaced class -
confirmed live against Piwigo, editing a 247-reference class stopped
cascading to any dependents at all. Fixed by converting back to the
bare last segment at the three points feeding that lookup; re-verified
the exact real-world scenario recorded when this phase was first built
(editing `AbstractNamedCachePool` cascades to 344 files again).

## Phase 8: Ingest `jetbrains/phpstorm-stubs` - DONE

Real PHPStan's actual stub dependency is `jetbrains/phpstorm-stubs`,
not php-src's own `.stub.php` files (a wrong assumption in an earlier
draft of this plan, corrected before implementation). Vendors the 558
real stub files (of 1052 total - the rest is phpstorm-stubs' own test
suite, plus a handful of non-stub housekeeping files), generated via
`build.rs` into a `PHPSTORM_STUBS_SOURCES` list at compile time.
`php_stubs.rs`'s existing `generate_from_source` pipeline needed no
changes - it now resolves 1000+ real function signatures instead of
two hand-picked extensions (`ctype`, `json`).

## Phase 9: Undefined-symbol rule family - DONE

**Shipped:**
- `ClassInfo::consts` and `Registry::lookup_const` (were entirely
  absent); class/interface extraction from the phpstorm-stubs corpus,
  so built-ins like `Exception`/`ArrayObject` are known as classes, not
  just function sources.
- Undefined-class detection on `Expr::New`, plus constructor argument
  count/type checking.
- Undefined-function detection on `Expr::Call`, including PHP's own
  global-namespace fallback for an unqualified call.
- Undefined-method detection on `Expr::MethodCall` (with a `__call`
  magic-method exemption, matching real PHPStan).
- **Undefined-property-on-read** detection (`Expr::PropertyAccess`),
  with a `__get` exemption. Deliberately asymmetric with the existing
  write-side check (a write can legitimately create a real, if
  deprecated, dynamic property; a read of one never declared or
  dynamically assigned just returns null with a warning in real PHP,
  so there's no case to stay lenient for).
- **`Expr::StaticAccess`/`StaticCall` existence checks** - class
  constants, static properties, and static methods (with a
  `__callStatic` exemption), self/parent/static-aware, mirroring
  `MethodCall`'s own structure and leniency. `Foo::class` and a fully
  dynamic member (`Foo::{$e}`/`Foo::$$e`) are both skipped, matching
  the parser's own documented encoding of those two cases; a dynamic
  class target (`$obj::method()`) is skipped too, matching `Expr::New`'s
  existing precedent for the same situation.

Along the way this phase found and fixed several real, independent
bugs (not hypothetical - each confirmed via a minimal repro against
real code before fixing): leading-backslash class/function references
were never normalized; `use`/`use function` imports were parsed but
never read, and only the *first* import in a group/comma-list `use`
was even kept; `Registry::index` never recursed into `if`/block bodies,
missing the extremely common `if (!function_exists(...)) { function
foo() {} }` polyfill idiom used throughout the ecosystem.

**Finishing this phase (2026-08-30) found 3 more real, substantial
gaps**, each surfacing as a wave of false positives on the very first
real-corpus check after shipping the new rule, each fixed before the
rule could be considered done:
- Backed enums' built-in `->value` (and every enum's `->name`) weren't
  registered as properties at all - provided by the language itself
  (`UnitEnum`/`BackedEnum`), not declared in source. ~2800 false
  positives, one per real `$case->value`/`$case->name` access, before
  synthesizing both onto `ClassInfo::from_enum_decl`.
- `&&`/`||` short-circuit narrowing wasn't modeled at all - neither for
  checking the right operand, nor for composing the enclosing `if`/
  `while` body's narrowing from a multi-part condition. `if ($x !==
  null && $x->prop)` (an extremely common guard idiom) checked its
  right side against the un-narrowed left-hand type. Fixed in both
  `check_expr` (short-circuit-aware traversal) and `apply_narrowing`
  (composes `&&`'s operands on its true branch, `||`'s on its false
  branch - the unrepresentable opposite branch is left alone for both,
  the same conservative choice used elsewhere).
- `apply_narrowing`'s `instanceof` handling only ever narrowed a bare
  variable, never a property-access chain (`$arg->value instanceof
  Variable && $arg->value->name` - a real, common AST-visitor idiom,
  caught on this project's own `tools/` directory). Fixed with a new
  `narrowing_key` helper extending the existing `locals` map (keyed by
  bare variable name today) to also carry narrowed property-chain
  types, via a collision-free `base->prop` key format.
- An enum's built-in `cases()` (`UnitEnum`) and a backed enum's
  `from()`/`tryFrom()` (`BackedEnum`) weren't registered as methods
  either, the exact same gap as the properties above - every real
  `Enum::cases()`/`::tryFrom()` call in the corpus was a false positive
  until fixed the same way.

Verified against the real 18923-file Piwigo+vendor corpus throughout:
first-party (`src/`+`tools/`) false positives from every one of these
new rules are 0, and the stable 3 undefined-symbol findings held
unchanged across all of it. Remaining vendor/-only noise traces to
distinct, understood, out-of-scope-for-now root causes: narrowing
mechanisms not modeled (equality/type-check narrowing on a property
chain, not just `instanceof`), `self::`/`static::` forwarding calls to
a method inherited from an ancestor a particular `vendor/` snapshot
doesn't fully resolve, traits referencing a constant/property/method
their eventual host class provides, and `@property` PHPDoc tags (not
parsed at all).

## Phase 9.5 (unplanned): Namespace-aware registry - DONE

Undefined-method detection was built, then reverted after real-scale
testing found it unsafe: the registry had no namespace modeling at
all, so a class-name collision between two unrelated packages (real,
quantified example: 1595 distinct colliding class names across Piwigo
+ its `vendor/`) could silently overwrite one class's real method
table with an unrelated one's.

Fixed properly: `registry::NameContext` (namespace + `use`-import map,
tracked *positionally* per file, since real files can have multiple
sequential `namespace { ... }` blocks) resolves every class/function
reference to a fully-qualified name before any registry key is read or
written. Undefined-method detection was re-enabled once this landed,
verified safe at scale (22,900 false positives -> 3, all confirmed
genuine).

Found and fixed 4 more independent real bugs while verifying this:
multi-parent interface inheritance walking (`lookup_method` etc. only
ever walked a single parent, wrong for interfaces which can `extends`
several); a real parser **operator-precedence bug** (`!` bound tighter
than `instanceof`, backwards from real PHP, silently breaking the
`if (! $x instanceof Y) { throw; }` guard-clause idiom); two PHPStan/
Psalm pseudo-type name collisions (`list`, `object`) that fell through
to being treated as literal class names.

**Re-audited 2026-08-30** with fresh namespaced adversarial tests
(existing tests for trait flattening, `@template` substitution, and
`instanceof` narrowing all happen to use non-namespaced PHP - the exact
blind spot that let the Phase 7 regression through). Trait flattening
and function-level `@template` substitution both check out fine. Found
and fixed 4 more real bugs, all variations on the same theme -
something that used to be a harmless bare name became visibly wrong
once namespace-qualification existed to mangle it:

- `lookup_own_signature`'s top-level-function branch looked functions
  up by their raw AST name, not their FQN (`Registry::index`'s actual
  key since Phase 9.5) - silently found nothing for any namespaced
  function, breaking type-checking accuracy inside its own body. Found
  via a new cross-file test combining a namespace + `use` import +
  method-call checking - a combination no existing test exercised.
- `apply_narrowing`'s `instanceof` handling treated `self`/`parent`/
  `static` as ordinary bare class names instead of resolving them
  against the enclosing class like every other such site already did
  (real code: amphp/amp's `if ($future instanceof self)` narrowed to a
  nonsensical `Amp\self`). This one looked like *silence*, not a wrong
  diagnostic, which is why nothing caught it before.
- `@return $this` (a real, PHPStan-documented convention identical to
  `@return static`) and class-constant-reference types like `self::RFC_*`
  (PHPStan's wildcard-constant convention) both weren't recognized as
  non-class pseudo-types at all, and got nonsensically
  namespace-qualified once that machinery existed.
- `catch (SomeException $e) { ... }`'s type binding built its
  `Type::Object` straight from the raw AST class name with zero
  resolution applied - not even leading-backslash stripping, the one
  normalization almost every other site in the codebase already had.
  Same silent-not-wrong symptom as the `instanceof self` bug above,
  caught the same way: a namespaced test asserting a positive
  diagnostic, not `d.is_empty()`. Found by a systematic sweep of every
  remaining `Type::Object(` construction site in `checker.rs` once the
  other four bugs above made the pattern obvious to look for.

## Phase 10: Class-level generics - DONE

`Type::Object` now carries real type arguments (`Collection<User>`
parses to `Object("Collection", [Object("User", [])])`, previously
discarded). A class's own `@template T` is recognized across all its
methods/properties (a real, previously-missing parser capability was a
prerequisite: `ClassDecl` never captured its own doc comment at all).
Type arguments substitute correctly at every method-call/property-
access/argument-check site. Deliberately not modeling variance,
template bound enforcement, or `@extends`/`@implements`
inheritance-chain substitution.

Found and fixed the same pseudo-type-collision bug again for the whole
PHPStan/Psalm string and int refinement family (`non-empty-string` -
2263 real occurrences in one corpus alone - `class-string`,
`positive-int`, and others).

## Phase 11: Source positions, levels, config - NOT STARTED

- Thread real `(line, column)` spans through the lexer, parser, and
  into `checker::Diagnostic`, replacing today's line-1 placeholder.
- A two-part level system mirroring real PHPStan: discrete rule-gating
  levels (0-6 equivalent) plus a separate strictness-parameter set for
  `is_assignable` (7-10 equivalent).
- A minimal config file and baseline mechanism.

## Phase 12+: Sketched, not detailed - NOT STARTED

- Bespoke per-function/method type extensions (`array_map`/
  `array_filter`/`preg_match`/`sprintf` - real PHPStan's largest single
  `Type/` subdirectory, `Php/`, is exactly this).
- Refinement/accessory types (`non-empty-string`, `literal-string`,
  ...) as a distinct precision layer beyond union/intersection - today
  these are only aliased to their base type (Phase 9.5/10's fix),
  not tracked with any real refinement.
- Broader rule catalog (`DeadCode`, `Comparison`, `Exceptions`,
  visibility enforcement, abstract-method completeness, enum
  exhaustiveness).
- CI-friendly output formats (JSON/table).
- LSP hover/go-to-definition (needs Phase 11's positions first).
- An extension/plugin API.

## Phase 13: Narrowing precision and Phase 9's remaining false-positive causes - SKETCHED, NOT STARTED

A new phase, split out of the "Known Limitations" audit below (every
item here previously had nowhere to live on this roadmap). Grouped
together because all of them are the same shape of problem: the
checker's flow analysis and inference are coarser than they could be,
in ways that either produce a real false positive on real code
(the first four) or just lose precision silently (the last three).

- **Property-chain narrowing beyond `instanceof`.** Phase 9's own
  narrowing fix (`narrowing_key`) only handles `$a->b instanceof X`;
  equality narrowing (`$a->b === null`) and predicate-function
  narrowing (`is_string($a->b)`) still only work on a bare variable.
  Real vendor/ noise (`~2000` diagnostics) traces directly to this.
- **Traits checked standalone.** A trait method referencing
  `$this->propOnHostClass` (expecting whatever class eventually `use`s
  it to provide that member - a common, legal pattern) gets flagged,
  since the trait's own body is checked with no knowledge of any
  future host class. Real PHPStan handles this via the trait's own
  `@property`/abstract-method declarations; this project has neither.
- **`@property`/`@property-read`/`@property-write` PHPDoc tags aren't
  parsed at all** - `docblock.rs` only handles `@param`/`@return`/
  `@var`/`@template`. A real, bounded, `@template`-sized addition.
- **`always_terminates` doesn't recognize `switch`/`try`/`match`** as
  terminating even when every one of their branches does - only
  `if`/`else` gets that treatment today. A guard clause written as a
  `switch` with no `default` fallthrough can have its narrowing
  incorrectly leak past it.
- **`for`/`foreach`/`switch`/`try` bodies share one unforked, unnarrowed
  scope** - only `if`/`while` fork `locals` into per-branch copies today.
- **Array-literal element types aren't tracked** - `[1, 2, 3]` always
  infers as `Array(Mixed)`, never `Array(Int)`.
- **`BinOp::Div` and PHP 8.5's `|>` pipe operator both always infer
  `Mixed`** - real division returns `int` or `float` depending on
  runtime values, and the pipe operator's result depends on resolving
  whatever expression sits on its right as a callable, neither
  attempted today.

## Known limitations

Every deliberate "this checker doesn't model X" boundary found in the
code, gathered in one place so none of them are only discoverable by
reading in-code comments. Each is either tracked under a phase above,
or is a permanent design boundary this project doesn't intend to
close - the two are labeled separately below so "not done yet" and
"not going to happen" aren't confused with each other.

**Tracked under a phase:**

| Limitation | Phase |
|---|---|
| No generic variance, template-bound enforcement, or `@extends`/`@implements` inheritance-chain substitution | 10 |
| Refinement/accessory types (`non-empty-string`, `positive-int`, ...) aliased to their base type, not tracked precisely | 12+ |
| No real source positions, level system, or config/baseline | 11 |
| No bespoke per-function type extensions, broader rule catalog, CI output formats, LSP hover/goto-def, plugin API | 12+ |
| Property-chain narrowing beyond `instanceof` (equality/predicate narrowing on `$a->b`) | 13 |
| Traits checked standalone, without their eventual host class's shape | 13 |
| `@property`/`@property-read`/`@property-write` PHPDoc tags not parsed | 13 |
| `always_terminates` doesn't recognize a terminating `switch`/`try`/`match` | 13 |
| `for`/`foreach`/`switch`/`try` share one unforked, unnarrowed scope | 13 |
| Array-literal element types not tracked (`Array(Mixed)` always) | 13 |
| Integer division and PHP 8.5's pipe operator (`BinOp::Div`/`Pipe`) always infer `Mixed` | 13 |

**Permanent design boundaries (not phase-tracked - not planned, by choice):**

- **Conditional PHPDoc return types** (`@return ($x is 'a' ? A : B)`) -
  documented in `types.rs`'s own module comment since Phase 2; these
  need real per-call-site type-parameter binding of a kind this
  project's PHPDoc parser deliberately doesn't attempt, the same way
  it deliberately doesn't parse `@template` bounds' enforcement.
- **A dynamic class target** (`$obj::method()`, `(new Foo())::CONST`)
  is never checked, by the same "don't guess" precedent `Expr::New`
  already set for a dynamic class-name expression - there's no static
  name to resolve at all, not a missing feature to build.
- **Integer overflow/precision, floating-point rounding, and other
  runtime-value-dependent behavior** - this checker only ever reasons
  about static types, never concrete values (Phase 1's own founding
  choice), so anything whose result depends on the actual runtime
  value rather than its type is out of scope by construction.

## Verification approach (used for every phase above)

- Unit tests for each new rule/type-system feature.
- Re-run against real corpora after every phase: Piwigo's `src/`
  (1491+ files), its `vendor/` (17387 files), php-src's own test
  corpus, and phpstorm-stubs' 571 real stub files.
- For new diagnostic rules: spot-check a sample of newly-surfaced
  diagnostics against real code by hand to confirm they're genuine
  findings, not false positives - this is what caught every bug listed
  above, before it shipped.
- **Test-coverage measurement (`cargo llvm-cov`), added 2026-08-30.**
  Manual code review kept finding real bugs this project's own test
  suite should have caught but didn't test the path at all - coverage
  gives an objective answer to "is this actually tested" instead of a
  subjective one. First run: 80.55% line / 87.25% function coverage
  overall, with `lsp.rs` and `main.rs` at a flat 0% (untestable without
  either refactoring a hardcoded-stdio loop into something generic, for
  `lsp.rs`, or spawning the real binary, for `main.rs`'s print-based
  CLI glue - both done). Closing every file's gaps this way (adding a
  test for what coverage flagged, after first manually verifying
  correctness) found a long tail of real bugs along the way, well
  beyond the closures/arrow-fn/match blind spot documented under Phase
  6 above - by the end: 90.53% line / 97.46% function coverage, 273
  tests total (up from 227 at the start of this session, including a
  new `tests/cli.rs` integration suite), zero of the remaining gaps
  found to hide anything real.

Full detail, adversarial fact-checking history, and a dependency
shortlist live in the planning doc this roadmap was distilled from
(kept outside this repo, in the assistant session that built Phases
8-10).
