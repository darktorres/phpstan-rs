# phpstan-rs roadmap

A from-scratch Rust PHP parser + type checker, aimed at growing toward
PHPStan feature parity. Phases 8+ are grounded in direct inspection of
`phpstan/phpstan-src` and `jetbrains/phpstorm-stubs` (not recalled
knowledge) and verified at real scale against Piwigo's own codebase
(`src/`, `tools/`) plus its `vendor/` tree (18923 files combined).

Status key:
- **DONE** - shipped and verified.
- **PARTIAL** - some of the phase's own scope shipped, some didn't.
- **PLANNED** - a real, detailed, source-grounded design exists; not
  started.
- **NOT STARTED** - a name and a rough idea only, no detailed plan yet.

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
diagnostics in Piwigo + `tools/` rose from 2221 to 2321, all
spot-checked genuine, once closures/arrow fns were finally checked).

Fixed with real PHP capture semantics: a closure only sees
`use (...)`-listed variables (not the enclosing scope), plus `$this`
auto-bound for a non-static closure inside a method; an arrow function
sees the whole enclosing scope by value (real PHP's implicit capture,
no `use` needed). `match` now infers a real union of its arm result
types instead of always `Mixed`.

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

## Phase 8.5 (unplanned): Stub arity precision - PLANNED

Found while researching Phase 12's priority list, not hypothetical -
each reproduced live against the compiled binary. Two distinct bugs
in how a stub function's minimum argument count gets generated,
both real false positives on some of PHP's most common functions.

**Bug 1 - version-gated parameters counted as permanently required.**
phpstorm-stubs represents "this parameter only existed in PHP versions
X-Y" with a `#[PhpStormStubsElementAvailable(from: ..., to: ...)]`
attribute on individual parameters, rather than maintaining separate
per-version signatures. `array_map`'s real stub declares four
parameters: `?callable $callback`, an `8.0`-only `array $array`, a
`5.3`-`7.4`-only untyped `$arrays`, and a variadic `array ...$arrays` -
the middle two represent the *same* conceptual parameter across two
PHP-version eras, never meant to both apply on one install. Neither
this project's parser nor `php_stubs.rs` recognizes the attribute, so
both count as ordinary, permanently-required parameters. Result: the
single most basic call to one of PHP's most common functions -
`array_map($callback, $array)`, 2 arguments - is flagged
`array_map() expects at least 3 argument(s), 2 given`.

The attribute itself is real and widespread (65 of the 558 vendored
stub files, 664 occurrences), but its mere presence doesn't mean a
given function is actually miscounted - checked directly against the
compiled binary, `str_replace`, `version_compare`, `array_filter`,
`dirname`, `file_put_contents`, and `rename` are all already correct
today despite `dirname` carrying one such attribute (its own
version-gated parameter already has a real default, `int $levels = 1`,
so it was never miscounted). `array_map` is the one confirmed,
reproduced case; each other affected function needs the same direct
check before being added to a fix list, not an assumption from the
attribute's presence alone.

