# phpstan-rs roadmap

A from-scratch Rust PHP parser + type checker, aimed at growing toward
PHPStan feature parity. Phases 8+ are grounded in direct inspection of
`phpstan/phpstan-src` and `jetbrains/phpstorm-stubs` (not recalled
knowledge) and verified at real scale against Piwigo's own codebase
(`src/`, `tools/`) plus its `vendor/` tree, using a frozen local copy
(`/home/torres/piwigo17-frozen`, `phpstan-rs.toml`-scoped) so
before/after diffs aren't disturbed by the live corpus's own file churn.

Status key: **DONE** shipped and verified. **PARTIAL** some of the
phase's own scope shipped, some didn't. **PLANNED** a real, detailed,
source-grounded design exists, not started. **NOT STARTED** a name and
a rough idea only.

This document has five sections after Current status: **Done**,
**Pending / partial**, **Newly identified gaps** (real feature-parity
gaps a periodic audit against `vendor/phpstan-src/src/Rules/` found with
no phase/tracking of their own yet - triage them into a numbered phase
when picked up, don't leave them here indefinitely), **Reverted**
(implemented, corpus-verified, then undone - kept so the real root cause
isn't re-discovered from scratch next time), and **Known limitations**
(deliberate boundaries not tied to any one phase). See **Verification
approach** at the end for the methodology behind every corpus number
cited above.

Detailed round-by-round investigation narrative (which real corpus file
surfaced which bug, in what order, fixed by which specific helper) is
deliberately not kept here once a phase settles down - it stays fully
recoverable from git history and commit messages. This file tracks
*what's true now*, not a lab notebook of how it got that way. (Rewritten
to this condensed form twice before, as the narrative regrows between
rewrites - see git history for `ROADMAP.md` if a past finding's own
reasoning is needed verbatim.)

---

# Current status

- Piwigo frozen-corpus diagnostic count (`phpstan-rs.toml`-scoped,
  `/home/torres/piwigo17-frozen`) - the primary false-positive-hunt
  metric - is 359 as of `c522416`/`860b2b3`, up from a 355 pre-batch
  baseline (`264d910`) with every one of the 4 net new diagnostics
  confirmed genuine (new `throws.unusedType`/type-precision findings
  from the `0247b04`/`d1d1d61` phase-work batch, Phases 17, 36, 37, 39,
  40, 42, 43, 48, 49, 50, 53, 61, 63, 64, 67, 69, 70, 71, 76, 77). That
  batch's own corpus diff *has* now been run (both scoped and
  `--no-composer` unscoped) and surfaced two real regressions, both
  fixed: a `NameContext::use_map` cross-kind collision (Phase 48.1's
  case-insensitive lookup let a `use function`/`use const` import
  answer an unrelated class reference of the same lowercased name -
  4602 false `staticMethod.notFound` diagnostics at its worst) and a
  missing body-presence guard on `check_too_wide_throw_type` (flagged
  every abstract/interface method's own `@throws` tag as unused).
  Every run since is saved to `.corpus-runs/{scoped,unscoped}/
  <commit-hash>.txt` (gitignored, see that directory's own README) -
  check there before re-running an old commit to get a comparison
  snapshot. Re-verify against real PHPStan before citing an exact
  number either way - it drifts release to release even without new
  checks shipping.
- Full Piwigo+`vendor/` corpus (~20,300 files, unscoped): 40,503
  diagnostics as of `c522416`, the overwhelming majority genuine
  findings in third-party code this project doesn't control. Re-run
  before citing a number - it drifts as vendored dependencies and this
  project's own precision both change.
- Test suite: 2694 unit tests, all passing (build/test/clippy/fmt
  clean as of `c522416`).

---

# Done

Foundational work (parser, type system, registry, narrowing engine,
LSP/incremental infra, output formats, config/baseline) plus every
phase-tracked rule family that's fully shipped. Grouped by area, not
strictly by phase number - a phase that's only partly done lives in
**Pending / partial** instead, even if most of it shipped.

## Parser, lexer, AST

Full PHP 7-8.5 grammar: enums, DNF types, `declare()`, goto/labels,
asymmetric visibility, clone-with, property hooks, attributes (`#[...]`
lexed into real AST nodes, not discarded), first-class callables,
match expressions. Real source spans threaded through the whole AST
(UTF-8-aware columns, O(log n) position lookup) back diagnostics, LSP
output, and baseline matching. A guard-clause assignment nested inside
a comparison/`&&`/`||`/`+`/`-`/`.`/cast operand with no enclosing
parens (`if (false === $x = fn())`) parses with real PHP's own actual
precedence, not just at the top of the whole expression.

## Registry / symbol resolution

Namespace- and `use`-import-aware name resolution, tracked positionally
per file (not "last `namespace` in the file wins"). Real
`jetbrains/phpstorm-stubs` (558 files) and `phpstan/phpstan-src`
(~1800 files, for PHPStan-extension-authoring code) vendored and parsed
into the registry alongside project code, with real stub-vs-real-class
merge semantics (a project's own `stubFiles`-style override merges
member-by-member rather than replacing a class wholesale) and duplicate/
conditional-declaration tiebreaking (arity/type-precision based) for the
many real "two declarations of one name, gated by PHP version" shapes
both corpora use. Composer-aware `vendor/` autoload-based symbol
resolution. A `define()`/`const`-based global-constant registry
resolves literal-shaped constant values project-wide.

## Type system

Generics with call-site substitution, `@extends`/`@implements` chain
substitution, template bound enforcement (`@template T of Bound`) and
variance checking (`__construct` exempted). String/array accessory
refinement types (`non-empty-string`, `non-falsy-string`,
`numeric-string`, `list<T>`, `non-empty-array<T>`) built on the existing
intersection-type mechanism, plus `IntegerRangeType`
(`positive-int`, `int<min,max>`, with a real range-arithmetic surface -
`+`/`-`/`*`/`/`/`%`/`**`/`<<`/`>>` all transcribed directly from real
PHPStan's own `InitializerExprTypeResolver`) and a real cast/coercion
method surface (`toInteger`/`toFloat`/`toString` legality, including
through `Intersection`). `print`/`InvalidPartOfEncapsedStringRule` and
the stdlib `IntegerRangeType`-consumer extensions are wired up too.
`Type::Array` carries a real key type, not
just a value type. `object{...}` shapes (including a genuinely
unsealed one, `array{..., ...}`), `value-of<T>`/`key-of<T>` (a
`value-of<T>` naming a real registered enum resolves via a one-time
deferred-resolution registry pass; other cases stay `Mixed`, matching
real PHPStan's own lack of a special case there), `OffsetAccessType`
(`SomeShape['key']`), and conditional PHPDoc return types (`@return
($x is T ? A : B)`, widened to the union of both branches) are all
parsed and resolved instead of mangling into bogus class names. Enum
cases have their own singleton `Type::EnumCase` identity, with
`::from()`/`::tryFrom()` narrowing to the exact matching case on a
literal argument. GMP/`BcMath\Number` operator-overload dispatch.
`Type::Callable(Option<Arc<CallableSig>>)`: a `Closure(...)`/
`callable(...)` PHPDoc shape's own per-parameter/return signature
parses into a real `CallableSig`, threaded through `Display`, the
three `is_assignable` sup-side arms, `member_possible_given_native`,
the `is_callable()` narrowing arm, and all four recursive `Type`-tree
walkers (`mark_template_params`/`substitute_type_aliases`/
`resolve_names_in_type`/`substitute_template`) - `is_subtype` is
deliberately lenient in both directions between a shape and the bare
marker, and between two different shapes (no real
contravariant-parameter/covariant-return comparison attempted), the
biggest false-positive-avoidance call in this feature (see Rule
catalog for its consumer-side contextual closure-body checking).
PHPDoc-inheritance backfill: a method correctly omitting its own
`@param`/`@return` in favor of an ancestor's/interface's tag resolves
through that tag instead of looking untyped, including through
anonymous classes and promoted-constructor-property inline `@var` tags,
with real explicit-vs-implicit `mixed` provenance surviving the
inheritance hop and the backfilled type reconciled against the child's
own native type hint the same way an *own* docblock already is (not
cloned in verbatim, which would let an ancestor's broader doc type
override a child's own correct, narrower native signature).

Generic template richness: real bound/variance/scope/default per
`TemplateParam`; bound-aware subtype/assignability; substitution
recursion into a generic class reference's own argument list;
variance-aware comparison of two concrete generic instantiations' type
arguments (`Collection<Cat>` and `Collection<Dog>` are no longer
mutually assignable just because argument variance was previously
ignored); real call-site variance annotation syntax (`Collection<covariant
Foo>`/`<contravariant Foo>`/`<*>`); generic-reference arity/
well-formedness checking, scoped to the `extends`/`implements`
ancestor-declaration site (matching real PHPStan's equally narrow
`ClassAncestorsRule`/`InterfaceAncestorsRule`, not its general-purpose
`GenericObjectTypeCheck`). `bind_template_param`'s call-site binding
recurses into a `Union` param type now (`class-string<T>|null`,
phpstorm-stubs' pervasive nullable-templated-parameter idiom, previously
silently unbound); `substitute_class_templates`'s own class-vs-method
template scoping compares `TemplateParam::scope`'s real declaration-site
identity rather than a same-named `HashSet` (a real bug: a method's own
`@template T` sharing its enclosing class's own class-level `@template T`
name - e.g. `ReflectionClass`'s class-level `T` and `getAttributes()`'s
own, unrelated method-level `T` - used to get eagerly captured and wiped
to `Mixed` by the class-level substitution before the method's own
call-site binding ever ran). `union_types` (Phase 68.3) now merges two
structurally different `Array(key, val)` members into one `Array(key1|
key2, val1|val2)` instead of keeping them as separate, redundant union
members - real corpus false negative this fixed
(`vendor/symfony/var-exporter/ProxyHelper.php`): a dynamic-key index-
assignment inside a loop's `if`-branch (assigning an object, then
`continue`) and its own fall-through path (assigning a different value)
each built their own separate `Array` member instead of merging into
one precise type, one of the two root causes behind the earlier,
dropped `arrayFilter.same`/`arrayFilter.alwaysEmpty` attempt (see
"Pending/partial", Phase 68). Corpus-verified project-wide: fixes the
identical redundant-union-member pattern across dozens of unrelated
findings (cleaner, correctly-merged displayed types), and surfaces
several new, plausible true positives from a real key type that was
previously under-reported (a dynamic array key genuinely built from
more than one source type, e.g. `int|string`, is now visible as such
project-wide instead of only the first-seen branch's own narrower key
type).

## Narrowing engine

`instanceof`, equality/identity, `isset`/`??`/`??=` (including the
offset/property-chain reversal - `isset($x['key'])`/`isset($this->prop)`
provably-always-set/never-null detection, and a provably-*absent*
offset getting its own distinct diagnostic, mirroring real PHPStan's
`IssetabilityDescriptor` chain-walking), common predicate functions
(`is_*`, `str_contains`/`str_starts_with`/`str_ends_with`,
`array_is_list`, `method_exists`-adjacent), PHPUnit assertion methods
(`assertIsString`/`assertNotNull`/etc.), De Morgan-aware compound
`&&`/`||` chains (cross-leaf, through parenthesized/cast wrappers,
through a closure's own `use (&$x)` by-ref write-back). `break`-reached
loop state merges into the post-loop scope; a `finally` block's own
unconditional exit point correctly overwrites the try/catch side's.
`finally`'s own entry scope (Phase 68.2) is no longer just the try
body's full-completion state - real PHP can jump into `finally` the
moment an exception is thrown anywhere inside the try body, including
before a variable it conditionally reassigns ever runs, so `finally`'s
own body is now checked against that pre-try state merged with the
normal completion state (a materially cheaper, still-sound
approximation of a real per-throw-point model, which stays out of
reach at reasonable size); only the net effect `finally`'s own body
actually produces carries forward to code after the whole
try/catch/finally statement, which still sees the narrower, unwidened
completion state.
`count()`/`array_is_list`/`str_contains`-family truthiness narrowing,
including the mirrored/negated `count($x) < 1` shape. Comparison-outcome
narrowing for `<`/`>`/`<=`/`>=` between known-numeric operands, and a
loose (`==`/`!=`) truth table between differing scalar literal kinds,
transcribed from real PHPStan's own `LooseComparisonHelper`.
`is_a()`/`class_exists()`-family narrowing was evaluated and declined
(near-zero corpus incidence, or - for `class_exists()` specifically -
an unobservable no-op given how this project already represents a
bare `class-string`). One narrow `isset`/`empty`/nullsafe residual,
left disclosed rather than chased further: a function bundling both a
legacy and modern native-typed variant behind a runtime
`PHP_VERSION_ID` branch (not just `function_exists()`) can still share
one registry slot with the wrong nullability assumption (4 corpus
occurrences) - investigated in depth, by all available static
reasoning it should already resolve correctly, but couldn't be
conclusively confirmed either way without an isolated corpus run.

Fixed-point loop-body convergence (`Checker::converge_loop_body`,
mirroring real PHPStan's own bounded `LOOP_SCOPE_ITERATIONS`): a
`While`/`DoWhile`/`For`/`Foreach` body is checked against a candidate
entry state, then re-checked with that entry unioned against the
previous pass's exit whenever the two differ, capped at 3 total passes,
with only the final pass's diagnostics kept (every earlier pass's own
diagnostics are discarded, not just its state). This is the foundation
the unreachable-code/constant-condition family below depends on - see
that entry and **Reverted** for the real soundness bug this fixed.

## Rule catalog (checker)

Symbol existence (classes/functions/use-imports/typehints, catch
clauses, `extends`/`implements`/`instanceof`/trait-use targets), plus a
`use`-imported build-tool-mangled-prefix check (`class.prefixed`).
Call-argument binding (named args, spread/unpack, by-ref legality,
constant-array-spread expansion - `f(...['a', 'b', 'named' => $c])`'s
own individually-unpacked elements type-checked against their real
target parameter, for the fully-static-shape case).
`MissingTypehintCheck`'s full level-6 battery. Property/method/constant/
class-constant visibility enforcement end to end (read, write, call,
static access, late-static-binding-unsafe `static::` access, PHP 8.4
asymmetric write-visibility), plus the whole first-class-callable family
running the same checks an ordinary call gets. Unused-symbol detection,
matching real PHPStan's three `UnusedPrivate*Rule` classes end to end:
unused private class constants (`classConstant.unused`), unused private
methods (`method.unused`), and all five unused/never-read/never-written
property shapes (`property.unused`/`.neverRead`/`.onlyWritten`/
`.neverWritten`/`.onlyRead`).
Duplicate-declaration scans within a class/trait/enum body, a
same-file case-insensitive class-vs-`use`-alias collision, and
duplicate top-level *function* declarations (a real, whole-project,
cross-file scan - distinguished from phpstorm-stubs' own established
legitimate multi-declaration/real-overload pattern rather than
conflicting with it). Override-
compatibility (visibility narrowing, missing/extra/optional/by-ref/
variadic parameter mismatches, `#[Override]`'s unconditional-fatal-error
half, running uniformly for a class's own methods and a trait-supplied
one). Nine declaration-site modifier-consistency checks plus the
class-constant `final`/native-type-modifier trio. Enum sanity and
generic-bound checking wired to enums.

Invalid-operand-combination detection (`ErrorType`): the
operand-validity matrix across binary/comparison/unary/inc-dec
operators, plus division/modulo-by-literal-zero detection. A `Union`'s
own legality stays "at least one member is legal" rather than real
PHPStan's stricter "every member" - a deliberate leniency choice for
this project's coarser unions.

Purity / side-effect model: a registry-build-time fixed point
(`registry/purity.rs`, not a Collector - see Phase 82) decides which
functions/methods/constructors are provably effect-free, flagging a
discarded statement-level call to one. A call is only ever resolved as
a dependency when its target is knowable without type inference
(`$this->`/`self::`/`static::`/`parent::`/`new self()`/`new static()`/
a literal class name/a plain function call by name); any other
receiver shape disqualifies the whole declaration, matching real
PHPStan's own identical "unknown callee" restriction. `@phpstan-impure`
tag validation is one-directional only (flags a declaration tagged
impure that this project's classifier can *prove* pure; the opposite
direction would conflate "genuinely impure" with "unknown,
conservatively disqualified" in this project's binary model - a real
false-positive risk). The same-family bare-statement `NoopRule` family
(`checker/noop.rs`) - a discarded `xor`/`&&`/`||`/ternary expression
whose result is unused (`logicalXor.resultUnused`/`booleanAnd.
resultUnused`/`booleanOr.resultUnused`/`ternary.resultUnused`), gated
on the whole expression being side-effect-free - predates the
transitive slice above. Unbuilt: the fuller ~20-kind impure-point
classification with `certain`-vs-`maybe` grading, and
`@phpstan-immutable` tag validation. One disclosed limitation inherited
from real PHPStan's own actual algorithm, not a bug of this project's
making: neither project counts constructor-promoted-property
initialization as an impure point, so a `parent::__construct(...)`
call chaining into a promoted-only ancestor constructor can look like
a discarded no-op even though it genuinely initializes state.

Property-hook checker validation (PHP 8.4): virtual-property detection
via an empirically-confirmed rule (a hooked property is virtual iff no
hook body references `$this->propName` anywhere in its own function
scope - verified against real `ReflectionProperty::isVirtual()`, since
neither vendored corpus documents the algorithm), `propertyGetHook.
noRead`/`propertySetHook.noAssign`, `SetPropertyHookParameterRule`'s
native-type-contravariance half, real function-like body checking for
hook bodies (undefined variables, return-type/generator-return
mismatches), and `PropertyVarianceRule` reusing the existing
variance-walk primitive, generalized to promoted constructor properties
too (get/set hooks, variance) - no longer limited to plain property
declarations.

`@internal` restricted-usage engine (function/method/static-method
calls, property/constant reads and writes, instantiation, `instanceof`,
`catch`, implicit `__toString()`, first-class-callables, and a named
function/method's *and* a closure/arrow-fn literal's own native
param/return type-hints), off by default. `@deprecated` usage
warnings across the identical access-shape set, off by default - a
class/interface/trait/enum-level `@deprecated` tag cascades down to
member usage, a deprecated-usage warning is suppressed from within an
already-deprecated scope (using another deprecated member from inside a
deprecated one is expected, not worth flagging), and `__toString()` gets
its own corrected message/identifier rather than sharing an unrelated
one.
Attribute existence-checking plus target-validity for every declaration
kind the parser captures an attribute list for (property/method/
function/param/class/trait/enum/enum-case/class-const/closure/arrow-fn/
property hook/global `const`), plus constructor-argument validation
(abstract/no-constructor/non-public-constructor checks, arity/named-
argument/by-ref/type checking, ancestor-chain-aware, case-insensitive
name resolution). `attribute.nonRepeatable` (a `HashSet` scoped fresh
per declaration site, matching real PHPStan's own per-`check()`
accumulator) and `attribute.deprecated` (gated on `check_deprecated_usage`,
reading a class's own `@deprecated` tag) are done too. PHP's native
`#[\Deprecated]` misuse on a class-like declaration itself
(`<kind>.deprecatedAttribute` - a real, unconditional PHP fatal error for
class/interface/enum, confirmed directly against real `php`; PHP-8.5-
gated for a trait) and `#[\AllowDynamicProperties]` misuse
(`<kind>.allowDynamicProperties`/`.allowDynamicPropertiesReadonly` - a
readonly class, enum, interface, or unconditionally a trait) are done.
Self-declaration-site parity (`attribute.interface`/`.enum`/`.abstract`/
`.constructorNotPublic`, generalizing what was originally just the
trait-only `attribute.trait` check) is done. `#[\Override]` on a
property is PHP-8.5-gated too (`property.overrideAttribute`). Every
attribute reference itself also gets `<kind>.nameCase` parity, reusing
the already-shipped case-sensitivity helper.
PHPStan-API-usage checks (instantiation, static calls, class-const
fetches, `instanceof`, `extends`/`implements`/trait-use, and a
syntactically-typed slice of method calls). Checked-exception
groundwork: `@throws`-vs-`Throwable` validity, `throws.void`, dead-catch
detection, and `finally`'s own exit-point-overwrite detection - the full
transitive throw-type model across the call graph remains open.
Uninitialized-property detection, both halves (off by default, given
this class of heuristic's own real false-positive history): the
"assigned anywhere in the class body" check, and real per-constructor
premature-access dataflow (including methods transitively called from
the constructor, resolved against the actual most-derived class).
`readonly`-specific handling and `__unserialize`/`__clone`-as-
constructor recognition turned out, on direct verification against
real PHPStan's own source, to never have been this rule's own scope at
all (an illegal *write* outside the constructor belongs to the
separate, already-built `ReadOnlyPropertyAssignRule`; real PHPStan only
treats `__unserialize`/`__clone` as constructors when a project's own
config opts in, which Piwigo's doesn't) - real PHPStan's *other*
readonly-specific check, a readonly property never assigned at all
anywhere in the class (`MissingReadOnlyPropertyAssignRule`/
`MissingReadOnlyByPhpDocPropertyAssignRule`,
`property.uninitializedReadonly`), is a genuinely separate, currently
unbuilt rule. One genuine, disclosed limitation remains: a called
method's own logic gated behind an *unrelated* property's runtime
value is invisible to this module, since it tracks property
initialization only, never arbitrary values - closing this would mean
re-implementing the main checker's narrowing engine inside this
deliberately standalone walker.
Return-statement completeness (`return.never`/`.void`/`.missing`, both
explicit and fall-through) - a separate, more basic check, the actual
returned expression's type against the declared return type
(`return.type`), is also built, for named functions, methods, closures,
and arrow functions alike. The genuinely-empty-concrete-method-body
case was built, corpus-verified, then reverted (see **Reverted** - a
real, ubiquitous IDE-stub convention this project's broader
verification corpus doesn't mirror the way real PHPStan's own
stub-loading does, not a soundness bug), and later re-shipped opt-in
(`Registry::check_empty_method_bodies`, default off). A generator's own
falls-off-the-end/bare-`return;`/`return $x;` completeness is checked
against its declared `TReturn` specifically (see the full `Generator<
TKey, TYield, TSend, TReturn>` shape below), not the whole
`Generator<...>` type - a real, independent bug fixed along the way:
`return $x;` inside a generator used to check against the *whole*
`Generator<...>` type, which no real `return` statement could ever
satisfy. PHP 8.5's `#[NoDiscard]` attribute is enforced for a
statement-level function/method/static-method call
(`function.resultDiscarded`/`method.resultDiscarded`/`staticMethod.
resultDiscarded`); the real `(void)`-cast carve-out is deliberately not
modeled (a disclosed, safe-direction simplification, not a gap).

**Generator/yield type modeling** (Phase 70). `Generator<TKey, TYield,
TSend, TReturn>` is a real, resolved shape throughout, not left
unmodeled. `yield $k => $v`'s own type is the enclosing declared
`TSend`; `yield from $expr`'s own type is `$expr`'s own `TReturn`, with
five compatibility diagnostics (`generator.nonIterable`/`.keyType`/
`.valueType`/`.sendType`/`.void`). `foreach` over a `Traversable`/
`Iterator`/`Generator`/`IteratorAggregate`-typed object infers real
key/value types too (`Checker::iterable_element_type`, targeting
`Iterator`/`IteratorAggregate` directly rather than the shared
`Traversable` ancestor - `Traversable`'s own phpstorm-stubs declaration
has no real `@template` tag of its own, only a `@template-extends
iterable<TKey, TValue>` referencing the `iterable` pseudo-type, which
desugars away before any template ever attaches). `IteratorAggregate`
resolves through its own `getIterator()`'s declared return type first,
falling back to the interface's own declared generics when that return
type doesn't resolve any further.

Format-string checking for `sprintf`/`printf`/`fprintf`/`vsprintf`,
including positional (`%N$`) specifiers, exact placeholder/argument-count
matching for `sprintf`/`printf`/`vsprintf`/`vprintf` (`fprintf` stays
"too few only" - it has no backing real-PHPStan rule at all), PHP 8.0's
`%h`/`%H` specifiers, and an opt-in strict-vs-castable placeholder-type
matching mode (`check_strict_printf_placeholder_types`, off by default).
Regex capture-group inference
(literal enumeration, tagged unions for one optional group/one
top-level alternation, numeric/non-empty-string fallback typing).
A Piwigo-corpus-driven batch of small builtin checks: function-name
case-insensitivity, `define()`'s PHP-8-removed third argument,
`random_int()` literal-bound ordering, useless-return-value builtins,
array-literal duplicate-key detection (key resolution goes through
`infer_expr`, not just a bare AST literal - a class constant, a
`define()`d global constant, and PHP's own bool/null key coercion
(`true`/`false` to the int key `1`/`0`, `null` to the empty-string key)
are all real, statically-known-scalar keys too), `array_values`/`array_filter`
no-op conditions, `array_multisort()`'s stub fix,
`call_user_func(_array)`'s argument re-checking (for the two
statically-resolvable shapes: a string-literal callable, or a
two-element array literal naming a class/object plus a literal method
name), `preg_quote()` delimiter cross-checking
(`argument.invalidPregQuote`), and `function_exists()`-guard
suppression for the undefined-function check (a disclosed,
project-specific adaptation with no direct real-PHPStan equivalent).
Dozens of individual stdlib array/string dynamic-return-type
extensions: `array_fill_keys`,
`array_search`, `array_intersect_key`, `array_flip`, `array_change_key_case`,
`array_rand`, `array_count_values`, `array_splice`, `array_replace(_recursive)`,
`array_pad`, array-pointer accessors (`array_pop`/`array_shift` included),
the `array_diff`/`array_intersect` family (a disclosed residual: the
vendored stub itself, not this checker, types the PHP8+ variadic tail
of `array_udiff`/`array_uintersect_assoc` as bare `Mixed`), `preg_split`,
`array_column`, `glob`, `explode`, `tempnam`, `version_compare`. PHP
8.4's `array_find`/`array_find_key`/`array_any`/`array_all` needed no
code change at all - the vendored stub already declares them with real
generic docblocks. `filter_var`'s `FILTER_VALIDATE_*` family (including
a literal `['options' => [...]]` array's own `default`/`min_range`/
`max_range` sub-keys and its `FILTER_NULL_ON_FAILURE`/
`FILTER_THROW_ON_FAILURE` flag-conflict), `FILTER_REQUIRE_ARRAY`/
`FILTER_FORCE_ARRAY`'s own per-element return type (wrapping the same
failure-fallback-unioned element type real PHPStan's own `getType()`
computes), PHP 8.5's `FILTER_THROW_ON_FAILURE`, and `filter_input()`
itself (reading an unknown offset out of a synthetic `array<string,
mixed>` standing in for `$_GET`/`$_POST`/etc., unconditionally unioning
in both `null` and `false` regardless of `FILTER_NULL_ON_FAILURE` - a
real, counter-intuitive but verified property of real PHPStan's own
`getOffsetValueType`; `FILTER_REQUIRE_ARRAY`/`FILTER_FORCE_ARRAY`
correctly bail to the generic stub for `filter_input()` rather than
silently ignoring the flag). `curl_getinfo`'s ~73-entry table, the
`date()`/`idate()`/
`pathinfo()`/`parse_url()`/`sscanf()`/`stat()` family,
`get_called_class`/`get_parent_class`/`gettype`/`get_debug_type`.
`DateInterval::__construct()`'s own duration-string grammar validation.
`DateTimeInterface::format()`/`DateInterval::format()` return-type
wiring (the shared `date()` format-character table for the former; a
hand-rolled, zero-valued-representative specifier table for the
latter - genuinely sound, not an approximation, for every fixed-width
specifier), `DateTimeInterface::diff()` narrowing its result to
`DateInterval&object{days: int}`, and `SimpleXMLElement::asXML()`'s
real arity-dependent overload split (one argument - a filename - is
`bool`; zero arguments is `string|false`).
`UnreachableStatementRule` (`deadCode.unreachable`, level 4), reported
once per maximal unreachable run without skipping analysis of the rest.

**Structural Callable/Closure type** (Phase 53). Contextual
closure-body checking (a callback's own inferred parameter types, not
a bare unhinted check) for `array_map`, `array_filter` (2-arg default
mode), `usort`/`uasort`, `array_walk` (2-param closures), and
`preg_replace_callback` (a generic `array<int|string, string>`
`$matches` shape) - now generalized to *any* `Closure(...)`-typed
parameter, not just those six special-cased functions, since
`Type::Callable` is now `Type::Callable(Option<Arc<CallableSig>>)` (see
Type system): a `Closure(...)`/`callable(...)` PHPDoc shape's own
per-parameter/return signature parses into a real `CallableSig`
instead of degrading to a bare marker. Invoking a callable-typed value
directly (`$fn(...)`) infers its declared return type and checks its
arguments (`Checker::check_dynamic_callable_args`) - which also fixed
a real, free bug along the way: a dynamic call's own callee
sub-expression was never independently `check_expr`'d before, so e.g.
an undefined property used as a callee produced zero diagnostics.
Declined: purity/template-map/assertion tracking on a `Type::Callable`
value - a verified real architectural mismatch (this project's
template-binding and purity-tracking machinery are both scoped to a
named, textually-declared signature, and a PHPDoc-only callable shape
has no body at all).

**Unreachable-code / constant-condition family.** The full, inferred-
type-trusting family: `if`/`elseif` (with real PHPStan's own
last-condition-no-trailing-else exemption), `while`/`do-while`, the
ternary operator, `&&`/`||` (three diagnostics per node, first-level-
gated), `xor`, a standalone `!$expr` check, and a bare function/method/
static-call condition whose own stub narrows to a constant (mirroring
real PHPStan's `FunctionCallConstantConditionHelper` - a shared-helper
deferral, not a separate Collector-based mechanism), plus
`NumberComparisonOperatorsConstantConditionRule` once the comparison-
outcome range API existed. `deadCode.unreachable` recognizes a `goto`
target as reachable rather than dead. Real PHPStan's own
`ConstantConditionInTraitRule` (reconciling a trait method's own
constant-condition finding across every class that uses it) is achieved
for free by `check_trait_methods`'s existing per-host re-check plus its
own span/message dedup, not a separate mechanism - the same
"shared-helper deferral, not a Collector" pattern this family already
uses for the bare-call-condition case above. This family was reverted once
(see **Reverted**) over a real loop-staleness soundness bug, then
rebuilt in full - trusting a condition's complete inferred type, not
just bare syntactic literals - once fixed-point loop convergence
existed. Rebuilding it surfaced a long tail of real, corpus-found bugs,
each fixed with its own regression test: loop condition re-checking
that bypassed convergence, a pass's own `continue`-reached states being
dropped from its returned exit state, `&&`/`||`'s "first level" gating
missing the bare-statement case, integer-range arithmetic collapsing to
plain `Int` on reassignment, `while (true)` retry-loops and legitimate
`goto`-driven state machines misfiring, and the `switch`/`continue`-as-
`break`-equivalent gotcha (including a `try`/`catch` as a case's own
last statement correctly not counting as reaching past the switch).

Most consequential among these: a systematic audit found and fixed
**~20 separate instances** of one recurring anti-pattern across the
whole checker - a `Type::Object`-only `match`/`let-else` with no
`Union`/`Intersection` arm, silently collapsing a union- or
intersection-typed receiver's property/method access to `Type::Mixed`
(losing real diagnostics project-wide, not just in one call site) - plus
a related ordering bug in every `Intersection`-searching helper (each
one searched the broader, pre-`instanceof` type before the narrower,
just-narrowed one, instead of the reverse, so a same-named member
declared on both resolved to the wrong, less-specific one first) and an
independent, more severe bug in inherited-docblock reconciliation
(`Registry::flatten_phpdoc_inheritance_one_pass` cloned an ancestor's
own broader `@return`/`@param` doc type onto a child override verbatim,
without reconciling it against the child's own narrower *native* type
hint the way an *own* docblock already does - silently widening a
correctly-typed override into something both falsely permissive on
input and falsely demanding on output). Every fix has its own dedicated
regression test, explicitly revert-verified; the family is corpus-clean
on both the scoped Piwigo diff and the full unscoped vendor-included
one. What's left is each a real, verified reason rather than an
oversight, not a to-do: match/switch dead-arm/exhaustiveness checking
and impossible-`instanceof`/redundant-`is_string()`-style detection on
a *non-constant* narrowed type are a different, larger feature this
family was never scoped to cover (it only ever catches a condition
whose inferred type is a constant boolean); a nested `switch` as a
case's own last statement isn't recognized as exiting the enclosing
function/loop (unlike a `try`, which is, since it introduces no new
`break`/`continue` scope of its own) - a nested switch's own
`continue N` levels would need reinterpreting one level shallower
first, real complexity rather than an oversight; a genuinely mixed
case-ending branch (one side break-equivalent, the other diverging a
different way) has the identical real complexity; full `goto`/label
control-flow modeling (narrowing a value correctly across an arbitrary
jump) remains out of scope.

**PHPDoc tag ecosystem** (Phase 36). `@phpstan-*`-prefixed tag
spellings; `@property`-family validation; `@phpstan-assert`/`-if-true`/
`-if-false` (including the narrowing they drive at call sites);
`@phpstan-type` and cross-class `@phpstan-import-type`;
`@phpstan-consistent-constructor`; `@phpstan-require-extends`/
`-implements`/`@phpstan-sealed`'s consumer/enforcement half (does a
using/extending class actually satisfy the tag) - the separate
declaration-validity half real PHPStan checks at the tag's own
declaration site (is it on a trait/interface, is the named target
itself a class/interface) isn't built; `@method`/`@mixin` synthesis into
`ClassInfo` (deferred past trait-flattening so a real trait method
can't be permanently shadowed, and excluding magic-only members from
abstract-method-implementation requirements); `@readonly`/
`@phpstan-readonly`; inline `@var`'s error-reporting half for a plain
assignment statement (`varTag.type`, requiring both directions of
`is_assignable` to fail, so a `@var` that merely *refines* a wider
native type isn't wrongly flagged); an enumerated `@phpstan-*`
typo-detection whitelist. None of this needed a full grammar-level
PHPDoc parser rewrite first - each tag has its own small, bounded,
self-contained sub-grammar, the same shape existing hand-written
tag parsers already establish. `@param-immediately-invoked-callable`/
`-later-invoked-callable`/`-closure-this` declaration-time validation
is built too, plus real `$this` injection for the latter at call sites
(the closure literal passed at the call site gets `$this` seeded to
the tag's own declared type) - an earlier assessment here called this
a genuine architectural mismatch needing live per-call-site
`Scope`-threading this project's checker has no equivalent for; that
turned out to be wrong once actually attempted; nothing left unbuilt
in this tag family.

**`Type::Array` key-type / offset engine** (Phase 50).
`InvalidKeyInArrayDimFetchRule` (reverted once over a template-
substitution bug - see **Reverted** - then reattempted successfully once
the real root causes, unrelated to the original write-up, were found:
several type-tree-walking functions missing an `object{...}`-shape
recursion arm, and inline-`@var` type resolution never marking template
parameters at all). The `array_key_exists()`-narrows-a-specific-
element-access equivalent of real PHPStan's `HasOffsetValueType`, built
on this project's own existing composite `narrowing_key` mechanism
rather than replicating real PHPStan's accessory-type hierarchy.
Unsealed-shape (`array{...}`) tracking: the `...` marker is now a real
sentinel field, gating both "definitely absent" narrowing and
`shapeAccess.notFound`. `ArrayShape`'s own int-vs-string key precision
remains unbuilt.

## Performance & tooling

CI-friendly output formats (`json`, `checkstyle`). Baseline suppression
(`--generate-baseline`/`--baseline`) and inline `@phpstan-ignore-line`/
`-next-line`/`<identifier list>` suppression, matching real PHPStan's
own trailing-vs-standalone placement rule exactly. `ignoreErrors:`
config support. `--level`/`--php-version`-gated portability checks -
`PhpFeature::NoncapturingCatch`/`ThrowExpression` are both declared with
the correct (8,0) threshold but not yet wired to any actual diagnostic
(real PHPStan's `NoncapturingCatchRule`/`ThrowExpressionRule` flag the
syntax outright below that version; a real, small gap despite the
correct groundwork already existing).
Incremental multi-file `Project` with cross-file dependency
invalidation. An 8MiB worker stack (fixing a real stack-overflow crash
on deeply-nested expressions), persistent structurally-shared `Locals`
(O(1) clone), `FxHashMap` for hot build-once maps, a criterion
regression-guard benchmark, and `hotpath`, an opt-in in-process
instrumentation profiler that catches effects system-wide `perf`
sampling is too coarse to see.

**LSP hover/go-to-definition** (Phase 17). A real Content-Length-framed
JSON-RPC server. Every name reference is resolvable without receiver-
type inference, via a `decl_locations` index plus real per-name span
recovery (needed because the parser used to flatten several reference
kinds into a bare, spanless `String`). An instance receiver's own type
(`$x->method()`/`$x->prop`) resolves via a throwaway per-request
`Checker` re-running `check_function`/`check_program` over just the
enclosing scope, matched against the target by exact span equality -
this correctly reaches inside closures/arrow-fns (no extra work needed;
the enclosing-function pointer was already left in place through them)
and at top-level script scope, and a `Union`/`Intersection` receiver
resolves every class that actually declares the member being navigated
to, not just the first. Local-variable-declaration navigation is backed
by real, checker-driven reaching-definitions rather than a hand-rolled
approximation of it: `Locals` gained an opt-in `provenance:
HashMap<String, HashSet<Span>>` side-channel, populated at every genuine
syntactic binding site (assignment, destructuring, parameter, `foreach`/
`catch`/`static`/`global`, `++`/`--`/`+=`, and - since real PHP silently
creates a variable at these sites too - a first-appearance by-ref call
argument, an array-index auto-vivification write, or `preg_match`'s own
`$matches` out-parameter) and merged through the exact same
`merge_scopes`/`converge_loop_body` machinery the checker itself already
trusts for everything else, so every existing branch-merge/loop/
termination-pruning nuance (a guard clause excluding a branch that
always exits, `foreach` correctly not offering its own pre-loop value,
precise `break`/`continue` handling) is inherited for free rather than
needing its own, separately-drifting re-implementation. Remaining,
disclosed as deliberate rather than unattempted: a closure's own
`use (&$x)`/`use ($x)` capture only resolves when the outer scope
already has tracked provenance for that name at the closure literal's
own point in the file; only lexically visible binding syntax and the
three auto-vivification shapes above update provenance - an ordinary
call's incidental effect on an already-declared variable's type
(`sort($arr)`, a by-ref widen of a pre-existing variable) deliberately
never does, so a read afterward still resolves to the variable's real
declaration.

---

# Pending / partial

Each entry: what's missing, and the root cause blocking it where one's
known. Numbered by the original phase for cross-reference; the number
is not a priority order. Cross-phase dependencies, when a phase's own
entry has one, are named inline in that entry's own text - a separate
dependency graph (`PHASE_DEPENDENCIES.md`) was maintained through most
of this project's development but retired once every edge it tracked
became satisfied (no live blockers left to graph).

**17 - LSP hover/go-to-definition (PARTIAL).** See Done for what
shipped. What's left is narrow and deliberate: a closure's own
`use (&$x)`/`use ($x)` capture only resolves when the outer scope
already has tracked provenance for that name, and only lexically
visible binding syntax updates provenance at all - an ordinary call's
incidental effect on an already-declared variable never does, on
purpose (a read should resolve to where the variable was really
declared, not every call that happened to touch it since).

**18 - Extension/plugin API (PLANNED).** Deliberately last - premature
before the rule/type-extension catalog is rich enough for a third party
to want to extend.

**19 - Fuzz testing (NOT STARTED).** `arbitrary`+`cargo-fuzz` for the
lexer/parser, `proptest` for `types.rs`'s subtyping logic. Dev-only.

**40 - PHPStan-API-usage checks (PARTIAL).** Done for instantiation,
static calls, class-const fetches, `instanceof`, `extends`/`implements`/
trait-use, a syntactically-typed slice of method calls (a directly-
typed parameter/closure-capture receiver, *and* now a `$this->prop->
method()` chain through a constructor-promoted/injected or ordinary
typed property - `Walker::class_prop_types`, a minimal, class-scoped
local type tracker, not general receiver-type inference), and three
general-purpose `Rules/Api/*` rules ROADMAP never separately tracked:
`RuntimeReflectionFunctionRule`/`RuntimeReflectionInstantiationRule`
(`phpstanApi.runtimeReflection` - `is_a()`/`class_parents()`/etc. and
`Reflection*` instantiation inside a class implementing a `PHPStan\*`
interface) and `OldPhpParser4ClassRule` (`phpParser.classRenamed` - a
bare reference to one of ten renamed nikic/php-parser v4 class names),
plus `PhpStanNamespaceIn3rdPartyPackageRule` (`phpstanApi.
phpstanNamespace`, a `namespace PHPStan\...;` declaration whose nearest
`composer.json` isn't named `phpstan/*` - the one rule of the four that
needed a real file path on disk, via a new `composer::find_composer_
package_name` upward walk). The `extends` mirror now derives
`phpstanApi.interface` (vs. `phpstanApi.class`) for an interface's own
multi-parent `extends` list, matching real PHPStan's own
`ApiInterfaceExtendsRule`/`ApiClassExtendsRule` split by declaring-node
kind, including the class-only `PHPStan\Analyser\MutatingScope`
extends-exemption; `instanceof`'s own class/interface/trait identifier
distinction (`phpstanApi.{class,interface,trait}`) was already correct
before this batch, confirmed by an existing passing test - ROADMAP's own
prior text calling both a fidelity gap was accurate for `extends` only.
Full, general receiver-type inference for an arbitrary method-call chain
(folding
this whole separate `Walker` into the main `Checker` pass, the way the
design doc's own recommendation described) remains a deliberate,
disclosed scope trim - the `$this->prop->method()` slice above covers
the single most common real shape without that larger architectural
change.

**41 - Purity / side-effect model, remaining (PARTIAL).** The
transitive slice and `NoopRule`'s own bare-statement family are done
(see Done). Unbuilt: the fuller ~20-kind impure-point classification
with `certain`-vs-`maybe` grading, and `@phpstan-immutable` tag
validation.

**42 - Checked-exception / throw-type model, remaining (PARTIAL, only
one sub-item left declined).** `@throws`-vs-`Throwable` validity,
`throws.void`, dead-catch detection, and `finally`'s own exit-point
overwrite are done (see Done). Now also done: transitive throw-point
collection across the call graph (`Checker::current_function_throw_
points`/`current_function_has_unannotated_call`, recorded at every real
`throw` and every already-resolved-signature call choke point); a
curated, disclosed subset of the per-callee dynamic-throw-type
extension cluster (`checker/throws/dynamic.rs` - `intdiv`,
`array_combine`, `assert`, `json_encode`/`json_decode`, `filter_var`,
`new DateInterval(...)` [reusing the Phase 78 spec validator],
`DateTime[Immutable]::sub()`, and the four `Reflection*` constructors;
`version_compare()` and the DOM/SimpleXML/DateTime-relative-string
extensions excluded, each for its own disclosed reason);
`TooWideFunctionThrowTypeRule`/`TooWideMethodThrowTypeRule`
(`throws.unusedType`, `checker/throws/too_wide.rs`); and an opt-in
"missing checked exception" family (`throws.missing`, gated on
`Registry::check_missing_checked_exceptions`, default off given the
real corpus's documented inconsistent `@throws` tagging - consumes only
a declaration's own *explicit* throw points, never a called function's).
Still declined: `MethodThrowTypeCovarianceRule` (no call-graph-wide
implicit-throw model exists to build it on - an implicit "void when
untagged" default would false-positive heavily given how inconsistently
this corpus tags `@throws`).

**45 - Declared-vs-actual-narrowest-type inference (PLANNED).** No
mechanism compares a declared type against the narrowest type a body
could actually produce. Splits into a return/property variant (unions
every actual return/assignment against the declared type) and a
by-reference-parameter variant. The by-ref half is only partly gated on
`@param-out` tag support this project doesn't have: real PHPStan's
`ParameterOutAssignedTypeRule`/`ParameterOutExecutionEndTypeRule` also
apply, tag-independent, to an *ordinary* native by-ref parameter
(`function f(int &$x)`) - checking every reassignment inside the body,
and the parameter's value at every `return`/execution-end point, against
its own declared native type. That tag-independent half needs no new
tag concept, just per-function reassignment-site tracking; only the
`@param-out`-specific widening (a documented, deliberately wider
by-ref-exit type) needs the tag concept itself.

**46 - PCRE pattern-validity checking (PLANNED).** No malformed-regex
detection (`regex_groups.rs` only extracts structure, never validates).
Needs a real `pcre2` binding - this project's first C dependency - since
Rust's own `regex` crate is a different-enough flavor to give wrong
answers on real PCRE patterns.

**48 - Symbol-existence pipeline, remaining stretch goals (PARTIAL).**
The existence-check MVP, typed-class-constant validation, multi-
namespace-per-file positional tracking, and a `use`-imported build-
tool-mangled-prefix check (`class.prefixed`) are all done (see Done).
`ClassCaseSensitivityCheck`'s own class-name-case half is now done too:
`Registry::canonicalize_class_name_case` (a one-time O(classes)
rewrite pass run right after indexing, before any ancestor-graph walk
reads `extends`/`implements`/`uses`) plus `Registry::lookup_class`
(mirroring `lookup_function`'s exact case-insensitive-fallback shape)
fix the underlying resolution bug, and a new `class.nameCase`/
`interface.nameCase`/`trait.nameCase`/`enum.nameCase` diagnostic
(`checker::resolve::check_class_name_case`) flags a differently-cased
`extends`/`implements`/trait-`use` reference on a class/enum
declaration - scoped to that slice of the ~15 MVP existence-check call
sites, not all of them, a disclosed narrower cut. `ExistingClassesInPropertyHookTypehintsRule`'s
existence-checking half is done (Phase 43 unblocked it); its other
sub-checks (a template type not referenced in a parameter, an
unresolvable native type, a native union type pre-PHP-8.0) turned out
to be generic `FunctionDefinitionCheck` sub-pieces this project has
never built for *any* function/method, not property-hook-specific - the
native-union-pre-8.0 case is additionally blocked by a real parser gap
(native and PHPDoc typehints collapse into the same `Type`
representation, with no way to tell which was written natively). Six
more real gaps from the wider `RuleLevelHelper`-equivalent pipeline
(~27 usage-location kinds total) are now closed too:
`@phpstan-assert`/`-if-true`/`-if-false` tag target-type existence,
`@phpstan-require-extends`/`@phpstan-require-implements`/`@phpstan-sealed`
RHS existence (all four reusing `check_type_class_names_exist`/
`class_name_exists`, `checker/narrowing.rs`'s `check_assert_tag_types`
and `checker/oop.rs`'s `check_require_tag_types`/`check_sealed_tag_types`),
and two real, previously-silent gaps fixed directly in `checker/expr.rs`:
a static method call and a static-property/class-constant access
through a genuinely undefined class (`Foo::bar()`/`Foo::$x`/
`Foo::CONST` where `Foo` doesn't exist at all) produced zero
diagnostics before - neither the "found, but member missing" branch nor
any "class not found" branch ever fired for them. `@template T of
Bound`'s existing leniency and a plain `use Foo;` import-site's own
non-existence-check (real PHPStan doesn't check that either) were
re-confirmed as correct, not gaps. What remains of the ~27-location
pipeline is a genuinely smaller residual than before - still a
size-XL/low-priority stretch goal, go one-by-one only if/when there's a
concrete reason to prioritize it.

**50 - `Type::Array` key-type / offset engine, remaining (PARTIAL,
almost everything done).** See Done for what shipped.
`ArrayShape`'s own int-vs-string key precision remains unbuilt,
genuinely separate from what shipped so far.

**54 - Generic template richness (PARTIAL, variance-aware assignability
and arity checking done).** See Done for what shipped. Declined, each
for a verified reason: real PHPStan's `Static` call-site/internal
variance kind and the whole `GenericAncestorsCheck`/
`GenericObjectTypeCheck` rule family (a genuinely different, unbuilt
rule, not a variance-kind gap in what's built); the ~19-concrete-
subclass internal type representation real PHPStan uses internally (no
independently observable behavior this project's single-variant-plus-
sentinel representation doesn't already produce); `generics.notGeneric`
(indistinguishable, in this project's own `@template`-tag model, from a
legitimate stub-granted-generics case on a real zero-`@template`
built-in). Newly identified, real, still-open gap: no constructor-
argument-driven class-level generic-type inference for `new X(...)`
exists at all, so `new ReflectionClass($view)` never binds `ReflectionClass<T>`'s
own class-level `T` from `$view`'s type the way real PHPStan's own
`ReflectionClassDynamicReturnTypeExtension` does - confirmed directly
(`(new ReflectionClass($view))`'s own inferred type stays plain
`ReflectionClass`, no type argument). Not built - a genuinely missing
general capability, not a narrow bug; would need its own design.

**56 - Grammar-level PHPDoc parser rewrite (PLANNED).** `docblock.rs` is
a lenient, hand-rolled ad-hoc string tokenizer (not built on the
`regex` crate at all - plain `split_whitespace`/char-scanning) that
silently drops what it doesn't recognize, unlike real PHPStan's
recoverable-error grammar parser. Not
a hard blocker for any current Phase 36 sub-item - each has its own
small, bounded, self-contained grammar already. This phase's real
motivation is narrower: robustness of the general type-expression
grammar itself (deeply nested generics/conditional types/complex
intersections), not corpus-evidenced as a live gap today. Size: XL.

**64 - Override-compatibility rewrite, remaining (PARTIAL, one sub-item
left).** Method- and property-override halves are done (see Done),
including the interface-vs-trait-vs-extends ancestor-lookup priority
fix (matching real PHPStan's `MethodPrototypeFinder` exactly, including
its own visibility-check-skip and self-type-resolution nuances for a
trait-sourced prototype) and property `#[Override]` attribute checking.
Property hooks are done too: `PropertyHookBody::Abstract` (a new AST
variant distinguishing "no hook" from "an abstract hook with no body")
plus `property.notReadable`/`.notWritable`/`.abstractOverridingNonAbstractHook`
(`checker/oop/property_overrides.rs`). PHP 8.4 `final`/`abstract`
property modifiers are done too - including on a promoted constructor
property (`public final int $x` in a constructor parameter list is
real, valid PHP, confirmed live; only `abstract` is genuinely
impossible there) - reporting `property.parentPropertyFinal`.
`method.nonStatic`/`method.static` (an override must match its
ancestor's own static-ness exactly, either direction) and the general,
non-`__construct` case of `method.abstractOverridingNonAbstract` are
done too, both `nonIgnorable` matching real PHPStan's own
`OverridingMethodRule` exactly - a new `ClassInfo::static_methods`
mirroring `abstract_methods`/`final_methods`, with the same trait-
flattening propagation `abstract_methods` already had (a real bug
found and fixed along the way: `static_methods` wasn't trait-
flattening-aware at all, a real corpus false positive on `Symfony\
Component\Cache\Adapter\RedisAdapter`'s trait-supplied `createConnection()`).
The `__construct`-specific branch of `abstractOverridingNonAbstract`
(triggered only when no normal prototype resolves at all) stays
unbuilt - a distinct code path, not a narrowing of the general case.
Still open: native-type property variance, blocked on giving
`PropertySig` a `ty`/`ty_native` split the way `FunctionSig` already
has.

**67 - `@internal` restricted-usage engine, remaining (PARTIAL).**
Calls, property/constant access, instantiation, `instanceof`, `catch`,
`extends`/`implements`/trait-use, both a named function/method's own
*and* a closure/arrow-fn literal's own native param/return type-hints
(`parameter.internal{Type}`/`return.internal{Type}`), first-class-
callables (`$obj->method(...)`/`Foo::method(...)`, wired via `d4efb41`
into the same `check_internal_method_call` an ordinary call already
runs), and the implicit `__toString()` call an echo/print/string-cast/
interpolation triggers (`Checker::check_implicit_tostring_usage`, real
PHPStan's own `RestrictedUsageOfDeprecatedStringCastRule` - previously,
incorrectly, bundled into this phase's own Declined list below as if it
were unbuilt) are all covered (see Done). Declined, a verified genuine
architectural mismatch rather than a scope trim: the separate, user-
configured restricted-usage mechanism (`RestrictedClassConstantUsageRule`
and eight further siblings - confirmed via exhaustive grep across
`vendor/phpstan-src/src`, exactly five real implementations exist at all,
all under `Rules/InternalTag/`, i.e. the `@internal` engine already built
above) - real PHPStan's own family are bare extension-point interfaces
with *zero* built-in implementation; every real instance is a hand-
written third-party class registered via `services.neon`, and this
project has no plugin-loading mechanism at all (Phase 18, deliberately
last). A purely declarative alternative (a static `restricted_symbols:
Vec<{kind, name, message}>` config list) would be a materially different
feature shape, needing its own new phase if ever wanted.

**68 - Array no-op diagnostics, remaining (PARTIAL).** Duplicate-key
detection and the two no-op `array_values`/`array_filter`-empty checks
are done (see Done). `arrayFilter.same`/`arrayFilter.alwaysEmpty` are
done too, but opt-in (`check_array_filter_falsy_noop`, default `false`)
rather than shipped on by default - an earlier attempt at them was
dropped entirely after corpus verification caught two live false
positives, both since root-caused to independent variable-type-tracking
imprecision unrelated to this check's own logic - both now fixed (see
Done, "Narrowing engine" and "Type system"). Flip the
`check_array_filter_falsy_noop` default to `true` now that both are
resolved (not attempted in the same pass as the fix itself, to keep
the corpus diff attributable to one change at a time).

**75 - String accessory case-tracking (PLANNED).** No `Lowercase`/
`Uppercase`/`DecimalIntegerString` accessory kinds; `strtolower()`/
`strtoupper()` and friends don't produce case-tracked types. Literal-
string is never inferred from concatenation, undermining the feature's
main real-world use (SQL-injection-safety checking). Size: M.

**77 - Runtime-capability modeling, remaining (PARTIAL).** Most of the
originally-scoped functions, plus `SimpleXMLElement`'s own magic-
property type resolution, are done (see Done). Reflection-class
modeling in the registry remains unbuilt - needs genuinely new
infrastructure this project doesn't have, with no corpus evidence of
heavy usage to justify building it speculatively.

**78 - DateTime/DOM/SimpleXML, remaining (PARTIAL).** `DateInterval`'s
own duration-string grammar, `DateTimeInterface::format()`/`DateInterval::
format()` return-type wiring, `DateTimeInterface::diff()`'s `days: int`
narrowing, and `SimpleXMLElement::asXML()`'s arity-dependent overload
split are all done (see Done). `DOMDocument`/`SimpleXMLElement` XML-content
validation and `DateTime`'s relative-date-string parsing both need real
PHP-runtime/XML-parser simulation this project has no way to do -
low-priority, possibly a permanent boundary rather than concretely
scheduled.

**80 - Object-shape/key-of/conditional-type syntax, remaining
(PARTIAL).** `object{...}` shapes, `value-of<T>`/`key-of<T>`,
conditional-return-type syntax, `value-of<T>` naming a real registered
enum, and `OffsetAccessType` are all done (see Done). `key-of<T>`
naming a class/enum reference is a deliberate non-gap (real PHPStan has
no special case there either). Letting a conditional return type pick a
specific call site's real branch instead of always widening to the
union of both was investigated and deliberately not attempted: the
condition itself is discarded at parse time today, so building this
would mean a new lazily-resolved `Type` variant touching every match
arm and call site - a genuinely large, cross-cutting change, independently
sized "M" before this was ever investigated.

**82 - Collector / cross-file aggregation architecture (RETIRED - real
architecture built as a registry fixed point instead; see Phase 41,
Done).** Real PHPStan's own `Collector`/`CollectedDataNode` machinery
exists to reconcile results across its *parallel worker processes* - a
problem this project's single-process, in-memory tool doesn't have.
Every real consumer this project actually needs (enumerated and read in
full across real PHPStan's 14 collectors) is better served by a
registry-build-time fixed point, the same shape `flatten_phpdoc_
inheritance`/`resolve_deferred_value_of` already use - Phase 41's own
transitive-purity model is the one genuine, high-value consumer, and
it's built. Phase 15's own cross-file dedup piece (2 of the 14
collectors) turned out not to need this architecture either, and is
also built (see Phase 15, Done). The remaining 5 collectors feed
genuinely narrow, low-value, self-contained rules, none built.

**83 - Whole-project duplicate class-name detection (PLANNED, low
priority).** A genuine architectural conflict, not just low-priority:
the registry's own "second unconditional declaration merges as a
targeted stub override" behavior (needed for real corpus correctness)
is indistinguishable, in this project's current design, from an
accidental duplicate. Real PHPStan tells them apart via a `stubFiles`
loading mechanism this project has no equivalent of - needs
file-provenance tracking as a genuine new prerequisite. Size: XL, low
value.

**84 - `RuleLevelHelper`-equivalent tri-state type-acceptance
architecture (PLANNED, low priority).** `is_assignable`/`is_subtype`
return plain `bool`, no tri-state (`Yes`/`No`/`Maybe`)-with-reasons
result. Re-evaluated as low-value: every "maybe" already resolves to a
concrete answer somewhere in the match arms today, so the rewrite would
touch every call site without changing any diagnostic's actual outcome.
Size: XL, low-medium value.

---

# Newly identified gaps (parity audit against real PHPStan Rules/)

A 20-agent workflow cross-checked every real Rule class under
`vendor/phpstan-src/src/Rules/` (~420 classes) against this project's
actual coverage and this file's own claims - a verification pass, not
a re-derivation from scratch. Most rules were confirmed either already
covered-and-documented or correctly excluded; real mismatches already
found their way into the phase entries above (see e.g. Phase 36, 40,
45, 62, 67 for corrections this same audit produced). What follows are
the genuinely new gaps it found: real, verified-against-source
feature-parity gaps with no existing phase, Pending entry, or
Won't-port/Known-limitations line covering them at all - not defects,
just undocumented scope, consistent with this project's own "favor
false negatives over false positives" precedent (a missing check, not
a wrong one). None of these have been triaged into a numbered phase
yet - that is deliberately left for a future scoping pass, not decided
here. Full evidence (file:line in both `vendor/phpstan-src` and this
project's own `src/`) for every item below lives in the audit
workflow's own journal (`wf_495e5a37-b39/journal.jsonl`, under this
session's own Claude project directory) if a future scoping pass needs
more than the one-line summary here.

## Classes

- **`Rules/Classes/ImpossibleInstanceOfRule.php`**: Real PHPStan's ImpossibleInstanceOfRule reports instanceof.alwaysTrue/instanceof.alwaysFalse/instanceof.invalidExprType whenever an `$x instanceof Foo` expression's own inferred type resolves to a literal boolean (or the dynamic class-name expr is not string|object) - phpstan-rs's own unreachable-code family deliberately excludes Instanceof from its generic if/while/ternary...
- **`Rules/Classes/NewStaticRule.php`**: Real PHPStan's NewStaticRule reports the well-known 'Unsafe usage of new static().' (identifier new.static) whenever `new static()` is used in a non-final class without a consistent-constructor guarantee; NewStaticInAbstractClassStaticMethodRule reports a related, more specific case (new.staticInAbstractClassStaticMethod) inside an abstract class's own static method.
- **`Rules/Classes/InstantiationCallableRule.php`**: Real PHP's grammar (and nikic/php-parser's New_ extending CallLike with isFirstClassCallable()) genuinely accepts `new Foo(...)` syntactically; real PHPStan's InstantiationCallableRule exists specifically to flag it as illegal ('Cannot create callable from the new operator.', identifier callable.notSupported) since it's a real semantic error, not a parse error.
- **`Rules/Classes/InstantiationRule.php`**: phpstan-rs's `new` expression check (src/checker/expr.rs, ExprKind::New arm) only implements two of InstantiationRule's several sub-checks: new.interface (instantiating an interface) and new.abstract (instantiating an abstract class).
- **`Rules/Classes/ExistingClassesInClassImplementsRule.php`**: phpstan-rs's own check_class_references_exist (src/checker/oop.rs:263-331) is deliberately, per its own doc comment, existence-only: it confirms an extends/implements/trait-use target resolves to *some* known symbol, but never checks it's the *right kind* of symbol.
- **`Rules/Classes/InvalidPromotedPropertiesRule.php`**: Real PHPStan's InvalidPromotedPropertiesRule enforces several legality constraints on constructor property promotion: promotion is only legal inside `__construct` (property.invalidPromoted otherwise), not inside an abstract constructor, never on a variadic parameter, and is itself gated on PHP 8.0+ (property.promotedNotSupported) with `final` promotion additionally gated on...
- **`Rules/Classes/ReadOnlyClassRule.php`**: The PHP-version-gate data for this exact rule already exists in the codebase (PhpFeature::ReadOnlyClasses / ReadOnlyAnonymousClasses in src/php_version.rs, with a doc comment explicitly naming ReadOnlyClassRule as the real-PHPStan source), but it is never actually consumed by any checker call site - a `readonly class Foo {}` (needs PHP 8.2+) or `new readonly class {}` (needs...

## Functions

- **`Functions/CallCallablesRule.php (and the related dynamic-target half of FunctionCallableRule.php's callable.nonCallable check)`**: Real PHPStan's CallCallablesRule checks a dynamic call `$callable(...)` (callee is an arbitrary expression, not a plain function name): flags a non-callable value (callable.nonCallable), an inaccessible private/protected array-callable method (callable.inaccessibleMethod), and runs full arity/parameter-type checking against the resolved callable's signature.
- **`Functions/IncompatibleDefaultParameterTypeRule.php (+ IncompatibleClosureDefaultParameterTypeRule.php, IncompatibleArrowFunctionDefaultParameterTypeRule.php)`**: Real PHPStan (level 2) checks that a parameter's default value expression's type is assignable to that parameter's declared type hint (identifier parameter.defaultValue) for functions, closures, and arrow functions alike. phpstan-rs's AST carries a `default: Option<Expr>` on every parameter but no checker code anywhere validates it against the parameter's type hint.
- **`Functions/ReturnNullsafeByRefRule.php (+ ArrowFunctionReturnNullsafeByRefRule.php)`**: Real PHPStan flags 'Nullsafe cannot be returned by reference' (identifier nullsafe.byRef) when a by-ref function/arrow-function returns an expression whose access chain passes through a nullsafe operator (?->). phpstan-rs has a `contains_nullsafe_access` helper already used for the analogous by-ref-argument check, but nothing calls it against a by-ref function/arrow-fn's own...
- **`Functions/InvalidParameterNameRule.php + InvalidLexicalVariablesInClosureUseRule.php + RedefinedParametersRule.php + VariadicParametersDeclarationRule.php`**: A family of small level-0 parameter/closure-use-list sanity checks (parameter named $this or a superglobal; a closure `use($this)`/`use($_GET)`/a use-variable duplicating a param name; a duplicate parameter name; a non-last variadic parameter) has no phpstan-rs equivalent at all - none of the identifiers (parameter.superglobal, parameter.this, closure.useThis,...
- **`Functions/ParameterCastableToStringRule.php + ParameterCastableToNumberRule.php + ImplodeParameterCastableToStringRule.php + SortParameterCastableToStringRule.php`**: Real PHPStan checks that array-argument values passed to specific stdlib functions (implode/join; array_combine/natsort/natcasesort/array_count_values/array_fill_keys; array_intersect/array_diff family; array_unique/sort/rsort/asort/arsort; and a castable-to-number variant) are actually castable to string/number, since these functions implicitly cast at runtime and a...

## Methods

- **`Rules/Methods/IncompatibleDefaultParameterTypeRule.php`**: Real PHPStan (level 2) checks that a method parameter's default value type is assignable to its declared parameter type, reporting identifier 'parameter.defaultValue'. phpstan-rs has no equivalent check anywhere - grepping the whole checker for default-value/parameter-type-compatibility logic finds nothing beyond override-compatibility's unrelated 'ancestor made it optional'...
- **`Rules/Methods/MissingMagicSerializationMethodsRule.php`**: Real PHPStan flags a non-abstract class implementing Serializable that lacks __serialize()/__unserialize() (identifier 'class.serializable', PHP-version-gated via serializableRequiresMagicMethods()). phpstan-rs's only Serializable-related logic is the unrelated 'enum cannot implement Serializable' check in oop.rs; the magic-method-presence check itself doesn't exist, and...
- **`Rules/Methods/AbstractMethodInNonAbstractClassRule.php`**: Real PHPStan's second half of this rule flags a non-abstract, non-interface method with no body at all (identifier 'method.nonAbstract') - real PHP's parser accepts this syntax even though it's semantically invalid, so PHPStan itself must catch it.
- **`Rules/Methods/OverridingMethodRule.php`**: Real PHPStan's OverridingMethodRule also reports (all unconditional PHP fatal errors, all 'nonIgnorable'): a non-static method overriding a static one and vice versa (method.nonStatic/method.static), and making a non-abstract *ordinary* method abstract on override (method.abstractOverridingNonAbstract, the general case - not just the constructor's own special no-prototype...
- **`Rules/Methods/OverridingMethodRule.php`**: Real PHPStan distinguishes a real `final` keyword ancestor (method.parentMethodFinal) from an ancestor only tagged `@final`/`@phpstan-final` in its docblock (method.parentMethodFinalByPhpDoc) - both unconditional-vs-convention-level overriding-final-method errors.
- **`Rules/Methods/MethodCallableRule.php`**: Real PHPStan gates first-class-callable syntax (`$obj->method(...)`) behind a PHP-8.1 version check, reporting 'callable.notSupported' when the configured target is older. phpstan-rs built an extensive PhpFeature/php_version.rs framework covering roughly a dozen similarly narrow PHP-version-gated syntax checks (noncapturing catch, throw-expression, promoted properties,...

## Properties

- **`Rules/Properties/DefaultValueTypesAssignedToPropertiesRule.php`**: Real PHPStan (level 3) checks a typed property's own literal default value against its declared type (e.g. `public int $x = "bad";` → property.defaultValue); phpstan-rs has no equivalent pass over property declaration defaults at all, and ROADMAP.md never mentions it.
- **`Rules/Properties/ExistingClassesInPropertiesRule.php`**: This rule's class.notFound half (referenced-class existence in a property's native+PHPDoc type) is correctly built and documented (Phase 48, generics.rs:184-190), but its other two identifiers — property.trait (a property typed as a trait, e.g.
- **`Rules/Properties/PropertiesInInterfaceRule.php, Rules/Properties/PropertyInClassRule.php`**: Real PHPStan has two whole rule classes validating PHP 8.4 property-hook *declaration-site* legality — PropertiesInInterfaceRule (an interface property must be hooked, public, non-readonly, non-static, non-abstract, non-final, hookless-of-body) and PropertyInClassRule (abstract/final/private/static/readonly/hooksNotSupported/virtualDefault combinations on a class's own hooked...
- **`Rules/Properties/ReadOnlyPropertyRule.php, Rules/Properties/ReadOnlyByPhpDocPropertyRule.php`**: Real PHPStan validates that a `readonly` property declaration is itself legal PHP (has a native type, no default value, isn't static, PHP version supports readonly) — and the identical four checks for the `@phpstan-readonly`/`@readonly` docblock-tag form.
- **`Rules/Properties/InvalidCallablePropertyTypeRule.php`**: Real PHP forbids `callable` as a property's native type (fatal error); real PHPStan flags it as property.callableType. phpstan-rs has no such check and ROADMAP.md never mentions it — the only 'missingType.callable' hit in the repo is an unrelated, already-disclosed different check (MissingPropertyTypehintRule's own callable-signature-richness gap).
- **`Rules/Properties/PropertyAssignRefRule.php, Rules/Properties/ReadOnlyPropertyAssignRefRule.php, Rules/Properties/ReadOnlyByPhpDocPropertyAssignRefRule.php, Rules/Properties/ReadingWriteOnlyPropertiesRule.php, Rules/Properties/WritingToReadOnlyPropertiesRule.php`**: A cluster of five real PHPStan rules around PHP 8.4 write-only/read-only (property-hook-derived) and asymmetric-visibility properties: assigning such a property by reference (plain, readonly-native, or readonly-by-phpdoc), reading a write-only property, and writing a not-writable property.

## Comparison

- **`Rules/Comparison/StrictComparisonOfDifferentTypesRule.php`**: Real PHPStan flags a `===`/`!==` comparison whose outcome is provably constant (operands' types disjoint => always false, or provably equal => always true) with identical.alwaysFalse/alwaysTrue and notIdentical.alwaysFalse/alwaysTrue.
- **`Rules/Comparison/ImpossibleInArrayHaystackFiniteTypesRule.php`**: Real PHPStan flags a call to in_array()/array_search()/array_keys() where a needle's finite type can never match any value in a constant-array haystack (function.impossibleHaystackValue, registered at level 4 via conf/config.level4.neon).
- **`Rules/Comparison/UsageOfVoidMatchExpressionRule.php`**: Real PHPStan flags using the result of a match expression whose every arm's result is a void-returning call (match.void). phpstan-rs's Match handling (both the statement-checking arm in checker/expr.rs and the type-inference arm) has no void-result-usage check at all, and this identifier/feature appears nowhere in src/ or ROADMAP.md.

## PhpDoc

- **`Rules/PhpDoc/IncompatiblePhpDocTypeRule.php, IncompatiblePropertyPhpDocTypeRule.php, IncompatiblePropertyHookPhpDocTypeRule.php, IncompatibleClassConstantPhpDocTypeRule.php`**: Real PHPStan reports a real diagnostic (parameter.phpDocType/return.phpDocType/property.phpDocType/classConstant.phpDocType, plus *.unresolvableType siblings) when a @param/@return/@var/property/class-constant PHPDoc type is incompatible with (not a sub/supertype of) its own native type hint; phpstan-rs has no such diagnostic anywhere - it silently reconciles/coerces the doc...
- **`Rules/PhpDoc/IncompatibleSelfOutTypeRule.php`**: @phpstan-self-out (asserting a method returns $this narrowed to a different subtype) is entirely unimplemented in phpstan-rs - the tag is only listed in the typo-detection whitelist so a real use of it isn't wrongly flagged as unknown, but it's never actually parsed into a type or consumed/checked (selfOut.type/.static/.unresolvableType, plus generic-type validation, have no...
- **`Rules/PhpDoc/FunctionAssertRule.php, MethodAssertRule.php (AssertRuleHelper)`**: Phase 48.7 closed the plain class-existence slice of this (`class.notFound` on a `@phpstan-assert`/`-if-true`/`-if-false` tag's own asserted type - `checker/narrowing.rs`'s `check_assert_tag_types`). Still open: the deeper assertion-*validity* half real PHPStan validates - assert.impossibleType (an assertion that can never happen); assert.alreadyNarrowedType; assert.unresolvableType; assert.trait; assert.unknownExpr; generic-type/missing-typehint checks on the asserted type.
- **`Rules/PhpDoc/FunctionConditionalReturnTypeRule.php, MethodConditionalReturnTypeRule.php (ConditionalReturnTypeRuleHelper)`**: Real PHPStan validates a conditional PHPDoc return type's own declaration: conditionalType.subjectNotFound (the condition's subject isn't one of the function's own @template types) and conditionalType.alwaysTrue/alwaysFalse (the condition is statically always one way, given the subject/target types).
- **`Rules/PhpDoc/InvalidPhpDocVarTagTypeRule.php, WrongVariableNameInVarTagRule.php, VarTagChangedExpressionTypeRule.php`**: phpstan-rs's inline-@var checking (Phase 36 Done, varTag.type) is deliberately and correctly scoped to only the single case of a plain assignment statement whose RHS type doesn't match the tag. Real PHPStan's much larger @var-tag-validity surface beyond that - WrongVariableNameInVarTagRule's own name-matching diagnostics for foreach/static/global/return/misplaced-declaration...

## Generics

- **`Rules/Generics/TemplateTypeCheck.php (shared by ClassTemplateTypeRule.php, InterfaceTemplateTypeRule.php, FunctionTemplateTypeRule.php, MethodTemplateTypeRule.php, TraitTemplateTypeRule.php)`**: Real PHPStan validates every @template tag's own declaration (not just usage sites): name collision with an existing class (generics.existingClass) or type alias (generics.existingTypeAlias), the bound type actually resolving to a real class and not a trait (class.notFound/generics.traitBound), the bound being one of the ~15 supported kinds (generics.notSupportedBound), a...
- **`Rules/Generics/MethodTagTemplateTypeCheck.php (used by MethodTagTemplateTypeRule.php, MethodTagTemplateTypeTraitRule.php)`**: Same TemplateTypeCheck declaration-validity family as above, applied to a class's own @method-tag template parameters, plus its own extra check: a @method tag's template name shadowing the enclosing class's own @template name (methodTag.shadowTemplate).
- **`Rules/Generics/MethodTemplateTypeRule.php (its own shadow-check, distinct from TemplateTypeCheck)`**: Beyond delegating to TemplateTypeCheck, MethodTemplateTypeRule itself flags a method's own @template tag reusing a name already declared as a @template on the enclosing class (method.shadowTemplate). phpstan-rs's own generic-template model does track per-scope template names (TemplateParam::scope) but never cross-checks a method-scoped template name against its class's own...
- **`Rules/Generics/VarianceCheck.php's own-declaration half (used by FunctionSignatureVarianceRule.php, MethodSignatureVarianceRule.php)`**: VarianceCheck::checkParametersAcceptor does two things: (1) walks parameter/return types for template-position violations (this half IS built in phpstan-rs as check_variance/walk_variance, generics.variance, and is correctly documented in ROADMAP), and (2) flags a function's or method's own @template tag declaring non-invariant variance at all...
- **`Rules/Generics/UsedTraitsRule.php`**: UsedTraitsRule validates a trait-use statement's own @use tag exactly the way ClassAncestorsRule validates @extends/@implements: incompatible/unresolvable type, missing @use despite using a generic trait, wrong trait named, bound/arity/variance checking on the trait's own template arguments, and call-site-variance-not-allowed.
- **`Rules/Generics/CrossCheckInterfacesHelper.php (wired into ClassAncestorsRule.php, InterfaceAncestorsRule.php, EnumAncestorsRule.php)`**: Real PHPStan cross-checks every path (own @implements/@extends plus every ancestor's own) by which a class/interface/enum reaches the same generic interface, flagging it (generics.interfaceConflict) if two different paths pin that interface's same template parameter to two different concrete types (e.g.

## Exceptions

- **`Rules/Exceptions/MissingCheckedExceptionInFunctionThrowsRule.php (+ MissingCheckedExceptionInMethodThrowsRule.php, MissingCheckedExceptionInPropertyHookThrowsRule.php)`**: Phase 42's own bonus item now covers the function/method half, opt-in (`throws.missing`, `Registry::check_missing_checked_exceptions`) - but deliberately simpler than real PHPStan's own version: it flags *any* explicit `throw` not covered by an `@throws` tag, not specifically one classified as a "checked exception" (real PHPStan's own `DefaultExceptionTypeResolver`-driven `exceptions.uncheckedException{Classes,Regexes}` classification isn't modeled). `MissingCheckedExceptionInPropertyHookThrowsRule`'s own property-hook coverage remains unbuilt (property hooks get no `@throws`-related checking at all - see the sibling `TooWidePropertyHookThrowTypeRule` bullet below for the same gap on the other rule family).
- **`Rules/Exceptions/TooWidePropertyHookThrowTypeRule.php + Rules/Exceptions/ThrowsVoidPropertyHookWithExplicitThrowPointRule.php`**: Property hooks get no @throws-related checking at all in phpstan-rs: check_get_hook/check_set_hook (src/checker/oop.rs) run undefined-variable, generator-return, and return-type/return.missing checks for hook bodies but never call check_throws_tag_validity or the body_contains_explicit_throw-based throws.void check that check_function runs for ordinary functions/methods...

## Arrays

- **`Rules/Arrays/IterableInForeachRule.php`**: Real PHPStan flags `foreach` over a subject whose inferred type isn't iterable at all (identifier `foreach.nonIterable`); phpstan-rs's Foreach handling in the checker never validates the subject's type before iterating, it just falls back to `Mixed` element type via `array_element_type(...).unwrap_or(Type::Mixed)`.
- **`Rules/Arrays/DeadForeachRule.php`**: Real PHPStan flags a `foreach` whose subject is provably an always-empty array (`foreach.emptyArray`, level 4) as dead code; phpstan-rs has no `isIterableAtLeastOnce()`-style check anywhere and never reports this.
- **`Rules/Arrays/ArrayDestructuringRule.php`**: Real PHPStan's list()/[] destructuring assignment checks that the RHS is actually array-like (`offsetAccess.nonArray`) and that each destructured key actually exists on it (via NonexistentOffsetInArrayDimFetchCheck). phpstan-rs's `bind_assignment_target`/`check_assignment_target` for an `ArrayLiteral` LHS only ever infers/binds - a non-array RHS silently falls to the `_ =>...
- **`Rules/Arrays/ArrayUnpackingRule.php`**: Real PHPStan flags `[...$arr]` when `$arr`'s key type could be string and the target PHP version predates 8.1's string-key unpacking support (`arrayUnpacking.stringOffset`). phpstan-rs's ArrayLiteral checking in `check_expr` never inspects `item.spread` at all - a spread item's value is only type-inferred, never validated against its key type or the configured PHP version.
- **`Rules/Arrays/UnpackIterableInArrayRule.php`**: Real PHPStan flags `[...$x]` when `$x` isn't iterable (`arrayUnpacking.nonIterable`). phpstan-rs's array-literal checking never validates a spread item's value type against iterability - see ArrayUnpackingRule finding above, same root cause (spread items aren't specially checked at all).
- **`Rules/Arrays/OffsetAccessWithoutDimForReadingRule.php`**: Real PHPStan flags `$x[]` used in a read position (a PHP compile-time fatal error PHPStan pre-empts statically) via the non-ignorable `offsetAccess.noDim`. phpstan-rs's parser parses `expr[]` identically in any postfix position (no read/write distinction), and `check_expr`'s Index arm silently returns `Type::Mixed` for a `None` dim with no diagnostic.
- **`Rules/Arrays/NonexistentOffsetInArrayDimFetchRule.php`**: Real PHPStan's NonexistentOffsetInArrayDimFetchRule/Check has two halves: (1) flag reading a nonexistent key (`offsetAccess.notFound`) across constant arrays, general arrays (opt-in) and shapes, and (2) flag indexing into a type that isn't offset-accessible at all (`offsetAccess.nonOffsetAccessible`, e.g. an int or a non-ArrayAccess object).
- **`Rules/Arrays/OffsetAccessAssignmentRule.php`**: Real PHPStan flags `$arr[$key] = $v;`/`$arr[$key] += $v;` when `$key`'s type is illegal for `$arr`'s own declared/known offset type (`offsetAssign.dimType`, shared by the sibling OffsetAccessAssignOpRule for compound-assignment operators).
- **`Rules/Arrays/OffsetAccessValueAssignmentRule.php`**: Real PHPStan flags assigning a value of the wrong type into a typed array/object offset (`offsetAssign.valueType`, e.g. `array<int,string> $a; $a[0] = 5;`). phpstan-rs's `write_index_into` always unions the new value's type into the container's element type rather than checking it against any pre-existing declared value type, so an incompatible value is never reported - it's...

## Constants

- **`Rules/Constants/ConstantRule.php`**: Real PHPStan's ConstantRule (level 1) flags any bare global-constant reference (Node\Expr\ConstFetch) not found in scope with 'constant.notFound'. phpstan-rs's equivalent path, Checker::global_constant_type in src/checker/expr.rs, looks up Registry::global_constants and simply falls back to Type::Mixed via bare_name_literal_type for anything not in the table (per its own doc...
- **`Rules/Constants/DynamicClassConstantFetchRule.php`**: Real PHPStan's rule validates PHP 8.3's dynamic class-constant-fetch syntax (Foo::{$expr}) - PHP-version gating (classConstant.dynamicFetch) and that the dynamic name expression is string-typed (classConstant.nameType). phpstan-rs's StaticAccess AST node (src/ast.rs:606, StaticAccess(String, String)) stores the member name as a plain string, with no way to represent a dynamic...
- **`Rules/Constants/MagicConstantContextRule.php`**: Real PHPStan flags __CLASS__/__TRAIT__/__FUNCTION__ or __METHOD__/__NAMESPACE__ used outside their respective context (outside a class/trait/function/namespace) as always-empty, with dedicated identifiers per magic constant. phpstan-rs's registry.rs explicitly documents (lines ~2815-2821) that 'this project has no dedicated magic-constant Expr kind of its own - a bare...
- **`Rules/Constants/OverridingConstantRule.php`**: Real PHPStan checks class-constant override compatibility against a parent class/interface prototype: overriding a final constant (classConstant.final), narrowing visibility (classConstant.visibility), narrowing/dropping a native type (classConstant.nativeType/.missingNativeType), and covariance of the PHPDoc value type (classConstant.type).
- **`Rules/Constants/ValueAssignedToDefineRule.php + Rules/Constants/ValueAssignedToGlobalConstantRule.php`**: Both rules validate a define()'d or const-declared global constant's assigned value against a type configured via PHPStan's 'dynamicConstantNames' config parameter (ConstantResolver::getConfiguredGlobalConstantType), reporting constant.defineValue / constant.value respectively.
- **`Rules/Constants/ValueAssignedToClassConstantRule.php`**: phpstan-rs's check_typed_class_constant_values (oop.rs) covers only the native-typed-constant half of this rule (classConstant.value for PHP 8.3 `const TYPE NAME = value;`). The rule's PHPDoc-@var-type half - an untyped `const NAME = value;` whose own inline @var doc type doesn't accept the literal value, reported as classConstant.phpDocType - is explicitly skipped, per...

## Variables

- **`Rules/Variables/AssignToByRefExprFromForeachRule.php`**: Real PHPStan flags `assign.byRefForeachExpr` when code assigns to (or property-fetches through) the same expression that was just bound as a `foreach ($x as &$expr)` reference variable, since that overwrites the last element of the iterated array — a well-known PHP gotcha.
- **`Rules/Variables/CompactVariablesRule.php`**: Real PHPStan's CompactVariablesRule reports `variable.undefined` when a literal string (or nested array-of-strings) argument to compact() names a variable not defined (or maybe-undefined) in scope. phpstan-rs's own `variable.undefined` machinery only fires for direct `$var` reads; the only compact()-aware code in src/checker/unused.rs marks compact()'s named variables as...
- **`Rules/Variables/UnsetRule.php`**: Real PHPStan's UnsetRule reports five distinct diagnostics on `unset(...)` targets: `unset.variable` (undefined variable), `unset.offset` (illegal/nonexistent array offset), `unset.readOnlyProperty`/`unset.readOnlyPropertyByPhpDoc`, `unset.hookedProperty`, and `unset.possiblyHookedProperty` (PHP 8.4).
- **`Rules/Variables/VariableCloningRule.php`**: Real PHPStan's VariableCloningRule reports `clone.nonObject` when `clone $expr` targets a non-cloneable (non-object) type. phpstan-rs's checker infers a type for `ExprKind::Clone(...)` but performs no cloneability validation at all — no `clone.nonObject`-equivalent identifier exists anywhere in the codebase, and ROADMAP.md never mentions cloning validation (its 'clone' hits...

## Operators+Cast

- **`Rules/Operators/BacktickRule.php`**: Real PHPStan warns (`backtick.deprecated`) that the backtick shell-exec operator is deprecated as of PHP 8.5, suggesting `shell_exec()` instead. phpstan-rs has no equivalent diagnostic anywhere, and ROADMAP.md never mentions BacktickRule, backtick deprecation, or the identifier at all.
- **`Rules/Operators/InvalidAssignVarRule.php`**: Real PHPStan flags three shapes on the left side of an assignment: nullsafe operator on the assignment target (`nullsafe.assign`), nullsafe operator on the right side of assign-by-ref (`nullsafe.byRef`), and any non-assignable LHS expression, e.g. a call result or literal (`assign.invalidExpr`). None of these are checked anywhere in phpstan-rs.
- **`Rules/Operators/PipeOperatorRule.php`**: Real PHPStan's PipeOperatorRule flags a PHP 8.5 pipe (`|>`) whose right-hand callable's first parameter is passed by reference (`pipe.byRef`), since the pipe operator always passes its value by-value. phpstan-rs parses `|>` but performs no legality checking on the callable at all.
- **`Rules/Cast/DeprecatedCastRule.php`**: Real PHPStan warns (`cast.deprecated`) on the non-standard cast spellings `(integer)`, `(boolean)`, `(double)`, `(binary)` as of PHP 8.5, recommending the canonical `(int)`/`(bool)`/`(float)`/`(string)` instead. phpstan-rs has no such diagnostic.
- **`Rules/Cast/UnsetCastRule.php`**: Real PHPStan hard-errors (`cast.unset`, non-ignorable) on the `(unset)` cast when targeting PHP >= 8.0, since PHP itself removed it. phpstan-rs infers a type for `(unset)$x` (Type::Null) but never checks the configured php_version to flag its use as removed.
- **`Rules/Cast/VoidCastRule.php`**: Real PHPStan flags the PHP 8.5 `(void)` cast two ways: `cast.voidNotSupported` when the configured PHP version predates 8.5, and `cast.void` (non-ignorable) whenever it's used as anything other than a first-level statement. phpstan-rs parses `(void)` as a legal, inert cast but never emits either diagnostic.

## Traits+InternalTag

- **`Rules/Traits/ConflictingTraitConstantsRule.php`**: Real PHPStan checks that a class constant overriding one supplied by a used trait keeps the same visibility, final-ness, native type, and (for immediately-used traits) the same value - reporting classConstant.visibility/.nonFinal/.final/.nativeType/.missingNativeType/.value.
- **`Rules/Traits/ConstantsInTraitsRule.php`**: Real PHPStan reports classConstant.inTrait when a constant is declared inside a trait body but the project's configured PHP version predates 8.2 (constants-in-traits support). phpstan-rs defines a matching PhpFeature::ConstantsInTraits variant with the correct (8,2) threshold in src/php_version.rs, but it is never consumed by any check call site - grepping the codebase for it...

## Keywords+Debug

- **`Rules/Keywords/ContinueBreakInLoopRule.php`**: Real rule walks a break/continue's enclosing-statement chain (via ParentStmtTypesVisitor) and reports continue.outOfLoop/break.outOfLoop (nonIgnorable) when the statement's numeric level doesn't resolve inside enough loop/switch nesting, or when a closure boundary is crossed first.
- **`Rules/Keywords/DeclareStrictTypesRule.php`**: Real rule validates that `declare(strict_types=...)`'s value is literally 0 or 1 (declareStrictTypes.value, nonIgnorable) and that the declare statement is the file's very first statement (declareStrictTypes.notFirst, nonIgnorable).
- **`Rules/Keywords/GotoUndefinedLabelRule.php`**: Real rule reports goto.labelUndefined (nonIgnorable) when a `goto` targets a label that doesn't exist in the function. phpstan-rs tracks goto targets (collect_goto_targets in src/checker/control_flow.rs) purely to mark a labeled statement as reachable for dead-code purposes - it never validates that every goto's target label actually exists anywhere in the function.
- **`Rules/Keywords/UnusedLabelRule.php`**: Real rule reports label.unused when a `label:` statement is never targeted by any goto in the function. phpstan-rs has no equivalent - collect_goto_targets only builds the set of names goto statements reference (used for reachability), it never inverts that to flag a Label statement with zero incoming gotos.
- **`Rules/Keywords/RequireFileExistsRule.php`**: Real rule resolves require/include/require_once/include_once path expressions (including the __DIR__ . 'x' magic-constant idiom and file_exists()-style guards) against the current working directory, include_path, and the analyzed file's own directory, reporting require.fileNotFound/requireOnce.fileNotFound/include.fileNotFound/includeOnce.fileNotFound when the resolved path is...
- **`Rules/Debug/{DumpTypeRule,DumpNativeTypeRule,DumpPhpDocTypeRule,DebugScopeRule}.php`**: These four rules are PHPStan's own ad-hoc developer-debugging hooks: `\PHPStan\dumpType()`/`dumpNativeType()`/`dumpPhpDocType()` print the inferred type/native-type/phpdoc-type of an expression at a point of interest (phpstan.dumpType/.dumpNativeType/.dumpPhpDocType), and `\PHPStan\debugScope()` dumps the whole scope's tracked variable types (phpstan.debugScope).

## Generators+Pure

- **`Rules/Generators/YieldFromTypeRule.php`**: Real YieldFromTypeRule reports `generator.nonIterable` when the expression after `yield from` is definitely (or, with reportMaybes, possibly) not iterable — a standalone check that only needs `exprType.isIterable()`, independent of the Generator<K,V,TSend,TReturn> generic-shape work.
- **`Rules/Pure/FunctionPurityCheck.php`**: FunctionPurityCheck reports several declaration-level diagnostics beyond the one pair phpstan-rs implements (`impureFunction.pure`/`impureMethod.pure`, correctly documented in Phase 41): `pureFunction.void`/`pureMethod.void` (a function proven/tagged pure but declared void, excluding constructors), `pureFunction.parameterByRef`/`pureMethod.parameterByRef` (pure but has a...

## EnumCases+Namespaces+Regexp+Ignore+Missing+Names+Types+Whitespace

- **`Rules/Whitespace/FileWhitespaceRule.php`**: Real PHPStan flags a file that begins with a UTF-8 BOM (identifier `whitespace.bom`) and a file whose last statement is a whitespace-only closing-tag remnant (identifier `whitespace.fileEnd`). phpstan-rs has no equivalent anywhere (no hits for BOM/whitespace.bom/whitespace.fileEnd in src/ or ROADMAP.md) and ROADMAP.md never mentions this rule class or feature area at all, not...
- **`Rules/Types/InvalidTypesInUnionRule.php`**: Real PHPStan rejects `mixed`/`never`/`void` when they appear inside a union or nullable type declaration on a param, return type, or property (identifiers `unionType.mixed`/`.never`/`.void` and `nullableType.mixed`/`.never`/`.void`). phpstan-rs has no such check, and ROADMAP.md never mentions it.
- **`Rules/Ignore/IgnoreParseErrorRule.php`**: Real PHPStan reports a non-ignorable `ignore.parseError` when a `@phpstan-ignore` comment's own identifier-list syntax is malformed. phpstan-rs's own `ignore_comments.rs` deliberately swallows this case with no diagnostic at all (confirmed by its own test name), and ROADMAP.md never discusses this rule or the silent-drop behavior anywhere.

---

# Reverted

Implemented, corpus-verified, then undone - not merely investigated and
declined without building (those live inline in **Pending**, next to
what they're a sub-item of). Kept here rather than deleted so the real
root cause is on hand if this is ever revisited, and so a future
"corpus doesn't confirm broad value" finding is checked against
[[feedback_dont_discard_correct_checks_ship_opt_in]]'s own distinction
before defaulting to another full revert.

- **Constant-boolean-condition family** (Phase 15; superseded - fully
  reinstated with real inferred-type trust, see Done). A real,
  demonstrated soundness bug, not a corpus-representativeness call,
  caused the original revert: this checker had no fixed-point loop
  analysis, so a variable reassigned inside a loop body kept its
  pre-loop type for the whole check, which both diagnostics
  misinterpreted as "always terminates"/"always true or false" on
  extremely common idioms. ~900 unverifiable false positives. Fixed by
  building fixed-point loop convergence and a small, unrelated
  `?? false`-collapse bug, then rebuilding the family in full rather
  than staying narrowed to bare syntactic literals.
- **`InvalidKeyInArrayDimFetchRule`** (Phase 50; superseded - reattempted
  and shipped successfully, see Done). The original revert's real
  soundness bug: a *used* generic template parameter's own type leaked
  through as a bogus namespace-qualified class reference instead of
  staying `Type::TemplateParam`, indistinguishable from a genuine
  invalid array key. The reattempt found the real root cause was
  narrower than originally framed (several type-tree-walking functions
  missing an `object{...}`-shape recursion arm, plus inline-`@var`
  resolution never marking template parameters).
- **Genuinely-empty-concrete-method-body check** (Phase 61; superseded -
  re-shipped opt-in, see Done). Verified individually correct and NOT a
  soundness bug - the corpus regression (+10,623, nearly all in
  `jetbrains/phpstorm-stubs`) was entirely one real, ubiquitous PHP
  convention (an IDE-stub/native-extension class declaring a native
  function's signature with a deliberately empty body) that real
  PHPStan itself never encounters, because it loads stub files through
  a separate, non-rule-checked mechanism this project's own broader
  verification corpus doesn't mirror. Re-shipped behind
  `Registry::check_empty_method_bodies` (default off), per
  [[feedback_dont_discard_correct_checks_ship_opt_in]].
- **`Foreach`'s own "may run zero times" pre-loop-state merge**
  (originally Phase 29, re-attempted and re-reverted since). A real,
  demonstrated soundness bug both times: giving `Foreach` the same
  `While`/`For`-style zero-iteration merge breaks the common "a loop
  mutates the very array it iterates via its own loop variable" idiom.
  Needs real fixed-point loop analysis (Phase 55) to do soundly, not a
  targeted patch - and a later re-attempt with that in place still
  regressed a real corpus file via a narrowing-wrapper interaction.
- **`@phpstan-type`/local type-alias support, first attempt** (Phase
  48). Implemented and reverted after a net-negative corpus regression,
  root-caused to a separate, real, pre-existing gap (`define()`-declared
  global constants resolving to bare `Mixed`). Historical only:
  successfully reattempted once that gap was closed, and is now shipped
  (see **Done**, Phase 36).
- **Anonymous-class ancestor-aliasing shortcut** (an ad hoc round
  during the Phase 14.5 corpus-cleanup series). Aliasing an anonymous
  class's inferred type to whichever single `extends`/`implements`
  target it names was implemented and corpus-verified, then reverted:
  unsound for the common "spy" test-double pattern (a class implementing
  a real interface but also declaring its own extra instrumentation
  members a test reads directly). A real fix needs registering the
  anonymous class's own full declaration under some stable synthetic
  name, not a type-inference tweak.

---

# Known limitations

Deliberate "this checker doesn't model X" boundaries not tied to any
one still-open phase above (a phase's own gap is described in its own
Pending entry instead).

## Known but not yet scheduled

- `use function ...`'s own existence check (`check_use_function_imports`)
  covers only `UseKind::Function` - a `use const Foo\BAR;` import's own
  existence (`constant.notFound`) isn't checked at all (no project-wide
  global-constant-existence registry exists reliable enough to avoid
  false positives, only project `define()`/`const` declarations), and
  neither `use` kind's own function/constant-name-case mismatch
  (`function.nameCase`) is checked.
- `callable-string`/`trait-string`/`interface-string`/`enum-string`
  stay aliased to plain `string` - no literal- or narrowing-based way
  to verify their content.
- `array_merge`'s key-aware shape-merging only applies when arguments
  are uniformly known-shaped or uniformly list-like, not a mix of a
  known shape and an unproven plain array. Every stdlib array function
  beyond the couple dozen this project special-cases remains untouched.
  Two narrow, low-value narrowing gaps: reversed `0 < count($x)` (only
  `count($x) > 0`/`>= 1` narrow), and `!empty($x)` not upgrading an
  already-`non-empty-string` value to `non-falsy-string` on a redundant
  nested guard.
- An overriding method's own parameter doesn't inherit a stricter
  ancestor/interface docblock refinement it doesn't redeclare itself
  when there's no *own* docblock inheritance case to trigger the
  already-shipped PHPDoc-inheritance backfill. Needs real interface-to-
  implementation PHPDoc inheritance for method signatures generally,
  params and returns, generic or not - a sizable feature on its own.
- No mechanism for narrowing correlated across two *separate* variables/
  statements (real PHPStan's `ConditionalExpressionHolder`) - genuinely
  hard to thread through `Locals`'s own O(1)-clone, structurally-shared
  design without a parallel mutable side-channel duplicating its
  branch-merge/invalidation semantics from scratch.
- `strtotime()`'s real accuracy and a raw SQL/DQL query's own result
  shape are both structurally infeasible without embedding a PHP
  runtime or a SQL parser respectively - confirmed real PHPStan needs
  exactly that.
- phpstorm-stubs' interleaved-multiple-parameter-list shape (two full
  parameter lists under the same names across PHP-version eras) isn't
  recognized - a residual tail of "expects at least N arguments"
  diagnostics, not fixed.
- A trait declared twice with genuinely different method signatures per
  conditional branch checks both bodies against whichever declaration
  won the registry slot, not the specific branch being checked - fixing
  it means a change to a heavily-used core path not attempted for this
  narrow an incidence.
- A long tail of smaller precision gaps (no per-location type-inference
  caching architecture; `IntersectionType` doesn't delegate a general
  `object&object`/`callable&array` combination; `static`/`$this` late
  binding has no attached class; no `BenevolentUnionType`; no
  `HasMethodType`/`HasPropertyType`-style structural capability
  assertion outside `ArrayShape`; `class-string<T>`'s own `instanceof`/
  sealed-hierarchy narrowing is unmodeled; no general `TypeCombinator`
  intersect/remove or generic recursive type-tree traverse/map
  combinator) - none block a specific missing rule, revisit only if a
  future corpus round traces a real false positive/negative to one.
- A dynamic-key array write's own key type isn't narrowed by a
  preceding `is_int()`/`is_string()`-family guard on the key expression
  itself, unlike an ordinary variable - `$arr[$row['id']] = ...;`
  inside `if (is_int($row['id'])) { ... }` still infers the array's own
  key type from `$row['id']`'s *unnarrowed* type, not `int`. Found via
  Phase 68.3's own corpus verification (Piwigo's own
  `CategoryAvailableListController.php:311`, real, if disclosed as
  correct-per-current-modeling rather than a regression from that fix)
  once `union_types`'s new `Array` merge (see "Type system") stopped
  silently hiding it.
- `array_filter()`'s own return-type inference never narrows away a
  provably-null/falsy *field* of a fixed-position `ArrayShape` input the
  way it does for a homogeneous `Array(K, V)`'s own value type - real
  PHP's own no-callback filtering can remove a specific shape field
  entirely (`array_filter([$a, $b])` with `$b` possibly `null` really
  can return a 1-element array missing key `1`, not a 2-element one
  with a narrower field 1 type), a data-flow-sensitive computation this
  project doesn't attempt. Found via Phase 68.2's own corpus
  verification (Piwigo's own `tests/Integration/GroupServiceTest.php:363`,
  `array_filter([$sourceId, $copyId])` passed to a `list<GroupId>`-typed
  parameter) - a real false positive from this separate, pre-existing
  gap, not from Phase 68.2's own try/finally fix (which correctly made
  `$copyId`'s own type visible as `null|GroupId` for the first time,
  simply surfacing this adjacent gap rather than causing it).

## Permanent design boundaries (not planned, by choice)

- Conditional PHPDoc return types needing real per-call-site type-
  parameter binding (distinct from the already-parsed union-widening
  approximation, see Done) - deliberately not attempted.
- A dynamic class target (`$obj::method()`) is never checked - no
  static name exists to resolve, matching `new $dynamicClass()`'s
  identical precedent.
- A generic bound checked against a class this project has no real data
  for (a PHAR-bundled dependency with no loose source vendored, or an
  unparsed local type alias) silently skips the check.
- Variance checking doesn't flip variance through a nested generic
  argument.
- Integer overflow/precision, floating-point rounding, and other
  runtime-value-dependent behavior are out of scope by construction -
  this checker only ever reasons about static types, never concrete
  values.
- `array_flip`/`compact`/`get_defined_vars` needing array-key-type
  tracking or a whole-scope variable lookup are deliberately not
  attempted, not a "not yet."
- A `Closure(...)`/`callable(...)` PHPDoc shape's own per-parameter/
  return signature degrades to plain `Type::Callable` rather than being
  modeled precisely.
- A `foreach` header's own inline `@var` annotation isn't supported
  (only a tag on a statement inside the body is) - a header tag would
  need to affect the loop body's own checking, and shipping that
  half-working was judged worse than not supporting it.

## Won't-port (real PHPStan features with no applicable Rust target)

An exhaustive 855-file audit against `vendor/phpstan-src/src/Rules/` and
`Type/` classified ~208 files this way. Recurring patterns, not
individually itemized:

- PHPStan's own BC-promise/reflection-migration machinery (`Api/*`,
  `GetTemplateTypeRule`) - gates on code referencing PHPStan's own
  `PHPStan\*` namespace; phpstan-rs is a Rust binary with no such
  namespace for analyzed code to misuse (Phase 40 above is the correct
  mirror-image feature). `RuntimeReflectionFunctionRule`/
  `RuntimeReflectionInstantiationRule`/`OldPhpParser4ClassRule` were the
  real, buildable exception this label previously mis-bucketed them
  under (they gate the *other* way - on the analyzed class itself
  implementing a `PHPStan\*` interface, not on a referenced class) -
  Phase 40 now builds all three, plus `NodeConnectingVisitorAttributesRule`
  itself (`phpParser.nodeConnectingAttribute` - never actually declined,
  just left un-triaged here) - `$node->getAttribute('parent'|'previous'|
  'next')` on a `PhpParser\Node` (sub)type receiver, gated the same
  `current_class_implements_phpstan_interface` way as its three siblings.
- `Playground/*` - registered only in phpstan.org's own issue-bot
  service, never the CLI analyzer.
- DI/plugin-architecture wrapper classes with zero logic of their own
  (auto-generated `RuleError<N>` builder permutations, trait-based
  interface-boilerplate-avoidance shims, `*TypeSpecifyingExtension`+
  `Registry` pairs) - phpstan-rs has no DI container or plugin model and
  no per-type-kind class hierarchy needing default no-op methods; every
  check is a hardcoded function/match arm. One real, narrow gap hides
  behind this label: `NonGeneralizableTypeTrait`'s own notes concede
  PHPStan's `generalize()` (loop-fixpoint type widening) has no
  phpstan-rs counterpart - a candidate for folding into Phase 55 if a
  corpus surfaces it.
- PHP-runtime/XML-shell-out techniques (`SimpleXMLElementConstructorThrowTypeExtension`) -
  PHPStan validates by literally constructing the real object at
  analysis time, since it runs inside PHP itself; phpstan-rs embeds no
  PHP runtime. `DateIntervalConstructorThrowTypeExtension`'s own throw-
  type contribution is no longer in this bucket - Phase 42 built it by
  reusing the hand-rolled spec-grammar validator Phase 78 already
  shipped (confirmed against real `php -l`/`php -r`, not runtime
  construction) instead of needing a real interpreter. The
  format-table half of `DateTime`/`date()` handling stays genuinely
  portable and is scheduled (Phase 77/78).
- A handful of one-off inapplicable rules: `AllowedSubTypesRule`
  (sealed-hierarchy extension point, inert even in stock PHPStan
  without third-party config); `ClassNameUsageLocationCreateIdentifierDynamicReturnTypeExtension`
  (PHPStan's own dogfooding self-analysis only); `InnerFunctionRule`
  (phpstan-rs intentionally does the opposite - recursing into nested
  named-function declarations, for the common `function_exists()`-guarded
  polyfill idiom - matching it would be a regression); `EnumCaseOutsideEnumRule`
  (structurally impossible in this AST, `EnumCase` only ever exists as
  a field of `EnumDecl.cases`); `ReadWritePropertiesExtension`
  (third-party DI extension-point interface, no plugin architecture to
  attach it to).

---

# Verification approach (used for every phase above)

- Unit tests for each new rule/type-system feature.
- Real PHPStan's own `PHPStan\Testing\assertType()`/`assertNativeType()`
  DSL calls are recognized (`phpstanRs.assertType`, `checker/expr.rs`) -
  comparing this project's own inferred type against the literal expected
  string lets real PHPStan's own vendored `nsrt/`-style fixture corpus
  double as ground-truth tests for this project too, not just its own
  hand-written unit tests.
- Re-run against real corpora after every change: Piwigo's `src/`, its
  `vendor/`, and a frozen local Piwigo copy with a `phpstan-rs.toml`-
  scoped config for stable before/after diffing (`LC_ALL=C sort` both
  snapshots before diffing - locale-sensitive sort otherwise manufactures
  phantom findings).
- Spot-check a sample of newly-surfaced diagnostics by hand to confirm
  they're genuine findings, not false positives.
- Location-based classification for every corpus diff: a changed line
  landing at an already-diagnosed (file, line) location is a safe
  reshape (e.g. a message going from generic "array" to precise
  "array<K, V>"); every genuinely new location is traced individually to
  ground truth before being accepted, never assumed safe from the raw
  count alone.
- For behavior claimed to match real PHPStan: verify against a real,
  current `vendor/bin/phpstan` run (message text, line number,
  identifier) or `\PHPStan\dumpType()`/live `php`, not recalled
  knowledge - re-verify after a long gap, since real PHPStan keeps
  evolving.
- Before reverting a fix after a corpus-verification regression: is
  this a demonstrated soundness bug (a minimal, corpus-independent
  repro shows the checker asserting something false), or is it
  individually correct but the corpus just doesn't confirm broad
  true-positive value? The former gets fixed or reverted; the latter
  gets shipped opt-in (`--check-...`, off by default), never fully
  discarded. See **Reverted**'s own intro.
- `cargo llvm-cov` test-coverage measurement - manual review kept
  finding real bugs the test suite should have caught but didn't
  exercise; close a file's gaps by writing a test for what coverage
  flags, only after first manually verifying correctness.
