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

## Phase 8.5 (unplanned): Stub arity precision - DONE

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

**The fix (bug 1), as shipped:** taught the *lexer* itself (not just the
stub-generation pipeline) to recognize
`#[PhpStormStubsElementAvailable(from: ..., to: ...)]` while skipping an
attribute (`Lexer::skip_stub_availability_attribute`). A *closed* range
(both `from` and `to` present) parameter with no `= value` default of
its own is dropped from the token stream entirely, along with its one
trailing separator comma if present (a trailing comma before `)` is
valid PHP the parser already tolerates, so nothing needs repairing on
the other side); an open-ended range (`from` only, still valid in the
latest PHP) or a parameter that already has a real default is left
alone, both falling back to an ordinary attribute-only skip. Genuinely
conservative about where the attribute appears: if a `{`/`;` turns up
before a parameter delimiter (the attribute is gating a whole
method/property instead, e.g. `DateTime::__construct`'s 5.5-8.2
overload), the text is left completely untouched.

This had to be a **lexer-level** fix, not something special-cased only
in `php_stubs.rs`'s generation pipeline: real projects commonly vendor
`jetbrains/phpstorm-stubs` itself as a transitive phpstan dependency (a
real, live example: Piwigo's own `vendor/jetbrains/phpstorm-stubs/`),
so the exact same attribute shape shows up in ordinary project/vendor
source being checked at runtime, not just in this project's own bundled
build-time corpus - an earlier draft of this fix lived only in
`php_stubs.rs` and, while it fixed the bundled corpus, left 1567 real
`array_map()` false positives (plus `setlocale`/`max`/`array_merge`/
`min`/`sprintf`/`getenv`/`strtok`) alive across Piwigo's own `vendor/`
tree for exactly this reason.

**The fix (bug 2), as shipped:** `DocBlock` gained an `optional_params`
set, populated from a `[optional]` marker on a `@param` tag
(`docblock.rs`); `FunctionSig::from_decl` (`registry.rs`) now treats
that marker as an implicit default when the native parameter has none
of its own. Found and fixed a second, more consequential bug while
verifying this against the real Piwigo corpus: the `@param` parser
never matched a **by-ref** parameter's name at all
(`@param string[] &$matches [optional]` - the real, common PHP-manual
docblock convention of prefixing a by-ref param's name with `&`), since
it looked for a bare `$name` token and `&$matches` doesn't start with
`$`. This silently dropped both the doc type *and* the `[optional]`
marker for every by-ref documented parameter - which turned out to be
the actual live false-positive cause for `preg_match`/`preg_match_all`/
`preg_replace`/`preg_filter`/`str_replace` against an older, real
phpstorm-stubs release (the one Piwigo's own `vendor/` happens to pin),
which models those functions' by-ref params as `[optional]`-with-no-
native-default rather than with a real `= null`. Fixed by stripping a
leading `&` before matching `$name`.

**Verified at real scale** (Piwigo's `src/` + `tools/` + `vendor/`,
18923 files): 46791 -> 42192 diagnostics, all the difference confirmed
to be exactly these arity false positives disappearing (`array_map`,
`array_keys`, `mt_rand`, `setlocale`, `max`, `min`, `array_merge`,
`sprintf`, `getenv`, `strtok`, plus the by-ref-driven
`preg_match`/`preg_match_all`/`preg_replace`/`preg_filter`/
`str_replace` false positives from Piwigo's own vendored older
phpstorm-stubs copy) - zero new first-party false positives, parse
pass rate unchanged (18923/18923), all 293 unit/CLI tests passing.

A residual ~384 "expects at least" diagnostics remain, mostly a
structurally different and harder pattern this phase deliberately
didn't chase: some stubs (`pg_query_params`, `pg_query`, several others)
interleave *multiple full parameter lists* under the same parameter
names across version eras (e.g. two separate `$connection` parameters,
one closed-range-with-default, one open-range-with-no-default, side by
side) rather than the clean "one superseded param + one modern
variadic" shape `array_map` has - out of scope here, a candidate for a
future phase if it turns out to matter in practice.

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
quantified example: 1595 distinct colliding class names across Piwigo,
plus its `vendor/`) could silently overwrite one class's real method
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
inheritance-chain substitution - a separate, forward-looking scope,
tracked as its own phase below (10.5) rather than as an open item
against this one, since this phase's own, narrower goal (call-site
substitution) is fully shipped.

Found and fixed the same pseudo-type-collision bug again for the whole
PHPStan/Psalm string and array refinement family (`non-empty-string` -
2263 real occurrences in one corpus alone - `class-string`, and
others).

## Phase 10.5 (unplanned): Generic variance and inheritance chains - DONE

Phase 10 deliberately scoped itself to call-site generic substitution
only. Real PHPStan's own `Generic/` (36 files in `src/Type/Generic/`)
is the fuller class-generics engine this phase grows toward: variance
(`@template-covariant`/`@template-contravariant`), template bound
enforcement (`@template T of Foo` actually checked at bind time, not
just parsed past), and inheritance-chain substitution via
`@extends`/`@implements` generic arguments.

**Shipped: inheritance-chain substitution.** `docblock.rs` now parses
`@extends Parent<Arg, ...>` and `@implements Iface<Arg, ...>` (neither
was parsed at all before); `ClassInfo` gained `extends_args`/
`implements_args` (`registry.rs`) carrying each tag's resolved
arguments, run through the same mark-template-then-resolve-names
pipeline every other member type gets, so an argument can itself
reference *this* class's own `@template` name (`class Box<T> extends
Collection<T> {}`, forwarding rather than fixing). A concrete subclass
fixing its parent's template param (`class IntCollection extends
Collection<int> {}`, itself not generic at all) now resolves
`Collection`'s own `T` to `int` for every inherited method/property.

**A real, non-hypothetical bug found and fixed while verifying this
against the real Piwigo corpus**: `checker::class_template_bindings`'s
first implementation walked the *entire* `extends`/`implements` chain
into one flat-merged `HashMap<String, Type>`, silently letting an
ancestor's own template name overwrite an unrelated same-named one
further down the chain - `T` is by far the most common single-letter
generic name in real PHPStan code, so two unrelated classes in one
chain both using it is common, not a corner case. Caught live:
`symfony/type-info`'s real `NullableType<T of Type> extends
UnionType<T|BuiltinType<...>>` (`UnionType` has its own, unrelated
`T`) produced a wrong-but-plausible-looking return-type mismatch on
`NullableType::getWrappedType()`, a method declared directly on
`NullableType` with nothing to do with `UnionType` at all. Fixed by
scoping every lookup to a specific *target* ancestor: `lookup_method`/
`lookup_property` gained `_with_owner` variants returning which class
actually declares the found member, and `class_template_bindings`
takes that owner as its `target`, walking the chain hop-by-hop and
returning bindings for *exactly* that one level - never a flat merge
across levels that don't share a template namespace. Verified: the
real corpus's diagnostic count is byte-for-byte identical before this
whole sub-feature and after (42192 both times) - the fix didn't just
suppress the one bug found, it structurally can't reintroduce it, and
the new capability itself hasn't yet been caught mattering on this
corpus (most real inherited-generic-config patterns route through
trait flattening instead of `@implements`, which resolves the member
directly on the receiver's own class before this mechanism's chain-walk
would ever trigger).

**Shipped: template bound enforcement.** `docblock.rs` parses `@template
T of Bound` (the bound was previously parsed past and discarded);
`ClassInfo` gained `template_bounds` (`registry.rs`). A new
`checker::check_generic_bounds` walks every declared type - a
function/method's params and return type, a class's own properties,
and its `@extends`/`@implements` arguments - flagging any generic
argument that doesn't satisfy its target class's own bound
(`Collection<Foo>` where `Collection`'s `@template T of Shape` and
`Foo` doesn't implement `Shape`). Checked once per declaration, where
the generic type is *written* (matching real PHPStan's own
`GenericObjectTypeCheck`), not at every call/access site.

**Also shipped alongside it: `@template-covariant`/
`@template-contravariant` tag *parsing*** (`covariant_templates`/
`contravariant_templates` on both `DocBlock` and `ClassInfo`) - the
data variance enforcement will need, though nothing consumes it for
variance checking yet (see "not yet shipped" below).

**Three more real bugs found and fixed verifying bound enforcement
against the real Piwigo corpus - each one this project's own
established "verify every new diagnostic, don't just trust the count"
discipline caught before shipping:**

1. Parsing `@template-covariant`/`@template-contravariant` at all
   turned out to fix a real, independent, pre-existing bug: real code
   using either tag (found in Piwigo's own `SortEntry<TField,
   TDir>`) previously had those names silently **not** recognized as
   templates at all (the tag fell into docblock.rs's catch-all), so
   `TField`/`TDir` were treated as literal, always-mismatching class
   names throughout that class's own signatures - 15 real first-party
   false positives, gone now that the tag is recognized.
2. `is_assignable` had never treated the native `object` type hint as
   a universal supertype of any class instance - nothing had compared
   against it that way before bound-checking started (confirmed with a
   minimal, generics-free repro: a plain `function f(): object { return
   new Foo(); }` was already wrong). Real-world trigger:
   `Doctrine\ORM\EntityRepository<T of object>`, the shape every one of
   Piwigo's own repository classes uses - ~25 first-party false
   positives from this alone.
3. An enum was never registered as implementing `UnitEnum`/
   `BackedEnum` in its own hierarchy data - only its synthesized
   `cases()`/`name`/`value` members existed (Phase 9's own fix), never
   the actual `implements` relationship. Real-world trigger:
   `doctrine/lexer`'s own `AbstractLexer<T of UnitEnum|string|int>`
   flagged every real backed enum passed to it.

**A fourth, deliberate leniency gap also added**: `check_generic_bounds`
now skips a bound check entirely (`is_type_fully_known`) if either side
mentions a class this project has no real registered data for - the
same "don't know, don't guess" precedent every undefined-symbol-
adjacent check here already follows. Covers two distinct real root
causes seen on this same corpus: a **PHAR-bundled dependency** (real
`phpstan/phpstan` ships with no loose source files at all - its own
`PHPStan\Type\ArrayType implements PHPStan\Type\Type` can't be
verified), and a **local `@phpstan-type` alias** this project doesn't
parse (`php-cs-fixer`'s own `@phpstan-type _AutogeneratedInputConfiguration
array{...}` used as an `@implements` argument - the same finding
Phase 10.5's inheritance-chain work already flagged and left
undocumented-but-unfixed; this leniency check happens to suppress it
too, though for a different reason than actually understanding the
alias).

Verified: the real corpus's `@template`-bound diagnostic count went
from 0 (feature not yet correct) through a brief peak of 317 (real
bugs found) back down to a clean 0 once all three bugs above were
fixed and the leniency guard added - overall diagnostic count net
*dropped* 937 (42192 -> 41255) since bug 1 alone fixed real
pre-existing false positives unrelated to bounds at all. Deterministic
across repeated runs (a `HashMap`-iteration ordering issue in the new
code, caught by literally re-running the same check twice, is now
sorted). 310/310 tests passing.

**Shipped: variance enforcement.** A new `checker::check_variance`
(called from `check_function` alongside bound-enforcement) flags a
`@template-covariant T` used in any method's *parameter* position, and
a `@template-contravariant T` used in any *return* position - a plain,
un-annotated `@template T` stays unrestricted, Phase 10's original
behavior. Recurses into `Array`/`Union`/`Intersection`/`ArrayShape`
(`T[]` in a param position is still flagged), but deliberately does
*not* recurse into another class's own generic type arguments (a `T`
nested inside `Collection<T>` isn't re-derived from `Collection`'s own
variance) or check properties at all (this project doesn't track
`readonly` yet, and a readonly property typed `T` is actually a
legitimate covariant-safe position - without that information,
checking properties would misflag every real one) - both narrower,
more conservative scope cuts than a full variance-flipping algorithm,
matching this project's general preference for fewer false positives.

**A real bug found immediately on the first real-corpus run, not a
false start**: every one of an initial 27 diagnostics, with zero
exceptions, was on a `__construct` method - the extremely common
`readonly` value-holder pattern (`public function __construct(private
readonly T $value) {}`). Real PHPStan's own variance rule explicitly
exempts constructors for exactly this reason: a constructor is only
ever called to build one specific, concrete instance (`new
Success<Cat>($cat)`), never invoked polymorphically through a
variable typed as the covariant supertype the way an ordinary method
can be - so a covariant `T` constructor parameter never actually
violates covariance safety. Fixed by exempting `__construct`
entirely; the real corpus's diagnostic count returned to exactly its
pre-variance-checking baseline (41255) with the fix in place -
consistent with a genuinely well-written, modern corpus having zero
real variance violations, not evidence the check doesn't work (18
new, targeted unit tests exercise it directly).

Verified against the 18923-file Piwigo corpus, deterministic across
repeated runs, 318/318 tests passing.

## Phase 11: Source positions, levels, config - DONE

Full column-precise spans (not a coarser per-statement-line
compromise) - real PHPStan's own precision. A large, multi-stage
phase: source positions end-to-end (lexer through the AST through
`Diagnostic` through CLI/LSP output), then a two-part level system,
then config/baseline - tracked as six ordered sub-stages, each its own
commit.

**Shipped - lexer emits real spans.** `token::Position`/`Span` (a real
`{start, end}` range, 1-indexed, columns counted in UTF-8 *characters*
rather than raw bytes - a real, demonstrated need: the actual
Piwigo/vendor corpus this project verifies against has first-party
files with genuine multi-byte UTF-8 inside real code, not just
comments). The
lexer precomputes a line-start table once per file (the standard
rustc-`SourceMap` technique) so converting a byte offset to a
line/column is a binary search, not a per-token rescan of the whole
file. `tokenize_with_spans()` is the new, real entry point;
`tokenize()` stays as a thin adapter over it for the handful of tests
that compare bare tokens. String interpolation (`"{$obj->prop}"`)
gets a real base position too, biasing the isolated fragment re-lex
back into true file coordinates instead of leaving it disconnected -
precision is coarser for a heredoc's own interpolation specifically
(its content has already had per-line indentation stripped, so an
exact per-occurrence bias isn't attempted; anchored at the heredoc's
own start instead) and for the two synthesized-string shorthand cases
(`$name->prop`/`$name[key]`, which reconstruct a string with no exact
sub-position mapping) - both honestly narrower, not blocking.

**A real, severe performance bug found and fixed verifying this
against the corpus, not hypothetical**: the first implementation of
byte-to-line/column conversion recounted every character from a
line's own start on every single call - fine for ordinary code, but a
genuine O(line length x tokens on it) blowup on a real file already in
this project's own test corpus (`amphp/hpack`'s generated Huffman
lookup table, a single line 1.25 million characters long) - measured
hanging for minutes on a file that used to check in well under a
second. Fixed with an incremental cache: since real tokenization asks
for strictly increasing byte offsets, caching the last conversion and
counting only the *delta* since then turns the pathological line back
into O(line length) total, not squared, while staying exactly correct
(just not delta-accelerated) for the rare backward/cross-line asks the
interpolation bias above can make. Re-verified: the same file now
checks in ~0.3s; the full 18923-file corpus's parse pass rate and
`--check-project` diagnostic count (41255) are both unchanged - purely
additive infrastructure, zero behavior change yet since nothing
downstream consumes spans until the next sub-stage.

**Shipped - spans threaded through the AST.** `Stmt`/`Expr` wrapped as
`{kind, span}` structs (`StmtKind`/`ExprKind` hold the original
variants unchanged), plus a `span` field added directly to
`FunctionDecl`/`ClassDecl`/`PropertyDecl`/`MethodDecl`/`ClassConst`/
`EnumDecl`/`EnumCase`/`Param`/`SwitchCase`/`CatchClause`. The wrapper
made the ~450-site migration across `parser.rs`/`checker.rs`/
`deps.rs`/`registry.rs`/`project.rs` safe by construction - Rust's
exhaustiveness checking turned every missed construction or match site
into a compile error, not a silent bug, exactly as the plan predicted.
Re-verified against the real corpus: the 1492-file `src/` subset still
parses 1492/1492 with the identical 2327-diagnostic count, and the
full 20286-file corpus (including vendor) still checks in ~1.7s with
no reappearance of the O(n²) bug found in the lexer stage - purely
additive, zero behavior change, since nothing downstream reads the new
spans yet.

**Shipped - diagnostics carry real spans.** `checker::Diagnostic`
gained a `span: Span` field. Most of the 16 push sites got it for free
from the `Expr`/`Stmt` already in scope - each pointing at the most
specific offending sub-expression (e.g. the mismatched argument
itself, not the whole call). `check_generic_bounds`/
`check_class_generic_bounds`/`check_variance`/`check_args`'s
arity-mismatch check each needed an explicit `span` parameter threaded
in from their caller, since none of them has a single `Expr`/`Stmt`
representing the construct being diagnosed. Verified with hand-checked
span assertions on 4 existing checker tests (argument-type mismatch,
arity mismatch, return-type mismatch, undefined variable) and against
the real corpus: the 1492-file `src/` subset still parses 1492/1492
with the identical 2327-diagnostic count, full 20286-file corpus
unaffected.

**Shipped - real CLI/LSP output positions, the phase's whole
user-visible payoff.** `Checker::into_diagnostics()` sorts every
diagnostic by `(line, col)` before returning - internal checks run in
traversal order (an assignment's right-hand side before its left,
properties before method bodies, ...), not necessarily source order,
so this is a real, load-bearing fix, not a defensive no-op (a
regression test exploiting the rhs-before-lhs case confirms it).
`--check`/`--check-project`/`--incremental-bench`/the default sample
mode all print real `path:line:col:` prefixes now, compiler-style.
`lsp.rs`'s `diagnostic_to_json` converts each diagnostic's real `Span`
into LSP's 0-indexed range, replacing the hardcoded `{0,0}`-`{0,1}`
every diagnostic used to get regardless of its actual position.
Spot-checked against the real Piwigo corpus: a reported `line:col`
lands exactly on the offending argument expression in the real source
file. Known, deliberately-accepted remaining imprecision: LSP wants
UTF-16 code-unit columns; this project's `Span` counts UTF-8
characters - identical for any line without astral-plane characters
(the overwhelming majority of real PHP source).

**Shipped - discrete rule-gating levels (the first of Stage 5's two
independent mechanisms).** `checker::Diagnostic` gained a `level: u8`
field. Each of the 16 push sites is tagged with a level loosely
mirroring real PHPStan's own level groupings - verified against a
fresh `phpstan/phpstan-src` clone's `conf/config.levelN.neon`
parameter flags rather than recalled from memory: 0 for the
undefined-symbol/arity family (matches PHPStan's own level-0 bucket,
though this project doesn't implement PHPStan's own
$this-only-vs-any-expression distinction for method/property checks -
a documented simplification, not an oversight), 2 for
undefined-property/array-shape-key (PHPDoc-derived structural checks),
3 for return-type/property-type mismatches (matches PHPStan's own
`checkPhpDocMethodSignatures` level exactly), 5 for argument-type
mismatches (matches PHPStan's own `checkFunctionArgumentTypes` level
exactly), and 6 for this project's own generic bound/variance checks
(no direct PHPStan equivalent - generics aren't level-gated there at
all). `--check-project` gained a `--level N` flag, defaulting to the
maximum level so existing report-everything behavior is unchanged
unless a user opts in. Verified against the real corpus: unfiltered
diagnostic count unchanged (2327), with sensible progressive counts
at each level threshold.

**Shipped - strictness parameters, completing Stage 5.**
`registry::Registry` gained `strict_mixed_arrays: bool` (defaults to
`false` via the existing `#[derive(Default)]`, so every one of its 10
existing construction sites kept compiling unchanged - a plain `pub`
field, not a constructor parameter, since it protects no invariant).
When set, `is_assignable` stops treating an unknown array literal's
type (`Array(Mixed)`) as freely compatible with any specific
array/shape type - the one real "mixed handling" leniency
`is_assignable` has purely on its own, matching real PHPStan's own
level-9-style "be strict about values with mixed type" philosophy.
`--check-project` gained a `--strict-mixed-arrays` flag, off by
default. Verified end-to-end: default output is byte-identical (2327
diagnostics), the flag surfaces 1814 additional real findings on the
Piwigo corpus (spot-checked genuine: untyped runtime-built arrays
returned where a specific element type or shape was declared).

