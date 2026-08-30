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

## Phase 9: Undefined-symbol rule family - PARTIAL

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

**Not started:** undefined-*property*-on-read detection, and
`Expr::StaticAccess`/`StaticCall` existence checks (static
methods/properties/constants). Not blocked on anything architectural -
just not yet built.

Along the way this phase found and fixed several real, independent
bugs (not hypothetical - each confirmed via a minimal repro against
real code before fixing): leading-backslash class/function references
were never normalized; `use`/`use function` imports were parsed but
never read, and only the *first* import in a group/comma-list `use`
was even kept; `Registry::index` never recursed into `if`/block bodies,
missing the extremely common `if (!function_exists(...)) { function
foo() {} }` polyfill idiom used throughout the ecosystem.

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

## Verification approach (used for every phase above)

- Unit tests for each new rule/type-system feature.
- Re-run against real corpora after every phase: Piwigo's `src/`
  (1491+ files), its `vendor/` (17387 files), php-src's own test
  corpus, and phpstorm-stubs' 571 real stub files.
- For new diagnostic rules: spot-check a sample of newly-surfaced
  diagnostics against real code by hand to confirm they're genuine
  findings, not false positives - this is what caught every bug listed
  above, before it shipped.

Full detail, adversarial fact-checking history, and a dependency
shortlist live in the planning doc this roadmap was distilled from
(kept outside this repo, in the assistant session that built Phases
8-10).