**Bug 2 - a stub modeling only its highest-arity overload.**
`array_keys(['a' => 1])` (the real, valid 1-argument form) and
`mt_rand()` (the real, valid 0-argument form) are also flagged as
missing required arguments, confirmed live - but neither stub uses
`PhpStormStubsElementAvailable` at all. Each is a single declaration
that only models the function's *highest*-arity overload
(`array_keys(array $array, mixed $filter_value, bool $strict = false)`;
`mt_rand(int $min, int $max)`), with no second, lower-arity declaration
the way `version_compare` gets (two full `function version_compare(...)`
declarations in the same file - happens to already resolve correctly,
more by luck of which one this project's last-write-wins generation
lands on than by design). This needs a different fix: either a small
hand-curated override in `stubs.rs` (already exists specifically to
win over a generated signature, per Phase 8's own two-tier design), or
teaching `php_stubs.rs` to treat a later `@param ... [optional]`
docblock hint as an implicit default even with no `= value` in the
native signature.

**The fix (bug 1):** when generating a signature from stub source,
recognize `#[PhpStormStubsElementAvailable(...)]` on a parameter whose
own range doesn't already carry a real default, and keep only the
*last* declared parameter of that name (phpstorm-stubs' convention:
later-declared ranges are newer PHP) - dropping the earlier,
superseded declaration entirely rather than counting it as a real,
separate required parameter.

**Why before Phase 12, not folded into it:** both are correctness
fixes to an already-shipped pipeline (Phase 8), not new capabilities -
the same category as Phase 9.5's retroactive fix to Phase 9's own
registry. Fixing these first keeps Phase 12's own before/after
diagnostic-count comparisons from being muddied by noise they already
cause.

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
  write-side check: a write can legitimately create a real, if
  deprecated, dynamic property; a read of one never declared or
  dynamically assigned just returns null with a warning in real PHP,
  so there's no case to stay lenient for.
- **`Expr::StaticAccess`/`StaticCall` existence checks** - class
  constants, static properties, and static methods (with a
  `__callStatic` exemption), self/parent/static-aware, mirroring
  `MethodCall`'s own structure and leniency. `Foo::class` and a fully
  dynamic member (`Foo::{$e}`/`Foo::$$e`) are both skipped, matching
  the parser's own documented encoding of those two cases; a dynamic
  class target (`$obj::method()`) is skipped too, matching `Expr::New`'s
  existing precedent for the same situation.

Along the way this phase found and fixed several real, independent
bugs (each confirmed via a minimal repro against real code before
fixing): leading-backslash class/function references were never
normalized; `use`/`use function` imports were parsed but never read,
and only the *first* import in a group/comma-list `use` was even kept;
`Registry::index` never recursed into `if`/block bodies, missing the
extremely common `if (!function_exists(...)) { function foo() {} }`
polyfill idiom used throughout the ecosystem.

**Finishing the last two rules (property-read, static-access) found 3
more real, substantial gaps**, each surfacing as a wave of false
positives on the very first real-corpus check after shipping the new
rule, each fixed before the rule could be considered done:
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
distinct, understood root causes now tracked under Phase 13: narrowing
mechanisms not modeled (equality/type-check narrowing on a property
chain, not just `instanceof`), traits referencing a constant/property/
method their eventual host class provides, and `@property` PHPDoc tags
(not parsed at all) - plus `self::`/`static::` forwarding calls to a
method inherited from an ancestor a particular `vendor/` snapshot
doesn't fully resolve, which isn't tracked anywhere (a resolution gap
in that one snapshot, not this project's own code).

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
and function-level `@template` substitution both checked out fine.
Found and fixed 4 more real bugs, all variations on the same theme -
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
PHPStan/Psalm string and array refinement family (`non-empty-string` -
2263 real occurrences in one corpus alone - `class-string`, and
others).

## Phase 11: Source positions, levels, config - NOT STARTED

- Thread real `(line, column)` spans through the lexer, parser, and
  into `checker::Diagnostic`, replacing today's line-1 placeholder.
- A two-part level system mirroring real PHPStan: discrete rule-gating
  levels (0-6 equivalent) plus a separate strictness-parameter set for
  `is_assignable` (7-10 equivalent).
- A minimal config file and baseline mechanism.

## Phase 12: Bespoke per-function/method type extensions - PLANNED

Real PHPStan's `src/Type/Php/` is 190 files - its single largest
`Type/` subdirectory (confirmed against a fresh clone of
`phpstan/phpstan-src`, commit `4f80a48`, rather than recalled from an
earlier planning session). Real extension-point interfaces, by real
count: 106 `DynamicFunctionReturnTypeExtension`, 21
`FunctionTypeSpecifyingExtension` (condition-narrowing, e.g. after
`is_array()`) + 22 `TypeSpecifierAwareExtension` (its companion, wires
the narrowing extension into the specifier registry), 12
`DynamicMethodReturnTypeExtension`, 9/8/6/4 `Dynamic{Static,}Method/
FunctionThrowTypeExtension` (a call's real throw-type, not just its
return type), 5 `FunctionParameterClosureTypeExtension` (types a
callback parameter using the *other* arguments already passed - what
`array_map($cb, $items)` needs to type `$cb`'s own parameter from
`$items`'s element type), 3 `FunctionParameterOutTypeExtension`
(by-ref output parameters, e.g. `preg_match`'s `$matches`).

Full parity with 190 files isn't the goal - a prioritized subset is.
Every function named below has already produced imprecise or wrong
output against the real 18923-file Piwigo+vendor corpus at some point
in this project's history - real signal for priority order, not a
guess from PHPStan's own list:

1. **`array_map`/`array_filter`/`array_reduce`/`array_walk`** - the
   closure-parameter-typing family (`FunctionParameterClosureTypeExtension`
   in real PHPStan) plus each one's own return-type extension. The
   single highest-value target: these four appear constantly in real
   code, and `checker::infer_call_return_type`'s existing template-
   binding machinery (built for Phase 2's `@template`, reused by Phase
   10's generics) is very likely already shaped right for this - a
   function whose param type is expressed as `callable(T): U` and
   return type as `array<U>` would let ordinary template substitution
   do the real work, once these stub signatures are written that way
   and array_map's own multi-array/multi-arg overload forms are
   modeled.
2. **`preg_match`/`preg_match_all`** - by-ref `$matches` shape
   inference from the pattern's own capture groups (named + numbered).
   The one extension here that's genuinely hard to approximate - needs
   real PCRE-aware parsing of the pattern string, not just the type
   system. No suitable Rust PCRE crate exists without pulling in a C
   dependency (a real `pcre2` binding); Rust's own `regex` crate is a
   different flavor of regex (no backreferences, different syntax
   edge cases) that would silently produce wrong answers on real PCRE
   patterns.
3. **`sprintf`/`printf`/`vsprintf`** - validate placeholder count/type
   against the format string's own literal content when it's a
   compile-time-known string literal (mirrors this project's own
   `check_shape_key_access`'s "only fires on a literal, never a
   dynamic value" precedent).
4. **`array_keys`/`mt_rand`** - lower-effort wins once Phase 8.5's
   second fix (the missing-lower-arity-overload bug) lands: both are
   confirmed-live false positives from a stub-generation gap, not
   functions actually needing a bespoke extension.
5. **`array_column`/`array_combine`/`array_flip`/`array_fill*`/
   `array_chunk`/`compact`/`get_defined_vars`** - shape-aware return
   types (mirrors this project's own existing `ArrayShape` inference,
   just applied to more built-ins).
6. **`is_array`/`is_string`/`is_int`/`is_callable`/`class_exists`/
   `function_exists`/`in_array`/`array_key_exists`/`method_exists`/
   `property_exists`** - `FunctionTypeSpecifyingExtension`-style
   condition-narrowing. This project's own `apply_narrowing` already
   has a `narrowing_type_for_predicate` table for a handful of these
   (`is_int`/`is_string`/etc., `checker.rs:1762`) - this item is about
   widening that same, already-proven mechanism to the rest of the
   list, not inventing a new one.

## Phase 13: Narrowing precision and remaining false-positive causes - PLANNED

Grouped together because all of them are the same shape of problem:
the checker's flow analysis and inference are coarser than they could
be, in ways that either produce a real false positive on real code
(the first four) or just lose precision silently (the last three).

- **Property-chain narrowing beyond `instanceof`.** Phase 9's own
  narrowing fix (`narrowing_key`) only handles `$a->b instanceof X`;
  equality narrowing (`$a->b === null`) and predicate-function
  narrowing (`is_string($a->b)`) still only work on a bare variable.
  Real vendor/ noise (~2000 diagnostics) traces directly to this.
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

## Phase 14: Refinement/accessory types - PLANNED

Real PHPStan's `src/Type/Accessory/` is exactly 15 files:
`AccessoryNonEmptyStringType`, `AccessoryNonFalsyStringType`,
`AccessoryLiteralStringType`, `AccessoryNumericStringType`,
`AccessoryLowercaseStringType`, `AccessoryUppercaseStringType`,
`AccessoryDecimalIntegerStringType`, `NonEmptyArrayType`,
`OversizedArrayType`, `AccessoryArrayListType`, plus four "shape
marker" types (`HasMethodType`/`HasOffsetType`/`HasOffsetValueType`/
`HasPropertyType` - used internally to represent "this generic
template type is known to have at least this member", not really a
user-facing refinement) and the shared `AccessoryType` base.

Today, Phase 9.5/10 alias every one of the string/array refinement
*names* this directory actually covers (`non-empty-string`,
`non-empty-array`, ...) straight to their unrefined base type
(`string`/`array`) - correct as a fallback, but a `non-empty-string`
value flowing back out of a function is then indistinguishable from a
plain `string`, so passing it somewhere that specifically demands
non-empty content isn't checked. (`positive-int`/`negative-int` are a
separate real PHPStan mechanism - `Type/IntegerRangeType.php`, a real
min/max-bounded type, not an `Accessory/` intersection marker - worth
its own future phase if this project ever wants real integer-range
precision, but out of scope for this one.)

This is real PHPStan's own `Intersection`-with-an-accessory-marker
design (`non-empty-string` is modeled internally as
`string&AccessoryNonEmptyStringType`) - this project already has a
real `Type::Intersection` that the same pattern could reuse directly:
add `Type::Refined(Box<Type>, RefinementKind)` (or reuse `Intersection`
with a small new marker variant) alongside the existing base-type
aliasing, threaded through `is_subtype`/`is_assignable` so a refined
type is still assignable to its own bare base type (one-way), plus a
small number of real narrowing sources that actually produce one (a
non-empty string literal, `trim()`'s known-safe cases, a `!empty($x)`
guard on a string).

## Phase 15: Broader rule catalog - PLANNED

Real, current per-category rule-class counts, filtered to files that
actually `implements Rule` (a raw file count per directory
overstates every category - each one also holds non-`Rule` helper
classes): Functions 44, Classes 39, Properties 32, Methods 30, PhpDoc
22, Comparison 22, Exceptions 16, Api 16, Generics 15, Variables 13,
Constants 13, Arrays 13, RestrictedUsage 10, Playground 9, DeadCode 9,
TooWideTypehints 7, Operators 7, Cast 7, Traits 4 (plus a 129-file
`RuleErrors/` that's shared error-message plumbing, not one rule per
file, so not a category to chase file-for-file either way).

The categories this project has nothing in yet, checked against real
file names rather than assumed from the category label alone:
- **DeadCode** (9 real rule files) - unreachable code after an
  always-terminating statement (this project's own `always_terminates`
  is a real, direct prerequisite already built), always-true/false
  conditions, unused private methods/constants (`UnusedPrivateMethodRule`,
  `UnusedPrivateConstantRule` - both real files in this category).
- **Comparison** (22 real rule files) - `match`/`switch` exhaustiveness
  (`MatchExpressionRule`), strict-comparison-of-incompatible-types
  (`StrictComparisonOfDifferentTypesRule`), always-true/false `in_array`/
  `array_key_exists` checks against a finite-valued haystack.
- **Exceptions** (16 real rule files) - uncaught/unthrown-type checks;
  needs a real throw-type model this project doesn't have at all yet
  (not even a `FunctionSig`-level "what can this throw" field).
- **Visibility enforcement / abstract-method completeness** - not
  their own top-level category in real PHPStan: they live inside
  `Rules/Classes/` (`AbstractMethodInNonAbstractClassRule`,
  `MissingMethodImplementationRule`) and `Rules/Methods/`+
  `Rules/Properties/` (`MethodVisibilityInInterfaceRule`,
  `AccessPrivatePropertyThroughStaticRule`) alongside those
  categories' other rules, not a separate bucket.

## Phase 16: CI-friendly output formats - PLANNED

Real PHPStan's `src/Command/ErrorFormatter/` holds 11 concrete
classes, 9 of them genuine output formats: `Table` (its own default),
`Json`, `Raw`, `Checkstyle`, `Junit`, `Github`, `Gitlab`, `Teamcity`,
`CiDetected` (auto-picks Github/Gitlab/Teamcity from environment
variables). The other 2 (`BaselineNeon`/`BaselinePhp`) aren't output
formats at all despite living in the same directory - they generate a
suppression file, a different feature (Phase 11's own
baseline-mechanism bullet already names this).

Realistic starting subset for this project, from the 9: `json` (the
most universally consumable) and `checkstyle` or `junit` (the two real
CI systems most commonly wire up directly) - needs Phase 11's real
source positions to be genuinely useful, since every format beyond
plain text expects a real line/column, not today's line-1 placeholder.

## Phase 17: LSP hover/go-to-definition - PLANNED

Needs Phase 11's real source positions first (today's LSP diagnostics
work despite the line-1 placeholder only because a diagnostic doesn't
need to point at anything more precise than "this file"; hover and
go-to-definition inherently do). Real PHPStan itself ships no LSP
server - this is squarely this project's own `src/lsp.rs` extending
its existing hand-rolled JSON-RPC loop (`run_loop`, real test
coverage as of Phase 9's audit work) with two more real LSP methods
(`textDocument/hover`, `textDocument/definition`), each needing a
"what declaration is at this exact position" lookup this project's
`Registry`/`Project` don't have any index for yet (today's lookups are
all name-keyed, never position-keyed).

## Phase 18: Extension/plugin API - PLANNED

The least concrete of all of these, deliberately last - real PHPStan's
own extension points (`DynamicFunctionReturnTypeExtension` and its
siblings from Phase 12, `Rule` from Phase 15, all wired together via
`phpstan/extension-installer`, already a real, vendored dependency in
the Piwigo corpus this project tests against) only make sense to
formalize into a stable public API once this project has enough of its
own built-in rules and type extensions (Phases 12-15) that a real
third-party author would want to add to them, rather than just use
them - premature before that.

## Known limitations

Every deliberate "this checker doesn't model X" boundary, gathered in
one place so none of them are only discoverable by reading in-code
comments. Each is either tracked under a phase above, or is a
permanent design boundary this project doesn't intend to close - kept
separate below so "not done yet" and "not going to happen" aren't
confused with each other.

**Tracked under a phase:**

| Limitation | Phase |
|---|---|
| `#[PhpStormStubsElementAvailable(...)]`-gated stub parameters, and stubs modeling only their highest-arity overload, both counted as more-required-than-they-are | 8.5 |
| No generic variance, template-bound enforcement, or `@extends`/`@implements` inheritance-chain substitution | 10 |
| No real source positions, level system, or config/baseline | 11 |
| No bespoke per-function/method type extensions (`array_map`/`preg_match`/`sprintf`/condition-narrowing, ...) | 12 |
| Property-chain narrowing beyond `instanceof` (equality/predicate narrowing on `$a->b`) | 13 |
| Traits checked standalone, without their eventual host class's shape | 13 |
| `@property`/`@property-read`/`@property-write` PHPDoc tags not parsed | 13 |
| `always_terminates` doesn't recognize a terminating `switch`/`try`/`match` | 13 |
| `for`/`foreach`/`switch`/`try` share one unforked, unnarrowed scope | 13 |
| Array-literal element types not tracked (`Array(Mixed)` always) | 13 |
| Integer division and PHP 8.5's pipe operator (`BinOp::Div`/`Pipe`) always infer `Mixed` | 13 |
| Refinement/accessory types (`non-empty-string`, `non-empty-array`, ...) aliased to their base type, not tracked precisely | 14 |
| Broader rule catalog (`DeadCode`, `Comparison`/exhaustiveness, `Exceptions`, visibility/abstract-method completeness) | 15 |
| No CI-friendly output formats (JSON/checkstyle/junit/...) | 16 |
| No LSP hover/go-to-definition | 17 |
| No extension/plugin API | 18 |

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
- **Test-coverage measurement (`cargo llvm-cov`)**, added while
  auditing Phase 6/9. Manual code review kept finding real bugs this
  project's own test suite should have caught but didn't exercise at
  all - coverage gives an objective answer to "is this actually
  tested" instead of a subjective one. Closing a file's gaps this way
  (write a test for what coverage flags, only after first manually
  verifying correctness) found a long tail of real bugs beyond the
  closures/arrow-fn/match blind spot alone, including refactoring
  `lsp.rs`'s hardcoded-stdio loop into something generic enough to
  drive with an in-memory buffer, and adding a `tests/cli.rs`
  integration suite (spawns the real compiled binary) for `main.rs`'s
  print-based CLI glue - both had been completely untested before.
  Current state: 90.53% line / 97.46% function coverage, 273 tests.
- **A fresh clone of `phpstan/phpstan-src`**, used to ground every
  numeric claim in Phases 8.5 and 12-18 in real, current source rather
  than recalled or assumed figures - re-verify against a fresh clone
  before trusting any of those numbers if picked up again after a long
  gap, since real PHPStan keeps evolving.

Full detail, adversarial fact-checking history, and a dependency
shortlist for Phases 8-10 live in the planning doc this roadmap was
originally distilled from (kept outside this repo, in the assistant
session that built them).