**Phase 11 Stage 5 is now fully DONE** (both discrete levels and
strictness parameters shipped).

**Shipped - Stage 6's baseline mechanism, real PHPStan's own
`baseline.neon` simplified.** New `src/baseline.rs`:
`--generate-baseline <file>` records every current diagnostic (keyed
by file path + logical location + message - deliberately not raw
line/col, so a baselined finding survives incidental code shifts
elsewhere in the same file) into a JSON file via `serde_json` (already
a real dependency from the LSP work, so this needed no new one);
`--baseline <file>` filters out anything already recorded, so existing
debt doesn't block CI while new issues are still caught. The file path
has to be part of the key, not just the location label, since many
diagnostics share the generic `"<main>"` label for top-level script
code across different files. Baseline generation runs after
level/strict-mixed-arrays filtering, so a baseline reflects exactly
what a normal run at that configuration would show. Verified against
the real corpus: 2327 entries baselined, 0 diagnostics on a re-run
with that baseline applied, default no-flag behavior unchanged.

**Shipped - the `clap` CLI migration.** Replaced `main.rs`'s
hand-rolled `args.get(1) == Some("--check-project")`-style dispatch
(plus the `extract_level`/`extract_bool_flag`/`extract_value_flag`
scanners) with a derived `Cli`/`Command` enum. Per an explicit
decision, this also adopted clap's idiomatic bare-subcommand syntax
(`phpstan-rs check-project <dir>`, no leading `--`) rather than
preserving the old flag-prefixed style - a deliberate breaking change
to the CLI surface, acceptable for this pre-1.0 internal tooling. The
old silent default (a token/AST/diagnostics debug dump when no
arguments were given at all) is now an explicit `demo` subcommand; a
bare `phpstan-rs` invocation prints help and exits non-zero (real
`arg_required_else_help` clap behavior) instead. Every `tests/cli.rs`
test updated to match. Verified: full test suite passing, real corpus
check unchanged (2327 diagnostics, same parse pass rate) - a pure
CLI-parsing-layer refactor.

**Shipped - the TOML config file, completing Stage 6.** New
`src/config.rs`: a minimal, fully-optional `Config { paths, level,
baseline, strict_mixed_arrays }`, loaded via `check-project`'s new
`--config` flag. CLI flags always take precedence over the matching
config value; `dirs` falls back to `config.paths` when no positional
directories are given (so `dirs` is no longer `required = true` on
the clap side - the "must know what to check" requirement is enforced
after consulting the config, with a real error when neither source
supplies anything); `--generate-baseline` stays CLI-only (generating a
baseline isn't a standing project setting the way checking against
one is). Verified: the real Piwigo corpus check is identical whether
invoked directly or via `--config` (2327 diagnostics either way).

**Phase 11 (source positions, levels, config) is now fully DONE** -
all six stages shipped: real column-precise spans end-to-end (lexer
through AST through diagnostics through CLI/LSP output), the full
two-part level system, and config/baseline (baseline mechanism, `clap`
migration, and TOML config file all shipped).

## Phase 12: Bespoke per-function/method type extensions - PARTIAL

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

1. **`array_map`/`array_filter`/`array_reduce` - DONE** (`array_walk`
   remains open, see below). Shipped as bespoke special cases in
   `checker::infer_expr`'s `ExprKind::Call` handling
   (`infer_builtin_call_return_type`), mirroring real PHPStan's own
   `src/Type/Php/` architecture directly - **not** the generic
   `callable(T): U` + `infer_call_return_type` template-unification
   design this section originally sketched. That design was tried
   during planning and rejected: it works for a closure with an
   *explicit* native param type hint, but for the far more common
   untyped style (`fn($x) => ...`), the closure's own standalone
   inferred return type has no way to know `$x`'s real type - it's
   `Mixed` in isolation, so the templated `U` would just bind to
   `Mixed` again, no real improvement. The actual fix needs the
   *other* argument's binding (the array's element type) to flow
   *into* the closure's own body-inference before computing its
   return type - a genuinely two-pass, per-function mechanism
   (`infer_closure_return_with_param_overrides`, generalized from
   singular to plural once `array_reduce`'s two-param callback
   `($carry, $item)` needed binding both at once), not something
   generic template unification does in one pass. `array_filter`
   needs no closure introspection at all: filtering never transforms
   elements, so its return is simply whatever the input array's own
   type already is. `array_reduce`'s result is ultimately whatever the
   callback's own return type is (a fold's return becomes the next
   call's `$carry`) - `$carry` binds to `$initial`'s own type (`Null`
   when the optional 3rd argument is omitted, matching real PHP's own
   default), `$item` binds to the array's element type, same as
   `array_map`. `array_map` is scoped to its common 2-arg single-array
   form; the real function's multi-array variadic form and
   `array_walk` (a by-ref-mutation function - a different, harder
   mechanism than return-type inference, since it needs checking the
   callback's own parameter types against the array rather than
   deriving a return type at all) remain unshipped. A real,
   pre-existing bug was found and fixed while wiring this up: for an
   unqualified builtin call inside a namespaced file (this project's
   own corpus is entirely namespaced), the bespoke dispatch was
   checking the wrong name - the failed namespace-qualified attempt,
   not the bare name that actually resolved via the global-namespace
   fallback. Verified against the real Piwigo corpus: default-mode
   diagnostic count unchanged (2327) throughout both commits, and the
   optional `--strict-mixed-arrays` mode's noise dropped (4141 → 4102)
   once `array_map`/`array_filter` shipped, unchanged again once
   `array_reduce` followed - consistent with genuine precision
   reducing false positives, not corpus noise.
2. **`preg_match`/`preg_match_all`** - by-ref `$matches` shape
   inference from the pattern's own capture groups (named + numbered).
   The one extension here that's genuinely hard to approximate - needs
   real PCRE-aware parsing of the pattern string, not just the type
   system. No suitable Rust PCRE crate exists without pulling in a C
   dependency (a real `pcre2` binding); Rust's own `regex` crate is a
   different flavor of regex (no backreferences, different syntax
   edge cases) that would silently produce wrong answers on real PCRE
   patterns.
3. **`sprintf`/`printf` - DONE** (`vsprintf`/`vprintf` remain out of
   scope - their arguments come from a runtime array, not enumerable
   variadic args, so there's nothing to statically compare against).
   New `checker::check_format_string_call`, wired into `check_expr`'s
   `Call` handling: validates a literal format string's placeholders
   against the arguments actually passed, two deliberately narrow
   checks to avoid false positives - too few arguments for the
   placeholders present (real PHP throws `ValueError` for this,
   unambiguous; "too many" is never flagged, since real `sprintf` just
   ignores extras), and an argument whose type couldn't possibly
   satisfy its specifier (an array for any specifier, or an object for
   a numeric one - never an object against `%s`, since PHP casts via
   `__toString`, extremely common and legitimate). `parse_format_specifiers`
   bails safely (no check at all) on anything it doesn't confidently
   understand: a positional specifier (`%1$s`), a dynamic (non-literal)
   format string, or an unrecognized specifier character - mirrors
   `check_shape_key_access`'s own "only a literal, never a dynamic
   value" precedent. **Found and fixed two real, independent bugs**
   verifying this against the *full* real corpus (`--check-project`
   over the entire corpus including `vendor/`, not just first-party
   `src/` - the first time this specific, expensive combination had
   been run this session): a real, previously-unguarded panic in
   `array_filter`'s own Phase 12 item 1 special case (indexed `args[0]`
   without checking `args` wasn't empty - a syntactically valid but
   semantically invalid 0-arg call parses fine and crashed the
   checker), and `non-negative-int` (a real, common PHPStan-ecosystem
   int-refinement pseudo-type, confirmed live in
   `sebastianbergmann/php-code-coverage`) missing from
   `types::parse_type_keyword_or_class`'s recognized list entirely,
   resolving as a bogus namespace-qualified `Object` instead of `Int` -
   invisible until this new check's `%d`-vs-argument-type comparison
   surfaced it as ~19 false positives. Fixing the pseudo-type gap alone
   dropped the full corpus's total diagnostic count 48496 → 48243,
   since the bogus `Object` type was quietly causing other, unrelated
   false positives too.
4. ~~**`array_keys`/`mt_rand`**~~ - fixed by Phase 8.5, not a Phase 12
   item after all: both were confirmed-live false positives from a
   stub-generation gap, not functions actually needing a bespoke
   extension.
5. **`array_column`/`array_combine`/`array_fill`/`array_fill_keys`/
   `array_chunk` - DONE**. Each a small, targeted addition to
   `infer_builtin_call_return_type`, reusing the existing
   `Type::Array`/`Type::ArrayShape` machinery - none needed new
   type-system features. `array_combine`: no transformation happens to
   the values, so the result is simply `$values`'s own type verbatim
   (mirrors `array_filter`). `array_fill`/`array_fill_keys`: every slot
   holds the same, already-known `$value`. `array_chunk`: wraps the
   original element type in one more array layer (`array<T>` in,
   `array<array<T>>` out). `array_column`: extracts one named field's
   type from an array of known `ArrayShape`s, only when the column key
   is a compile-time-known literal (mirrors `check_shape_key_access`'s
   own "only a literal, never a dynamic value" precedent). `array_flip`
   (would need tracking array *key* types, not modeled by this
   project's `Type::Array` at all) and `compact`/`get_defined_vars`
   (build their result from a string-named/whole-scope variable lookup,
   a different and much more dynamic mechanism than anything shipped so
   far) are deliberately **not attempted** - real, permanent model
   limitations, not a "not yet" gap. Verified against the real corpus:
   default-mode diagnostic count unchanged (2327); under
   `--strict-mixed-arrays`, the noise dropped slightly further
   (4102 → 4100), confirmed via a before/after diff to be almost
   entirely the same real findings gaining more precise "got" type
   wording, not findings silently disappearing.
6. **`is_array`/`is_string`/`is_int`/`is_callable`/`class_exists`/
   `function_exists`/`in_array`/`array_key_exists`/`method_exists`/
   `property_exists`** - `FunctionTypeSpecifyingExtension`-style
   condition-narrowing. This project's own `apply_narrowing` already
   has a `narrowing_type_for_predicate` table for a handful of these
   (`is_int`/`is_string`/etc., `checker.rs:1762`) - this item is about
   widening that same, already-proven mechanism to the rest of the
   list, not inventing a new one.

## Phase 13: Narrowing precision and remaining false-positive causes - PLANNED

**Three real, independent bugs found and fixed via a direct audit,
2026-08-30** (user question: "what is the current count of findings
over the corpus?", followed by cross-checking `src/`-only findings
against the full corpus for a codebase expected to read as clean).
All three were namespace/pseudo-type resolution bugs that had been
silently live for an unknown stretch of this project's history, each
found by spot-checking one suspicious-looking diagnostic message
rather than guessing:
1. A native type hint's leading `\` (PHP's own "already fully
   qualified" marker, e.g. `\Doctrine\ORM\QueryBuilder`) was stripped
   by `types::parse_type_atom` before `registry::resolve_names_in_type`
   ever saw it, silently re-qualifying every such reference against
   the current file's own namespace. Full-corpus diagnostic count
   dropped 48243 → 44716 fixing this alone. Commit `9a28d50`.
2. A quoted PHPDoc literal-string type (`'ASC'|'DESC'`, a real,
   documented PHPStan/Psalm convention) wasn't recognized at all -
   the quotes became part of a literal "class name," then got
   namespace-qualified into nonsense (`Piwigo\Db\'ASC'`). Dropped the
   count a further 44716 → 44525. Commit `ba48ba8`.
3. `checker::resolve_pseudo` only resolved a *bare* `self`/`static`/
   `parent`, never one wrapped in a `Union` - the common
   `?self`/`self|null` factory-method return type (e.g.
   `tryFromArray`-style static constructors) never resolved `self` at
   all, and `is_assignable`/`is_subtype` have no arm accepting a
   concrete class instance against a still-unresolved `SelfRef`.
   Dropped the count a further 44525 → 44353. Commit `c941d14`.

Real Piwigo `src/`'s own diagnostic count dropped 2327 → 2270 across
these three fixes. **Two more things surfaced by this same audit,
tracked here rather than fixed on the spot:**

- **Ternary conditions don't narrow their own branches.** `infer_expr`'s
  `ExprKind::Ternary` arm infers `then_`/`else_` against the same,
  un-narrowed `locals` the ternary itself sees - unlike `if`/`while`,
  which fork narrowed scopes via `apply_narrowing`. Real, live
  consequence: `is_string($x) ? $x : 'default'`'s `then` branch still
  sees `$x`'s wider, un-narrowed type (confirmed via a real call site,
  `BatchManagerUnitPageRenderer.php`'s `strip_tags(is_string(...) ? ...
  : '')`), contributing to some of the remaining `string, null|...`
  argument-type-mismatch noise. Fits this phase's existing
  "property-chain narrowing" item conceptually (both are `apply_narrowing`
  coverage gaps), but is a distinct code path (ternary *inference*, not
  `if`/`while` condition narrowing) needing its own fix.
- **A batch of `src/`-level findings audited only at the pattern level,
  not individually verified yet**: `explode`/`substr`/`strtotime`/
  `strip_tags`/`strrpos`-style `string, string|false` or `string,
  string|null` mismatches (dozens of occurrences each - possibly more
  stub-arity/version gaps in the Phase 8.5 family, possibly the
  ternary-narrowing gap above, possibly genuine), `count()` argument
  type mismatches against `array|Countable`, and several
  `__construct()`/setter-style argument-type mismatches
  (`CLASS::css()`, `CLASS::from()`) whose "expects X, got Y" shape
  looks plausible but hasn't been traced to a specific root cause. Real
  next step: sample and trace each pattern the same way the three
  fixed bugs above were found, not guess at a fix.
- **Separately, a real CLI/UX gap noticed during this same
  conversation**: `check-project` has no way to load `vendor/` for
  type-resolution context while only *reporting* diagnostics for
  `src/` - passing both directories analyzes and reports on both,
  producing tens of thousands of vendor-code diagnostics nobody wants.
  Real PHPStan solves this via its own `paths:` config (what to
  analyze) versus the Composer autoloader (used only for type
  information, not analyzed itself) - this project would need an
  equivalent "load for context, don't report" path split, not full
  Composer-autoloader parsing. Not yet scoped into a specific stage;
  candidate for Phase 16 (CI-friendly output) or its own small addition
  to Stage 6-style CLI work.

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

## Phase 19: Fuzz testing - NOT STARTED

Complements, not replaces, the real-corpus testing this project relies
on throughout: corpus testing finds bugs by construction (real code
humans wrote), fuzzing finds a different class of bug (adversarial,
byte-level malformed input) - the lexer/parser's own panic-based error
handling makes a malformed-input crash a real, if narrow, risk
category the corpus alone can't rule out. `arbitrary` + `cargo-fuzz`
for the lexer/parser specifically; `proptest` for `types.rs`'s
subtyping logic (reflexivity, transitivity) as Phase 10/10.5's
generics and Phase 11's stricter union/nullable handling make that
logic more complex. Both dev-only, never shipped in the release
binary. Not yet designed in any real detail - name and rough intent
only, deliberately deferred while positions/levels/config (Phase 11)
and the type-extension/rule-catalog phases (12-15) are still open.

## Phase 20: Performance - NOT STARTED

Not yet a real, detailed plan - just a named placeholder, motivated by
a real incident: Phase 11's own lexer-span work shipped a genuine
O(line length x tokens on it) blowup that hung for minutes on a real
file already in this project's test corpus (a single 1.25-million-
character generated line), caught only because every phase's own
verification step includes a real-corpus check - not by any dedicated
performance-regression tooling, which doesn't exist yet. Candidate
scope once this gets real design attention: a small benchmark suite
(current parse/check throughput on the standing Piwigo+vendor corpus,
so a future regression like Phase 11's is caught by CI rather than by
chance) and profiling on pathological inputs (extremely long lines,
deeply nested expressions, very large single files) rather than only
"many ordinary files," which the existing corpus already covers well.

## Known limitations

Every deliberate "this checker doesn't model X" boundary, gathered in
one place so none of them are only discoverable by reading in-code
comments. Each is either tracked under a phase above, or is a
permanent design boundary this project doesn't intend to close - kept
separate below so "not done yet" and "not going to happen" aren't
confused with each other.

**Tracked under a phase:**

| Limitation | Phase |
| --- | --- |
| No real source positions, level system, or config/baseline | 11 |
| No bespoke per-function/method type extensions (`array_map`/`preg_match`/`sprintf`/condition-narrowing, ...) | 12 |
| A handful of stubs (`pg_query_params`, `pg_query`, ...) interleave multiple full parameter lists under duplicate parameter names across version eras rather than Phase 8.5's clean superseded-param-plus-variadic shape, and still miscount arity | 12 |
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
  project's PHPDoc parser deliberately doesn't attempt.
- **A dynamic class target** (`$obj::method()`, `(new Foo())::CONST`)
  is never checked, by the same "don't guess" precedent `Expr::New`
  already set for a dynamic class-name expression - there's no static
  name to resolve at all, not a missing feature to build.
- **A generic bound checked against a class this project has no real
  data for** (`checker::is_type_fully_known`, Phase 10.5) silently
  skips that check rather than guessing - the same "don't know, don't
  guess" precedent as the two boundaries above, applied to a
  PHAR-bundled dependency (real `phpstan/phpstan` ships with no loose
  source at all) or a class name that's actually a local
  `@phpstan-type` alias this project doesn't parse.
- **Variance checking (Phase 10.5) doesn't cover properties, or flip
  variance through a nested generic argument.** Properties: this
  project doesn't track `readonly` at all, and a *readonly* property
  typed `T` is actually a legitimate covariant-safe position -
  checking properties without that information would misflag every
  real one. Nested arguments: a `T` found inside another class's own
  `Collection<T>` type argument is checked the same as a direct
  occurrence, not re-derived from `Collection`'s own variance - a full,
  correct algorithm needs real per-class variance composition this
  project doesn't attempt.
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
