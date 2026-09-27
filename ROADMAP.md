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
**Pending / partial** (a phase with some of its scope shipped),
**Planned (phases 85+)** (every numbered phase not started yet, whether
it's a parity gap, a beyond-parity idea, a top-100-extension survey
finding, or a Psalm-catalog finding), **Reverted** (implemented,
corpus-verified, then undone - kept so the real root cause isn't
re-discovered from scratch next time), and **Known limitations**
(deliberate boundaries not tied to any one phase). A newly found gap
gets a numbered phase in **Planned** right away rather than sitting in
an untriaged list. See **Verification approach** at the end for the
methodology behind every corpus number cited above.

Detailed round-by-round investigation narrative (which real corpus file
surfaced which bug, in what order, fixed by which specific helper) is
deliberately not kept here once a phase settles down - it stays fully
recoverable from git history and commit messages. This file tracks
*what's true now*, not a lab notebook of how it got that way. (Rewritten
to this condensed form several times before, as the narrative regrows
between rewrites - see git history for `ROADMAP.md` if a past finding's
own reasoning is needed verbatim.)

---

# Current status

- Piwigo frozen-corpus diagnostic count (`phpstan-rs.toml`-scoped,
  `/home/torres/piwigo17-frozen`) - the primary false-positive-hunt
  metric - was 342 as of `c8555bd`, down from a 343 pre-batch baseline.
  Substantial further work has shipped since that measurement (this
  session's own 8-item rule-parity batch, Doctrine ORM/SQL-reflection
  phases, the whole-program dead-code family, and more - see Done) -
  re-verify against real PHPStan before citing an exact number either
  way; every run is saved to `.corpus-runs/{scoped,unscoped}/
  <commit-hash>.txt` (gitignored, see that directory's own README) for
  comparison against an old commit.
- Full Piwigo+`vendor/` corpus (~20,200 files, unscoped): 40,459
  diagnostics as of `c8555bd`, the overwhelming majority genuine findings
  in third-party code this project doesn't control - not re-verified
  line-by-line at this scale (best-effort secondary coverage, per
  **Verification approach**), only spot-checked per new-finding bucket.
  Re-run before citing a number - it drifts as vendored dependencies and
  this project's own precision both change.
- Test suite: 4499 `#[test]` functions as of 2026-09-22 (counted from
  source, not a fresh `cargo test` run; last full run was all passing,
  build/test/clippy clean).

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
precedence, not just at the top of the whole expression. `break`/
`continue` with a parenthesized level expression (`break (1);`/
`continue (2);` - both legal, real PHP grammar) parses instead of
aborting the whole file; the level constant-folds (literal int,
parenthesized, or unary-negated).

**Grammar-level PHPDoc parser** (Phase 56). `src/phpdoc_grammar/` is a
real tokenizer + recursive-descent parser (replacing `docblock.rs`'s
former lenient, hand-rolled ad-hoc scanner) covering nullable/union/
intersection types, generics (including real PHPStan's own call-site
variance markers, `Collection<covariant Foo>`/`<contravariant Foo>`/
`<*>`), array shapes, callable/`Closure` signatures, int ranges, literal
types, const-fetch types, conditional types, and every tag structure
`DocBlock` reads, producing a real `PhpDocNode` AST plus recoverable
`PhpDocParseError` diagnostics (`DocBlock.parse_errors`, unconsumed by
any rule yet). Verified via a full unscoped Piwigo corpus diff, which
caught and fixed 5 real grammar gaps (an HTML fragment mistaken for a
generic-argument list; a leading `\` on a fully-qualified callable type;
`callable(Type $name): R`'s own optional parameter name; a bare wildcard
suffix losing its `*`; Psalm's `!Type` negation syntax degrading
instead of unwrapping) - the scoped corpus diff stayed empty throughout.

## Registry / symbol resolution

Namespace- and `use`-import-aware name resolution, tracked positionally
per file. Real `jetbrains/phpstorm-stubs` (558 files) and
`phpstan/phpstan-src` (~1800 files) vendored and parsed into the
registry alongside project code, with real stub-vs-real-class merge
semantics and duplicate/conditional-declaration tiebreaking for the many
real "two declarations of one name, gated by PHP version" shapes both
corpora use. Composer-aware `vendor/` autoload-based symbol resolution.
A `define()`/`const`-based global-constant registry resolves literal-
shaped constant values project-wide, including every real built-in/
extension constant phpstorm-stubs itself declares; `use const`'s own
existence check (`constant.notFound`) is unconditional (constants are
case-sensitive in PHP). A class/interface/trait declaring the same
method name twice in one body (phpstorm-stubs' own version-split/
overload idiom) merges into one positional-union signature, the same way
two top-level free-function declarations already do; a trait declared
twice under a conditional with genuinely different signatures checks
each branch's own body against its own literal signature, not whichever
declaration won the registry's single slot.

**Symbol-existence pipeline: name-case checking, full coverage**
(Phase 48). `ClassCaseSensitivityCheck`'s class-name-case half
(`Registry::canonicalize_class_name_case`, `Registry::lookup_class`
mirroring `lookup_function`'s case-insensitive fallback) covers every
native-syntax class-name reference and every PHPDoc-typed position too
(param/return types, property positions including `@property`, `@method`/
`@mixin`/`@phpstan-type` targets, property hooks' own `set()` parameter,
`@phpstan-assert`-family targets, `@phpstan-require-extends`/
`-implements`/`-sealed` RHS, closure/arrow-fn literal hints) - a
*backfilled* (inherited-docblock) position deliberately never gets
case-refs propagated, since the raw text belongs to an ancestor file's
own `use` imports. Also fixed in the same corpus round: `Parser::
take_name_segment`'s own keyword-as-identifier recovery reconstructed an
identifier from the keyword's own canonical lowercase spelling rather
than the original source bytes, so `class Enum { ... }` (a real class
literally named the same as the `enum` keyword) silently corrupted its
own declared name.

`function.duplicate` was corrected from always-on to opt-in (Phase 63
follow-up) - real PHPStan's own `DuplicateFunctionDeclarationRule` is
`#[ValidatesStubFiles]` and never runs against ordinary project files at
all, only a project's own configured `stubFiles:` list; now gated behind
`Registry::check_duplicate_function_declarations`, default off. **Whole-
project duplicate class-name detection** (Phase 83) is a real, deliberate
stricter-than-PHPStan feature: new opt-in `check_duplicate_class_
declarations` (off by default) flags the same class/interface/trait/enum
FQN declared unconditionally more than once project-wide, with a new
`Registry::stub_paths`/`--stub-path` config designating a deliberate
override location (excluded from the count, still loaded for context).
**Anonymous class stable synthetic naming**: a `new class { ... }`
instance now registers into the registry under a real, stable synthetic
name (`"AnonymousClass" + md5(...)`, mirroring real PHPStan's own
`AnonymousClassNameHelper` algorithm) instead of inferring bare
`Type::Mixed` or aliasing to whichever interface it implements (unsound
for the "spy" test-double pattern - see Reverted). **PHP 8.2+ trait
constants** are now composed into a using class's own registry entry -
`flatten_traits_one_pass` merges `consts` the same way it already merges
`methods`/`properties`, closing a real false-positive class
(`self::CONST` from inside a trait method, checked against every host).

## Type system

Generics with call-site substitution, `@extends`/`@implements` chain
substitution, template bound enforcement (`@template T of Bound`) and
variance checking. String/array accessory refinement types (`non-empty-
string`, `non-falsy-string`, `numeric-string`, `list<T>`, `non-empty-
array<T>`), plus `IntegerRangeType` (`positive-int`, `int<min,max>`,
with a full range-arithmetic surface across every operator, including
silent int-to-float overflow promotion on `+`/`-`/`*`) and a real cast/
coercion method surface. `Type::Array` carries a real key type, not just
a value type; `ArrayShape`'s own int-vs-string key precision
(`types::shape_key_type`) is wired into assignability/subtyping - an
all-string-keyed shape no longer silently satisfies `array<int, string>`.
`object{...}` shapes (including a genuinely unsealed `array{..., ...}`,
whose `...<K, V>`/list `...<V>` tail the PHPDoc grammar parses as
phpdoc-parser does; `non-empty-array{}`/`non-empty-list{}` shapes too,
and `object{}` rejects unkeyed items and a `...` tail),
`value-of<T>`/`key-of<T>`, `OffsetAccessType` (`SomeShape['key']`), and
conditional PHPDoc return types are all parsed and resolved. A
conditional return type now resolves to a specific call site's real
branch (`Type::Conditional`, `types::resolve_conditional`) rather than
always widening to the union of both branches, with a safe union
fallback when no bound argument exists to resolve against. Resolution is
trinary: provably-distinct literals (and enum cases, `true`/`false`,
out-of-range ints) pick the false branch, an omitted argument resolves
against its parameter's default value, and a call's own templates that
no argument inferred resolve to their bounds (`resolveToBounds`), while
an erased receiver's class templates stay the `Mixed` wildcard.
`enum-string`/`callable-string`/`interface-string`/`trait-string` all
get real content verification, stricter than real PHPStan draws for the
latter two. Enum cases have their own singleton `Type::EnumCase`
identity, with `::from()`/`::tryFrom()` narrowing to the exact matching
case. GMP/`BcMath\Number` operator-overload dispatch.
`Type::Callable(Option<Arc<CallableSig>>)`: a `Closure(...)`/
`callable(...)` PHPDoc shape's own per-parameter/return signature parses
into a real `CallableSig`, threaded through every recursive type-tree
walker, `Display`, and `is_assignable` - including real contravariant-
parameter/covariant-return subtyping between two different `Closure(...)`
shapes (`callable_sig_is_subtype`, reusing the identical variance-
composition pattern generic template arguments already use).

A PHPDoc type is resolved and kept apart from its native hint
(`ParamSig::doc_ty`, `FunctionSig::return_doc_ty`), and once the whole
hierarchy is indexed a member the native hint provably excludes - or a
fully known, unrelated class name - is dropped from the effective type,
with a wider member standing in for the native alternatives it covers
and any untouched native alternative added back (real PHPStan's
`TypehintHelper::decideType`, applied per member rather than all-or-
nothing); `@return static` survives a native `self`. A constant-named
PHPDoc type (`FTP_ASCII|FTP_BINARY`) is that constant's value
(`mightBeConstant`), `@phpstan-import-type` aliases and deferred
`value-of`/`Class::CONST` types (including `@extends`/`@implements`
arguments) resolve before inheritance copies them, and templates keep
their first-appearance order across `@template`/`@phpstan-template`
spellings. `Closure(...)` parses as a real `Closure` with that signature
(PHPStan's `ClosureType`), always the global class; `self` resolves
inside callable shapes, an inherited `self` binds to its declaring
class, and a `parent::`/`self::`/`static::` call's `static` return is
the calling class (`DateTime`/`DateTimeImmutable`'s fluent methods are
patched to return `static`, as they do at runtime). Bitwise `&`/`|`/`^`
over int literals fold to exact values, and an enum case is assignable
to itself inside generic arguments.

PHPDoc-inheritance backfill: a method correctly omitting its own
`@param`/`@return` in favor of an ancestor's/interface's tag resolves
through that tag, including through anonymous classes and promoted-
constructor-property inline `@var` tags, with real explicit-vs-implicit
`mixed` provenance surviving the hop and the backfilled type reconciled
against the child's own native type hint (not cloned verbatim) - this
maps the inheriting class's own templates through the ancestor binding
(an interface's `list<T>` under `@template-implements Factory<M>`
becomes `list<M>`, then the subclass's `@extends` binding), recursing
into callable shapes and conditional types too - this
now also fires for an ancestor type mentioning a method-scoped `@template`
(not just a class-scoped one), verified safe since call-site binding is
purely name-keyed. Generic template richness: real bound/variance/
scope/default per `TemplateParam`; bound-aware subtype/assignability;
substitution recursion into a generic class reference's own argument
list; variance-aware comparison of two concrete generic instantiations'
type arguments; real call-site variance annotation syntax; generic-
reference arity/well-formedness checking scoped to the
`extends`/`implements` ancestor-declaration site. `union_types` merges
two structurally different `Array(key, val)` members into one
`Array(key1|key2, val1|val2)` instead of keeping them as separate,
redundant members.

**Class-constant types and constant type references.** A class
constant's value is typed exactly: an array literal is an exact shape
(real PHPStan's `ConstantArrayType`), and a reference to another
constant (`self::A`, `Foo::BAR`, inside an array too) resolves through
a fixpoint pre-pass of the deferred-type pass. A cycle, or a missing
constant, ends up as `mixed`. The PHPDoc constant-type syntax
(`Foo::BAR`, `Foo::PREFIX_*`, `Foo::*`, `self::X`,
`value-of<Foo::CONST>`) resolves to the union of the matching
constants' types (inherited and interface constants included, own
declarations first) instead of collapsing the whole union to `mixed`.
`Foo::class` is a class-string. Conditional return types resolve
trinarily like `ConditionalTypeForParameter` (a maybe-subject keeps
both branches, deduplicated). A template-typed conditional survives
reconciliation with a nullable native hint, and a native `?` is never
unioned onto one. `array<T>` binds `T` from `list`/`non-empty-array`
intersections and from array shapes.

**String accessory case-tracking** (Phase 75). New `AccessoryKind`
variants `LowercaseString`/`UppercaseString`/`DecimalIntegerString`,
produced by `strtolower`/`strtoupper`/`ucfirst`/`mb_convert_case`/etc. A
full `resolve_concat_type` port (real PHPStan's own
`InitializerExprTypeResolver::resolveConcatType`) makes concatenation
infer `literal-string`/`non-falsy-string` instead of collapsing to plain
`string`, closing this feature's main SQL-injection-safety use case. A
new `format_php_float` module gives concat's float-to-string coercion
PHP's real fixed-14-significant-digit algorithm.

**Level-aware type-acceptance** (Phase 84). `Registry::configured_level:
Option<u8>` plus `Registry::accepts_at_configured_level` give
`project.level` real teeth at 8 call sites, mirroring real PHPStan's own
`RuleLevelHelper::accepts()` (below level 8, `Null` is stripped from an
accepted Union; below level 7, a Union comparison uses `.any()` instead
of `.all()`). `checkExplicitMixed`/`checkImplicitMixed` (real level
9/10) are done too: `Type::Mixed` widened to `Type::Mixed(bool)`
(explicit vs. implicit provenance, matching real PHPStan's own
`MixedType::isExplicitMixed()`, with `PartialEq` staying provenance-
blind), plus a full per-site explicit-vs-implicit construction audit
against every real `new MixedType(true)` site in `vendor/phpstan-src`. The receiver half (Phase 139, `RuleLevelHelper::findTypeToCheck`'s
`StrictMixedType` branch) is built too: at level 9 (explicit `mixed`)
or 10 (implicit), calling a method, reading a property, or taking an
offset on a `mixed` value reports `Cannot call method foo() on mixed.`
and its siblings, via `Registry::strict_mixed_receiver_level`. An
unconfigured level never triggers it.
`checkBenevolentUnionTypes` is done too - confirmed real PHPStan's own
default (`false`) matches this project's pre-existing lenient
`AccessoryKind::Benevolent` behavior already; the flag opts *into* real
PHPStan's stricter, non-default behavior, and `Registry::check_
benevolent_union_types` supplies it at the identical 4 call sites real
PHPStan's own flag touches.

A long tail of small, real stdlib/type-inference precision fixes, each
corpus-verified and independently regression-tested: `array + array`'s
key/value precision (mirroring `InitializerExprTypeResolver::
getPlusType`'s general case, `ArrayList`/`NonEmptyArray` accessories
preserved); `array_keys()`'s 2-/3-arg key-type propagation and
`array_filter()`'s first-class-callable type-check-predicate
recognition; `usleep()`'s stub over-refinement past what real PHPStan
actually checks (`functionMap.php` never consults phpstorm-stubs'
PHPDoc for a function it already covers) and the same root cause's
broader confirmation (`vendor/phpstan-src/stubs/` is a separate,
un-ingested bundle - a disclosed, well-scoped future item, not
attempted generally); `proc_open()`'s by-ref `$pipes` output;
`__serialize()`/`__debugInfo()`'s real, unconditional explicit-`mixed`
return-type override; removal of the (never actually read by real
PHPStan) `#[ArrayShape(...)]` attribute mechanism, replaced by
`functionMap.php`-verified per-function patches for the ~20 real
functions it actually refines (`imagecolorsforindex`, `getdate`,
`getimagesize`, `curl_getinfo`'s adjacent table, and more);
`pathinfo()`'s own `dirname`-field optionality now correctly keyed off
the input's own `non-empty-string` accessory rather than hardcoded
present. A `foreach` by-ref value variable now widens its own subject
array's tracked element type (`CategoryCatsRenderer::render`'s own
shape); a real generics-substitution gap (calling a generic interface's
templated method through a concrete class that directly re-declares it)
is fixed, including a regression in the first attempt (a signature read
for its own declaring class's own body-check path, not just external
call sites) caught by the unscoped corpus diff. Subtyping properties via
`proptest` (`src/types/tests/subtyping_properties.rs`) found and fixed
two real soundness bugs: `Type::FloatLiteral(NAN)` failing reflexivity,
and an `Intersection`-vs-`Union` arm-ordering bug violating `A & B <: B`.
A new `AccessoryKind::Benevolent` marker (mirroring real PHPStan's
`BenevolentUnionType`) makes a bare PHPDoc `Type[]`-shorthand array's key
type benevolent. A union containing `mixed` alongside other members now
correctly collapses to plain `mixed` in every construction path, not
just the common one.

## Narrowing engine

`instanceof`, equality/identity, `isset`/`??`/`??=` (including the
offset/property-chain reversal and a provably-*absent*-offset diagnostic,
mirroring real PHPStan's `IssetabilityDescriptor` chain-walking), common
predicate functions (`is_*`, `str_contains`-family, `array_is_list`,
`method_exists`-adjacent), PHPUnit assertion methods, De Morgan-aware
compound `&&`/`||` chains (cross-leaf, through parenthesized/cast
wrappers, through a closure's own `use (&$x)` by-ref write-back).
`break`-reached loop state merges into the post-loop scope; a `finally`
block's own unconditional exit point correctly overwrites the try/catch
side's, and its own entry scope is the pre-try state merged with normal
completion (real PHP can jump into `finally` from any throw point, not
just full try-body completion). `count()`/`array_is_list`/
`str_contains`-family truthiness narrowing (including the mirrored/
negated `count($x) < 1` shape); comparison-outcome narrowing for
`<`/`>`/`<=`/`>=` between known-numeric operands; a loose (`==`/`!=`)
truth table between differing scalar literal kinds, transcribed from
real PHPStan's own `LooseComparisonHelper`. Five further narrowing
precision gaps: the reversed `count()` comparison order; `!empty($x)`
upgrading an already-`non-empty-string` to `non-falsy-string`; a
dynamic-key array write's own key type narrowed by a preceding
`is_int()`/`is_string()`-family guard; `array_filter()`'s no-callback
default filtering narrowing a provably-null/falsy `ArrayShape` field;
`array_merge($shape, $plainStringKeyedArray)` staying field-aware.
`method_exists`/`property_exists` true branches now guard `method.
notFound`/`property.notFound` the same way `function_exists()` already
guards `function.notFound`. `is_a()` narrowing (Phase 167)
reuses `instanceof`'s own primitives directly.

**Fixed-point loop-body convergence** (Phase 55, `Checker::
converge_loop_body`, mirroring real PHPStan's own bounded
`LOOP_SCOPE_ITERATIONS`): a
`While`/`DoWhile`/`For`/`Foreach` body is checked against a candidate
entry state, then re-checked with that entry unioned against the
previous pass's exit whenever the two differ, capped at 6 total passes,
with only the final pass's diagnostics kept. (Phase 85's A1 raised the
cap from real PHPStan's 3: real PHPStan widens literal types after one
pass, so 3 passes is cheap there, but this project's plain union
merging does no early widening, so a higher cap costs compute on
non-convergent loops and never costs precision.) This is the foundation the
unreachable-code/constant-condition family depends on - see that entry
and **Reverted** for the real soundness bug this fixed. A closure
literal's body (both a plain `Closure` and `ArrowFn`) routes through the
identical convergence machinery, not a single unconditional pass -
applies uniformly to every closure literal, not gated to one builtin.

A cluster of narrow, corpus-justified slices of real PHPStan's own
`ConditionalExpressionHolder`, each independently assessed and filed
rather than declined by association (the *general*, unbounded mechanism
stays declined - see Known limitations): correlating a boolean flag
variable's own truthiness to the condition it was assigned from
(`condition_aliases`); same-branch discriminant-variable correlation
(`branch_implied_facts` - a `$subject === value` true branch implying
some *other* variable became non-null/a concrete object/enum case, keyed
by `(subject, value)` and replayed anywhere the identical discriminant
recurs, with a self-referential-implication guard to avoid overwriting a
freshly re-narrowed fact); same-condition-expression-recurrence
narrowing (Phase 170, a bare-variable or `isset(...)` discriminant,
replayed on both branches); disjunctive-syllogism narrowing (Phase 162,
`if ($a || $b) { if ($b) {} else { /* $a true */ } }`, restricted to a
bare variable/`(bool) $variable` atom); equality-comparison leaves in
the OR/AND-chain narrowing mechanism (Phase 171, `$x === null || $x ===
0` narrows directly, and the flag-variable-reuse form of the same
shape); a nullsafe chain's own proven-non-null result implying its
receiver is non-null too (Phase 172); `foreach` header inline `@var`
annotation (Phase 161, the loop-variable rebind previously overwrote
whatever the annotation preamble had set); `branch_implied_facts`
carried into a `use (...)`-capturing closure literal, filtered to the
captured names (Phase 175, `RateService::rate`'s own closure re-testing
an outer `if ($userAnonymous)` discriminant); by-ref closure-
capture write-back now unions every real exit point's own snapshot
(mirroring the existing by-ref-*parameter* mechanism) instead of reading
whichever state a single sequential walk happened to land on -
previously lost a mutation made only on an early-return path; an
if-body-set flag correlates with the `if`'s own (negated) condition;
whole-container per-field facts from a by-ref `foreach`
(`container_field_facts`, harvested from `bind_assignment_target`'s
existing per-key entries and propagated onto a later dynamic-keyed read
of the same container); branch-gated array-accumulator tracking
(`discriminant_array_facts`, consulted as a fallback for a `Type::
Conditional` declared-return-type check specifically); dynamic-key array
writes resolved when the key is known by *type*, not just AST syntax.
Several of these needed a genuine second fix after a later corpus
re-run: `condition_alias_targets` generalized to collect every distinct
subject in a compound condition (not just one) plus a `Unary(Not, ...)`
leaf arm; the proxy-discriminant scan gained a `Type::EnumCase` case
alongside `True`/`False`/`Object`; a plain `Closure`'s (unlike an
`ArrowFn`'s) fresh scope needed captured facts carried across explicitly.

**`match.unhandled`** - a `match` with no `default` whose condition
isn't fully covered by its arms (real PHPStan's own `MatchExpressionRule`,
found during unrelated Psalm-catalog research). A deliberately
independent computation from the arm-narrowing loop's own accumulated
state: re-derives the subject's own type, expands a plain enum-typed
subject into the `Union` of its real cases first (`expand_enum_cases`,
since `narrow_type_excluding` can only filter an already-Union'd type,
never expand a bare `Object`), then excludes every arm condition's own
known-single-value type. Also proves the `match(true)` idiom's own
exhaustiveness (the last arm's condition checked against every earlier
arm's own accumulated exclusion, via the existing constant-condition
oracle and falling back to the same range-comparison machinery
`smaller.alwaysTrue`'s own family uses - a wiring gap, not a missing
capability), and honors real PHPStan's own `UnhandledMatchError`-catch/
`@throws`-tag exemptions (two new `Checker` fields tracking the current
enclosing `catch` types and declared `@throws` type). **`match.
alwaysTrue`/`match.alwaysFalse`** - the dead-arm half of the same real
rule - checks each arm condition as `$matchCondition === $rawCondition`
via the shared `registry::identical_result` primitive (see
`identical.alwaysFalse`/`.alwaysTrue` below), against an independently-
tracked `remaining_subject_ty` (confirmed by direct testing that
`accumulated`'s own narrowed subject value stays the bare, unexpanded
enum type across repeated exclusions - the identical reason `match.
unhandled`'s own computation is independent). A `next_arm_is_dead` flag
mirrors real PHPStan's own per-arm (not per-condition) skip once any
condition proves always-true; the last arm is unconditionally exempted
from `alwaysTrue` (a plain `$i === $armsCount - 1` check, not the
`elseif`-style config gate `identical.alwaysTrue` itself honors) - a
trailing `default` arm still counts as "something after," so a
genuinely redundant last named arm followed only by an unreachable
`default` still gets reported.

## Rule catalog (checker)

Symbol existence (classes/functions/use-imports/typehints, catch
clauses, `extends`/`implements`/`instanceof`/trait-use targets, a
`use`-imported build-tool-mangled-prefix check), plus `use function`'s
own case check. Call-argument binding (named args, spread/unpack, by-ref
legality, constant-array-spread element-by-element type checking).
`MissingTypehintCheck`'s full level-6 battery. Property/method/constant/
class-constant visibility enforcement end to end, plus the whole
first-class-callable family running the same checks an ordinary call
gets. Unused-symbol detection matching real PHPStan's three
`UnusedPrivate*Rule` classes end to end, plus a PHP 8.4 asymmetric-
visibility property's own narrower dead-write check, and
`UnusedConstructorParametersRule` under its own
`constructor.unusedParameter` identifier (any promotion modifier,
attribute classes, and constructors dictated by an interface exempt,
anonymous classes included). Duplicate-
declaration scans within a class/trait/enum body, a same-file
case-insensitive class-vs-`use`-alias collision, and duplicate top-level
*function* declarations (a real, whole-project, cross-file scan,
distinguished from phpstorm-stubs' own legitimate multi-declaration
pattern).

**Override-compatibility**, essentially complete: visibility narrowing,
missing/extra/optional/by-ref/variadic parameter mismatches,
`#[Override]`'s unconditional-fatal-error half (methods and properties
alike), the interface-vs-trait-vs-extends ancestor-lookup priority fix
(matching real `MethodPrototypeFinder` exactly), an overriding method's
own casing mismatch against its parent/interface (`method.nameCase`),
property hooks (`property.notReadable`/`.notWritable`/
`.abstractOverridingNonAbstractHook`), PHP 8.4 `final`/`abstract`
property modifiers including on a promoted constructor property,
`method.nonStatic`/`.static` and the general
`method.abstractOverridingNonAbstract` case, both real-keyword and
docblock-only (`@final`/`@phpstan-final`) ancestor-final checks for
methods and properties, and native-type property-override variance
(`property.nativeType`/`.missingNativeType`/`.extraNativeType`). Nine
declaration-site modifier-consistency checks plus the class-constant
`final`/native-type-modifier trio. Enum sanity and generic-bound
checking wired to enums. Every `extends`/`implements`/trait-`use`
target checked against the kind of symbol it resolves to
(`class.extendsInterface`/`.extendsTrait`/`.extendsEnum`,
`classImplements.*`, `enumImplements.*`, `interfaceExtends.*`,
`traitUse.*`, `interface.traitUse`), plus `class.extendsFinalByPhpDoc`
and `class.readOnly`/`.nonReadOnly` parent readonly-parity, anonymous
classes included. `InvalidPromotedPropertiesRule`'s promotion
legality (`property.invalidPromoted` outside a non-abstract constructor,
on a variadic parameter, or `final` below PHP 8.5;
`property.promotedNotSupported` below PHP 8.0), on every function-like
including closures, arrow functions, and a trait method aliased as
`__construct`; a `readonly`/`final`/set-visibility-only parameter now
promotes with implicit `public` visibility. `ReadOnlyClassRule`'s version gate
(`readonly class` below PHP 8.2, an anonymous one below 8.3). `InstantiationRule` end to end: `new.trait`/`.enum`/
`.interface`/`.abstract` (never for `new static`, nor a dynamic
`new $expr()`), `new.noConstructor`, `new.privateConstructor`/
`.protectedConstructor`, `new.nonObject`, `class.noParent`, and
`outOfClass.self`/`.static`/`.parent`. `NewStaticRule` (`new.static`, exempting final
classes, `@phpstan-consistent-constructor`, interface-pinned, `final`,
or abstract-prototype constructors) and
`new.staticInAbstractClassStaticMethod`; `new Foo(...)` reports
`callable.notSupported`. `ImpossibleInstanceOfRule`
(`instanceof.alwaysTrue`/`.alwaysFalse` from a registry-aware
always/never/maybe verdict over the checked expression's type, the
always-true half exempt as the last `elseif`; a dynamic target typed as a
class-name literal or `class-string<C>` decided too;
`instanceof.invalidExprType` for a dynamic target that can't name a
class). A constant-condition
finding inside a trait is kept only when every using host reports it
with the same verdict, matching `ConstantConditionInTraitRule`. `MissingMagicSerializationMethodsRule`
(`class.serializable`, PHP 8.1+, anonymous classes included). A bodiless non-abstract method outside an interface
(`method.nonAbstract`), from a new `FunctionDecl::has_body`; an
empty-bodied constructor's unused parameters are now reported too. First-class-callable syntax below PHP 8.1
(`callable.notSupported`, `FunctionCallableRule`/`MethodCallableRule`/
`StaticMethodCallableRule`). `StaticMethodCallCheck`'s remainder:
`method.staticCall` (an instance method called statically without a
compatible `$this`), `method.callToAbstract`/
`staticMethod.callToAbstract`, `staticMethod.nonObject`, and
`outOfClass.*`/`class.noParent` for `self::`/`static::`/`parent::`
calls; `@method static` tags and enum `cases()`/`from()`/`tryFrom()` are
now recorded in `ClassInfo::static_methods`. `OverridingMethodRule`'s
`method.tentativeReturnType` (PHP 8.1+, a stub's `#[TentativeType]`
return replacing `method.childReturnType`, `#[\ReturnTypeWillChange]`
exempt). Property declaration legality for plain and promoted
properties: `PropertiesInInterfaceRule` and `PropertyInClassRule`'s full
hook/modifier matrix and PHP 8.4/8.5 gates, `ReadOnlyPropertyRule`,
`ReadOnlyByPhpDocPropertyRule` (with `@phpstan-allow-private-mutation`
modeled, which also exempts its own class's writes from
`property.readOnlyAssignNotInConstructor`), `property.callableType`, and `property.unresolvableNativeType`. The
write-only/read-only/by-ref cluster: `property.writeOnly` and
`assign.propertyReadOnly` (a `@property-read`/`@property-write` tag, or a
virtual hooked property missing that hook, via `PropertySig::
is_readable`/`is_writable`), and `property.assignByRef`/
`.readOnlyAssignByRef`/`.readOnlyByPhpDocAssignByRef` for `$x = &...`. Once per
function-like (function, method, closure, arrow function, hook `set`
parameter): `parameter.superglobal`/`.this`/`.duplicate`/
`.variadicNotLast`, and `nullsafe.byRef` for a by-ref function-like
returning through `?->` (`FunctionDecl`/`ArrowFn` now record `by_ref`). `parameter.defaultValue` for closure and arrow-function
literals (`IncompatibleClosureDefaultParameterTypeRule`/
`IncompatibleArrowFunctionDefaultParameterTypeRule`). `return.missing` now follows
`MissingReturnRule` exactly: a nullable type is owed a return too (level 2
when PHPDoc-only, `checkPhpDocMissingReturn`), a native `mixed` is owed
one, and closures with a native return type are checked. `CallCallablesRule`'s
remainder: invoking a value that names a function or method (`'strlen'`,
`'Foo::bar'`, `[$obj, 'method']`, `['Foo', 'bar']`, an `__invoke`
object) checks the resolved signature's arity and argument types, plus
`callable.inaccessibleMethod`. The castable-array-argument family
(`ParameterCastableToStringRule`/`ToNumberRule`/
`ImplodeParameterCastableToStringRule`/`SortParameterCastableToStringRule`,
flag-aware for the sort functions). `ImpossibleInArrayHaystackFiniteTypesRule`
(`function.impossibleHaystackValue` for `in_array()`/`array_search()`/
`array_keys()` against a constant-array haystack). `UsageOfVoidMatchExpressionRule` (`match.void`). `DeadForeachRule` (`foreach.emptyArray`). `OffsetAccessWithoutDimForReadingRule`
(`offsetAccess.noDim`; a by-reference argument slot and `$r = &$a[]`
legally create the element). `ArrayUnpackingRule`'s
`arrayUnpacking.stringOffset` below PHP 8.1. `NonexistentOffsetInArrayDimFetchCheck`'s
definite half as `offsetAccess.notFound` (replacing this project's own
`shapeAccess.notFound`): a literal key missing from a sealed shape, or
ruled out by a general array's/list's key type, on reads and on every
destructured key (`ArrayDestructuringRule`'s per-key half). `OffsetAccessAssignmentRule`/
`OffsetAccessAssignOpRule`/`OffsetAccessValueAssignmentRule`
(`offsetAssign.dimType`/`.valueType`: a string written through `[]` or a
non-int offset, an `ArrayAccess` object given an offset or value its
`offsetSet()` rejects). `OverridingConstantRule` (`classConstant.final`/
`.visibility`, and at level 3 `.nativeType`/`.missingNativeType`/`.type`),
from `ConstSig`'s new `is_final`/`native_ty`/`has_phpdoc_type`. `ValueAssignedToClassConstantRule`'s `@var`
half (`classConstant.phpDocType`, and `classConstant.value` against a
typed constant's `@var`). `ConstantRule` (`constant.notFound`, level 1; a
`defined('NAME')` test exempts `NAME` flow-sensitively, through the new
`Locals::guarded_constants` narrowing) and
`MagicConstantContextRule` (`magicConstant.outOfClass`/`.outOfTrait`/
`.outOfFunction`/`.outOfNamespace`). A `define()` anywhere - a function
or method body, a loop, a `try`, inside an expression like `defined('X')
|| define('X', 1)` - now registers its global constant. `dynamicConstantNames` (config
`dynamic_constant_names`, a list or a name-to-type table, global and
`Class::NAME` constants): a configured constant reads as its configured
or general type, and `ValueAssignedToDefineRule`/
`ValueAssignedToGlobalConstantRule`/`ValueAssignedToClassConstantRule`'s
configured half report a declared value that type rejects
(`constant.defineValue`/`constant.value`/`classConstant.value`). `DynamicClassConstantFetchRule`
(`classConstant.dynamicFetch` below PHP 8.3, `classConstant.nameType`),
from a new `ExprKind::DynamicClassConstFetch` that also checks the name
expression itself. `ConflictingTraitConstantsRule` (visibility,
finality, native type against every used trait at any depth, and value
against immediately used ones). Statement keywords per function-like
body and the file's top level: `ContinueBreakInLoopRule` (plus a
non-positive level and `continue.switch`), `GotoUndefinedLabelRule`/
`UnusedLabelRule` (plus duplicate labels and jumps into a loop, a
`switch`, or a `finally`, or out of a `finally`), and
`DeclareStrictTypesRule` (plus block-mode `strict_types`, `ticks`/
`encoding` value types, and unknown directives) from a new
`StmtKind::Declare`. `RequireFileExistsRule` (`{include,includeOnce,
require,requireOnce}.fileNotFound`): a path built from literals,
constant string types (every member of a union), `__DIR__`/`__FILE__`,
`dirname()` with levels, concatenation, and constants declared in the
same file (resolved through their initializers, so `__DIR__` there is
this file's), checked against the cwd, the file's own directory, and
`include_path` (PHP's default plus any `set_include_path()`/
`ini_set('include_path', ...)` in the file); a directory is not a file,
and a path the file tests with `file_exists()`/`is_file()`/
`is_readable()`/`is_writable()`/`is_executable()` is skipped.
`FileWhitespaceRule`: a leading UTF-8 BOM (`whitespace.bom`), whitespace
before the first `<?php` (`whitespace.fileStart`), and whitespace after
the last `?>` of the file, a namespace, or a `declare` block
(`whitespace.fileEnd`); the lexer keeps the single newline after `?>`
as part of the tag, as PHP does.
Native type declaration legality (`type_declarations.rs`, every
parameter, return, property and class constant): `InvalidTypesInUnionRule`
(`unionType.{mixed,never,void}`, `nullableType.*`) plus `?null`,
duplicate members (resolved names, case-insensitive, `bool|false`),
`true|false`, `object` with a class type, `iterable` with `array`/
`Traversable`, non-class intersection members (`static` included),
`void`/`never`/`static` parameters and properties, `void`/`never`/
`callable`/`static` class constants, and below PHP 8.2 standalone
`null`/`false`, the `true` type, and DNF types.
A class or enum declared inside a block now gets the declaration-level
checks too, the registry-keyed ones only when the registry entry was
built from that declaration.

Invalid-operand-combination detection (`ErrorType`): the operand-
validity matrix across binary/comparison/unary/inc-dec operators, plus
division/modulo-by-literal-zero detection.

**Purity/side-effect model**: a registry-build-time fixed point decides
which functions/methods/constructors are provably effect-free, flagging
a discarded statement-level call to one - a call only resolves as a
dependency when its target is knowable without type inference. Same-
family bare-statement `NoopRule`: `xor`/`and`/`or` (reported even when
the statement assigns a local, with the precedence tip), `&&`/`||`,
ternary, and the generic `expr.resultUnused` naming the expression
through a php-parser-style expression printer (`printer.rs`,
first line plus `…` for multi-line constructs). Its purity gate treats
`isset`/`empty` as pure and `exit`/`die` as effects, and adds effects
only types reveal: a property read through `__get` or a `get` hook, an
object offset read through `offsetGet`, and an object stringified
through `__toString`. `@phpstan-impure` tag validation (one-directional: flags a
tagged-impure declaration this project's classifier can *prove* pure).
For a declaration real PHPStan considers pure, every real impure-point
occurrence its body actually contains is reported
(`impure.<id>`/`possiblyImpure.<id>`, `certain`-vs-`maybe`), plus
`pure{Function,Method}.parameterByRef`/`.void` - gated on the tag alone,
not the classifier, since gating on both made the void check fire on
every ordinary untyped-but-provably-pure void declaration.
`@phpstan-immutable` is recognized and treated identically to
`@phpstan-readonly`.

**Property-hook checker validation** (PHP 8.4): virtual-property
detection via an empirically-confirmed rule (a hooked property is
virtual iff no hook body references `$this->propName`), `propertyGetHook.
noRead`/`propertySetHook.noAssign`, `SetPropertyHookParameterRule`'s
native-type-contravariance half, real function-like body checking for
hook bodies, and `PropertyVarianceRule` - generalized to promoted
constructor properties too, not just plain declarations.

**`@internal` restricted-usage engine**: function/method/static-method
calls, property/constant reads and writes, instantiation, `instanceof`,
`catch`, implicit `__toString()`, first-class-callables, and native
param/return type-hints on a named declaration *and* a closure/arrow-fn
literal - off by default. `@deprecated` usage warnings across the
identical access-shape set, off by default, with class-level cascade and
suppression from within an already-deprecated scope. Attribute
existence-checking plus target-validity for every declaration kind the
parser captures an attribute list for, plus constructor-argument
validation, `attribute.nonRepeatable`/`.deprecated`, PHP's native
`#[\Deprecated]`/`#[\AllowDynamicProperties]` misuse checks, and
self-declaration-site parity.

**PHPStan-API-usage checks** (instantiation, static calls, class-const
fetches, `instanceof`, `extends`/`implements`/trait-use, a syntactically-
typed slice of method calls, plus `RuntimeReflectionFunctionRule`/
`RuntimeReflectionInstantiationRule`/`OldPhpParser4ClassRule`/
`NodeConnectingVisitorAttributesRule`, gated the "analyzed class
implements a `PHPStan\*` interface" way).

**Checked-exception/throw-type model**: `@throws`-vs-`Throwable`
validity, `throws.void`, dead-catch detection, `finally`'s own exit-
point-overwrite detection, transitive throw-point collection across the
call graph with a curated set of per-callee dynamic-throw-type
extensions, `TooWideFunctionThrowTypeRule`/`TooWideMethodThrowTypeRule`
(opt-in for protected/public non-final methods), the same family ported
onto property hooks, `MethodThrowTypeCovarianceRule` (opt-in), and an
opt-in "missing checked exception" family with no real PHPStan
equivalent at the function/method level.

**Uninitialized-property detection**, both halves, off by default: the
"assigned anywhere in the class body" check, and real per-constructor
premature-access dataflow (including transitively-called methods,
resolved against the most-derived class). **Return-statement
completeness**: `return.never`/`.void`/`.missing` (explicit and
fall-through) plus the actual-returned-value-vs-declared-type check
(`return.type`) for named functions, methods, closures, and arrow
functions; the genuinely-empty-concrete-method-body case ships opt-in
(see Reverted); a generator's own completeness is checked against its
declared `TReturn`, not the whole `Generator<...>` type; PHP 8.5's
`#[NoDiscard]` is enforced statement-level.

**Generator/yield type modeling** (Phase 70): `Generator<TKey, TYield,
TSend, TReturn>` is a real, resolved shape throughout - `yield`/`yield
from`'s own types, five compatibility diagnostics, and `foreach` over
any `Traversable`/`Iterator`/`Generator`/`IteratorAggregate`-typed
object inferring real key/value types. `YieldFromTypeRule`'s standalone
`generator.nonIterable` (Phase 138: a `yield from` whose operand isn't
iterable) is built too, in `check_yield_from_compatibility`.

Format-string checking for `sprintf`/`printf`/`fprintf`/`vsprintf`/
`vprintf` (positional specifiers, exact placeholder-count matching, PHP
8.0's `%h`/`%H`, an opt-in strict-vs-castable mode). Regex capture-group
inference. A large batch of small builtin checks (function-name case-
insensitivity, `define()`'s removed third argument, `random_int()`
bound ordering, useless-return-value builtins, array-literal duplicate-
key detection through real constant resolution, `array_values`/
`array_filter` no-op conditions including the element-type-based
`arrayFilter.same`/`.alwaysEmpty` sub-checks, `array_multisort()`'s stub
fix, `call_user_func(_array)`'s argument re-checking, `preg_quote()`
delimiter cross-checking, `function_exists()`-guard suppression) and
dozens of individual stdlib array/string/date dynamic-return-type
extensions (`array_fill_keys`, `array_search`, `array_intersect_key`,
`array_flip` including a real per-field shape flip, `array_change_key_
case`, `compact()`/`get_defined_vars()` built straight off `Locals`,
`array_rand`, `array_count_values`, `array_splice` with its by-ref
effect, `array_replace(_recursive)`, `array_pad`, array-pointer
accessors, `preg_split`, `array_column`, `glob`, `explode`, `tempnam`,
`version_compare`, `filter_var`/`filter_input`'s full `FILTER_VALIDATE_*`/
`FILTER_REQUIRE_ARRAY`/`FILTER_FORCE_ARRAY`/flag-conflict family,
`curl_getinfo`'s ~73-entry table, the `date()`/`pathinfo()`/
`parse_url()`/`sscanf()`/`stat()` family, `get_called_class`/
`get_parent_class`/`gettype`/`get_debug_type`, `DateInterval::
__construct()`'s duration-string grammar, `DateTimeInterface::format()`/
`DateInterval::format()`/`::diff()`, `SimpleXMLElement::asXML()`'s
arity-dependent overload, `reset`/`end`/`next`/`prev`'s by-ref
exemption, `array_unique()`'s preserved non-emptiness). `new DateTime()`/
`new DateTimeImmutable()`'s own constructor-string validation
(`src/php_date_parser.rs`, hand-rolling the format families PHP's real
grammar accepts most often, cross-checked against a real PHP 8.5
binary) - deliberately conservative, silent on an unrecognized shape.
`UnreachableStatementRule` (`deadCode.unreachable`, level 4).

**Structural Callable/Closure type** (Phase 53): contextual closure-body
checking generalized to *any* `Closure(...)`-typed parameter via
`Type::Callable(Option<Arc<CallableSig>>)`, not just the six originally
special-cased functions; invoking a callable-typed value directly infers
its declared return type and checks its arguments. **Dynamic static-call
target resolution** (`$obj::method()`, Phase 158):
`ExprKind::StaticCall`'s receiver is now a real `StaticTarget` enum
(`Name`/`Expr`) instead of discarding a non-literal receiver into a
`"?"` sentinel - existence, visibility, arg-type checking, return-type
inference, and LSP hover/go-to-definition all now work through
`$obj::method()`, not just `Foo::method()`. A closure/arrow-fn literal's own value carries its declared
signature too - an unhinted, non-generator arrow fn's return is its
body expression's own inferred type, and an unhinted closure's is its
own return statements' union (`void` with none, `null` added when it
can fall off its end), computed from the closure body's own return
points, which no longer leak into the enclosing function's too-wide-
return accumulator. A literal passed to a shaped parameter is typed
in that context, with its unhinted parameters taken from the shape
(`Type::closure_literal`: an `Object("Closure") &
Callable(Some(sig))` intersection, displayed `Closure(int): string` the
way real PHPStan displays a `ClosureType`). That means invoking a
closure-valued variable checks its arguments and infers its declared
return type, and passing a literal to a shaped `Closure(...)`/
`callable(...)` parameter is checked structurally, while `->bindTo()`/
`instanceof Closure`/a plain `Closure`/`callable` parameter keep
resolving through the object member. Callable-shape comparison follows
real PHPStan's `CallableTypeHelper::isParametersAcceptorSuperTypeOf`
accepts rules: fewer parameters on the passed callable is fine, an
optional accepting parameter paired with a required passed one isn't,
and `mixed` counts as any. Each parameter/return comparison is
class-hierarchy-aware, and an accepting shape's still-unbound method
template (`callable(T): T` on a generic method) is inferred from the
passed closure, so only its bound constrains the parameter. A hinted
closure's return is narrowed by what its body returns (real PHPStan's
`intersectButNotNever`: `function (): array { return ['a' => 1]; }` is
`Closure(): array{a: int}`), keeping the native type when the two are
disjoint.

**Unreachable-code / constant-condition family**: the full, inferred-
type-trusting family - `if`/`elseif` (with real PHPStan's own
last-condition-no-trailing-else exemption), `while`/`do-while`, ternary,
`&&`/`||` (three diagnostics per node, first-level-gated), `xor`, a
standalone `!$expr` check, and a bare function/method/static-call
condition whose own stub narrows to a constant, plus
`NumberComparisonOperatorsConstantConditionRule`. This family was
reverted once (see **Reverted**) over a real loop-staleness soundness
bug, then rebuilt in full once fixed-point loop convergence existed.
Most consequential among the bugs the rebuild surfaced: a systematic
audit found and fixed **~20 separate instances** of one recurring
anti-pattern - a `Type::Object`-only `match`/`let-else` with no
`Union`/`Intersection` arm, silently collapsing a union/intersection-
typed receiver's property/method access to `Type::Mixed` project-wide -
plus a related `Intersection`-search ordering bug and an independent,
more severe bug in inherited-docblock reconciliation (an ancestor's
broader doc type cloned onto a child override verbatim, without
reconciling against the child's own narrower native hint).

**PHPDoc tag ecosystem** (Phase 36), essentially complete: `@phpstan-*`-
prefixed spellings; `@property`-family validation; `@phpstan-assert`/
`-if-true`/`-if-false` including call-site narrowing; `@phpstan-type`
and cross-class `@phpstan-import-type`; `@phpstan-consistent-
constructor`; `@phpstan-require-extends`/`-implements`/`-sealed`'s
consumer/enforcement half; `@method`/`@mixin` synthesis into `ClassInfo`;
`@readonly`/`@phpstan-readonly`; inline `@var`'s error-reporting half
for a plain assignment (`varTag.type`); an enumerated typo-detection
whitelist; `@param-immediately-invoked-callable`/`-later-invoked-
callable`/`-closure-this` declaration-time validation plus real `$this`
injection at call sites.

**Opt-in `missingType.callable`** (Phase 139). A `callable`/`Closure`
written with no `(params): return` signature, at every
`missingType.iterableValue` position (property, class constant,
function/method parameter and return), behind
`check_missing_callable_signature`, real PHPStan's own
`checkMissingCallableSignature` (off by default there too). It recurses
into a written signature's own parameter/return types, and each
message is its rule's iterable-value sentence with "no value type
specified in" swapped for "no signature specified for", which is how
real PHPStan's six rules word it.

**`phpDoc.parseError`** (Phase 130, `InvalidPhpDocTagValueRule`, level
2). It covers declaration docblocks (via `check_unknown_phpstan_tags_on`)
and every statement docblock except a non-assignment expression
statement, and only tags phpdoc-parser itself parses: a plain `@type`/
`@assert`/`@import-type` is never reported, and neither is a
`@psalm-`/`@phan-` tag. The PHPDoc grammar gained the phpdoc-parser
rules the corpus needed:
- `@param`/`@param-out`/`@property*` require a `$variable`, and `@var`
  requires a type.
- A type can't start with a non-`$this` variable.
- A description right after a type must be whitespace-separated and
  can't start with `|`/`&`.
- `T[K]` (no space before `[`) is an offset-access type.

A tag phpdoc-parser has no dedicated parser for, whose value opens
with `(`, is validated as a Doctrine annotation argument list, the way
it is there (`@url (image.jpg) ...` is invalid, `@ORM\Column(...)` isn't).
An invalid tag contributes no type at all, matching real PHPStan's
`InvalidTagValueNode`. PHPDoc `//` comments end at their own line, so
URLs in descriptions survive. On piwigo16 this matched all 155 of real
`vendor/bin/phpstan`'s hits with zero false positives, at the time of
the corpus comparison (the Doctrine case was the last one added).

**`@psalm-*`/`@phan-*` tag spellings** (Phase 179). `docblock::parse`
now reads real PHPStan's `PhpDocNodeResolver` spelling set, not just
plain and `@phpstan-` tags, in its precedence order: plain, then
`@phan-`, then `@psalm-`, then `@phpstan-`, each later spelling
overriding. That covers `var`/`param`/`return`/`method`/`template*`/
`type`/`import-type`/`param-out`/`require-*`/`pure`/`readonly`/
`immutable`/`consistent-constructor`, plus renamed spellings
(`@psalm-inheritors` means `@phpstan-sealed`, `@phan-real-return` means
`@return`, `@phan-side-effect-free` means `@pure`, `@phan-read-only`
means `@readonly`, `@phan-extends`/`-inherits` mean `@extends`). The
assert family follows `resolveAssertTags`'s own "first prefix with any
assert wins" rule (`@phpstan-` > `@psalm-` > `@phan-`), so
webmozart/assert-style `@psalm-assert`-only libraries now narrow with
no per-library table. A foreign-prefixed tag whose type doesn't parse
is skipped (the nearest equivalent of `shouldSkipType`).
Psalm-only vocabulary (`@psalm-suppress`, `@psalm-internal`, ...) is
ignored, as is `@psalm-(readonly-)allow-private-mutation`, since the
private-mutation exemption isn't modeled. Four raw-text pre-filters
that matched only some spellings (pure, `template`, template variance,
inline `var`) now match every one; the inline-`var` one had been
silently skipping `@phpstan-var` too. Not yet corpus-verified: expect a
large type-inference shift across `vendor/`.

**`Type::Array` key-type/offset engine** (Phase 50):
`InvalidKeyInArrayDimFetchRule` (reverted once, reattempted successfully
- see Reverted); the `array_key_exists()`-narrows-a-specific-element
equivalent of `HasOffsetValueType`; unsealed-shape (`array{...}`)
tracking with a real `...` sentinel field gating both narrowing and
`offsetAccess.notFound`; `ArrayShape`'s own int-vs-string key precision.

Opt-in array-key cast warnings (`array.keyCast`/`offsetAccess.keyCast`).
Opt-in `constructor.missingParentCall`. Closure `use (...)` capture
checks: an undefined by-value capture, `use ($this)`, `use` of a
superglobal, and a capture name colliding with a parameter. PHPStan-API
vendor-directory check (`is_phpstan_code` requiring the target's own
declaring file to live under `vendor/`).

**Too-wide return/property types, by-ref parameter checks, `@param-out`**
(Phase 45, plus its property-assignment sibling from Phases 166/174): a
new `@param-out Type $name` tag unlocks `return.unusedType`/
`property.unusedType`/`staticProperty.unusedType` (opt-in for protected/
public non-final methods, using real PHPStan's own trinary "only a
definite no flags a member" comparison, not a boolean one) and
`parameterByRef.type`/`.executionEnd`/`.tooWide` built on a genuine
per-exit-point accumulator, not a flat "ever assigned" union. Destructuring
targets and `foreach`-rebind coverage included. The property-assignment
sibling needed a genuinely new two-pass (collect-then-consume) project-
wide checking architecture (`Project::collect_property_assign_facts`/
`check_file_with_property_assign_facts`) to resolve a trait-donated
write correctly regardless of file-processing order - `check_file`
itself is unchanged, so incremental/LSP use pays no cost.

**Collector/cross-file aggregation architecture** (Phase 82, fully
closed): real PHPStan's own `Collector` machinery reconciles results
across parallel worker processes, a problem this single-process tool
doesn't have - every real consumer is better served by a registry-
build-time fixed point (the purity model above) or this project's own
per-host trait re-check dedup. `NamedArgumentParameterMethodCallsCollector`/
`OverridingMethodRenamesParameterCollector`'s own real consumer
(`argument.parameterRenamedInSubtype`) is built as a genuine whole-
project fact merge, opt-in.

A cluster of small, mostly opt-in checks with no direct real-PHPStan
precedent, or ported from `phpstan-strict-rules`: "did you mean?" tips
on `method.notFound`; boolean-trap detection (a bare literal passed
positionally into a bare-`bool` parameter); `@see` tag target
validation; duplicate-operand detection (`$x === $x`, `$a - $a`);
constructor-property-promotion suggestion; "only booleans in a
condition" family (mirroring `phpstan-strict-rules`' `BooleansInConditions`);
strict `checkDynamicProperties` in `isset`/`empty`/`??`/`??=`; swapped-
adjacent-argument-name detection; `never`-return-type suggestion;
missing-`default:`-arm detection (Phase 147); missing-exception-message detection;
base-Exception/Error-class-thrown detection; discarded-`new`-exception
detection; assertion-less ("risky") PHPUnit/Pest test detection.

**Whole-program diagnostic-emission pipeline**, built as the
prerequisite for every whole-program check below (Phase
92): `Project::check_whole_program` resolves each finding through
`decl_locations` (no longer gated behind `retain_source`) and applies
the same per-line suppression an ordinary diagnostic gets; the real
per-file Tier-2 use-site index a whole-program check needs is folded
into the existing primary parallel check pass rather than doubling
total check time. Its first consumer, **dead-enum-case detection**
(`enumCase.unused`), flags every enum case never referenced via
`EnumName::CaseName`, a `match`/`switch` arm value, or a literal
`::from()`/`::tryFrom()` argument - a `::cases()` call or a dynamic
member access on the enum escapes the whole enum, conservatively.
**Dead-function/dead-class detection** (`function.unused`/`class.unused`,
Phase 86's B2) is built on `Project::use_sites`' own canonical,
namespace-resolved Tier-1 index (not the raw-string `Project.dependents`
map, confirmed too imprecise). **Namespace-level circular-dependency
detection** (`namespace.circularDependency`, 86's B3) reads directly off
`Registry.classes`' own already-resolved `extends`/`implements`/`uses`
edges and runs Tarjan's SCC - trades completeness (a cycle formed only
through method-body calls is out of scope) for zero false edges.
**Dead-abstract-class detection** (`class.deadAbstract`, Phase 93) flags
an `abstract class` with zero concrete descendants anywhere, a permanent
structural fact. **Whole-application unused protected/public member
detection** (`method.unused`/`property.unused`/`classConstant.unused`,
86's B1, closing out Phase 86): a real escape-hatch mechanism for
dynamic member access (a literal-string dynamic name now records a real
reference instead of escaping; a computed name escapes every class the
receiver's type could reach), plus exclusions for PHP magic methods, any
method overriding a real ancestor's own same-named method (generalizing
past an initial hand-maintained SPL-interface table), and PHPUnit/Pest
test-method/lifecycle-callback recognition - each exclusion tier was
added after measuring real Piwigo noise, not assumed up front (3807 →
2506 → 193 findings across the three iterations).

**Doctrine ORM support** (Phase 168, all sub-items a-k
built). Entity metadata resolution from `#[ORM\Entity]`/`#[ORM\Id]`/
`#[ORM\*ToMany]`/`#[ORM\*ToOne]`/`#[ORM\Table]`/`#[ORM\Column]`
attributes, reusing each mapped property's already-resolved native type
rather than a from-scratch metadata resolver. Symbolic `QueryBuilder`
fluent-chain resolution (`select`/`addSelect`/`from`/`join`/`innerJoin`/
`leftJoin`, including a chain split across a variable assignment via a
`condition_aliases`-style replay). A small hand-rolled DQL select-item
parser (bare alias, dotted field, `IDENTITY(...)`, the five aggregate
functions, `CASE WHEN`, `NEW Dto(...)`, a custom registered function, a
nested scalar subquery) and a result-shape builder mirroring real
Doctrine's default `HYDRATE_OBJECT` hydration exactly (aggregate
function return types transcribed directly from `phpstan-doctrine`'s own
walker). `getQuery()`/`getResult()`/`getOneOrNullResult()`/
`getSingleResult()`/`execute()`/`toIterable()`/`getArrayResult()`/
`getSingleColumnResult()`/`getSingleScalarResult()`/`getScalarResult()`
all wired to real row-shape inference, plus direct `EntityManager::
createQuery($dql)` sharing the identical select-item grammar. Custom
DQL function return types via config. Entity inheritance
(`#[ORM\DiscriminatorMap]`) produces a union across every mapped
subclass for a polymorphic base-entity select.

**Live SQL query reflection** (a new `[sql_reflection]` config block,
absent by default with a byte-identical corpus diff confirmed at every
sub-phase). Names a live database connection (MySQL/Postgres/SQLite,
each via a real driver crate) and/or a static per-engine DDL dump
(parsed via `sqlparser`). A `Doctrine\DBAL\Query\QueryBuilder` chain
folds into one literal SQL string; resolution tries the static schema
catalog first, falling through to a live, lazily-connected reflector
that `PREPARE`s (never executes) the resolved SQL and reads real
column metadata - per-engine placeholder syntax is rewritten for each
driver's real `PREPARE` grammar (`?`/`:name`/`$N`), column types are
translated per real per-engine semantics (`ENUM` to a literal-string
union cross-referenced from the static catalog even on the live path;
`TINYINT(1)` to `Bool`; SQLite's `NUMERIC` affinity to `Int|Float`).
Wired into `Connection`/`QueryBuilder`'s real terminal methods, with a
separate parameter-validation check cross-referencing every real
placeholder against `setParameter`/`Connection`'s own bound values,
including the `ArrayParameterType::*` `IN (...)`-list exemption. Views,
CTEs, derived tables, and `UNION` resolve correctly on the live path for
free (the real engine's own query planner does the work); the static-
schema-only path needed its own derived-table/CTE resolution. Also
covers `COUNT(...)` refinement to `int<0, max>`, a second connection-
independent parameter-type-validation source (`ParameterType::INTEGER`
bound to a literal non-numeric string), session-init SQL for parity with
an app's own connection setup, and a schema-drift check comparing a
static catalog against a live connection's real columns at startup. A
Doctrine entity's own already-resolved native property type is preferred
over a fresh DB-column-type translation whenever it's a genuine
narrowing.

Opt-in call-site function/method-name-case checking, plus a real
case-insensitive-method-call bugfix found along the way (`lookup_method`/
`lookup_method_with_owner` were exact-spelling-only, unlike class/
function lookups). **PCRE pattern-validity checking** (Phase 46): a new
`pcre2-sys` FFI dependency (gated behind a default-on Cargo feature)
validates every real `preg_*` pattern argument (`regexp.pattern`), plus
the no-closing-delimiter case and version-gated delimiter/modifier
wording - fixed a real shared bug along the way in leading-whitespace-
tolerant delimiter detection (a real PCRE behavior, not a corner case,
given PHP 7.3+ flexible heredoc indentation).

**`@internal`'s remaining PHPDoc/attribute surface, and `restricted_
symbols`** (Phase 67): the `@internal` engine now covers every PHPDoc-
tag class-name position and attribute usage too. A new, separate
declarative `restricted_symbols` config (mirroring `spaze/phpstan-
disallowed-calls`) bans a specific function/method/static-method/
property/class-constant/class project-wide with a custom message,
opt-in, including its own real filter DSL: path-scoped `allow_in`/
`allow_except_in`; class/method-attribute-scoped allow rules; `allowInUse`
for a plain `use Foo;` import; `allowInClassWithMethodAttributes`; and
`allowInParamTypes`/`ReturnType` (materially bigger than the rest - these
*invert* an entry's own default at exactly one type-hint position). The
parameter-value-conditions filter family (`allowParamsAnywhere`/
`allowParamsInAllowed` and their `disallow`/`ParamFlags` variants,
matching a call's own argument value/type/class-name pattern or a
bitmask), plus the `allowInMethods`/`allowInFunctions`/`allowInInstanceOf`
zone directives those need.

Class-specific `ReflectionMethod<T>`/`ReflectionProperty<T>` modeling
(Phase 77, a synthetic `@template-covariant TClass` stub patch, plus a real
`Type::ReflectedMember` for a specific reflected member's own value
type). `DOMDocument::createElement()`/`SimpleXMLElement` validation
(XML `Name`-production tag-name checking, XML well-formedness checking
via a new `roxmltree` dependency). Constants declared inside a trait
need PHP 8.2+. Three PHP-8.x-version-gated cast diagnostics
(`cast.unset`/`.voidNotSupported`/`.void`/`.deprecated`).
`InvalidAssignVarRule` (Phase 135), all three cases (`nullsafe.assign`/`.byRef`/
`assign.invalidExpr`) - needed a genuine new `ExprKind::AssignRef`
variant, threaded through every real consumer. `BacktickRule`
(`backtick.deprecated`). `PipeOperatorRule` (`pipe.byRef`, resolved
against the right-hand side's own literal syntax rather than a full
parameter-shaped callable type).

**`function.alreadyNarrowedType`/`function.impossibleType`**: a single-
argument type-check call (`is_string($x)`, `is_int($x)`, ...) whose
outcome is already statically decided, scoped to exactly the predicate
table the narrowing engine already trusts (real PHPStan's own
additional `in_array()`/`method_exists()`/`assert()` reasoning isn't
attempted). Reuses the same disjointness/subtyping primitives
`assert.impossibleType`/`.alreadyNarrowedType` already established for
the PHPDoc-tag version of this idea. **`and`/`or` word-operator
identifier fidelity**: a new `BinOp::LogicalAnd`/`LogicalOr` (parsed
only by the word-keyword precedence tier) lets `booleanAnd`/`logicalAnd`
report under their real, distinct identifiers/message sigils, with
every purely-semantic consumer (narrowing, `infer_binary`) widened to
treat both spellings identically. **`parameter.defaultValue`**/
**`property.defaultValue`**: a parameter's/property's own default value
must be assignable to its declared type, resolving a bare top-level
`@template`-typed parameter to its own bound first, and never
reporting a default of unknown (`mixed`) type, which real PHPStan's
direct `accepts()` call always accepts; the property check
replicates real PHPStan's own untyped-property `null`-default exemption
exactly. **`parameter.phpDocType`/`return.phpDocType`/
`parameter.notFound`/`parameter.notByRef`** (`IncompatiblePhpDocTypeRule`,
level 2, `checker/phpdoc_type_compat.rs`): a function/method's own
`@param`/`@param-out`/`@return` type must fit its native hint -
"incompatible" when no value satisfies both, "not subtype" when only
some do - via a trinary, hierarchy-aware `Registry::is_super_type_of`
(unions, templates by bound, conditionals by branch, benevolent unions
by member, final/interface-aware class relations, `No` only with the
whole hierarchy indexed); a tag naming no parameter, or `@param-out` on
a by-value one, is reported too, in tag order. Matches real PHPStan
47/47 on the Piwigo corpus. `property.phpDocType` applies the same to a
property's `@var` (a promoted property's constructor `@param`/inline
`@var`, its "PHPDoc type"). **`parameter.unresolvableType`/
`return.unresolvableType`/`property.unresolvableType`**
(`checker/phpdoc_unresolvable.rs`) decide on the tag's parsed type node
where PHPStan's `TypeNodeResolver` yields an `ErrorType` or implicit
`never`: an invalid offset access (`string[optional]`), an unimported
`Closure(...)` in a namespace (still typed as the global `Closure`),
a missing class or constant in `Foo::BAR`/`Foo::X_*`, `self`/`static`/
`parent` with no class, an empty `int<5, 1>`, a disjoint `int&string`,
too many built-in generic arguments, and an unknown hyphenated keyword.
Matches real PHPStan 55/55 on the Piwigo corpus. `closed-resource` is
its own type, disjoint from an open `resource` (`is_resource()` rejects
it), and class constants and enum cases keep declaration order
(`IndexMap`), so a `match` remainder and a `Foo::X_*` union list them as
PHP's own `getConstants()` does. **`assert.unresolvableType`** and the
missing-type checks on an asserted type complete `AssertRuleHelper`;
**`varTag.unresolvableType`** covers every statement-level `@var`,
reported on the comment's own line (`Stmt::doc_line`).
**`WrongVariableNameInVarTagRule`** is built in full:
`varTag.differentVariable`/`.variableNotFound`/`.noVariable`/
`.multipleTags` for assignments (including destructuring), `foreach`,
`static`, `global`, `return`/`throw` and every other statement, with
superglobals and a script's top-level `$argv`/`$argc` counted as
defined, and `varTag.misplaced` above a class, interface, enum,
function or method. Every doc comment directly above a statement is
read, not only the last (`doc_comments_all`), and a bare `$` in a
PHPDoc (`$$x`) is no variable name. **`VarTagTypeRuleHelper`** and
**`VarTagChangedExpressionTypeRule`** check every value a statement's
`@var` tags describe - an assignment (and each destructured item), a
`foreach` subject/key/value, a `static` default, a `return`/`throw`
expression, and an existing variable a tag re-types: `varTag.nativeType`
against the value's native type (literals only have to overlap, an empty
array widens to `array`), then `varTag.type` when the tag shares no value
with the inferred type (or, for a PHPStan `Type`, isn't a subtype, plus
`phpstanApi.varTagAssumption`). Native types come from
`Checker::native_expr_type` and a per-variable native map on `Locals`
(parameters' hints, assignments; cleared by any rebind, by-ref argument
or by-ref closure `use`; unioned at merges) - `mixed` whenever unknown,
so it can only miss. `varTag.trait` and `class.notFound`/`class.prefixed`
cover the tags of every statement. Every named tag on a statement is
applied (not only the last), a `foreach` key tag types the key, and
`@psalm-`/`@phan-` typed tags parse with the same grammar as `@phpstan-`
ones.
**`IncompatiblePropertyHookPhpDocTypeRule`** checks a hook's own
`@param`/`@param-out`/`@return` against its native signature (a `set`
hook's explicit or implicit `$value`, returning `void`; a `get` hook
returning the property's type). **Generic callables** -
`callable<T of B = D>(T): T`, `Closure<...>(...)`, and the
`pure-callable`/`pure-Closure` spellings - parse in both grammars and
keep their templates (`CallableSig::templates`, scoped
`CALLABLE_TEMPLATE_SCOPE`, never substituted or bound by an enclosing
signature); calling one binds them from the arguments
(`CallableSig::instantiated_return`). **`GenericCallableRuleHelper`**
validates those templates like `@template` tags (existing class/alias
name, unsupported bound, default outside its bound, required after
optional) and reports `callable.shadowTemplate` against the function's,
method's or class's templates, on `@param`/`@param-out`/`@return` and a
property's `@var`. A plain function's own `@template` tags get the same
declaration checks a method's always had.
`missingType.iterableValue` reads "iterable type array" exactly as real
PHPStan's descriptions do. A second declaration of a top-level function
(another file, a polyfill branch, a merged overload) is checked against
its own signature: the registry records which declaration fills each
function slot (`function_decl_sites`), and any other declaration is
rebuilt and finalized on its own (`Registry::finalize_standalone_sig`).
**`conditionalType.subjectNotFound`/`.alwaysTrue`/`.alwaysFalse`** and
a conditional's unknown-parameter `parameter.notFound` validate each
conditional type a declaration's own PHPDoc writes. A template-subject
conditional (`(T is 'array' ? A : B)`) is now modeled, not collapsed to
its branches: it resolves once `T` is bound, by a call's inference or a
class's type arguments, falling back to the template's declared default
on an erased receiver. `Registry::is_super_type_of` compares arrays and
shapes structurally (optional vs required keys), keeps `string` a
`Maybe` for `class-string`, and treats an unresolved constant reference
or unknown class name as proving nothing. **`@phpstan-self-out`/
`@phpstan-this-out`** (and the `@psalm-` spellings) are real: after
`$x->method(...)`, `$x` (or a narrowable `$this->prop`) takes the tag's
type, its templates bound as for a return type; `selfOut.type`/
`.static`/`.unresolvableType` validate the declaration. `self<T>`/
`static<T>` keep their type arguments (resolved to the class with those
arguments) instead of dropping them, and a bare prefixed-only tag
(`@assert`, `@self-out`, `@type`, ...) no longer applies. **`function.alreadyNarrowedType`** answers
"always true" through that strict relation, so `is_string()` on a
benevolent `(int|string)` key is no longer flagged. **`identical.alwaysFalse`/`.alwaysTrue`, `notIdentical.
alwaysFalse`/`.alwaysTrue`**: a `===`/`!==` comparison whose outcome is
provably constant, via a new shared `registry::identical_result`
primitive - two operands reducing to the same single finite value decide
both directions via `Type::eq`; otherwise a new `registry::types_
provably_disjoint` decides only the "always false" direction (real
PHPStan's own `isSuperTypeOf` is trinary, a "provably no overlap"
answer plain `is_assignable` can't give - disjoint on a scalar-family
mismatch, or two unrelated classes/enums, neither an ancestor of the
other, both closed `ClassKind::Class`/`Enum`, never `Interface`/`Trait`).
**`return.empty`**: a bare `return;` against a return type that requires
one - this project's own pre-existing check for this shape was reporting
under the wrong identifier *and* the wrong condition (gated on whether
`null` happened to be assignable, silently exempting every nullable/
union return type); both fixed, now matching real PHPStan's own
`Type::isVoid()`-based condition exactly.

**Receiver-type-checks phase**: nine real-PHPStan identifiers for "this
receiver's own type provably can never support the operation at all" -
`method.nonObject`/`property.nonObject`, `offsetAccess.
nonOffsetAccessible`, `foreach.nonIterable`, `offsetAccess.nonArray`
(destructuring), `clone.nonObject`, `callable.nonCallable`, and the two
spread/unpack shapes `arrayUnpacking.nonIterable`/`argument.
unpackNonIterable`. Deliberately stricter than real PHPStan's own
filtered-Union default (a `Union` is flagged the moment *any* member is
confirmed closed) - confirmed via a follow-up corpus diff to produce
zero false positives on piwigo16.

## Performance & tooling

CI-friendly output formats (`json`, `checkstyle`, `junit` - a failing test case per diagnostic and a passing one per clean file) and a `friendly` terminal format (each diagnostic framed by its surrounding source lines, span underlined, code syntax-highlighted on a colour terminal). Baseline suppression
(`--generate-baseline`/`--baseline`, on by default reporting an
unmatched baseline entry): entries record their occurrence count and
absorb at most that many (reporting "expected N, found M" when fewer
occur), an opt-in identifier-anchored match mode
(`baseline_match_identifier`) keeps an entry matching after its message
is reworded, and `baseline_per_identifier` writes one file per
identifier behind an `includes` index and inline `@phpstan-ignore-line`/`-next-line`/
`<identifier list>` suppression, read from real comment tokens with
upstream's placement rules and identifier-list grammar; a malformed list
is `ignore.parseError` (non-ignorable, upstream's messages), and opt-in
`report_unmatched_ignored_errors` reports ignores that suppressed
nothing (`ignore.unmatchedLine`/`ignore.unmatchedIdentifier`, traits
exempt), applied once after whole-program checks and level filtering. `ignoreErrors:` config support.
`--level`/`--php-version`-gated portability checks (`PhpFeature::
NoncapturingCatch`/`ThrowExpression` are declared with the correct
threshold but not yet wired to a diagnostic - a real, small residual).
Incremental multi-file `Project` with cross-file dependency invalidation.
An 8MiB worker stack, persistent structurally-shared `Locals` (O(1)
clone), `FxHashMap` for hot build-once maps, a criterion regression-
guard benchmark, and `hotpath`, an opt-in in-process instrumentation
profiler. `check-project`'s own parse and check phases both run on a
shared rayon thread pool, files pre-sorted largest-first to shrink the
straggler tail. Class-hierarchy member lookups are memoized (Phase 95's
C3): per-class method/property/constant owner maps
(`method_owner_by_class` and siblings) and each class's full ancestor
set are precomputed once the class graph freezes, so
`lookup_method`/`lookup_property`/`lookup_const`/`is_subclass_of` no
longer re-walk the ancestor BFS on every call.

**Composer/dependency hygiene** (opt-in `check_composer_hygiene`,
`project/composer_hygiene.rs`, reading the root `composer.json` and
`vendor/composer/installed.json` through `composer/manifest.rs`), none
of which PHPStan has: every class/function reference in first-party
code is resolved to the installed package declaring it -
`undeclared.composerDependency` (a package only installed transitively),
`misplaced.composerDependency` (a `require-dev` package used from
production-autoloaded code), `devOnly.composerDependency` (a `require`
package only code autoload-dev loads uses), `unused.composerDependency`
(a `require`/`require-dev` entry nothing references, class-name strings
counting as use, symbol-less package types, packages shipping binaries,
and packages registered with Pest/PHPStan/Psalm/Laravel auto-discovery
exempt), with `suggest` entries and
`class_exists()`-probed uses accepted; `misplaced.autoloadDevSymbol`
(production code using a class only `autoload-dev` loads);
`psr4.pathMismatch`/`psr4.caseMismatch` (a class declared where its
longest PSR-4 prefix doesn't map it, or only matching up to letter case,
`classmap`/`files`/`exclude-from-classmap` respected);
`provide.unimplemented` (the PSR virtual packages, the multi-interface
http-message/http-factory ones requiring every interface);
`ext.notRequired` (a built-in whose phpstorm-stubs directory is an
optional extension - the build now records each stub's extension - with
no `ext-*` requirement, `suggest`, required polyfill, or
`extension_loaded()`/`function_exists()` probe);
`composerScript.notFound`/`composerScript.notPublicStatic` for
`Class::method` scripts; and `composer.platformPhpInconsistentWithRequire`
through a Composer constraint evaluator (`composer/constraint.rs`).
Vendor-context discovery follows a path repository's `vendor/<name>`
symlink out of `vendor/` instead of dropping it, promotes path-repository
packages to analysed files, and runs once per analysed directory's own
nearest `composer.json` root rather than only the working directory's.

**Declarative architecture rules** (`[[architecture]]` in the config,
`architecture.rs`/`project/architecture.rs`, the `carlosas/phpat`
model): selectors over namespace/class-name patterns (prefix or
`/regex/`), `extends`/`implements`/`uses_trait`/`applies_attribute`, and
abstract/final/readonly/interface/trait/enum/exception/attribute/
standard-class flags, each with nested `excluding`; relations
`should_not_depend_on`/`can_only_depend_on`/`should_depend_on`,
`should_not_construct`, `should_(not_)extend`/`implement`/`use_trait`,
`should_be_final`/`abstract`/`readonly`, `should_be_named`,
`should_apply_attribute`, and `should_have_only_one_public_method`. A
class's dependencies are the classes its declaration and body name plus
every class in its members' PHPDoc-refined types, built-ins excluded;
each forbidden one is reported where it's used.

**Construct and type bans** (`banned_constructs`/`banned_types`,
`banned.rs`/`project/banned.rs`, the `ergebnis`/`strict-rules`/
`roave/no-floaters` families): `eval`, `isset`, `empty`, `compact`,
`extract`, `exit`/`die`, `@` (recorded from tokens, since the parser
drops it), `switch`, `goto`, `global`, named arguments, `=&`,
by-reference parameters and returns, `?:`, `==`/`!=`, `$a[] =`,
backticks, `@phpstan-ignore` comments (non-ignorable), superglobals,
variable variables, and dynamic member names - each `banned.<name>`; a
banned type (`float`, other native types, or a class) in the
PHPDoc-refined signatures of functions and methods, property types and
typed constants, plus float literals and `(float)` casts for `float`
(`banned.type`). `restricted_symbol_presets = ["multibyte-strings"]`
adds the twelve byte-oriented string functions with `mb_*`
counterparts to `restricted_symbols`.

**Closed-by-default class shape** (opt-in `check_closed_classes`,
`project/class_shape.rs`, `ergebnis`' `FinalRule`/
`PrivateInFinalClassRule`/`FinalInAbstractClassRule`): `class.notFinal`,
`method`/`property`/`classConstant.protectedInFinalClass` and
`method.notFinalInAbstractClass` - skipping, from the closed-world
registry, a class something extends, a method a subclass overrides, a
member the parent declares, and Doctrine-mapped entities.
**Parent-call requirements** (`[[parent_calls]]`: class, method,
first/last/any/never invocation, yes/no/maybe content; built-in
PHPUnit `setUp`/`tearDown`/`setUpBeforeClass`/`tearDownAfterClass`/
`assertPre`/`PostConditions` and Codeception `_before`/`_after` entries
on by default, `default_parent_calls = false` to drop them), each
requiring the call only when the overridden implementation isn't
empty: `parentCall.missing` (with a fix inserting the call when the
parent needs no arguments), `.notFirst`, `.notLast`, `.forbidden`,
`.noContent`, `.extraContent`.

**`php-language-extensions` access attributes**
(`registry/access_attributes.rs` into `ClassInfo::access`/
`method_access`, enforced by `checker/access_attributes.rs`):
`#[Friend(...)]` (`attribute.friend`: only listed classes may call a
method or construct the class), `#[NamespaceVisibility]`/`#[Package]`
(`attribute.namespaceVisibility`), `#[TestTag]` (`attribute.testTag`:
only test code - a PHPUnit/Codeception test class, a `Test(s)`
namespace segment, a `test(s)` directory or `*Test.php` file),
`#[RestrictTraitTo(...)]` (`attribute.restrictTraitTo`),
`#[InjectableVersion]`/`#[CheckInjectableVersion]`
(`attribute.injectableVersion`, element types of collections
included), `#[Sealed([...])]` feeding the `@phpstan-sealed` check, and
`#[MustUseResult]` feeding the discarded-result check; a class may
always use its own members. (The package-boundary visibility item is
Phase 94 P2's.)

**Assertion and class-argument-return tables** (`assertion_tables.rs`,
`registry/assertion_table.rs`): `[[assertions]]` entries (function or
class+method, parameter, type with `!` for negation, always/true/false)
become `@phpstan-assert`-family infos on the named declaration, applied
once per registry rebuild; built-in entries cover Nette Tester's
`Assert`. `[[class_argument_returns]]` entries make a method return the
class its argument names (`Foo::class` or a string naming a known
class), optionally intersected; built-in entries cover Mockery's
`mock`/`spy`/`namedMock`/`instanceMock` (`&Mockery\MockInterface`). A
container's `get()` is left to configuration, since PSR-11 promises
nothing about what an id resolves to.

**PHPUnit mock hygiene** (opt-in `check_mock_hygiene`,
`project/mock_hygiene.rs`, the `rector/mockstan` family):
`mock.expectsAny`, `mock.atLeastZero`, `mock.withWithoutExpects`
(argument constraints on a stubbed method are never verified),
`mock.conflictingReturns`, `mock.methodWithoutExpects` (a variable or
`$this` property assigned from `createMock`/`createPartialMock`/
`getMockBuilder()->getMock()`, not `createStub`, configured without
expectations), `mock.unionPropertyType` (native or `@var` union of a
mock and a real class), `mock.onlyMockProperties`, and
`mock.bannedClass` for `mock_banned_classes`.

**Exception hygiene** (opt-in `check_exception_hygiene`, the Nette/
`thecodingmachine`/`shipmonk` family): `catch.previousNotPassed` (a
`throw new X(...)` inside a `catch` that never passes the caught
variable, when `X`'s constructor accepts a previous exception; nested
catches are checked on their own) and `catch.baseWithoutRethrow` (a
catch of `Throwable`/`Exception`/`Error` with no unconditional
top-level `throw` in its body).

**Null-coercion and enum-exhaustiveness idioms** (`checker/
strict_idioms.rs`, the `rector/type-perfect`/`shipmonk` families).
Opt-in `check_null_coercion`: `isset.object`/`empty.object` (on a
variable or property holding an object - a null check or `instanceof`
says what's meant; an offset probe is exempt) and possibly-null values
PHP silently coerces - `interpolation.nullable`, `concat.nullable`,
`binaryOp.nullable`, `assignOp.nullable`. Opt-in
`check_enum_exhaustiveness`: `match.enumDefault`, `enum.switch`, and
`enum.ifChain` (two or more `===` comparisons of one subject against one
enum's cases). Member access on `mixed` is the level-9/10
`method`/`property.nonObject` family, and indexing a plain object is
`offsetAccess.nonOffsetAccessible`.

**`phpstan-strict-rules` ports** (`checker/strict_rules.rs`). Each
family has its own opt-in flag, like upstream's per-rule toggles:
- `check_illegal_constructor_calls`: `constructor.call`, which also
  covers Psalm's `DirectConstructorCall`. A static `X::__construct()`
  passes only from a constructor, or from a trait method some class
  imports as its constructor, calling a parent class's.
- `check_dynamic_static_calls`: `staticMethod.dynamicCall`, including
  first-class callables.
- `check_loop_variable_overwrite`: `foreach.keyOverwrite`,
  `foreach.valueOverwrite` and `for.variableOverwrite`, through
  destructuring.
- `check_switch_case_types`: `switch.type`.
- `check_strict_function_calls`: `function.strict`, for
  `in_array`/`array_search`/`array_keys`/`base64_decode`, plus
  `iterator_to_array()` leaving `$preserve_keys` to its default.
- `check_strict_array_filter`: `arrayFilter.strict`; a callback that may
  be `null` is a level 8 finding.
- `check_useless_casts`: `cast.useless`.
- `check_implicit_array_creation`: `variable.implicitArray`. It says
  "might not exist" when the function assigns the variable earlier, or
  anywhere within a loop enclosing the assignment.
- `check_arithmetic_operands`: the `OperatorRuleHelper` family,
  `plus.leftNonNumeric` through `pow.rightNonNumeric`, `unaryMinus`/
  `unaryPlus` and `preInc`/`postInc`/`preDec`/`postDec` `.nonNumeric`.
  GMP and `BcMath\Number` count as numbers under `++`/`--`.

The remaining strict-rules families already exist under other flags:
booleans in conditions, `banned_constructs` for empty, short ternary,
loose comparison, backticks and variable variables, and
`constructor.missingParentCall`, `method.nameCase` and
`closure.useThis`.

**Operand and comparison policies from other rule packages**, each
opt-in:
- `check_comparable_types` (`shipmonk`'s
  `AllowComparingOnlyComparableTypesRule`): `comparison.nonComparable`/
  `comparison.differentTypes` for `< > <= >= <=>`.
- `check_mixed_type_operands` (`voku`'s `ExtendedBinaryOpRule`/
  `ExtendedAssignOpRule`): `binaryOp`/`assignOp` `.nonStringOperand`/
  `.nonArrayOperand`.
- `check_loose_haystack_comparison`: `function.looseComparison`, a
  loose `in_array`/`array_search`/`array_keys` where some needle/value
  type pair can be `==` without being `===`. It is a superset of
  `voku`'s `InArrayLooseComparisonRule`, and the PHP 7 int-vs-non-numeric-
  string flip only counts when the configured version is below 8.0.
- `check_identity_vs_equals` (`gosuperscript`): `identical.equalsMethod`,
  `===`/`!==` between objects whose class defines a public `equals()`
  accepting the other side; the class's own `equals()` is exempt.
- `check_native_type_hints`: `missingNativeType.parameter`/`.return`
  suggest the tightest native type a PHPDoc-only type allows under the
  configured PHP version. Without a configured version, only PHP 7.1
  types are suggested. Suggestions that would break the declaration are
  left out: untyped overridden parameters, overriding methods lacking a
  compatible return type, incompatible defaults, magic methods. A
  closure parameter whose type the callee fixes gets the same
  suggestion. From PHP 8.3 an untyped class constant gets one too
  (`missingNativeType.classConstant`), unless a subclass redeclares it
  outside the suggested type.
- `check_magic_numbers` (`sidz/phpstan-rules`): the `magicNumber.*`
  family covers numeric literals (also under unary `+`/`-`, a `(string)`
  cast, and numeric strings unless `magic_numbers_ignore_numeric_strings`)
  in 13 positions: call and `new` arguments (not `define()`'s), property
  and parameter defaults, arithmetic/bitwise/comparison/logical
  operands, `return`, assignment, ternary branches, and `switch`/`match`
  subjects and conditions. `magic_number_exemptions` (default `[0, 1]`)
  is compared by value, sign included, which upstream's string
  comparison gets wrong for negative numbers.

**PSR-3 logger calls** (`checker/psr3.rs`, from `staabm/phpstan-psr3` and
`sfp-phpstan-psr-log`). They apply to `Psr\Log\LoggerInterface`
receivers and Laravel's `Log` facade.
- Opt-in `check_psr3_logging`:
  - `psr3.interpolated`: a concatenated or interpolated value that isn't
    a number, bool, literal string or enum. For a variable, its native
    type is used.
  - `psr3.messageNotStatic`.
  - `psr3.placeholderDoubleBraces`/`psr3.placeholderInvalidChar`.
  - `psr3.placeholderMissingContext`/`psr3.placeholderMissingKey`.
  - `psr3.placeholderNotStringable`: a context value a placeholder
    interpolates that has no string form.
  - `psr3.invalidLevel`, `psr3.contextKeyNotString`, and
    `psr3.contextType` (an `exception` value that isn't a `Throwable`).
  - `psr3.contextKeyPattern`, against `psr3_context_key_pattern`.
- Opt-in `check_psr3_exception_context`: `psr3.contextMissingException`,
  a Throwable variable in scope but no `exception` context key.

Context keys without a placeholder are not flagged: PSR-3 lets context
carry arbitrary extra data.

**Implicit current-time dependence** (`checker/current_time.rs`, opt-in
`check_current_time_dependence`, from `slam/phpstan-extensions`):
- `currentTime.time`: `time()`/`microtime()`.
- `currentTime.defaultTimestamp`: `date`, `gmdate`, `idate`, `getdate`,
  `localtime`, `strftime`, `gmstrftime` or `unixtojd` without their
  timestamp or with `null`, and `mktime`/`gmmktime` with fewer than six
  components.
- `currentTime.relative`: `strtotime()` without a base, `date_create()`
  and `new` of any `DateTimeInterface` class with no date or a relative
  one. "Relative" follows `php_date_parser::depends_on_current_time`
  (keywords, `+1 day`, `next monday`, `first day of ...`, a bare time of
  day), not upstream's six fixed words.
- `currentTime.createFromFormat`: a format with no `!`/`|`/`U`, whose
  unparsed fields come from now.
- `currentTime.requestTime`: `$_SERVER['REQUEST_TIME']` and
  `REQUEST_TIME_FLOAT`.

**Ini directives** (`checker/ini_directives.rs`): `ini.removed`
(default on once `php_version` is set) and, under
`check_deprecated_usage`, `ini.deprecated`, for `ini_get`/`ini_set`/
`ini_alter`/`ini_restore`/`get_cfg_var`. The table follows php-src
`UPGRADING` 7.2-8.5, and directives deprecated only for changing
(`assert.*` in 8.3, the 8.4 session settings) are flagged only in
`ini_set`/`ini_alter`.

**LSP hover/go-to-definition** (Phase 17): a real Content-Length-framed
JSON-RPC server. Every name reference resolves without receiver-type
inference via a `decl_locations` index; an instance receiver's own type
resolves via a throwaway per-request `Checker` re-run matched by exact
span equality, correctly reaching inside closures/arrow-fns and
resolving every class a `Union`/`Intersection` receiver's member could
come from. Local-variable-declaration navigation is backed by real,
checker-driven reaching-definitions (`Locals::provenance`, populated at
every genuine syntactic binding site and merged through the same
`merge_scopes`/`converge_loop_body` machinery the checker already
trusts, so every branch-merge/loop/termination-pruning nuance is
inherited for free).

**LSP completeness**: `textDocument/codeAction` fix wiring, with
fixes populated for (and `check-project --fix` applying): every
`*.nameCase` mismatch (same-width rewrite keeping the written
qualification, a call-site method name located within the call by a
`within` fix resolved against the source), a `#[\Override]` overriding
nothing (removing its attribute group, or just its entry in a shared
group), a wholly unused private constant/method/property (deleting the
declaration with its doc comment and attributes, whole lines when it
has them to itself, only when it isn't sharing a declaration), and a
missing `parent::__construct()` when the parent constructor needs no
arguments; whole-project `textDocument/references`/`rename` backed by a real
two-tier use-site index (Tier 1 syntactic, kept live; Tier 2 needs
receiver-type resolution, built lazily); `signatureHelp`; `workspace/
symbol`; `documentHighlight`; `semanticTokens` (unconditional `@internal`/
`@deprecated`-usage highlighting); `inlayHint` (parameter-name and
inferred-local-type hints); `typeDefinition` (jumps from an inferred
type to its own class declaration(s)); `documentSymbol` (a per-file
hierarchical outline); member/static-access/class-name completion.
**LSP server hardening and the VSCode extension client**: trustworthy
workspace-wide diagnostics resolving a project's own real config before
loading; non-blocking concurrency with debounced rechecking and stale-
result dropping via a per-URI generation counter; full protocol
completeness (watched-file changes, multi-root workspaces, live config
reload, UTF-8 position encoding, cancellation); a real
`vscode-languageclient` extension under `editors/vscode/` with binary
auto-download/caching and its own test harness and CI workflow. Local-
variable references/rename and a bundled `.vsix` (no esbuild/webpack
yet) remain small, disclosed residuals.

**Fuzz testing infrastructure** (Phase 19): `cargo-fuzz` targets for the
raw lexer/parser pipeline (with a 64MiB worker stack matching
production), a token-stream-level target, a structured-PHP-fragment
generator going through the full `check-project` pipeline, and a direct
`types::parse_type_string` fuzzer - plus a libFuzzer dictionary built
from this project's own token tables. Dev-only, not a CI gate; one real,
still-open crash (a synthetic `Token::InterpString` with garbage
`Position` values) awaits root-causing.

**Extension API, all 3 axes** (Phase 18): declarative `restricted_
symbols` config (axis 1); a public `ReflectionRule` trait over
already-latent-public reflection data, explicitly not parity with real
PHPStan's flow-sensitive `Rule<TNodeType>` (axis 2); a public `trait
Rule` mirroring real PHPStan's own `Rule`/`Scope` pair exactly, with
`wants_expr`/`wants_stmt` filtering and a read-only `Scope` facade over
`Checker`'s flow-sensitive state (axis 3) - compile-time-registered,
same-binary rules only; `dlopen` dynamic loading and NEON-style
registration both stay declined outright.

---

# Pending / partial

Each entry: what's missing, and the root cause blocking it where one's
known. Numbered by the original phase for cross-reference; the number
is not a priority order. Cross-phase dependencies, when a phase's own
entry has one, are named inline in that entry's own text - a separate
dependency graph (`PHASE_DEPENDENCIES.md`) was maintained across two
earlier stretches of this project's development, each time retired once
every edge it tracked became satisfied and reinstated only once a fresh
batch of new phases (158-175, the most recent) produced live edges
again. Retired a second time once Phases 166/169/174 shipped and left
no live phase-to-phase edge behind - the one remaining insight it held
(the two-pass checking architecture Phases 166/174 built, see Done's
own "Too-wide return/property types" entry, is necessary but not
sufficient for general cross-function array correlation) is folded into
that boundary's own entry below instead of a standalone file.

**48 - Symbol-existence pipeline, remaining stretch goal (PARTIAL, small
residual).** The MVP, name-case family, and a fresh 20-file audit of
every real `class.notFound`-reporting Rule class are all done (see
Done) - the audit found the pipeline far more complete than originally
estimated (`instanceof`/`catch`/`@mixin`/`@method`/`@phpstan-type`/
`@property` were all already built), closed the one real gap it found
(inline `@var` existence-checking), and fixed two real, independent bugs
along the way (`array{...}`'s 3 sibling shape kinds - `list{...}`/
`non-empty-list{...}`/`non-empty-array{...}` - mis-parsing as a bogus
class name; a bare `Traversable<T>`/`Iterator<K,V>` PHPDoc tag never
recursing into its own generic argument for case-checking/existence-
checking/type-compatibility purposes alike). `GenericAncestorsCheck`'s
own generic-*argument* existence-checking leniency was re-verified by
actually prototyping and corpus-testing it, not just re-reading its own
doc comment - confirmed still correctly load-bearing (28 of 40 new
findings were genuine soft-dependency/`@psalm-type`/PHP-Scoper-rewrite
false positives the leniency exists to avoid). What remains is smaller
still - a genuine residual, but size-XL/low-priority; go one-by-one only
with a concrete reason to prioritize.

**54 - Generic template richness (PARTIAL, small residual).**
Variance-aware assignability/arity checking, `TemplateTypeCheck`'s
declaration-validity family, `UsedTraitsRule`'s bound/arity/variance
half, `CrossCheckInterfacesHelper`'s cross-path diamond-generic conflict
detection, and `generics.notGeneric` (reversing an earlier decline,
confirmed not to conflict with a project's own configured stub files -
which load as context, never through this checker's own diagnostic
pass) are all done (see Done) - a real merge-precedence bug (an
unscoped run's "analyzed file always wins" rule wrongly overwriting a
correct vendored stub with a same-named third-party one) and 6 real SPL
iterator classes missing their own `@template` tags in *every* available
source were found and fixed along the way. `TemplateTypeVariance::
createStatic()`/`Bivariant` were both re-verified (not carried over
unexamined) to have zero independently observable behavior this
project's model doesn't already produce. Each rule's own real-PHPStan-
specific residual stays open: bound/default existence-checking against
this project's inherently partial registry; `UsedTraitsRule`'s own
separate tag-validity diagnostics and its call-site-variance-never-
allowed rule; a handful of genuinely narrow, real-PHPStan-internal
reflection nuances (a native/internal generic class's own concrete
subclass, one level removed).

**Diagnostic usability gaps (found 2026-09-24, crate-split fixture
review).** Findings that are correct but reported on the wrong line, twice,
or under a shared identifier, plus one false-positive source; the named
fixture shows each one. The pure correctness bugs from the same review are
listed in `CRATE_SPLIT_PLAN.md` (A2, "Known bugs found during A2").
- Wrong line: a property hook's own problems (`propertySetHook.*`,
  `propertyGetHook.*`, its `@param`/`@return`/`@throws` checks,
  `parameter.trait`) are reported on the property, not the hook
  (`property_hooks/*`); a PHPDoc tag error (`phpDoc.parseError`,
  `phpDoc.phpstanTag`) on the declaration, not the tag's line, since doc
  comments carry no span (`phpdoc_parse_errors/*`); `return.missing` on the
  function, not where it falls off the end; `generator.returnType` on the
  function, not the `yield`; an invalid `@use` tag and a missing trait on
  the class, not the `use` statement; a trait-supplied method's override
  problems on the using class, not the trait method; a declaration's span
  starts after its attributes, not at them.
- Reported twice: an asserted unknown class is also "already narrowed"
  (`phpstan_assert_tags/*`); reading any key of `array{}` is not reported
  as a missing offset, but its `null` result then is, as an argument
  (`reading_a_dynamic_key_from_a_freshly_empty_array_infers_null`); a
  strict-arithmetic operand report on top of an already-invalid operation
  (`strict_rules/arithmetic_operands__*`); `return.unusedType` beside a
  return value the return type rejects (`value_of_on_a_backed_enum_...`);
  `property.protected` beside `property.assignByRef` for a reference to a
  protected property; `assign.invalidExpr` beside `nullsafe.assign`.
- Noise: `PHP_VERSION_ID` comparisons are reported always true/false under
  a configured `php_version`, though they are deliberate version guards
  (`php_version_id_reflects_the_configured_php_version_...`).
- Shared identifiers: checks that reuse another check's identifier cannot
  be ignored separately - override-with-different-casing as
  `method.nameCase` (also the call-site case check), XML validity as
  `argument.type`.
- False positive: `PhpParser\Node` and the rest of nikic/php-parser are
  unknown classes to a PHPStan rule author, since php-parser is not a stub
  source (`phpstan_api_method_calls/*`).

---

# Planned (phases 85+)

Every phase from 85 up that isn't built yet. Each one is **PLANNED**
unless its entry says otherwise. A phase (or sub-item) that ships moves
into **Done** and its entry is deleted here, not left in place with a
flipped status tag. Numbers are sequential by source, not by priority.
Pick items the same way any other Pending/partial phase gets picked up.

**Numbering and sources:**
- **85-124** (Parts 1-9) are "beyond parity" findings: concrete,
  source-grounded opportunities for phpstan-rs to do things real PHPStan
  doesn't do, or can't do as well, usually because parity work turned up
  a real architectural reason why not. There's one phase per lettered
  section, and sub-items keep their letter and number (85's own
  "A1"-"A6"). Every file:line citation, into `vendor/phpstan-src/src`,
  `vendor/phpstan-strict-rules/src`, and this project's own `src/`, was
  checked against the vendored and working trees when it was written.
  Line numbers drift, so re-check a citation before relying on it.
  (These phases were once kept in a separate `BEYOND_PHPSTAN.md`. See git
  history for the original numbered-finding text.)
- **125-139** come from a parity audit against real PHPStan's
  `Rules/`, one phase per `Rules/` category. See that sub-section's own
  intro.
- **140** is the long-tail bucket from **Known limitations**' "Known but
  not yet scheduled" list. Its sub-items (140a-140d) live there, not
  here.
- **141-157** come from a one-by-one source read of the top 100
  Packagist `phpstan-extension` packages by download count (see
  `TOP100_PHPSTAN_EXTENSIONS.md`/`TOP100_RESEARCH_NOTES.md`). Genuinely
  new, portable findings got their own phase or became a sub-item on an
  existing one. Single-framework/single-library glue (Symfony, Doctrine,
  Laravel, Drupal, WordPress, TYPO3, CakePHP, Magento, SilverStripe,
  Nette, CodeIgniter, GLPI, Maho, and every single-library
  mock/assert/type-extension package) is named once per recurring theme
  instead of once per package. Each of those ~40 packages was checked
  individually, not judged by its framework's name, and each needs that
  framework's own runtime metadata model, which phpstan-rs has no
  equivalent of.
- **158-175** were a batch of narrowing-engine and corpus-driven
  follow-ups. Everything in it has shipped (see Done) except 173, which
  has its own sub-section. 165 was a duplicate of
  already-shipped work and was removed.
- **176-178** (Part 11) come from an exhaustive read of Psalm's own
  diagnostic catalog, cross-checked against real PHPStan and this
  project's `src/`.
- **179+** come from re-verifying every open item against the codebase
  directly (2026-09-22). That pass also turned up gaps with no earlier
  phase, and each gets the next number in whichever section it belongs
  to.
- **181** (Part 12) is a product feature rather than a finding: a local
  web viewer for one analysis's results (2026-09-26).
- **182** (Part 13) analyses Latte templates natively, parsing them into
  the AST instead of compiling them to PHP first; Piwigo's own
  compile-then-analyse pipeline is its reference (2026-09-27).

A number between 85 and 179 with no entry here (86, 92, 93, 99,
102-104, 106, 108, 135, 138, 158-172, 174, 175, 179) belongs to a phase that has fully
shipped, and Done records it.

Two structural facts most of the beyond-parity findings (Parts 1-9 and
Part 11) keep coming back to:

- **phpstan-rs is a native binary for every user, always.** Real
  PHPStan is a PHP process, so the speed and precision it can afford
  are capped by interpreter cost. Its team is partway through building
  a separate, opt-in native C++ extension ("Turbo",
  `vendor/phpstan-src/turbo-ext/`, `src/Turbo/`) to win some of that
  back for users who install it. Several findings are phpstan-rs getting
  a Turbo-tier win by construction, with no separate extension, no build
  matrix, and no `PHPSTAN_TURBO=0` fallback path.
- **phpstan-rs checks one closed-world project (app + vendored deps) in
  one process with one shared, frozen `Registry`.** Real PHPStan has to
  stay sound for arbitrary downstream consumers of a library it's
  analyzing. Its parallelism runs as separate OS worker processes with
  no shared memory, which is why it has its own
  `ArenaCache`/shared-memory-arena subsystem to recover some of what it
  loses by not sharing. phpstan-rs has neither constraint: it runs as a
  single process with real shared references and sees the whole program
  by default.

---

Lane ownership for the remaining large phases (two instances working in
parallel): Lane A owns H (Phase 87), X (Phase 91), P (Phase 94) and Part
11; Lane B owns Part 10, V (Phase 89) and AA (Phase 97), plus any Part 11
item that is purely a new rule, named to Lane A before it's taken.

## Part 1: Precision, whole-program analysis, and closed-world inference

Sections A through P below share one root cause: phpstan-rs analyzes one
closed-world project+vendor universe in a single process with one frozen
`Registry`; real PHPStan must stay sound for arbitrary downstream consumers
of a library it's analyzing, and its per-file reflection is fixed before a
function's body is even walked. Every finding here is either PHPStan
approximating for that reason where phpstan-rs doesn't have to, or a
whole-program fact PHPStan's own architecture (14-collector
`CollectedDataNode` machinery, per-run discarded dependency graphs,
parallel worker processes with no shared memory) makes expensive to compute
that phpstan-rs's single live `Registry` gets nearly for free.

**The shared prerequisite for every whole-program *diagnostic* phase
(86-94, and the composer-hygiene checks) is already
built.** It was found and closed while implementing Phase 92; see Done's
"Whole-program diagnostic-emission pipeline" for the full design. This
project used to have no whole-project, non-per-file
diagnostic-emission path at all, and `Project::decl_locations` used to
be gated behind `retain_source` (off in batch `check-project` mode).
Both are fixed. Any of these phases can build directly on
`Project::check_whole_program` instead of re-deriving that
infrastructure: gate a new opt-in `Registry` bool, and resolve findings
through `decl_locations` exactly the way Phase 92's own
`checker::whole_program::dead_enum_cases` does. Some citations below
still describe `DeclLocations` as LSP-only, `retain_source`-gated. That
was true when they were written and isn't anymore.

### Phase 85 — More precise than real PHPStan itself (§A)

Real PHPStan approximates in these spots for its own architectural
reasons; phpstan-rs doesn't share the reason, so it can be exact where
PHPStan chooses not to be.

#### A2. Unroll `foreach` over small literal arrays, with a higher ceiling than PHPStan's 16/8

Real PHPStan re-walks a `foreach` body once per key/value pair when the
subject is a small compile-time-known `ConstantArrayType`
(`tryProcessUnrolledConstantArrayForeach`,
`StmtHandler/ForeachHandler.php:544-663`), binding each iteration's exact
literal element type - so a diagnostic inside the body can be exact
per-iteration (e.g. a `match` arm unreachable only on the 2nd of 3 known
elements). It caps this at `FOREACH_UNROLL_LIMIT = 16` total keys and
`FOREACH_UNROLL_NESTED_LIMIT = 8` for nested unrolled loops
(`ForeachHandler.php:68-69`) because each extra unrolled iteration re-walks
the entire body through PHPStan's per-statement Scope-copy/Rule-dispatch
machinery - real, measurable cost. phpstan-rs has no such mechanism at all
today: `check_foreach` (`src/checker/stmt/loops.rs:526-583`) always takes
the beyond-the-cap path, unconditionally unioning every `ArrayShape` field
into one coarse element type and checking the body once, even for a trivial
2-element literal. Building this as a new feature - phpstan-rs's
`Type::ArrayShape` is a flat `Vec<(String, Type, bool)>`
(`src/types.rs:52-58`) and `check_stmt` is a plain function call, no
DI/PHPDoc-reparse per statement - could ship with a materially higher
default ceiling (or one scaled by the body's own statement count) and still
finish faster than PHPStan does today at its lower ceiling.

**Effort: large** (new feature, not a tuning knob).

#### A3. When general array key-existence checking gets built (Phase 132's `NonexistentOffsetInArrayDimFetchRule` remainder), use a higher power-set bailout than PHPStan's 16384

Real PHPStan's `TypeUtils::flattenTypes` (`Type/TypeUtils.php:134-186`)
expands a `ConstantArrayType` with N optional keys into all 2^N
present/absent variants, cartesian-producing across multiple constant arrays
being flattened together, so downstream checks (including
`Rules/Arrays/NonexistentOffsetInArrayDimFetchCheck.php`) reason exactly
about which key-presence combinations are possible. It bails to the
un-expanded type past an estimated 16384 variants
(`TypeUtils.php:150-169`) because each materialized variant is a real
`ConstantArrayTypeBuilder` object allocation. phpstan-rs's own
`ArrayShape` power-set enumeration would be a cheap `Vec` clone-plus-flag-flip
per variant, with none of that object-graph overhead - when this rule's
missing half gets built, its bailout threshold should come from a benchmark
against phpstan-rs's own per-variant cost, not a reflexive import of
PHPStan's 16384.

**Effort: medium**, and only relevant once A3's parent rule (Phase 132)
is scheduled - fold this into that phase's design rather than tracking
separately.

#### A4. Check `BenevolentUnionType` results by default, instead of leaving them permanently unchecked

phpstan-rs now has a `BenevolentUnionType` marker
(`AccessoryKind::Benevolent`, used for `Type[]`-shorthand key types) and
an opt-in `checkBenevolentUnionTypes` flag at the same 4 call sites real
PHPStan's flag touches (Phase 84, see Done). Two things are still open.
First, operand results: `check_binary_operand_validity`
(`src/checker/operators.rs`) still bails immediately once either operand
is `Mixed`, and `infer_binary`'s `BinOp::Add` arm
(`src/checker/expr.rs:3560-3608`) falls through to a bare `Type::Int` for
any combination it can't range-reason about - discarding the fact that PHP's
`+`/`&`/`|` on two `mixed`s has a real, narrow result shape. Real PHPStan
computes that narrower shape explicitly
(`InitializerExprTypeResolver::getBitwiseAndType`/`getBitwiseOrType` return
`new BenevolentUnionType([new IntegerType(), new StringType()])` when both
operands are `MixedType`, `InitializerExprTypeResolver.php:1077-1079,
1137-1139, 1197-1199`) - then **systematically declines to check it**:
`RuleLevelHelper::findTypeToCheckImplementation`/`filterTypeWithMethod`
route any `BenevolentUnionType` through the same skip path unless
`checkBenevolentUnionTypes` is true (`RuleLevelHelper.php:130-142,
260-293`), and that flag ships hardcoded `false` in `conf/config.neon:77`,
never raised by any of `conf/config.level0.neon` through `level10.neon` -
confirmed by grepping every level file - and re-pinned `false` in dozens of
PHPStan's own `RuleTest` classes. The flag exists to protect PHPStan's
installed base from a BC break; phpstan-rs has no installed base or BC
promise to protect. It could compute the same narrowed union (mirroring
the `InitializerExprTypeResolver` arms) - the first open piece - and,
the second, turn `checkBenevolentUnionTypes` on by default instead of
matching real PHPStan's `false` default, catching bugs real PHPStan
silently waves through at every strictness level, including 9 and 10.

**Effort: medium.**

#### A5. Auto-infer closed (sealed) hierarchies from whole-program subclass enumeration, not just an explicit `@phpstan-sealed` tag

Known limitations' own "Known but not yet scheduled" list (140b) names
`class-string<T>`/`instanceof` sealed-hierarchy narrowing as an unmodeled
gap - phpstan-rs already tracks `@phpstan-sealed` for declaration
enforcement (Phase 36), and porting that same tag to drive narrowing would
only be parity. The genuinely-beyond move: real PHPStan's sealed narrowing
(`ObjectType::changeSubtractedType`, `ObjectType.php:1766-1818`, correctly
collapsing to `NeverType` once every allowed subtype is exhausted) only ever
fires when fed data from an explicit author-written `@phpstan-sealed`/
`@psalm-inheritors` tag (`SealedAllowedSubTypesClassReflectionExtension.php:16-19`,
gated on `count($classReflection->getSealedTags()) > 0`) or a real PHP enum
- for an ordinary untagged class, `ObjectType::isInstanceOf`
(`ObjectType.php:1726-1750`) always returns `Maybe`, and there is no
descendant-enumeration API anywhere in `Reflection/` (confirmed: no
`getSubclasses`/`descendantClasses`/`getKnownSubclasses`). This is
deliberate on PHPStan's side, not a fixpoint/perf limit: PHPStan is one
binary used to analyze both applications and reusable libraries, and a
library's downstream consumers can add subclasses PHPStan never sees, so
auto-inferring closedness from one repo's visible files would be unsound
for that core use case. phpstan-rs's own verification model is different -
closed-world, whole-program analysis of an application plus its vendored
dependencies as one fixed universe (exactly this project's own corpus
methodology). In that setting, "zero subclasses found anywhere in the
analyzed set" is sound. phpstan-rs's registry already builds the
project-wide class graph this would read from
(`src/registry/hierarchy.rs:44`, `all_ancestor_names`) but only indexes the
parent direction - adding a reverse (parent-to-children) index and treating
a subclass-free non-final class/interface as implicitly closed would let
phpstan-rs narrow `instanceof`/`class-string<T>` chains that real PHPStan,
run on the identical codebase with the identical missing annotation,
cannot. (The same reverse index is reused directly by P1 and U1 below.
Phase 93, the now-shipped dead-abstract-class check, cited this item
too, but turned out not to need the reverse index at all: an inverted
`all_ancestor_names` was enough for its own narrower "any concrete
descendant" existence query.)

**Effort: large.**

#### A6 remainder. Uniform int-to-float overflow promotion in range arithmetic - real PHPStan only got this right for one of four operators

**Status: mostly shipped.** `Add`, `Sub`'s constant-shift fast path, and
`Mul` now union `Float` in on genuine `i64` overflow (see Done's
"silent int-to-float overflow promotion"). Still open, and small:
`sub_int_range_full`'s general range-minus-range case (right side not a
single constant) still silently discards a would-be float bound as
unbounded. That function is a faithful transcription of real PHPStan's
own multi-branch logic, so it was left untouched rather than risk
introducing a bug into carefully verified code. The original finding,
kept for its grounding:

phpstan-rs's `int_range_bounds`-based arithmetic (`BinOp::Add`/`Sub`/`Mul`,
`src/checker/expr.rs`) computed result bounds with plain native
integer math and never considered that PHP's real result silently promotes
to `float` once a bound exceeds `PHP_INT_MAX`. But
real PHPStan's own analogous path, `InitializerExprTypeResolver::
integerRangeMath` (`InitializerExprTypeResolver.php:2321-2563`), is itself
inconsistent about this: only the Div branch unions in `new FloatType()`
on overflow (line 2533); the Plus (2363-2376), Minus (2377-2400), and Mul
(2427-2441) branches compute bounds with plain arithmetic and fall through
to a shared tail (2556-2563) that explicitly *discards* any float bound as
unbounded int rather than unioning it in. So for the common case of
adding/subtracting/multiplying two wide `int<min,max>` ranges, real
PHPStan's own static type silently claims a plain `int` even when the
runtime value could be a `float` - a genuine, confirmed inconsistency in
PHPStan's own arithmetic modeling (the Div branch proves the technique was
known and cheap), not a deliberate universal design choice. phpstan-rs's
range arithmetic is plain native Rust math with none of PHPStan's
combinatorial-cost pressure elsewhere in that same file (its
`CALCULATE_SCALARS_LIMIT = 128`-gated exact-value cartesian path is the
actual cost tradeoff there) - a single cheap `PHP_INT_MAX` boundary check
per bound, applied uniformly to all three operators, costs nothing extra
and closes a gap real PHPStan has never closed for its own three most-used
arithmetic operators.

**Effort: small.**

---

### Phase 87 — Whole-program value-flow diagnostics (§H)

The shipped whole-program dead-code family (Phase 86's B1/B2/B3 and
Phases 92/93, see Done) answers *existence* questions. The items here
reuse the same whole-project registry-fixed-point architecture (Phase
41's purity model) to ask about the *values* flowing through the
program instead.

#### H1. "Dead flexibility" parameter detection (always the same literal argument, project-wide)

A registry-build-time pass resolves every call's arguments to declared
parameters via the same `resolve_call_args` helper `checker/calls.rs`
already calls per-site (`calls.rs:1033`), records each resolved argument's
already-inferred literal `Type` (`Type::IntLiteral`/`StringLiteral`/etc.,
`src/types.rs:32,35`, produced by `infer_const_type`,
`function_sig.rs:278-293`) into a project-wide
`HashMap<(callee, param_index), (site_count, distinct_values)>`. A
parameter with 2+ call sites where every single site resolves to the exact
same literal gets a diagnostic at its declaration - it's never actually
varied, a candidate to hardcode or drop. Real PHPStan's only
cross-call-site aggregation mechanism is its 16-collector machinery (`find
vendor/phpstan-src/src -iname '*Collector*'`), and the one real example
close to this shape, `NamedArgumentParameterMethodCallsCollector.php`
feeding `MethodCallWithPossiblyRenamedNamedArgumentRule.php:29-71`, only
aggregates named-argument *parameter names*, never argument *values* -
confirming no PHPStan rule anywhere reasons about whether an argument's
literal value is constant across every call.

**Effort: large.**

#### H2. Whole-program inferred "effectively NoDiscard" detection

Extend the existing per-call-site `Checker::in_bare_statement_position`
signal with a project-wide aggregation: for every call to a
project-defined, non-void-returning function/method, record whether that
call's value was discarded. A callee with 2+ recorded sites where the
value was discarded at every single one gets a diagnostic at its
declaration - the fact a real `#[\NoDiscard]` attribute would assert,
discovered from observed usage instead of requiring the author to
hand-annotate it (the same move A5 already makes for sealed hierarchies).
Real PHPStan's only general "flag a discarded return value" mechanism is
the real PHP 8.5 `#[\NoDiscard]` attribute, checked purely from the
callee's own declared attribute (`CallToMethodStatementWithNoDiscardRule.php:
21-70` and siblings) - never from how callers actually behave; its one
other "useless return value" rule
(`UselessFunctionReturnValueRule.php:25-28,42-49`) is hardcoded to exactly
three stdlib functions with a known-constant return value, not a general or
inferred mechanism. phpstan-rs already has every per-site primitive this
needs: `FunctionSig.no_discard` (`registry/function_sig.rs:120-130`) is
exactly the bit an inferred version would set automatically, and
`check_useless_return_value_call` (`checker/expr/array_ops.rs:342-354`)
already answers "is this exact call's result unused" per occurrence.

Psalm ships the whole-program half of this in production too
(`UnusedReturnValue`/`PossiblyUnusedReturnValue`, `ClassLikes.php:1922-1953`)
and its implementation has two refinements worth folding into this
item when it's built: it exempts "probably fluent" methods (a
`return $this;` builder-chain method) from the check entirely
(`!$method_storage->probably_fluent`, `ClassLikes.php:1928`), and it
splits severity by visibility the same way phpstan-rs's own now-shipped
whole-application unused-member family (Phase 86 B1/B2) does -
`private` (the whole call-site universe is visible, so "unused" is
definite) versus broader visibility (softer "possibly unused," since an
external/reflection-based caller could exist).

**Effort: medium.**

#### H3. Whole-program property-type narrowing from every assignment site

A project-wide pass walks every `$this->prop = rhs` assignment across
every file that declares or extends a given class (via the registry's
subclass-aware hierarchy, the same walk the now-shipped Phase 86 B1
already runs for member *usage*), recording the RHS type `checker/expr.rs`'s
`ExprKind::Assign` arm already infers at that site (`expr.rs:1486`)
immediately before `check_property_assignment` (`expr.rs:1509`). If every
recorded site project-wide is a strict subtype of the property's declared
type (e.g. a `string $status` property only ever assigned one of 3 known
literals anywhere in the codebase), flag the declaration suggesting the
type could be narrowed to a literal union or backed enum. Real PHPStan's
property-assignment checking only ever validates one site at a time
against the declared type - no rule or collector intersects the RHS types
seen across every assignment site project-wide (the same 16-collector
inventory confirms nothing is collected for property-assignment values).

**Effort: medium.**

---

### Phase 88 — Closed-world inference replacing PHPStan's doc-tag-only gaps (§U)

Six cases with an identical shape: real PHPStan resolves a fact purely
from an author-written PHPDoc tag on the *immediate* declaration, with no
fallback when the tag is missing, because its per-file reflection is
fixed before the body is even walked (no persisted whole-program inferred
summary can feed a caller). phpstan-rs already proves this exact
fixed-point shape works for a different fact (Phase 41's transitive
purity) and could apply it to all six.

#### U1. Resolve `method_exists()`/`property_exists()` narrowing to real closed-world signatures, not PHPStan's open-world dummy stub

Real PHPStan's `method_exists`/`property_exists` narrowing
(`Type/Php/MethodExistsTypeSpecifyingExtension.php:45-102`) attaches a
`HasMethodType`/`HasPropertyType` accessory marker carrying only the
member *name* (`Type/Accessory/HasMethodType.php:47-65`) - its
`getMethod()` resolves to a `DummyMethodReflection`
(`Reflection/Dummy/DummyMethodReflection.php:19-30`, even faking
`getDeclaringClass()` as `stdClass`) backed by a
`TrivialParametersAcceptor` (`Reflection/TrivialParametersAcceptor.php:
40-56`): zero declared parameters, variadic, every type `Mixed`. This is
architecturally forced, not a missed optimization - PHPStan stays sound
for arbitrary downstream classes it never sees, so after
`method_exists($x,'foo')` succeeds it genuinely cannot know which
concrete class `$x` is, only that *some* class somewhere has a callable
named `foo`. phpstan-rs's `narrowing_type_for_predicate` table
deliberately excludes `method_exists`/`property_exists` today
(`checker/narrowing/predicates.rs:300-390`, `checker/tests/narrowing.rs:
190-203` documents the exclusion) - but it already keeps one frozen,
whole-program `Registry.classes: FxHashMap<String, ClassInfo>`
(`src/registry.rs:38`) where every `ClassInfo` carries real, typed
`methods`/`properties` (`registry/class_info.rs:151-152`), not reflection
stand-ins. Inverting that into a member-name -> declaring-classes reverse
index (the same additive move A5/P1 already make, and the now-shipped
Phase 86 B1 already made, from structures
built for other reasons) lets a literal-string `method_exists($x, 'foo')`
guard narrow `$x` to the union of every real class anywhere in the closed
project+vendor universe declaring `foo` - a sound, computable, finite
answer no open-world tool can give - so a following `$x->foo(...)`
resolves against real parameter/return types instead of a signature-blind
placeholder.

**Status: the parity half is built, the beyond-parity half isn't.** A
`method_exists($x, 'foo')`/`property_exists($x, 'foo')` true branch now
records a per-receiver guard (`Locals::guarded_methods`/
`guarded_properties`, `checker/narrowing.rs`), which suppresses
`method.notFound`/`property.notFound` for the rest of that branch,
mirroring real PHPStan's `HasMethodType` effect. `$x`'s *type* is still
not narrowed, so `$x->foo(...)` resolves to nothing rather than to the
real declaring classes' signatures. That narrowing is what U1 proposes.

**Effort: large.**

#### U2. Registry-level transitive `@throws` inference, replacing PHPStan's docblock-only per-hop requirement

Real PHPStan's `getThrowType()` is populated purely from the parsed
`@throws` tag, resolved *before* the function's own body is ever walked
(`Analyser/StmtHandler/FunctionHandler.php:58-90`,
`Reflection/Php/PhpFunctionFromParserNodeReflection.php:256-258`).
`MissingCheckedExceptionInThrowsCheck::check`
(`Rules/Exceptions/MissingCheckedExceptionInThrowsCheck.php:32-65`) only
ever looks at the current function's own body plus each callee's already-
fixed, doc-sourced throw type - nothing feeds a callee's *real*
per-body-walk throw points back into its own reflection for a caller to
consult (compounded by PHPStan's parallel-worker analysis model). This
directly explains why phpstan-rs's own `MethodThrowTypeCovarianceRule`
port only ships opt-in (see Done's checked-exception entry): an implicit
void-when-untagged default would false-positive heavily given how
inconsistently this corpus tags `@throws`. Both projects currently have
no real inferred fallback, so "untagged" is indistinguishable from
"throws nothing."
Extend the already-built `registry/purity.rs::resolve_transitive_purity`
fixed point with a sibling that computes each function/method's *real*
union of thrown checked-exception types from its own body (explicit
`throw`s plus resolved callees' computed throw sets) independent of
whether an `@throws` tag exists at all, and use that computed value -
not the raw tag - as what `checker/throws/missing.rs`'s
`check_missing_checked_exceptions` consults for a caller's coverage. This
also makes the covariance rule safe to turn on by default: with a real
computed throw type available even for untagged functions, the
false-positive risk that keeps it opt-in no longer applies.

**Two concrete follow-on consumers, both from the top-100 Packagist
survey:**
- `nette`'s own `RethrowExceptionRule` idea (flag a `try` block
  containing a call that can throw a specific exception subtype when
  the only `catch` present is a bare `\Exception`/`\Throwable` with no
  more-specific clause and no re-`throw`) becomes a strictly more
  powerful, framework-independent check once U2 exists. It works for
  *any* throwing call project-wide instead of Nette's own hardcoded
  per-method exception table. Fold it in as a U2 consumer, not a
  separate framework-specific item.
- **U2a.** `pepakriz/phpstan-exception-rules` confirms that an
  `UnreachableCatchRule`/`DeadCatchUnionRule` pair is real, buildable,
  and previously uncatalogued: a `catch (Specific $e)` clause (or one
  member of a `catch (A|B $e)` union) that matches nothing the `try` body
  can actually throw, once U2's aggregate throw type is known. It's a
  real dead-code family specific to exception handling, and a small
  sub-item of its own once U2 lands.

**Effort: large.**

#### U3. Registry-level by-ref parameter out-type inference, closing the same doc-tag-only gap

Real PHPStan's `ParameterOutExecutionEndTypeRule` - the rule that would
catch a by-ref parameter ending up incompatible with its declared type
after execution - literally short-circuits on
`$outType = $parameter->getOutType(); if ($outType === null) { return
[]; }` with an inline comment that it "only runs when `@param-out` is
present" (`Rules/Variables/ParameterOutExecutionEndTypeRule.php:80-83,
97`); `getOutType()` is sourced purely from the `@param-out` tag
(`Analyser/PhpDocsResolver.php:186`) through the same doc-parsed-before-
body-walk pipeline U2 uses. The sibling `ParameterOutAssignedTypeRule`
only checks direct, syntactic `$x = ...` assignments inside the *current*
function against the plain declared type (`Rules/Variables/
ParameterOutAssignedTypeRule.php:63-69`) - it never follows a value
through a nested by-ref call either. phpstan-rs's own Phase 45 has since
shipped the local half (`@param-out` plus `parameterByRef.type`/
`.executionEnd`/`.tooWide`, built on a real per-exit-point accumulator -
see Done's "Too-wide return/property types" entry). The extension beyond
a purely local walk: when the tracked variable is itself
passed by-ref into *another* function inside the same closed-world
registry, resolve that callee's own real computed out-type (from the same
fixed point, recursively) instead of requiring a hand-written tag on it -
the same whole-program, single-process property U1/U2 already exploit,
with no per-file/parallel-worker reflection-before-walk ordering forcing
a fallback to "no info" the moment a tag is missing.

**Effort: large.**

#### U4. Closed-world inference of `@phpstan-self-out` for the straight-line case

Real PHPStan resolves a fluent method's narrowed return type (`$this->
foo()->bar()` chains) purely from the parsed `@phpstan-self-out` tag on
the immediate declaration (`Analyser/PhpDocsResolver.php:205`, the same
doc-parsed-before-body-walk pipeline U1-U3 cite) - when the tag is
absent, `MethodCallHandler.php:180-200` does nothing at all, no fallback
inference from the callee's own body is ever attempted even though the
body is fully visible to PHPStan at that same moment.
`MissingMethodSelfOutTypeRule.php:35-42` confirms the tooling never even
tries: it short-circuits the instant `$selfOutType` is null instead of
asking whether one could be inferred. Worth noting in both directions:
its own `IncompatibleSelfOutTypeRuleTest` data file shows PHPStan never
cross-checks a self-out tag's *claim* against what the body actually does
either - the tag is trusted unconditionally either way. phpstan-rs can
compute a registry-level self-out fact for the tractable subset PHPStan
never even validates: a void/self/static-returning method body consisting
of direct, unconditional `$this->prop = <expr>;` assignments with no
intervening branch/loop (the same straight-line bar U3 already applies
to by-ref tracking). Reusing `registry/purity.rs`'s fixed-point shape
(already the shared mechanism U1-U3 cite) to walk every such method
project-wide, computing the resulting property type through the same
assignment-target inference `checker/assignment.rs` already runs for any
ordinary `$this->prop = expr`, and exporting it as a per-method inferred
fact that call-site handling can consult to narrow the receiver exactly
the way a real tag would - falling back to an author-written tag when
both exist, since a tag may assert more (e.g. generic widening) than the
body alone proves. (The tag itself is supported - `FunctionSig::self_out`,
applied after each call - so this is only the inference half.)

**Effort: large.**

#### U5. Cross-check `@phpstan-require-implements`/`@phpstan-require-extends` against actual trait usage, and use it to widen `$this` in phpstan-rs's own orphan-trait fallback

Two paired closed-world checks real PHPStan's architecture cannot
perform, not just one. **(1)** For every trait carrying a require tag,
collect the set of member names the trait's own method bodies actually
access via `$this->` that aren't already satisfied by the trait's own
declared members/ancestors, and compare that set against the required
type's real (registry-known) member set. When the required type declares
members the trait body never references anywhere, report the requirement
as broader than the trait's own body demonstrably needs - a stale or
copy-pasted `@phpstan-require-implements`/`-extends`. **(2)** Use the
required type's real member set to widen `$this` in the exact fallback
path phpstan-rs already added *beyond* real PHPStan: checking a trait
with zero using classes in the closed project standalone
(`checker/function_decl.rs::check_trait_methods`, its own doc comment
explicitly framing this as real behavior beyond doing nothing) - so a
legitimate `$this->requiredInterfaceMethod()` call in a library trait
with no in-repo consumer stops false-positiving as undefined, while a
call to a member the tag does *not* cover still correctly flags (a
diagnostic real PHPStan structurally can never produce, since it never
walks that trait body in this scenario at all). `RequireImplementsRule`/
`RequireExtendsRule` only ever check whether a concrete *using* class
satisfies the tag's claim - never whether the trait's own body needs that
much; `RequireExtendsCheck` only validates the tag's own syntax/target
validity, also never usage-based. More fundamentally, real PHPStan's
trait method bodies are only ever walked through a real using class's
`InTraitNode` - a bare `Stmt\Trait_` node is used only for ignore-comment
bookkeeping, never body-walked directly, and `ScopeContext::enterClass`
throws if given a trait at all (confirmed: grepping the whole of
`NodeScopeResolver.php` for `Trait_` returns zero hits). So a trait with
no using class anywhere in the analyzed file set has its body never
walked by real PHPStan at all - it structurally cannot ever compare a
require-tag's claim to actual usage in that case, and in the has-consumer
case it never needs to since `$this` resolves against the real
consumer's real interface directly. phpstan-rs's registry already keeps
`require_extends`/`require_implements` as separately-resolved, real
class-name lists on `ClassInfo`, fully independent of `flatten_traits`'s
method/property merge - `current_trait` is consulted today only for
deprecation-suppression, never member resolution, confirming the tag's
real member set is present in the registry but simply unconsulted in the
standalone fallback. `checker::oop::check_require_tags` already proves
the consumer-compliance half is ported; this is the usage-based half
neither project has, made newly computable because phpstan-rs's
single-process registry can look up the required type's real, typed
member set at any point - unlike PHPStan's per-file reflection-before-
walk pipeline U1-U4 already document as the systemic blocker for this
whole tag family.

**Effort: large.**

#### U6. Infer an implicit `@phpstan-assert` fact for the structurally obvious guard-clause-only function body

At registry-build time, when a function/method's entire body is exactly
one `if (!<condition on $param>) { throw ...; }` statement (no other
statements before or after), phpstan-rs can syntactically read the guard
condition the same way `narrowing_type_for_predicate` already interprets
an `if` condition for ordinary flow narrowing, and synthesize an assert
fact into `FunctionSig::assert` for that declaration - the exact field
every existing consumer (`check_assert_call`/`check_assert_tag_types`,
the purity gate) already reads uniformly regardless of whether the tag
was author-written. When an explicit `@phpstan-assert`-family tag is
already present, it always wins and the inferred fact is not
computed/used at all (mirroring U4's own stated fallback order: an
explicit tag may assert more, e.g. generic widening, than the body alone
proves). Confirmed directly: `FunctionHandler::processStmt` resolves the
assertion list purely from `PhpDocsResolver::getPhpDocs()` and passes it
into `MutatingScope::enterFunction()` *before* the function's own body is
ever walked - the reflection object callers see is fixed at that point
with zero fallback when no tag exists. The one rule that touches the
tag for validation, `AssertRuleHelper::check()`, only checks the tag's
own syntactic/type validity against the parameter's *declared* type - it
never inspects the function's own body to confirm or derive the
assertion, confirming (like U4's own finding) the tag is trusted
unconditionally in both directions once present, and simply absent with
no substitute when it is not. Unlike U2/U3's whole-program fixed point,
this is a single-function, non-recursive syntactic pattern match
(recognizing exactly one shape) - it doesn't need iteration to a fixed
point at all, only the condition-to-type extraction the narrowing
machinery already has, run once per candidate declaration. The result
slots into the pre-existing `FunctionSig::assert` field every consumer
already reads uniformly, so no new consumption path is needed - only a
new producer.

**Effort: medium.**

---

### Phase 89 — Whole-program structural duplication (§V)

Extends D3 (duplicate branch bodies within one function, Part 3 below)
and the now-shipped D7 (structurally-identical operands within one
expression, Done - "duplicate-operand detection") to
duplication *across* declarations project-wide - a different AST scope
neither one touches.

#### V1. Whole-program duplicate function/method body detection

For every top-level function and class/trait method body project-wide,
build a canonical structural fingerprint (a postorder hash over
`stmt_children`/`expr_children`, each local/parameter alpha-renamed to a
positional slot, `Span` already excluded by those walkers) and bucket
declarations by it; within a bucket, confirm an exact alpha-equivalent
match with a `Stmt`-list equality helper (the same recursive
shape-matching technique `is_switch_break_equivalent` already uses,
generalized from one statement to a statement list). For a matching pair
with no `is_subclass_of`/`classes_using_trait` relationship (skip
legitimately related pairs) and a body exceeding a minimum statement
count (skip trivial getters/setters), emit an opt-in
`method.duplicateBody`/`function.duplicateBody` diagnostic - a DRY/
extract-trait candidate the author likely doesn't know exists. PHPStan's
`Rules/` has no body-content comparator anywhere - grepping both vendored
trees for "identical"/"duplicate" turns up only name-collision rules
(`DuplicateFunctionDeclarationRule.php`, `DuplicateDeclarationHelper.php:
26-60`) and array-literal keys, never two differently-named declarations'
bodies - architecturally awkward for PHPStan's own `Rule::processNode`
interface, which visits one node per call with no project-wide sibling
corpus to pair against without separate collector infrastructure.
phpstan-rs is single-process with every file's full parsed body already
resident project-wide (`FileState.ast: Vec<Stmt>`, `project.rs:28`), and
the generic `stmt_children`/`expr_children` walkers already thread
through every node kind - a ready-made traversal for both hashing and
equality with no new AST plumbing; `Registry::is_subclass_of`/
`classes_using_trait` (`registry/hierarchy.rs:12,76`) already answer the
false-positive gate. (AI below reuses this same machinery for the
inverted case: a *legitimate* override whose body happens to be
identical to its ancestor's.)

**Effort: medium.**

#### V2. Whole-program divergent-clone detection: near-duplicate bodies differing by exactly one leaf

Extends V1's fingerprint pass with a second, coarser pre-filter hash
keeping only statement-kind/operator-kind shape (ignoring literal values
and identifier names) to bucket "shape-identical" bodies; within a
bucket, walk two alpha-canonicalized ASTs together and count leaf-level
divergences (a differing literal, a differing operator, an identifier
reference breaking the alpha-mapping). Exactly one (or a small,
config-bounded number of) divergence in an otherwise-identical pair of
unrelated declarations is flagged as `function.divergentClone`/
`method.divergentClone` - a correctness signal, not style: a maintainer
fixed a bug or corrected a threshold in one copy and forgot the sibling
exists. Same absence in PHPStan as V1, plus this specifically needs a
bounded near-miss/tree-distance comparison, which nothing in its rule set
attempts for any purpose - `Rules/Comparison/` reasons purely from each
expression's inferred type, never a structural distance between two
trees. Reuses V1's canonicalization and hash-bucket machinery directly;
only the final comparison step (count divergences instead of requiring
zero) is new.

**Effort: large.**

### Phase 90 — Whole-program redundant-override detection (§AI)

Reuses V1's alpha-equivalence body-canonicalization/hashing machinery,
but inverts the relationship gate: instead of excluding pairs where
`is_subclass_of`/`classes_using_trait` holds (V1's false-positive guard
against flagging a legitimate override as an accidental duplicate), fire
*only* when that relationship holds and the specific overriding method
resolved via `lookup_ancestor_method_with_owner` has a body alpha-
equivalent to the ancestor method it overrides. Such an override changes
nothing behaviorally - plain inheritance already produces the identical
effect - so it's either dead code (safe to delete, rely on inheritance)
or a copy-paste mistake where a divergent edit was intended but never
made. Emits an opt-in `method.redundantOverride` diagnostic pointing at
the overriding declaration, with the ancestor location as a tip, skipping
trivial 0-1 statement bodies (bare `parent::foo()` forwarders, a common
deliberate idiom for documentation/covariance/attribute purposes) using
the same minimum-statement-count guard V1 already uses for its own
triviality filter. Real PHPStan's `OverridingMethodRule` (408 lines)
implements the entire override-checking family - parent-final checks,
parameter/visibility comparison helpers, signature rules, `#[Override]`
validation - but never once reads `getBody()`/statement content from
either the overriding or ancestor method; every check operates on
signature reflection data only. Grepping both vendored `Rules/Methods/`
directories for any body-content comparator returns zero hits - the only
"duplicate"-adjacent rules anywhere are name-collision detectors, never a
comparison between two differently-scoped method bodies. phpstan-rs is
single-process with every file's full parsed AST already resident
project-wide, so pairing an overriding method's body against its
resolved ancestor's needs no new traversal infrastructure - V1's own
canonical fingerprint and statement-list equality comparator apply
unchanged to this inverted case. The only new wiring: for each class's
declared methods (already iterated by `check_method_overrides`), resolve
the ancestor via `lookup_ancestor_method_with_owner` (already used by
`method_overrides.rs`'s own compatibility checking) and run V1's equality
helper directly against the two bodies - no relationship-exclusion filter
needed at all, since here the relationship is required rather than
disqualifying.

**Effort: small.**

---

### Phase 91 — Whole-program parameter-type narrowing from call-site arguments (§X)

#### X1. Narrowing from the set of concrete object types actually passed

Distinct from H1 (literal *value* constancy - never fires on an object
argument, since a value differs on every call by construction) and H3
(property assignment sites, a different AST location entirely). A
project-wide pass extends the exact per-call-site data `check_args`
already computes: for a call's resolved argument (from
`resolve_call_args`, `checker/calls.rs:1040-1071`), the checker already
infers `arg_ty = self.infer_expr(arg, locals)` (`calls.rs:411`) before
comparing it to the parameter's declared type in the ordinary check.
Instead of discarding `arg_ty`, when it resolves to a concrete,
non-generic `Type::Object(class_name, [])`, push `class_name` into a
project-wide `HashMap<(callee_key, param_index), HashSet<String>>` -
after all files are checked, for every declared parameter whose own type
is "wide" by construction (a single interface or non-final abstract
class, `ClassInfo.kind`/`is_abstract`, `registry/class_info.rs:146-148`,
or a `Union` of 2+ named classes) with 2+ real call sites and exactly one
distinct concrete class seen, flag the declaration suggesting the type
could be narrowed to that one class - or, for the interface/abstract
case, that the wider type is never actually exercised project-wide. Real
PHPStan's cross-call-site aggregation surface is its 14 real `Collector`
implementations (enumerated via `find ... -iname '*Collector*'`), none of
which collects per-call argument *types* - the closest,
`NamedArgumentParameterMethodCallsCollector.php`, only aggregates
named-argument *parameter names* (already H1's own citation). Its only
parameter-adjacent "too wide" rule family, `Rules/TooWideTypehints/`
(read `TooWideTypeCheck.php:294-349` in full to confirm), reasons only
about a by-ref/`@param-out` parameter's own value *after* the function
body assigns to it - single-function-local reasoning, never a
caller-supplied argument type aggregated across real call sites. No
PHPStan rule anywhere narrows an ordinary (non-out) parameter's declared
type from what callers actually pass. phpstan-rs needs no new inference
machinery here, only retaining and aggregating a value already computed
and then thrown away, threaded across the same single shared `Registry`
every other closed-world finding in this roadmap already grounds - no
cross-process reconciliation problem PHPStan's Collector architecture
exists to solve.

**Effort: large.**

#### X2. Narrowing from array non-emptiness, not object identity

Extends X1's project-wide call-site aggregation pass to a second
dimension: for every declared parameter typed plain `array` (no
`non-empty-array`/shape refinement already present), track at every real
call site (via the same `resolve_call_args` resolution and the same
already-inferred `arg_ty` X1's own citations establish) whether the passed
argument is provably non-empty - reusing the array-non-emptiness test
that already exists for exactly this purpose, `Self::
is_known_non_empty_array` (`checker/expr/array_ops.rs:315-327`), which
already handles a non-empty `ArrayShape` literal, an intersection
carrying the `NonEmptyArray` accessory, and a union where every branch is
non-empty - precisely the "non-empty literal, or an already-narrowed
non-empty-array-typed variable/expression" disjunction this needs, with
zero new logic for the per-call-site test itself. After all files are
checked, for every plain-`array`-typed parameter with 2+ real call sites
where non-emptiness held at every one, flag the declaration suggesting
`array` be narrowed to `non-empty-array`. Real PHPStan's cross-call-site
aggregation surface is its concrete `Collector` implementations (14
found, enumerated directly); none collects per-call argument
emptiness/type data at all - the only parameter-narrowing-adjacent rule,
`TooWideTypehints/TooWideTypeCheck.php`'s `checkParameterOutType`, reads
only a by-ref/`@param-out` parameter's own value *after* the function
body assigns to it, single-function-local, never a caller-supplied
argument's shape aggregated across real call sites project-wide.
phpstan-rs already threads every file's checking through one shared
`Registry` in a single process (the same closed-world precondition X1's
own citations establish), so this variant's project-wide aggregation
needs no cross-process reconciliation - uniquely for this dimension, the
per-call-site test itself isn't new inference at all, only the
project-wide retain-and-aggregate step (mirroring X1's own design) is.

**Effort: medium.**

**Evidence that X1's exact mechanism already ships** (top-100 Packagist
survey): `rector/type-perfect`'s own
`PublicClassMethodParamTypesCollector`/`NarrowPublicClassMethodParamTypeRule`
is X1's proposed mechanism, running live on real PHPStan's Collector
architecture (which core PHPStan has no first-party rule of its own
for). Cite it when X1 is picked up. The same package's
`NarrowReturnObjectTypeRule` is the return-type sibling. It's closer in
spirit to the already-built too-wide-return-type check (Phase 45, see
Done) than to X1 itself.

---

### Phase 94 — Closed-world precision unlocking Pending techniques further (§P)

Two cases where finishing an in-progress phase using phpstan-rs's own
closed-world registry (the same reverse-hierarchy index A5 already
proposes for sealed-hierarchy narrowing) doesn't just reach parity - it
resolves something real PHPStan is structurally, permanently unable to
resolve, for reasons unrelated to cost.

#### P1. Resolve provably-closed virtual dispatch inside the transitive-purity fixed point

Extend Phase 41's transitive-purity fixed point (`registry/purity.rs`) so
a `$this->method()` call through a non-final, overridable method isn't
unconditionally disqualified. Add the same reverse (parent-to-children)
class index A5 proposes (extending `hierarchy.rs`'s forward-only
`all_ancestor_names`), enumerate every real override of the target method
anywhere in the closed analyzed set, and treat the call as a dependency
on all of them; if the reverse index proves zero overrides exist anywhere
in the project+vendor universe, resolve the call exactly like a final
method already is. Real PHPStan's `PossiblyPureCallTransitivePurityResolver::
resolveCall` (`Rules/DeadCode/PossiblyPureCallTransitivePurityResolver.php:
168-207`) unconditionally disqualifies the whole caller unless the method
is private, final, or its declaring/called-on class is final - no attempt
to enumerate real overrides - and the identical gate is duplicated
independently in `FunctionPurityCheck::check`
(`Rules/Pure/FunctionPurityCheck.php:126-141,142-184`), confirming this is
deliberate, structural policy everywhere PHPStan reasons about call-target
purity: it has no descendant-enumeration API anywhere in `Reflection/`
because it must stay sound for a library's downstream consumers who can
add subclasses it never sees - an open-world constraint, not a cost cap.
phpstan-rs's `is_non_overridable` (`registry/purity.rs:503-524`) currently
mirrors this restriction verbatim, by the module's own explicit design
comment - but phpstan-rs analyzes one closed-world project+vendor universe
in one process with one frozen `Registry`, the identical property that
already lets A5 treat "zero subclasses found" as sound. Given the
identical source and the identical missing `final` keyword, phpstan-rs
could resolve what real PHPStan is architecturally forced to treat as
unknowable. Distinct consumer from A5 (narrowing), the now-shipped Phase
86 B1 (unused-member detection), and H2 (NoDiscard inference) - this one
drives the purity
dependency-resolution fixed point and `@phpstan-pure` validation
specifically.

**Effort: large.**

#### P2. Resolve real Composer package boundaries for `@internal` enforcement, instead of the root-namespace-segment heuristic both projects currently share

Phase 67's already-shipped `@internal` engine approximates "is the caller
in the same package as the `@internal` symbol" by comparing only the
first `\`-delimited namespace segment of caller vs. target. Swap that for
a real composer-package-boundary lookup using the
`find_composer_package_name`/`detect_phpstan_dependency` infrastructure
the composer-hygiene checks already establish exists in this
codebase - resolve both the caller's file and the target's declaring file
to their owning `composer.json` package name, and report only when the
packages genuinely differ. Closes both directions of the mismatch: false
negatives (two separately-versioned composer packages sharing one root
namespace segment - any two same-vendor packages like `symfony/console`
and `symfony/http-foundation`, both rooted at `Symfony\`) and false
positives (one composer package legitimately spanning more than one
top-level namespace). Real PHPStan's `RestrictedInternalUsageHelper::
shouldBeReported`/`getRootNamespaceFromName`
(`Rules/InternalTag/RestrictedInternalUsageHelper.php:36-57`) - the shared
engine behind all five `RestrictedInternal*UsageExtension` classes -
decides package membership purely via the bare first namespace segment,
with no composer.json awareness anywhere: architecturally forced, since at
the point a `Rule`/`Scope` processes one file, PHPStan's reflection layer
has no persistent whole-program "which package declares this symbol" index
to query (the same absence the composer-hygiene checks confirm). phpstan-rs's
`internal_usage.rs` mirrors this exact heuristic on purpose today (its own
doc comment names it "the common Composer-package-as-top-level-namespace
convention") - but `composer::find_composer_package_name`
(`src/composer.rs:144-155`) already exists for a different rule family,
computed once per whole-project run rather than discarded between files.
Wiring it into `internal_usage.rs`'s boundary check costs no new
architecture - only an already-resolved real package name in place of a
namespace substring.

**Effort: medium.**

**Reinforced by two independent real packages** (top-100 Packagist
survey). `dave-liddament/phpstan-php-language-extensions`'
`Package{Static,New,Method}CallRule` family overlaps this item
directly: it enforces real Composer-package-boundary visibility via
attributes, and is worth reading as a second reference implementation.
`pb30/phpstan-composer-analysis`, which wraps the standalone
`shipmonk/composer-dependency-analyser` tool, needs the identical
class-FQN → owning-package index for its own, differently-shaped Phase
142 checks. The two should share one
`composer::find_composer_package_name`-style implementation, not
duplicate it. Phase 145e overlaps P2 as well.

---

## Part 2: Native-performance wins real PHPStan needs a separate extension for

Real PHPStan's own team ships (or is still building) these as opt-in native
C++ ("Turbo") because their stock PHP implementation can't afford them.
phpstan-rs is already native for every user - it doesn't need the extension
architecture, only the data structure.

### Phase 95 — Native-performance (§C)

#### C1. Type interning + memoized union/intersect/remove (TypeCombinator caching), on by default

Real PHPStan's own `TypeCombinatorCache` states plainly that the stock PHP
implementation "performs no caching at all" for union/intersect/remove
calls, and that the native version exists because "roughly 91% of the calls
in a self-analysis run repeat an argument tuple that was already computed"
(`Type/TypeCombinatorCache.php:7-23`) - real PHPStan's own documented
biggest lever, shipped only behind the separately-built Turbo extension, so
most installs (source checkouts, unsupported platforms, `PHPSTAN_TURBO=0`)
never get it. phpstan-rs needs this architecture just as badly: its own
`checker/type_union.rs` doc comments record a real hang this project hit -
`gdb` caught a `check-project` run spinning inside `Type::eq`, called from
`union_types`, called from `merge_scopes` on every `if`/`else` - worked
around with an ad hoc `MAX_UNION_MEMBERS = 12` cap that collapses to
`Mixed` past a threshold, rather than the root fix (memoize, intern) real
PHPStan's own team applied to the identical problem shape. A
structural-key memo cache plus an interning arena for `union_types` (and
future intersect/remove helpers) gives every phpstan-rs user PHPStan's
Turbo-tier win by default, no opt-in extension, no build matrix.

**Effort: large.**

---

## Part 3: New rule categories and design-pattern suggestions

Not approximation gaps - PHPStan's entire `Rules/` catalog (Api, Arrays,
Cast, Classes, Comparison, Constants, DeadCode, Exceptions, Functions,
Generators, Generics, Methods, Missing, Names, Namespaces, Operators,
PhpDoc, Properties, Pure, Regexp, RestrictedUsage, TooWideTypehints,
Traits, Types, Variables, Whitespace) simply has no rule of these shapes at
all, confirmed by grep.

### Phase 96 — New rule categories with zero PHPStan precedent (§D)

#### D1. Function/method complexity and maintainability metrics

Zero PHPStan rule is keyed on a function/method body's own *shape* (branch
count, nesting, parameter count) rather than a type/existence/soundness
fact (`grep -rli 'complexity|cyclomatic|npath' vendor/phpstan-src/src/` -
nothing). phpstan-rs already has the traversal infrastructure for free: a
generic `stmt_children`/`expr_children` walk helper (`src/ast_walk.rs:39,
136`) and a single per-body entry point every function/method already
passes through (`src/checker/function_decl.rs`). Tallying decision points
(if/elseif/case/catch/`&&`/`||`/`?:`/loop conditions) for McCabe cyclomatic
complexity, max nesting depth, and parameter count (`FunctionSig.params.len()`,
`src/registry/function_sig.rs:96`) is additive to a walk that already
happens for control-flow analysis - exactly the "hardcoded function/match
arm" shape every existing phpstan-rs check takes, with a configurable
threshold mirroring how other opt-in thresholds already read from
`config.rs`.

**Which algorithm to build against** (top-100 Packagist survey):
`tomasvotruba/cognitive-complexity` implements Campbell/SonarSource's
"Cognitive Complexity" metric specifically. It weighs nested control
flow more heavily than a flat sequence of branches, unlike plain
cyclomatic complexity, so build against that named algorithm rather
than an unspecified complexity metric. **D1b.** The same package's
`ClassDependencyTreeRule` sums a class's constructor-injected
dependencies' complexity *transitively* (this class's complexity plus
everything `__construct()` pulls in, recursively) and flags a total
over a threshold. That's a distinct, novel "god object with a deep,
complex dependency graph" smell, and it needs only recursive
constructor-param-type resolution through the already-live closed-world
registry.

**Effort: medium.**

#### D3. Identical-branch / structurally-duplicate conditional-body detection

Neither `Rules/Comparison/` (`IfConstantConditionRule`,
`MatchExpressionRule`, `SwitchConditionRule`, etc.) nor `Rules/DeadCode/`
(`NoopRule`, `UnreachableStatementRule`, etc.) contains a rule comparing two
branch *bodies* for structural equality - every one evaluates a single
condition's possible truth values, never a pair of branches whose
conditions are both satisfiable but whose bodies happen to be identical
(`grep -rli 'identical|duplicate' vendor/phpstan-src/src/Rules/` - only
`DuplicateKeysInLiteralArraysRule.php`/`DuplicateFunctionDeclarationRule.php`,
both about *declarations*, never bodies). phpstan-rs's `Stmt`/`StmtKind`
AST (`src/ast.rs:24-40`) with its generic `stmt_children` walker
(`src/ast_walk.rs:136`) already threads through both `If` and
`Match`/`Switch` handling - a structural-equality comparison over that AST
(ignoring `Span`) is a local helper comparable to the recursive
shape-matching `is_switch_break_equivalent` already implements for a
different purpose in `src/checker/control_flow.rs`, not new architecture.
Almost always either dead code (the condition can never matter) or a
copy-paste bug where one branch was meant to differ. (V1/V2 in Part 1
extend this same idea to duplication *across* declarations, a different
AST scope.)

**Effort: medium.**

#### D4. Narrow, single-function-scoped taint check: superglobal input reaching a dangerous sink

Within one function/method body only (no interprocedural or
library-boundary tracking), flag a value syntactically traceable back to
`$_GET`/`$_POST`/`$_REQUEST`/`$_COOKIE` (through same-function plain
reassignment/concatenation/interpolation, no recognized sanitizer)
reaching `eval`, `system`/`exec`/`shell_exec`/`passthru`/`popen`/
`proc_open`, `unserialize`, or a non-literal `include`/`require` path.
Real PHPStan has zero taint infrastructure - grepping both vendored trees
case-insensitively for "taint" turns up only substring false-positives
(e.g. "uncer-TAINTY"); the closest adjacent mechanism, `RestrictedUsage`'s
outright function-name banning, has no notion of data provenance at all,
so it can't distinguish `eval($safe_constant)` from `eval($_GET['x'])`.
phpstan-rs already has every structural piece a narrow, same-function
version needs cheaply: superglobals are already a recognized, named set
(`is_superglobal`, `checker/assignment.rs:377-392`); `eval`/`include`/
`require` are already special-cased at one call site
(`checker/expr.rs:194-198`); the per-function `Locals` map already carries
a precedent second persistent side-channel alongside its type map (the
existing `provenance: ProvenanceMap` field, `checker.rs:107-131`, built
for LSP go-to-definition). Be honest about false-positive risk in the
writeup when this ships - a narrow, single-function-scoped version is much
safer than an interprocedural one, and should stay that way.

**Effort: medium.**

#### D5 remainder. `@covers`/`#[CoversClass]` target-class existence

`@see`'s own validation is built (see Done). The one piece still open
is the sibling of `brainbits/phpstan-rules`' `CoversClassExistsRule`:
validating that a PHPUnit `@covers`/`#[CoversClass]` tag's target class
exists. It was originally scoped to fold into D5 rather than be a
separate check. `phpstan/phpstan-phpunit`'s own
`ClassCoversExistsRule`/`ClassMethodCoversExistsRule` covers the same
ground (see `TOP100_RESEARCH_NOTES.md`).

**Effort: small.**

---

### Phase 97 — Whole-program design-pattern suggestion rules (§AA)

Opt-in suggestions in the shape the now-shipped Phase 86 B1/B2 and
Phase 87's (§H) items already establish: a
whole-program fact PHPStan's architecture can't cheaply compute (would
need its 14-collector `CollectedDataNode` machinery to reconcile across
parallel workers) that phpstan-rs's single-process, closed-world registry
gets almost for free. Several of these also expose the same directional
asymmetry: real PHPStan's machinery only ever *validates* an existing
stricter annotation, never *suggests* adding one.

#### AA1. Whole-program "readonly candidate" suggestion for properties assigned exactly once

For every declared, non-readonly, non-static, non-promoted property, a
registry-build-time pass walks every `$this->prop = rhs`/`self::$prop =
rhs` assignment site across the property's whole subclass-aware
hierarchy and counts syntactic assignment *sites* (not RHS types - a
different question from H3, which asks whether every RHS *type* across
sites is narrower than the declared type; a property assigned once with
a wide, non-narrowable RHS like a fresh `new DateTime()` triggers this
but never H3, and a property assigned five times with five different
literal-subtype values triggers H3 but never this). If the total count
project-wide is exactly 1, and that single site is a plain `Assign`
(never compound, never by-ref) lexically inside the declaring class's own
`__construct()`, the property is a mechanically safe "add readonly"
candidate - real PHP readonly semantics require initialization from
exactly the declaring class's own scope, exactly once, so a single-site
constructor-only assignment already satisfies both constraints before the
diagnostic is even emitted. Real PHPStan's readonly machinery only ever
validates a property *already* declared readonly against illegal
re-assignment - `MissingReadOnlyPropertyAssignRule.php:33` and its
PHPDoc sibling both explicitly skip any property that isn't already
flagged readonly; it never reasons in the opposite direction. No
PHPStan collector aggregates per-property assignment *counts* across
files at all. phpstan-rs already has every primitive this needs,
unconnected: the exact "is this write inside the declaring class's own
constructor" signal already exists in `check_property_assignment`
(`checker/property_assignment.rs:57-70`, the readonly-violation check's
own gate, reusable in the opposite direction); `checker/unused/
member_usage.rs`'s existing per-class `MemberUsage.writes: HashSet<
String>` needs only to become a project-wide *count*; `PropertySig.
is_readonly` (`registry/class_info.rs:26`) is exactly the flag a
suggested codemod would flip.

**Independent confirmation** (top-100 Packagist survey):
`shipmonk/phpstan-rules`' `EnforceReadonlyPublicPropertyRule` ships the
*enforcement*-mode sibling of this idea, a hard requirement rather than
a suggestion. It's real and valued in production, and worth citing when
AA1 is picked up. It's also a reason to offer two modes: the suggestion
scoped above, plus an opt-in hard-enforcement mode matching shipmonk's
stricter default.

**Effort: medium.**

#### AA2. Suggest narrowing a public constructor to private/protected when it's only ever called from one factory method

After the registry is built, walk every `new ClassName(...)` call site
project-wide and, per class with a `Public` constructor, record the set
of distinct enclosing-method identities (`current_class` +
`current_method`, already tracked live on `Checker`,
`src/checker.rs:510`) from which `new` targets it. If that set has
exactly one member across the whole closed-world project, and that
member is a static method of the class itself (a `create()`/
`fromArray()`-style named constructor) rather than another instance
method or the class's own constructor, suggest narrowing the constructor
to enforce the factory as the sole entry point - excluding classes with
zero call sites, abstract classes, classes extended elsewhere (a
subclass needs a visible-enough constructor), and classes whose name
escapes into any dynamic-instantiation-adjacent construct (`new $var(...)`,
`ReflectionClass`, `unserialize`), mirroring the conservative
all-or-nothing escape-hatch `MemberUsage::escaped` already uses for the
analogous private-member-usage problem. Real PHPStan has no factory/
named-constructor concept anywhere in its rule set (grepped both
vendored trees for "factory"/"named constructor" - nothing but an
unrelated `StaticTypeFactory` import); more fundamentally, its only
whole-application member-usage rule gates on `isPrivate()` specifically
because determining every call site of anything non-private needs its
own cross-process collector reconciliation (the same cost the
now-shipped Phase 86 B1 already had to pay around) - the
same cost applies identically to aggregating every `new ClassName(...)`
call site's enclosing method across the whole project. phpstan-rs
already does the two things this needs simultaneously during ordinary
checking: `ExprKind::New`'s resolution of every constructor call's target
class (`checker/expr.rs:1697`) and `Checker`'s live tracking of the
enclosing method (`checker.rs:510`) - a project-wide pass only needs to
record `(target_class, current_class, current_method)` triples during the
existing walk.

**Effort: medium.**

#### AA4. Class-level `readonly class` shorthand when every property independently qualifies under AA1

Once AA1's per-property pass has run, add one class-level aggregation
step: if a class declares at least one non-static property, has zero
promoted constructor parameters and zero static properties, and *every*
declared property independently passed AA1's single-constructor-site
test, emit one class-level suggestion pointing at PHP 8.2's whole-class
`readonly class Foo { ... }` shorthand instead of N separate per-property
suggestions (suppressing AA1's own diagnostic for that class once the
aggregate fires, to avoid redundant noise). Real PHPStan's readonly
machinery only ever validates a property already declared readonly
against illegal reassignment, or validates *consistency* of an
already-declared `readonly class` modifier against a subclass - neither
direction ever proposes *adding* the modifier, matching AA1's own
confirmed absence of any "suggest readonly" rule concept; no PHPStan
collector aggregates per-property assignment counts across an entire
class's property set either. A genuine, useful implementation detail
surfaced during grounding: phpstan-rs's parser already fully discards the
fact that a class body was introduced with the `readonly` keyword -
`parse_class_decl` tracks `class_is_readonly` as a local variable used
only to desugar into each member's own `is_readonly` flag, never stored
on the returned `ClassDecl` itself - so AA1's own per-property scan
already naturally self-excludes an already-`readonly class`-declared
class (every property already has `is_readonly=true` and is filtered out
before this aggregation would even run); the only genuinely new piece is
the positive aggregation - iterating a class's plain (non-promoted)
declared properties and checking all of them qualified.

**Effort: small.**

#### AA6. Suggest `@template-covariant`/`@template-contravariant` when actual usage already proves it safe

`TemplateTypeVariance::validPosition()` treats an invariant-declared (or
undeclared - the default) template type as valid in *any* position - so a
plain `@template T` never produces a diagnostic no matter how it's
actually used in the class body. `VarianceCheck::check()` and its two
callers (`PropertyVarianceRule`, `MethodSignatureVarianceRule`) only ever
compare an *already-declared* variance against the position found - no
code path anywhere in those files, or in `TemplateTypeFactory.php`/
`TemplateType.php`, aggregates every usage site of an undeclared/
invariant template parameter to compute what variance would actually be
sound (grepping phpstan-src for "infer.*variance"/"guessVariance"/
"computeVariance" returns nothing; phpstan-strict-rules has no
variance-related rule at all). phpstan-rs's own mirror of this exact
mechanism, `check_variance`/`check_property_variances`, early-returns
before doing any walk at all when a class has only default-invariant
templates - identically to real PHPStan's silent no-op. But the
traversal this needs already exists in the same file: `walk_variance`
already composes a `Position` (In/Out/Inv) through every nested generic
slot for a class's own template occurrences, driven by the same per-class,
declaration-time call sites already wired for properties and methods -
no new data, no cross-file/registry-wide pass, just a second,
always-run, undeclared-template variant of the same walk that *records*
positions instead of comparing them against a pre-existing declared
variance. If every recorded position across the whole class was `Out`
(with at least one occurrence), suggest `@template-covariant T`; if every
position was `In`, suggest `@template-contravariant T`; any `Inv`
occurrence, any mix, or zero occurrences means no suggestion at all -
matching real PHP variance soundness exactly, and mirroring AA1's
"exactly one qualifying condition or nothing" conservatism.

**Effort: medium.**

---

### Phase 98 — PHP 8.4 property-hook purity enforcement (§AB)

Real PHPStan's newest feature support (asymmetric visibility, property
hooks) is exactly where self-inconsistencies would be expected, unlike
its decade-old core mechanisms. (AB2, dead-write detection for `public
private(set)` properties, has shipped - see Done.)

Real PHPStan's own `PropertyHooksProcessor::processPropertyHooks`
(`Analyser/PropertyHooksProcessor.php:113-158`) explicitly gathers
`ImpurePoint`s from a hook body via `pushNodeGatherer` (property
assignments recorded at lines 123-131) and stores them on
`PropertyHookReturnStatementsNode` (which exposes `getImpurePoints()`) -
but `Rules/Pure/` contains only `PureFunctionRule.php`/`PureMethodRule.php`
(consuming the ordinary function/method return-statements nodes), and
`Rules/DeadCode/*WithoutImpurePointsCollector.php` has only Function/
Method/Constructor variants - no `PurePropertyHookRule` and no
property-hook-consuming collector exists anywhere. The impure-point data
property hooks compute is gathered and then never read by any Rule or
Collector - hooks get zero purity enforcement despite PHPStan's own
plumbing being built for exactly that purpose. phpstan-rs already has
both pieces this needs, just not connected to hooks: the registry-build-
time transitive purity classifier (`registry/purity.rs`) and the
one-directional `@phpstan-impure` tag-validation check
(`check_declared_purity_tag`, `checker/noop.rs:267-304`), currently
wired into exactly one call site - ordinary function/method declarations
(`checker/function_decl.rs:198`). `checker/oop/property_hooks.rs`'s
`check_get_hook`/`check_set_hook` (lines 141, 276) never call it. Since
every phpstan-rs check is a hardcoded function call with no DI/Rule-
registry indirection, adding one more call site to an already-built,
already-tested classifier is additive wiring that closes a gap real
PHPStan's own source shows it gathers the data for but never consumes.

**Effort: medium.**

---

### Phase 100 — Opt-in algorithmic-complexity heuristic (§AF)

Explicitly heuristic, the same acknowledged low-confidence framing D6
and D7 established before both shipped (see Done).
Inside a loop body (`self.
loop_depth > 0`, already-live infrastructure - `checker.rs:728`, already
consulted directly by other heuristic checks at
`checker/expr/probe_checks.rs:154,209,379,454`), when the checker sees a
call to `in_array()`/`array_search()`/`array_key_exists()` whose haystack
is a plain variable (not an inline literal) with an unbounded `Type::
Array(k, v)` shape rather than a small fixed `ArrayShape`, that variable
is never appended to anywhere in the enclosing loop body (so it isn't
being progressively built - a legitimate incremental-lookup pattern), and
it isn't the loop's own iteration subject (so it isn't a self-scan/dedup
pattern), emit a low-confidence `loop.quadraticLookup` diagnostic:
"linear lookup into an already-built array repeated once per outer-loop
iteration - O(n*m); consider `array_flip()`/`isset()` or building an
index before the loop." Ships strictly opt-in with an explicit
false-positive caveat that a small-but-unbounded-at-runtime haystack
makes this a non-issue the check can't resolve. PHPStan's entire
`Rules/` catalog reasons about types, existence, and soundness, never
runtime execution cost - grepping both vendored trees for "complexity"/
"quadratic"/"performance" as a rule concept returns nothing; there is no
rule keyed on a lookup's placement relative to a loop, or on an array's
role as a repeatedly-scanned lookup table. Distinct from D1 (which
reasons about a function's own control-flow shape, never a variable's
role as a collection) - keyed on a loop's own iteration relationship to
a separate collection's construction site.

**Effort: medium.**

---

## Part 5: LSP / editor-native capabilities

Real PHPStan ships no first-party LSP or interactive query API at all -
`FileAnalyser::analyseFile` (`Analyser/FileAnalyser.php:81`) is its sole
analysis entry point, always a whole file, always from a fresh CLI
invocation, with no resident process and no persisted per-file/per-project
index of any kind. phpstan-rs's LSP server already keeps a resident,
incrementally-updated `Project`/`Registry` (`src/project.rs:69-114`) - the
items below are gaps in already-built plumbing, not new architecture.
Most of what this part originally proposed has shipped (see Done's
"LSP hover/go-to-definition" and "LSP completeness" entries, plus the
LSP server hardening and VSCode extension client in "Performance &
tooling"). What's left is narrower residual work: finer-grained
incremental re-check (G), two single-file/whole-project stragglers
(`codeLens`, `willRenameFiles`), and derivation-trace explainability
(T).

### Phase 105 — Finer-grained incremental re-check (§G)

#### G1. Sub-file (per-function) real-time re-check on `didChange`

Every `didChange` today unconditionally calls `Project::update_file`
(`src/lsp.rs:169-206, 638-650`), which re-parses the *whole file* and
re-runs a fixed sequence of whole-registry passes (`project.rs:356-363`)
on every keystroke, even when the edit stays inside one function body and
touches no declaration at all. Yet phpstan-rs already has, and already
uses for hover, a mechanism that type-checks an isolated function against
the frozen registry with none of that rebuild: `run_probe_scope`'s
`ProbeScope::Function` arm calls `checker.check_function(...)` directly
(`lsp.rs:363-378`) with no re-parse and no registry re-indexing. Real
PHPStan has no notion of checking less than a whole file at all -
`FileAnalyser::analyseFile` is the sole entry point, and even its own
result cache invalidates at file granularity only
(`ResultCacheManager::restore`/`process`, `ResultCacheManager.php:
440-602`), read from disk once per whole CLI invocation, never queried
continuously from a resident process. Detecting that an edit's diff falls
entirely within one function's span and doesn't add/remove/rename a
declaration, then re-running just `check_function` over that function
against the still-valid registry (falling back to full `update_file` on
save or any declaration-shaped edit), gives sub-file, sub-registry-rebuild
latency for the dominant editing case.

**Effort: large.**

#### G2. Per-symbol signature-shape fingerprinting to skip re-checking unaffected dependents

Give every declared symbol a cheap fingerprint covering only its
externally-visible signature (param types/names/variadic/by-ref/defaults,
return type, visibility, ancestry), explicitly excluding `FunctionSig::
body` (already correctly excluded from equality reasoning per that
struct's own doc comment, `src/registry/function_sig.rs:214-233`). In
`Project::update_file` (`project.rs:305-388`), instead of unconditionally
seeding `changed_symbols` with every name the file declares regardless of
whether its shape actually differs (lines 321-322, 341-342), seed it only
with names whose fingerprint changed - `transitive_affected_files`'s
existing per-symbol BFS (`project.rs:404-448`, already keyed by bare name
via `dependents: FxHashMap<String, HashSet<PathBuf>>`, `project.rs:71`)
then naturally narrows to only the dependents of the specific symbol(s)
whose shape moved. Real PHPStan already does something similar but only
at whole-*file* granularity, and its own data structures make finer
resolution structurally impossible without a rewrite:
`ResultCacheManager::exportedNodesChanged` (`ResultCacheManager.php:
684-717`) correctly skips a file's dependents on a body-only edit, but the
invalidation edge is stored per *file* (`NodeDependencies::
getFileDependencies` explicitly dedups by `$dependencyFile`,
`NodeDependencies.php:37-64,60`) - so if a file declares two classes and
only one's signature changes, PHPStan reanalyzes *all* dependents,
including ones that only reference the untouched class, because the
stored edge can't say which declaration a dependent came from. This is
architecturally forced by PHPStan's per-run, discarded-and-rebuilt
dependency graph, not a missed optimization; phpstan-rs's own doc comment
already names this exact gap as future work ("straightforward to tighten
later", `project.rs:67-68`). Distinct from section Q below (whole-program
API BC-diffing): Q compares two independently-loaded, disjoint registry
snapshots to report public-surface breaks as the actual product; this
compares shapes within one live incremental session on the *same* tree to
drive re-check scheduling.

**Effort: large.**

---

### Phase 107 — `textDocument/codeLens` reference-count annotations (§AH)

Advertise `code_lens_provider` and add a `"textDocument/codeLens"` arm to
`handle_request`. For each declaration in the requested file (walked the
same way the now-shipped `documentSymbol` walks `Project::ast(path)`),
emit one `CodeLens` whose title is a use-site count (e.g. "3 references")
sourced by aggregating the two-tier use-site index the now-shipped
`references`/`rename` support already built (Done) -
grouping its per-symbol spans by declaration and taking the count. This
was originally proposed as depending on a not-yet-built index; that index
now exists, so this item is strictly smaller than it was. Two pieces are
still new work: per-declaration count aggregation (the existing index is
built for enumeration, not as a rollup count), and the `CodeLens`
response serialization/wiring itself. Real PHPStan ships no LSP or
resident analysis server of any kind - `FileAnalyser::analyseFile` is its
sole entry point, always a fresh whole-file CLI invocation with no
persisted cross-file use-site index and no channel to push
editor-rendered annotations without modifying the analyzed source.

**Effort: medium.**

---

### Phase 109 — `willRenameFiles` (§AO)

Distinct from the now-shipped `textDocument/rename`: this is triggered by
a file-explorer drag/move with no cursor position at all, and its core
computation (deriving a new expected FQN from a new file path) is the
*reverse* direction of what `rename` does - `rename` only ever resolves
symbols already named in source text, never infers a name from a path.
Also distinct from `psr4.pathMismatch` (which flags a path/FQN mismatch as a
diagnostic on files that already sit where they are - it never fires on
a rename event, never computes a *new* FQN, and never rewrites anything):
this performs an active edit at the moment of the rename gesture, using
R1's underlying prefix-map data but the mirror-image arithmetic
(path-to-FQN, not FQN-to-path). On a file-explorer move of a `.php` file,
compute the FQN the file *used* to declare via a `DeclLocations.classes`
reverse lookup and the FQN the *new* path implies (a new `path_to_fqn`
computation: longest-prefix match the new path against each
`autoload.psr-4`/`autoload-dev.psr-4` entry, strip the matched
directory, turn remaining segments into namespace segments, use the
filename stem as the short class name). If the two FQNs differ, respond
with one `WorkspaceEdit` rewriting the moved file's own `namespace`/
`class` declaration plus every project-wide use-site span of the old FQN.
Real PHPStan ships no LSP or IDE-integration module of any kind - the
same structural absence this whole part documents. Honestly disclosed as
depending on two things: R1's PSR-4 prefix-map access (not yet built) and
the use-site span index the now-shipped `rename` support already built.

**Effort: large.**

---

### Phase 110 — Diagnostic explainability: derivation traces (§T)

Real PHPStan's only type-introspection tool, `PHPStan\dumpType()`,
computes and prints one final type per call site via a plain `sprintf`
(`Rules/Debug/DumpTypeRule.php:32-50) with no accumulated reasoning kept -
each `Scope::getType()` call that fed it is discarded the instant the
message string is built, and it requires editing the analyzed source and
re-running the whole CLI, since PHPStan ships no LSP or resident process
to query interactively at all. Distinct from the now-shipped live hover
(a single inferred `Type` wired into hover for one span, computed fresh
each probe): both items below trace the *chain* of operations that
produced a type, an entirely different data shape.

#### T1. Interactive branch-narrowing derivation trace for a diagnostic

Extend `Checker::apply_narrowing` (`src/checker/narrowing.rs:38`, called
from ~30 sites across the checker) so that, under an opt-in trace mode,
each narrowing step pushes a (condition span, branch taken, mechanism,
before-type, after-type) record into a new per-`Locals` side-channel
modeled directly on the existing `provenance: ProvenanceMap` field and its
`track` gate (`checker.rs:113-132,224`). At a branch join, `merge_scopes`
(`control_flow.rs:403-439`) already shows exactly how such a side-channel
composes across paths - it unions types via `union_types` and does
`provenance.extend(...)` - so a trace channel merges into a DAG (both
branches' contributing steps kept) with zero new merge logic to invent.
Surface it by reusing the exact one-shot instrumented-recheck pattern
already proven for hover: `probe_span`/`probe_hit`
(`checker.rs:775-776`) plus `run_probe_scope`/`ProbeScope`
(`lsp.rs:321-378`) - a new LSP request passes the diagnostic's own span
and narrowing key instead of a hover position, re-runs `check_function`
once with tracing on, and returns the recorded step DAG. For a
non-interactive CI fallback, render a summary into the diagnostic's own
`metadata: HashMap<String, serde_json::Value>` field (`checker.rs:327-333`,
which every `Diagnostic` already carries and no current call site
populates).

**Effort: large.**

#### T2. Per-pass loop-convergence widening trace, for a diagnostic whose type came from fixed-point iteration rather than branch narrowing

`Checker::converge_loop_body` (`control_flow.rs:331-384`) runs up to
`MAX_PASSES = 6` passes, each producing a full `Locals` clone unioned via
`merge_scopes` into the next entry state - then, for every pass except the
one finally kept, explicitly discards both that pass's diagnostics
(`self.diagnostics.truncate(...)`) and its `Locals` state. For a
diagnostic on a loop variable whose converged type built up over several
passes (pass 1 sees `int`, pass 2's branch adds `string`, final
convergence adds `null`), under the same trace mode as T1, a lightweight
per-pass diff can be appended to a trace record instead of being silently
overwritten - "pass 2 added `string` via this branch; the final pass
added `null` via that one." Structurally different from T1: iteration-
indexed widening has no controlling condition at all, only a pass index,
so it needs its own step kind rather than reusing the narrowing-step
shape. Real PHPStan's identical fixed point
(`NodeScopeResolver::LOOP_SCOPE_ITERATIONS = 3`,
`NodeScopeResolver.php:120`) discards each pre-convergence pass's scope
the same way, but additionally widens literal/constant types to their base
type after only the first candidate pass
(`GENERALIZE_AFTER_ITERATION = 1`, line 121) - destroying the literal type
identity a trace would even need to show, on top of having no interactive
query surface at all. phpstan-rs's convergence loop does plain type-union
merging with no early generalization (Phase 85's A1, now shipped, used
that same fact in the *other* direction - raising the pass cap for more
precise final types; this is about retaining and exposing the discarded
intermediate passes for explainability instead) - each pass's precise
`Locals` state is already fully materialized in memory immediately before
being thrown away purely to keep iteration bounded, so capturing a
before/after diff there is retaining data that already exists for one more
statement's worth of scope, not computing anything new.

**Effort: medium.**

---

## Part 7: Config/strictness composability beyond PHPStan's single global level

Real PHPStan's parameters (including every opt-in rule) resolve exactly
once into one frozen DI container for the whole run
(`ContainerFactory::create`, `DependencyInjection/ContainerFactory.php:
102-170`); rule registration is queried only by AST node type, never by
file path (`Rules/Registry.php:9-15`, `Rules/LazyRegistry.php:36-53`) - a
rule is on-or-off for literally every file in a run. The only path-aware
mechanism anywhere in its config schema is `ignoreErrors[].path`/
`excludePaths`, strictly subtractive (suppress an already-computed error,
or skip a file entirely) - it can never turn a rule ON for a subset that's
off elsewhere. phpstan-rs has no DI container to rebuild per invocation
and no parallel-worker-process model to keep consistent - one `Registry`
is built once (`Project::finish_loading`, `project.rs:277`) and a fresh,
cheap `Checker` is constructed per file (`Project::check_file`,
`project.rs:453`) that already carries per-file state alongside the
shared `&Registry`.

### Phase 112 — Config/strictness composability (§J)

#### J1. Per-directory additive opt-in checks

phpstan-rs's own `Config` (`src/config.rs`) already has opt-in bool flags
(`check_uninitialized_properties`, `check_internal_usage`, etc.) that flow
into single `Registry` bool fields, read uniformly at every check site.
Resolving a small path-pattern list once at load time into a per-file-path
override table, consulted before falling back to the project-wide bool,
costs nothing architecturally - no new dispatch mechanism, no re-running
rule registration, just one extra lookup at the point `check_file` already
builds a `Checker` for that path. Real ADDITIVE per-directory strictness
(a stricter check ON only in one subtree) that PHPStan's frozen-container
model has no path to support at all.

**Effort: medium.**

#### J2. Per-directory strictness tiers in one run, with cross-directory calls still checked consistently

Splitting a codebase into a stricter `src/New/` and looser `src/Legacy/`
in real PHPStan requires either one level for everything, or two entirely
separate invocations with no shared registry and no cross-directory
soundness (a call from legacy into new code isn't checked against the new
code's stricter rules at all, since level-gated rules attach to the file
being *visited*, not the callee's declared strictness) -
`CommandHelper::begin` (`Command/CommandHelper.php:246-310,401`) resolves
`$level` as one scalar for the entire container build, with no code path
reading a per-path level list. phpstan-rs's single, always-shared
`Registry` means a file-path-keyed tier lookup (generalizing J1's
override table from on/off to a small set of named tiers) doesn't require
duplicating or re-resolving any graph - the whole project, including
cross-directory calls, stays in one pass with one shared registry.

**Effort: large.**

---

## Part 9: Type-system policy, baseline robustness, reverted-feature fixes, and API diffing

### Phase 121 — Type-system representation policy lessons (§M)

Not performance tradeoffs (Part 1/2) or missing features - real PHPStan's
own source code admits, in its own comments, four places where an early
design decision now has to be permanently carried forward for users it
already shipped to. phpstan-rs has no installed userbase to protect and
can simply not repeat the mistake; each item below is a forward-looking
policy recommendation, not a bug to fix.

#### M1. One unsealed-array-shape representation, never a null-vs-explicit-marker duality

PHPStan's `ConstantArrayType::$unsealed` is `?array{Type,Type}`, defaulting
to `null` (ambiguous, `isUnsealed()` answers `Maybe`) for non-bleeding-edge
users or an explicit `[NeverType(true), NeverType(true)]` marker for
bleeding-edge users (`ConstantArrayType.php:176-192`) - because flipping
every existing user's array-shape comparison semantics in one release was
judged too disruptive, so the toggle exists to opt in gradually. Its own
`equals()` documents the fallout directly (lines 937-943): a legacy-null
shape and a marker-sealed shape that mean the same thing compare unequal,
forcing a permanent second code path (`legacyIsKeysSupersetOf` at line
2934, `legacyMergeWith` at 3112, both still invoked whenever either side
is legacy-null) that ripples into consumer code too
(`ArrayMergeFunctionDynamicReturnTypeExtension.php:126-136` explicitly
checks `isUnsealed()->maybe()` to decide whether it may even attempt the
newer result shape). phpstan-rs's `ArrayShape` already carries
unsealedness as one in-band sentinel field (a literal `"..."` entry,
`src/types.rs:55`, tested by a 3-line `is_unsealed_shape`,
`checker/narrowing/array_shape.rs:23-25`) - the recommendation is to keep
it that way as deliberate policy when the tracked unsealed-shape
superset/merge reasoning (A3's territory) is actually built out: implement
it once, unconditionally, never add a nullable "ambiguous/legacy" tier
alongside an explicit marker, since there's no prior release's comparison
behavior to preserve bit-for-bit.

**Effort: small** (a standing policy, not a build item on its own).

#### M2. Keep the `Type` layer closed - no public trait, so old accessor names can be deleted instead of deprecated-and-kept-forever

PHPStan's `Type` interface, implemented by every concrete type class *and*
by any third-party extension's custom Type class (it's `@api`), still
declares 7 `@deprecated`-tagged methods every implementor must fully
implement, because an interface method can't be made optional and removing
one breaks anyone implementing `Type` directly (`Type.php:129-136,
186-197`; confirmed still fully implemented in `ObjectType.php`,
`UnionType.php`, `ConstantArrayType.php`). Deprecation buys no removal
here - the debt is permanent by construction, not a cleanup backlog.
phpstan-rs's `Type` is a closed Rust `enum` (`src/types.rs:22-23`) matched
exhaustively by free functions, never a public trait outside code
implements, so a rename/removal is a same-repo refactor, never a
permanent API-surface commitment - already the practice in this codebase
(`Registry::lookup_property`/`lookup_property_with_owner` etc.,
`registry/hierarchy.rs:157,163,556,563,573,583`, no deprecated sibling
kept alongside the current one). The concrete forward policy: keep
declining to expose the type/registry layer as a public trait for
external plugins, matching Known limitations' own Won't-port precedent on
DI/plugin architecture, so this debt category structurally cannot begin.

**Effort: small** (a standing policy).

#### M3. One canonical member order for Union/Intersection types, never a describe()-only sorted cache alongside it

PHPStan's `UnionType`/`IntersectionType` must keep both a canonical
`$types` array (whose order is part of the type's own value - `getTypes()`
callers merge array shapes in that order) and a separate
`$sortedTypesCache` used only for human-readable `describe()` output,
because an earlier version sorted `$types` in place inside `describe()`,
silently corrupting later `getTypes()` calls - both classes' own code
comments admit this outright (`UnionType.php:64-70`,
`IntersectionType.php:79-85`: "It used to be sorted in place... making an
immutable value object's observable state depend on what had been called
on it before"). phpstan-rs's `Type::Union`/`Intersection` are plain
`Vec<Type>` (`src/types.rs:121`) whose `Display` impl
(`types/display.rs:106-116`) takes `&self` and never writes back - there
is exactly one order everywhere, enforced by ordinary Rust immutable-borrow
semantics rather than by discipline. Forward policy: if union-display
readability is ever improved, do it by collecting into a locally sorted
`Vec` inside the `Display` call only, never by mutating the stored
`Vec<Type>` itself.

**Effort: small** (a standing policy).

#### M4. Never split builtin function signatures into a legacy-loose stub and an opt-in stricter delta

Real PHPStan ships two parallel builtin-function stub files for the same
functions: a loose/legacy one and a strictly more precise one (e.g.
`php_uname`'s `mode` parameter narrowed from bare `string` to a literal
union of its six valid values; `array_rand`'s `input` narrowed from
`array` to `non-empty-array` and `num_req` from `int` to `positive-int`;
six `DOMDocument` methods' `filename`/`source` parameters narrowed from
`string` to `non-empty-string`).
`FunctionSignatureMapProvider::getSignatureMap()` only merges in the
stricter file when `$this->stricterFunctionMap` is true, and that flag
(`featureToggles.stricterFunctionMap`) defaults to `false` in
`conf/config.neon:38`, flipped to `true` only by `conf/bleedingEdge.neon:7`
- functionally the same opt-in-for-BC gating as `BleedingEdgeToggle::
isBleedingEdge()` (M1's own mechanism), just expressed as a plain DI
parameter instead of the static call. It exists because narrowing a stub
an existing user's code already analyzes cleanly against could surface
brand-new errors for them - the stricter signatures are demonstrably
correct (verified in PHPStan's own bleeding-edge files), but PHPStan
can't ship them as the *only* signature for exactly that BC reason.
phpstan-rs models builtin signatures directly in Rust (e.g. `array_rand`
in `checker/expr/builtin_calls.rs:745`) with no stub-file/toggle
machinery at all - the forward policy is to keep it that way: always
encode the most precise known signature for a builtin (matching what
real PHPStan's own bleeding-edge file already proves correct), never add
a parallel looser variant gated behind a flag, since there is no
installed userbase whose already-passing analysis a narrower builtin
stub could suddenly break.

**Effort: small** (a standing policy, same shape as M1-M3).

---

### Phase 123 — Reattempting reverted features with real PHPStan's actual technique (§O)

The **Reverted** section lists features
phpstan-rs once built, corpus-verified, then undid after real false
positives. Both items below found that the *revert* was correct given
what phpstan-rs had available at the time, but that real PHPStan solves
the identical case with a specific technique phpstan-rs's attempt never
used - not a coincidence PHPStan avoided the bug, a real mechanism to
port.

#### O1. Foreach's zero-iteration merge: real PHPStan's asymmetric, certainty-downgrading join instead of a symmetric type-union

Real PHPStan does NOT give `foreach` the same zero-iteration merge
treatment as `while`/`for`. `ForeachHandler.php` computes
`$exprType->isIterableAtLeastOnce()` (`Type.php:174`, implemented per-type,
e.g. `ArrayType.php:233`) and branches three ways
(`ForeachHandler.php:374-390`): definitely->=1 iteration keeps the pure
post-body scope; definitely 0 iterations reverts to the pure pre-loop
scope; the "maybe empty" case calls
`MutatingScope::processAlwaysIterableForeachScopeWithoutPollute()`
(`MutatingScope.php:4122-4169`) - which is NOT a symmetric type-union
merge. For each variable, it keeps the post-loop *type* outright and only
downgrades the certainty (yes->maybe), never unioning the pre-loop type
back in for a variable that already existed before the loop - exactly the
case a real idiom needs: a foreach body that mutates its own iteratee
array through the loop variable
(`foreach ($items as $day => $data) { $items[$day]['x'] = ...; }`).
phpstan-rs's only branch-join primitive, `merge_scopes`
(`src/checker/control_flow.rs:403-440`), unconditionally computes
`union_types(then_ty, else_ty)` for every shared variable - purely
symmetric, no certainty concept. The Reverted entry for `Foreach`'s
zero-iteration merge and `checker/stmt/loops.rs:584-615`'s own inline
comment
confirm phpstan-rs's original and re-attempted foreach zero-iteration
merges both reused this same symmetric primitive and both regressed on
exactly that self-mutating-iteratee idiom (named by the real corpus
function that surfaced it, `CalendarMonthly::buildMonthCalendar`) -
unioning the pre-loop array shape back in re-introduces a stale element
type every real iteration actually overwrites. Confirming this is
foreach-specific in PHPStan, not a general technique: `while`/`for`'s own
zero-iteration handlers use plain symmetric `mergeWith()`, the same tool
phpstan-rs's existing (non-reverted) while/for merge already uses
successfully - foreach just borrowed the wrong one. phpstan-rs already
has the prerequisite Phase 55 fixed-point loop convergence built
(`converge_loop_body`); the missing piece is narrowly a second, asymmetric
join primitive mirroring `processAlwaysIterableForeachScopeWithoutPollute`,
gated on the same "runs >=1 time" provability. Going beyond real PHPStan:
it carries a legacy `polluteScopeWithAlwaysIterableForeach` config flag
(`NodeScopeResolver.php:184,807`) forcing it to maintain BOTH the
symmetric and asymmetric code paths for third-party BC; phpstan-rs has no
such burden and could adopt the more-precise asymmetric merge
unconditionally, everywhere a loop join happens - one code path instead
of PHPStan's two.

**Status: narrowed, partly shipped.** A corpus-driven investigation
found that the *specific* false-positive pattern this item was scoped
to fix (a loop-accumulator array read back as `T|null` instead of `T`)
had a narrower real cause one level below the merge algorithm itself.
`union_types` never simplified away a genuinely empty `ArrayShape`
(`array{}`) once a compatible general `Array(key, val)` member was
already in the same union. Real PHPStan's own `TypeCombinator::union()`
already does that redundant-subtype absorption. The fix went directly
into `union_types` (`checker/type_union.rs`), and `converge_loop_body`'s
own merge algorithm wasn't touched, so the self-mutating-iteratee
regression the asymmetric join exists to avoid is unaffected. That fix
alone removed 16 of the pattern's 70 known scoped-corpus findings. The
general asymmetric-join proposal above is still open for whatever the
narrower fix didn't reach. The same investigation root-caused a
separate `int|string` array-key-imprecision cluster to a missing
`BenevolentUnionType` marker on `Type[]`-shorthand-derived key types,
which has since been built (see Done's `AccessoryKind::Benevolent`
entry). The full corpus citations, and the by-ref-foreach-mutation
trade-off this fix's own verification surfaced, are in Known
limitations' "Known but not yet scheduled" list (140c, and the
single-loop-accumulator entry).

**Effort: medium.**

---

### Phase 124 — Whole-program public-API backward-compatibility diffing (§Q)

A new capability with no PHPStan-core or `phpstan-strict-rules`
equivalent at all - grepping both vendored trees for
BackwardCompat/BCBreak/ApiDiff turns up nothing except PHPStan's own
inward-facing `Rules/Api/*` family (flags analyzed code referencing
PHPStan's *own* internal namespace, an unrelated concept). `analyseFile`
is PHPStan's sole entry point: always one fresh CLI invocation over one
filesystem snapshot, no notion of loading and comparing two points in
time. `Project::new()`/`load_file` take no arguments blocking two
independent, simultaneously-live `Project`/`Registry` snapshots in one
process - the diff is a plain field-by-field walk over two frozen
`HashMap<String, ClassInfo>`s.

#### Q1. `phpstan-rs api-diff <old-ref> <new-ref>`: structural public-API diff between two Registry snapshots

Loads two independent `Project`/`Registry` instances (one from a git-ref
checkout or old vendored copy, one from the current tree) and structurally
diffs every public (non-`@internal`) symbol's `ClassInfo`/`FunctionSig`/
`PropertySig`/`ConstSig`: a removed public method/property/constant, a
parameter type narrowed (via `is_subtype`) so a previously-legal caller is
now rejected, a return type widened so a previously-guaranteed narrower
type can't be assumed, a visibility narrowed or a symbol newly marked
`@internal`, a method/class newly `final`, a removed interface/parent, or
a non-final class gaining a new abstract method breaking existing
subclasses. phpstan-rs already builds every signature-shape field this
needs as ordinary struct fields, not reconstructed via reflection each
run: `ParamSig`/`FunctionSig` (`registry/function_sig.rs:95-176`,
including visibility, `is_internal`, deprecated, native vs docblock
types) and `ClassInfo`/`PropertySig`/`ConstSig`
(`registry/class_info.rs:23-205`, including `is_abstract`/`is_final`/
`extends`/`implements`/`final_methods`/`final_properties`). `Registry` is
frozen once per `finish_loading()` and is a plain in-memory `HashMap`, so
two independently-loaded, independently-frozen registries can coexist in
one process with no shared-mutable-state hazard - real PHPStan's per-run,
discard-after-use DI container has no analogous ability to keep two full
snapshots alive and comparable at once. Distinct from G2 in Part 5
(signature fingerprinting): G2 compares shapes within one live incremental
session on the *same* tree to drive re-check scheduling; this compares
two independently-loaded, disjoint snapshots to report public-surface
breaks to a human/CI as the actual product.

**Effort: large.**

#### Q2. PHPDoc-level generic/template-bound narrowing as its own break category, invisible to native-signature-only diffing

Extends Q1 with a break category specific to phpstan-rs's PHPDoc-aware
generics: a class's `@template T of Bound` tightened between two
snapshots (`@template T of object` narrowed to `@template T of
Countable`), or an `@extends`/`@implements` type-argument binding
changed, silently rejects type arguments a caller's existing code still
passes - even though the native PHP declaration (and therefore the
byte-identical Reflection-visible signature) never changed. This break
category has no PHP-native-signature representation whatsoever: a
`@template`/`@extends`/`@implements` bound lives purely in a docblock
string, invisible to any BC tool working from the generic PHP Reflection
surface alone (the same surface real PHPStan's own runtime-reflection
rules are limited to, per Known limitations' Won't-port note on
`RuntimeReflectionFunctionRule`/`RuntimeReflectionInstantiationRule`) - two
classes whose native declarations are byte-for-byte identical would
compare as "no change" to such a tool. phpstan-rs's registry already
carries generic bounds as first-class, already-resolved struct fields, not
a string to re-parse: `ClassInfo::templates`, `ClassInfo::
template_bounds: HashMap<String, Type>` (Phase 10.5's already-enforced
`@template T of Bound`), and `ClassInfo::extends_args`/`implements_args:
HashMap<String, Vec<Type>>` (`registry/class_info.rs:178-199`) - all
already on the exact same `ClassInfo` Q1 already walks, so comparing them
across snapshots (checking narrowing direction via `is_subtype`, not just
inequality) is additive to Q1's own walk, not new architecture.

**Effort: medium.**

---

## Parity-audit gaps against real PHPStan `Rules/` (phases 125-139)

Source: a 20-agent parity audit that checked every real Rule class
under `vendor/phpstan-src/src/Rules/` (~420 classes) against this
project's actual coverage and this file's own claims, confirmed against
source rather than recalled. Most rules turned out to be covered and
documented already, or correctly excluded. What's below are the real,
source-verified gaps it found that had no phase, Pending entry, or
Won't-port/Known-limitations line of their own. Each phase is one of
that audit's category groupings, and each bullet is one of its findings,
lightly condensed. The full per-finding evidence (file:line in both
`vendor/phpstan-src` and this project's own `src/`) is in the audit
workflow's journal (`wf_495e5a37-b39/journal.jsonl`). A later
identifier diff, direct `vendor/bin/phpstan analyse` (level 10) against
`phpstan-rs check-project` on `~/piwigo16`, took every real identifier
with no matching literal anywhere in this project's source, checked it
against this list, and added the real gaps the audit had missed.

### Phase 131 — Generics (`Rules/Generics/` gaps)

- `MethodTagTemplateTypeCheck` — a `@method` tag's own inline template
  parameters (distinct from a real method's `@template`) aren't
  modeled at all, including the template-name-shadows-class-template
  check.
- `UsedTraitsRule`'s own remaining diagnostics — incompatible/
  unresolvable type on the `@use<...>` tag itself, an `@use` tag naming
  a trait the class doesn't actually use, and its one real behavioral
  difference from `@extends`/`@implements` (a call-site variance
  annotation is *never* valid inside `@use<...>`, unlike the other two)
  — Phase 54 already closed the bound/arity/variance half.

### Phase 134 — Variables (`Rules/Variables/` gaps)

- `AssignToByRefExprFromForeachRule` — assigning to (or property-
  fetching through) the same expression just bound as a `foreach ($x as
  &$expr)` reference — a well-known gotcha, unchecked.
- `CompactVariablesRule` — `compact()`'s own literal-string-argument
  variable names checked for definedness; phpstan-rs's
  `variable.undefined` only fires for direct `$var` reads today.
- `UnsetRule` — five distinct `unset(...)`-target diagnostics
  (undefined variable, illegal/nonexistent offset, readonly-property
  variants, hooked/possibly-hooked property).
- `NullCoalesceRule`/`IssetCheck` remainder — the `.variable`/
  `.offset`/`.property` probes ("always exists and is not nullable", and
  "never defined") are built for `??`, `isset()`, and `empty()`
  (`check_pointless_null_probe`/`check_pointless_offset_or_property_probe`,
  `checker/expr/probe_checks.rs`). Two pieces are left. First,
  `nullCoalesce.expr`/`isset.expr`/`empty.expr`: the same "can never be
  null" verdict for an arbitrary non-chain expression on the left
  (a call result, a cast). Real PHPStan gates this behind
  `checkAdvancedIsset`, and it's the 5-corpus-hit item. Second, the
  newer `nullCoalesce.unnecessary`: `$x ?? null`/`$x ??= null` where
  the left side is always set, so the whole coalesce is a no-op.

(`VariableCloningRule` — `clone.nonObject` — is now built; see Done,
receiver-type-checks phase.)

### Phase 180 — `treatPhpDocTypesAsCertain: false`

Real PHPStan's `treatPhpDocTypesAsCertain` parameter (default `true`)
decides whether always-true/always-false checks
(`instanceof.alwaysTrue`, `function.alreadyNarrowedType`,
`identical.alwaysTrue`, the whole constant-condition family) trust a
PHPDoc-only type as certain. Set to `false`, they only fire on what the
*native* types alone prove. Composer and Monolog both ship with it set
to `false`, and a Lane B corpus run found it explains most of their
remaining `instanceof.always*` hits. phpstan-rs has no such option:
every consumer sees the PHPDoc-refined type only. Building it needs the
native-only type alongside the refined one at those decision points
(real PHPStan's `Scope::getNativeType`), plus a config key and the
per-rule "because the type is coming from a PHPDoc" tip real PHPStan
adds when the option is on.

**Effort: large** (a second, native-only type channel through the
narrowing engine).

## Part 10 — Top-100 Packagist `phpstan-extension` survey (new phases)

Every one of the top 100 `phpstan-extension` packages on Packagist by
download count was cloned and read from real source, one at a time (see
`TOP100_RESEARCH_NOTES.md` for the full per-package notes this section
condenses). ~40 packages were single-framework/single-library glue
(Symfony, Doctrine, Laravel, Drupal, WordPress, TYPO3, CakePHP, Magento,
SilverStripe, Nette, CodeIgniter, GLPI, Maho, Laminas, and every single-
library mock/assert/type-extension package) — each needs that
framework's own runtime metadata model phpstan-rs has no equivalent of,
confirmed individually rather than assumed from the framework's name;
none produced a portable finding and none are re-listed phase-by-phase
below. A further ~10 were abandoned/deleted/duplicate/config-only with
nothing to port. The phases below are what remained: genuinely new,
portable ideas, plus several real, independent confirmations of ideas
already scoped above (folded in there instead of repeated here).

### Phase 153 — Actionable TODO-comment family

A genuinely novel, well-designed family turning `// TODO` comments into
live, checkable facts instead of text that rots forever
(`staabm/phpstan-todo-by`) — directly complements Phase 96's D5 (`@see`
tag validation) in spirit (checking a comment's own claim against
reality) but is a distinct, separately-valuable idea:

- **153a (small, pure/self-contained).** `// TODO: fix by 2024-01-01`
  flags once that date has passed — compare a parsed date literal
  against wall-clock "now," no network needed.
- **153b (small, pure/self-contained).** `// TODO: remove when php >=
  8.3` / `// TODO: ... when symfony/console >= 6.0` flags once the
  project's own configured PHP version, or a specific installed
  Composer package version (resolvable from `composer.lock`),
  satisfies the condition.
- **153c (medium, network-dependent — a real design departure worth
  flagging honestly).** `// TODO: JIRA-123` flags once that real ticket
  is closed, via a live HTTP call to Jira/GitHub Issues/YouTrack at
  analysis time — no existing phpstan-rs check does live I/O; scope
  and gate this separately from 153a/153b if pursued at all.

### Phase 155 — Small, cheap opinionated additions (bucket)

Each independent, self-contained, low-effort:

- **155a (small).** Flag a file declaring more than one top-level
  class/interface/trait/enum (a real PSR-1-adjacent convention many
  projects want) — a trivial file-level top-level-declaration count
  (`symplify/phpstan-rules`).
- **155b (small).** Require explicit visibility on class constants (PHP
  7.1+ allows it) (`prestashop/phpstan-prestashop`).

### Phase 156 — Whole-project type-coverage ratchet (§ reinforces Part 1's aggregate-fact theme)

A **project-wide type-coverage ratchet**: real PHPStan Collectors tally,
project-wide, what fraction of return types/param types/property
types/class constant types are actually declared (plus
`declare(strict_types=1)` file coverage), then a Rule fails the run if
the percentage is below a configured required threshold — a CI-friendly
"coverage must not regress" gate, not a per-site diagnostic
(`tomasvotruba/type-coverage`). phpstan-rs already computes every
individual missing-type-hint diagnostic this would aggregate
(`missingType.*`) — this is purely a project-wide count-and-threshold-
compare on top of data phpstan-rs already has per-site, the same
"whole-program aggregate from per-site data already computed" shape
Part 1 already establishes repeatedly. **Effort: medium.**

### Phase 157 — Database-migration hygiene family (low priority, no current corpus consumer)

A real, coherent, novel category (`bellangelo/phpstan-migration-rules`,
duplicated across Laravel-migration and Phinx variants) — but tied to a
specific migration framework's own schema-builder DSL, so still
framework-specific in the same sense as the DI-container/ORM packages
(needs that framework's own schema-builder call shape). Noted for
completeness, low priority given this project's own target corpus
(Piwigo is a plain PHP app, not built on Laravel-migrations/Phinx) and
would need a schema-builder-DSL-aware checker layer from scratch with no
existing precedent — not a natural next step without a concrete first
consumer:

- Require explicit charset/collation on new tables.
- Ban positional column placement (`->after('column')`) — a real
  zero-downtime-deployment/DB-engine-portability hazard.
- Ban SQL `ENUM` columns — well-known "hard to modify later"
  anti-pattern.
- One table per migration file.
- Ban raw SQL strings — force the schema-builder API for portability/
  reviewability.
- Opinionated forward-only-migration policy — discourage writing
  reversible `down()` methods.

---

Confirmed **no new finding, no action needed** from the remaining ~50
top-100 packages (each individually verified from real source, not
assumed from the package name): every Symfony/Doctrine/Laravel/Drupal/
WordPress/TYPO3/CakePHP/Magento/SilverStripe/Nette/CodeIgniter/GLPI/
Maho/Laminas/PHP-DI/PSR-11-container framework-integration package
(needs that framework's own runtime metadata model); every single-
library type-extension package with no independent second confirmation
(Mockery, Prophecy, Stripe SDK, Faker, Dibi, bonami/collections,
Deployer stubs); Blade/Twig/Latte template-compiler packages (no port:
they compile templates to PHP for PHPStan to read, where phpstan-rs parses
templates natively instead - Phase 182); Pest's own plugin (entirely built
around Pest's global-function DSL, no portable rule surface without
building Pest-DSL-detection infrastructure first); `#[Sealed]`-attribute
subtype validation (already covered by real PHPStan's own native
`@phpstan-sealed` tag, already scoped in Phase 67's design); a live-
network-calling Bitbucket "code insights" error formatter (a CI-vendor-
API-integration concern outside a local diagnostic formatter's natural
scope, unlike Phase 154's pure-text formats); WordPress-core API
version-gating (needs a WordPress-specific version-tagged symbol
database with no pure-PHP trigger to justify vendoring it);
`AutoMapper+`'s own BC-compat `ignoreErrors` config (not a checker
mechanism at all); and `phpstan/phpstan-php-parser`'s self-analysis
stub-precision package (not a rule/check package). Full per-package
detail and citations: `TOP100_RESEARCH_NOTES.md`.

---

## Narrowing-engine follow-up (phase 173)

The one phase left over from the 158-175 correlated-narrowing batch.
Everything else in that batch has shipped (see Done's narrowing-engine
cluster).

### Phase 173 — Same-function parallel dynamically-keyed array correlation

Reevaluated a third time (the cross-variable-correlation cluster's own
"parallel dynamically-keyed arrays" item) after two prior over-
pessimistic passes on neighboring items in the same cluster - the
same-function case (both the populating loop and the later read in one
method, matching every corpus citation: `ActivityRepository::
findDailyActionCountsSince`, `HistoryRepository::findGroupedCountsSince`,
`ImageRepository::findExtensionBreakdown`) is bounded and buildable,
unlike the genuinely cross-function generalization (still declined -
see Known limitations' correlated-narrowing entry for exactly why).

Design mirrors `condition_aliases` (`checker.rs:104-177`), not
`AccessoryKind`: a correlation between two *named* variables needs
real invalidation-on-rebind, which a payload-carrying `AccessoryKind`
variant baked into a `Type` value has no story for (every existing
variant is a stateless unit variant, `types.rs:191-205`, precisely
because presence/absence is all `ArrayList`/`NonEmptyArray` ever need -
a named cross-reference is a different shape of fact entirely). Add a
new `Locals` side-table, `co_indexed: HashMap<String, HashSet<String>>`,
populated and purged the same way `condition_aliases` already is:

- Producer side: recognize a loop body where two (or more) array
  variables each receive an unconditional write
  (`$arr[$k] = ...;`/`$arr[$k] += ...;`) at the syntactically-identical
  key expression, at the same statement-list nesting depth with no
  intervening conditional/early-exit between them, every iteration -
  after the loop, record each pair's mutual correlation.
- Consumer side: a `foreach ($counterByKey as $k => ...)` whose subject
  carries a `co_indexed` entry - a later `$dateByKey[$k]` read inside
  that loop body, where `$k` is provably the loop's own key variable,
  skips the "possibly undefined index" diagnostic.
- Purge exactly like `condition_aliases`: rebinding either correlated
  variable (a fresh `declare`, not a same-shape `insert`) invalidates
  the pair.

**Effort: medium** — new side-table plus two new detection passes
(producer loop-shape recognition, consumer key-provenance matching);
larger than a single leaf-arm addition but bounded and self-contained,
reusing `condition_aliases`'s own invalidation discipline rather than
inventing a new one.

---

## Part 11: Findings grounded in Psalm's production implementation

Every part above is grounded in what real PHPStan does or doesn't do.
This part instead cross-checks against Psalm (`vimeo/psalm`), the other
major PHP static analyzer, doing the same kind of comparison the
`phpstan-strict-rules` ports did, but against a whole
separate codebase's own diagnostic catalog: all 325 distinct issue
classes under `src/Psalm/Issue/`, read exhaustively (every file, not a
sample chosen by promising-sounding
name) and cross-checked against both `vendor/phpstan-src` (to rule out
that real PHPStan already covers the same ground) and phpstan-rs's own
`src/` (to rule out that this project already covers it a different
way, often under different diagnostic-identifier naming than Psalm
uses). Most of the 325 turned out to be one of three things: ordinary
type-checking phpstan-rs already has (confirmed via phpstan-rs's own
`"identifier.string"` literal census cross-checked against each Issue
class's actual Psalm emission site, not assumed from the name), real
PHPStan parity gaps with no Psalm-specific angle at all (folded into
the parity-audit list instead - this research pass added
`MatchExpressionRule`, since shipped, see Done's `match.unhandled`), or
Psalm-internal implementation detail with no general applicability (see
§AR below). What survived all three filters is below.

The Psalm clone was not `vendor/psalm` (the pinned submodule this
project uses purely as a corpus-validation target). It was a separate,
ad hoc `git clone https://github.com/vimeo/psalm.git` one directory up
from this repo (`../psalm`), checked out for this research pass only.
Every citation in this part was checked against that clone the same
way the other parts' citations were checked against the vendored
trees.

### Phase 176 — Concrete new checks with no PHPStan or phpstan-rs precedent (§L)

#### L1. `ClassMustBeFinal` - suggest `final` on a class that's never subclassed and isn't public API

Psalm ships a whole-program design-pattern suggestion phpstan-rs has no
equivalent of at all: a class gets flagged for `final` when it's
non-abstract, non-enum, non-interface, not already `final`, isn't part
of the codebase's declared public API, and - checked against the whole
analyzed project - is never subclassed anywhere
(`consolidateAnalyzedData`, `Internal/Codebase/ClassLikes.php:864-897`;
`Issue/ClassMustBeFinal.php`). This is a genuinely new rule *category*
for this project (Part 3's "new rule categories" theme), not a variant
of an existing check - nothing in `src/checker/` currently reasons about
whether a class *should* be sealed based on observed whole-program
subclassing, only about enforcing an already-declared `final`/
`@phpstan-sealed`. phpstan-rs's registry already has everything this
needs in the closed-world, single-`Registry` shape Part 1's own thesis
argues for repeatedly: `all_ancestor_names`
(`registry/hierarchy.rs:105`) gives the parent-direction walk, and A5's
proposed reverse (parent-to-children) index - already called for by A5,
P1, and U1 elsewhere in this roadmap, and the exact mechanism the
now-shipped Phase 93 dead-abstract-class check ended up building - is
exactly the "is this
class ever extended" query this item also needs. A `public_api`-
equivalent gate is already close at hand too (`is_internal`/`@api`
tracking this project already carries per `FunctionSig`/`ClassInfo`).
Different from Phase 144a, which is an opinionated rule that every class
be `final` unless it's `abstract`. L1 is a whole-program inference that
only fires on classes provably never extended.

**Effort: medium** (mostly the reverse-index dependency shared with A5).

#### L2. `MissingOverrideAttribute` - suggest adding `#[\Override]` where a method silently overrides one

phpstan-rs already validates the *presence* of a `#[\Override]`
attribute is correct - `own_sig.has_override_attr && !has_real_ancestor`
flags an attribute on a method that doesn't actually override anything
(`checker/oop/method_overrides.rs:273-282`). Psalm checks the reverse
direction phpstan-rs has no equivalent of: a method that genuinely
overrides a parent/interface method but is *missing* `#[\Override]`
(`FunctionLikeAnalyzer.php:2029-2056`), exempting `__construct`,
trait-defined methods, and `__toString` unless the class implements
`Stringable`. This is nearly free to add: the exact `has_real_ancestor`/
`ancestor` lookup `method_overrides.rs:273` already computes at this
call site is everything the reverse check needs too - it's an `if
!own_sig.has_override_attr && has_real_ancestor` sitting right next to
the check that's already there, with the same exemption list
`method_overrides.rs`'s own `__construct`-special-casing (line 365-369)
already establishes for the neighboring compatibility check.

**Correction: this one does have PHPStan precedent.** Real PHPStan ships
the same check as `method.missingOverride`, an opt-in rule gated behind
`checkMissingOverrideMethodAttribute` (off by default), as
`method_overrides.rs`'s own comment at the `method.override` check
notes. That makes it a parity item rather than a Psalm-only one. Mirror
PHPStan's identifier and opt-in gate, and use Psalm's exemption list as
a cross-check.

**Effort: small.**

#### L4. `ConstructorSignatureMismatch` - a subclass constructor requiring *more* parameters than its parent's

Real PHP (and PHPStan, and phpstan-rs - confirmed at
`checker/oop/method_overrides.rs:365-369`) deliberately exempts
`__construct` from the general override signature-compatibility check
entirely, since a subclass is legitimately allowed a totally different
constructor shape from its parent's. Psalm still flags one narrow slice
of that exempted space: a subclass constructor that requires *more*
required parameters than the parent's (`MethodComparator.php:210-225`) -
a latent `ArgumentCountError` the moment that subclass is instantiated
polymorphically (a factory, a DI container, `new $class(...)` via
`class-string<Base>`) by code that only knows the parent's shape.
Neither PHPStan nor phpstan-rs checks this at all today. Cheap to add:
`method_overrides.rs`'s existing `compatibility_ancestor`/`ancestor`
resolution for `__construct` (line 378) already has the parent
constructor's signature in hand right where the general exemption is
applied - this is a narrow carve-out of that exemption, not new
resolution machinery.

**Effort: small.**

#### L5. `StringIncrement` - flag `$str++`/`++$str` on a non-numeric string

A real, common PHP gotcha with zero coverage on either side: PHP's `++`
on a non-numeric string performs Perl-style alphabetic increment
(`'a'` becomes `'b'`, `'z'` becomes `'aa'`, `'Az'` becomes `'Ba'`) rather
than any kind of arithmetic, which essentially never matches what a
developer intended when reaching for `++`
(`ArithmeticOpAnalyzer.php:395-416` - `$has_string_increment` set
specifically when the left operand is a `TString` that isn't
`TNumericString`/a numeric `TLiteralString`, and the parent node is a
pre/post-increment). Confirmed absent from real PHPStan
(`grep -rli "string.*increment" vendor/phpstan-src/src` - nothing) and
from phpstan-rs.

**Effort: small** - a type check on the operand of `ExprKind::PreInc`/
`PostInc` (or the by-ref-assignment-inc idiom) whose inferred type is
`Type::String` without the numeric-string accessory.

#### L6. `RiskyTruthyFalsyComparison` - flag a boolean-context check whose union type mixes "always truthy/falsy" and "data-dependent" members

Flags `if ($x)`-shaped boolean-context checks where `$x`'s inferred
union type mixes members whose truthiness is a foregone conclusion
(`true`, a non-empty-string literal) with members whose truthiness is
genuinely data-dependent (plain `int`, plain `string`) -
(`Internal/Analyzer/Statements/ExpressionAnalyzer.php:148-172`, message:
"...contains type(s) ..., which can be falsy and truthy... Use strict
comparison instead"). The underlying gotcha is real: PHP's falsy set
(`0`, `'0'`, `''`, `[]`, `null`, `false`) spans multiple otherwise
unrelated types, so an implicit truthy check on a mixed-truthiness union
can silently misbehave for members the author wasn't thinking about.
Confirmed absent from real PHPStan
(`grep -rli "risky.*truthy\|truthy.*falsy" vendor/phpstan-src/src` - only
unrelated infra hits) and from phpstan-rs, whose `narrowing.rs` already
computes exactly which atomic types are always-truthy/always-falsy
internally (for branch narrowing) but never surfaces "this comparison
mixes both kinds" as its own diagnostic.

**Effort: medium** - reuses `narrowing.rs`'s existing truthy/falsy
classification per union member; the new part is applying it as a
standalone diagnostic at condition sites rather than only for narrowing.

#### L7. `UnnecessaryVarAnnotation` - flag an inline `@var` that adds no information over plain inference

phpstan-rs's `check_inline_var_annotation_type`
(`checker/stmt.rs:978`) already mirrors real PHPStan's
`VarTagTypeRuleHelper::checkExprType`
(`vendor/phpstan-src/src/Rules/PhpDoc/VarTagTypeRuleHelper.php:100-140`)
- read directly, that helper only ever checks *compatibility* (is the
`@var` type a valid super/subtype of the inferred type -
`varTag.nativeType`/`varTag.type`), never *redundancy*. Psalm adds the
redundancy case on top: when an inline `@var`'s type is identical to
what inference already produces without it (and isn't narrowing away
from `mixed`), it's dead weight worth flagging for removal
(`ForeachAnalyzer.php:190-205`, `AssignmentAnalyzer.php:233-239`).
Cheap: `check_inline_var_annotation_type` already has both the parsed
docblock type and the natively-inferred type sitting side by side at
the exact call site this needs.

**Effort: small.**

#### L8. `RedundantCast` / `RedundantFunctionCall` / `RedundantIdentityWithTrue` - semantic no-ops that aren't whole-statement no-ops

Three related, lower-value-but-real checks bundled together: `(string)
$x` where `$x` is already `string` (`CastAnalyzer.php:955`); a call like
`array_values()` whose transformation is a no-op given the argument's
already-known type (`NamedFunctionCallHandler.php:564-574`); and `$x
=== true` where `$x` is already `bool`, where the comparison itself is
redundant (`AssertionFinder.php:2204-2214`). All three are distinct from
phpstan-rs's existing `checker/noop.rs`, which (mirroring real
PHPStan's own `NoopRule`) only flags a bare *statement* with zero
effect - these fire even when the cast/call/comparison's result is
actually used in a larger expression. Cosmetic/code-quality tier rather
than bug-catching, so lower priority than L1-L7 above, but genuinely
absent from both real PHPStan and phpstan-rs.

**Effort: small** each, once the "what does this type's cast/call/
comparison actually change" comparison exists as a shared helper.

#### L9. `InvalidTraversableImplementation` / `InvalidInterfaceImplementation` - implementing a marker interface directly instead of through its required base

One real PHP rule, two Psalm issue classes for its two built-in
instances. `class Foo implements Traversable` directly (rather than via
`IteratorAggregate`/`Iterator`) is a genuine PHP compile-time fatal
error, with a short, fixed exception list of SPL/extension classes PHP
itself special-cases (`PDOStatement`, `Ds\Collection`, `DOMNodeList`,
`DatePeriod`) - `ClassAnalyzer.php:2130-2144`, condition: `$fq_interface_
name_lc === 'traversable' && !$storage->abstract && !isset($storage->
class_implements['iteratoraggregate'|'iterator']) && ` (four more
`parent_classes` exceptions). The identical rule applies to `Throwable`
(must extend `Exception`/`Error`, not implement `Throwable` directly) -
same file, `ClassAnalyzer.php:2148-2156`, same condition shape. Confirmed
absent from both real PHPStan (`grep -rli "cannot implement.*Traversable\|
ImplementsTraversable\|toStringReturnType" vendor/phpstan-src/src` -
nothing for either) and phpstan-rs (`grep -rln "Traversable" src` turns
up only `foreach`-subject type-resolution comments, never a
class-*declaration*-time check).

**Effort: small** - a declaration-time check on `implements`, reusing
whatever ancestor-interface-name resolution `oop.rs`'s existing interface
checks already have in hand.

#### L10. `InvalidToString` - `__toString()`'s inferred return type must be `string`, even with no declared return type

PHP enforces `__toString()` returning `string` as a runtime `TypeError`
regardless of whether the method has an explicit native return type -
extremely common in code written before PHP made this checkable at all.
Psalm validates the *inferred* return type against `string`
(`ReturnTypeAnalyzer.php:311-328`), which is what makes this distinct
from ordinary declared-vs-actual return-type checking: when there's no
declared return type, phpstan-rs's general return-type check has
nothing to compare against and stays silent. Confirmed absent from real
PHPStan (`find vendor/phpstan-src/src -iname '*ToString*'` turns up
`ImplicitToStringCallHelper` and stub-related type machinery, never a
`__toString`-return-type rule) and from phpstan-rs (no `__toString`
return-type validation anywhere in `src/checker/`).

**Effort: small** - a magic-method-name special case in the same place
`function_decl.rs`/`oop.rs` already validate a method's returns against
its type, requiring `string` regardless of whether one was declared.

#### L11. `AbstractMethodCall` - calling an abstract static method directly

`Foo::abstractStaticMethod()` (a real PHP fatal error - there's no
implementation to run) is confirmed absent from both real PHPStan
(`grep -rli "abstract.*method.*cannot.*call\|CallToStaticMethodOnAbstract"
vendor/phpstan-src/src` - nothing) and phpstan-rs (no
`staticMethod.abstract`-shaped identifier in the existing static-call
identifier census: `staticMethod.notFound`/`.private`/`.protected`/
`.deprecated`/`.internalClass`/`.resultDiscarded`/`.resultUnused`, never
one for a directly-called abstract target).

**Effort: small** - the static-call resolution `staticMethod.notFound`
already runs through has the resolved method's `is_abstract` flag
available; this is an extra condition on an already-resolved lookup, not
new resolution machinery.

#### L12. `NonVariableReferenceReturn` - a by-ref-returning function returning a non-variable

`function &foo() { return 5; }` (or any non-variable/non-property
expression) is illegal for a `by_ref`-declared function - there's no
real storage location to hand back a reference to
(`ReturnAnalyzer.php:237-249`, condition: `$storage->signature_return_
type->by_ref && !($stmt->expr instanceof Variable|PropertyFetch|
StaticPropertyFetch)`). Confirmed absent from real PHPStan (no
`OnlyVariable`/`returnByRef`-shaped rule in `vendor/phpstan-src/src`) and
from phpstan-rs (no by-ref-return-target validation anywhere in
`src/checker/`).

**Effort: small** - a check on `StmtKind::Return`'s expression kind,
gated on the enclosing function's own `by_ref` flag (already tracked for
by-ref *parameters* via `checker/by_ref_params.rs`'s existing machinery,
just not yet applied to the return side).

#### L13. `InvalidStringClass` - `new $plainStringVar()`, stricter than real PHPStan's own `new.nonObject`

Real PHPStan's own `InstantiationRule::checkClassNameExprType`
(`Rules/Classes/InstantiationRule.php:93-116`) accepts *any* `string`-typed
value for `new $x()` unconditionally (`$acceptedType = StringType() |
ObjectWithoutClassType()`), with no further validation that the string
is actually a `class-string` - a real gap even in PHPStan's own model
that Psalm closes by default: `NewAnalyzer.php:1065-1075` rejects a
plain (non-`class-string`-narrowed) `string`, and unconditionally rejects
a `TNumericString` regardless of config, since a numeric string can
never validly name a class. phpstan-rs mirrors PHPStan's own gap here,
not just its behavior - confirmed via `checker/expr.rs:2040-2043`'s own
comment: a dynamic `new $className()` target "has no concrete name to
check" and is skipped entirely, not merely under-validated.

**Effort: small** - a type check at the same `ExprKind::New` site already
cited, requiring the resolved type be `class-string`-narrowed (or an
object) rather than an unnarrowed `string`.

#### L14. `ImpurePropertyFetch` - reading (not just writing) a mutable object's property inside a declared-pure context

The one `Impure*`-family member phpstan-rs doesn't already cover: Phase
41's purity-disqualification walk (`registry/purity.rs:367-445`,
`expr_disqualifies`) already treats property *writes*, static access,
static variables, superglobal reads, and by-ref assignment as
disqualifying - matching real PHPStan's own `ImpurePoint` taxonomy
exactly (confirmed via `FunctionPurityCheck.php`, itself already cited
in this project's Phase 41 follow-up notes) - but neither phpstan-rs nor
real PHPStan disqualifies a plain property *read*
(`Internal/Analyzer/Statements/Expression/Fetch/AtomicPropertyFetchAnalyzer.php:507-520`,
condition: `$context->pure && !$class_storage->external_mutation_free`).
Psalm's stricter model: reading a mutable object's property inside a
`@psalm-pure` function isn't side-effecting, but it isn't
referentially-transparent either - the same "pure" call could observe a
different value on a second invocation if something else mutated the
object first, breaking the memoization-style guarantee purity is
usually used for. Narrow and debatable (arguably philosophy, not a bug),
but genuinely absent from both real PHPStan and phpstan-rs, and cheap to
add since the exact "is this a mutable-object receiver" check the
condition needs is already computed one line away for the write-side
check.

**Effort: small.**

### Phase 177 — Severity tiering as a systemic diagnostic policy, not a single rule (§AP)

#### AP1. Split "definitely wrong" from "possibly wrong" across type-compatibility diagnostics

Confirmed real PHPStan has no general definite-vs-possibly severity
split anywhere (`grep -rli possibly vendor/phpstan-src/src/Rules` turns
up only unrelated hits - purity/dead-code/named-argument rules, never a
systemic severity tier), and phpstan-rs mirrors that same flat model:
`checker/calls.rs`'s `argument.type` reports one message shape whether
the passed value is *definitely* the wrong type or a nullable/mixed
union that merely *might* be. Psalm splits this at the exact decision
point, repeated across its whole argument/operand/array-access/iterator
diagnostic family: `$input_type->isNull()` gets the hard `NullArgument`
error, `$input_type->isNullable()` (the union merely *contains* null
among other members) gets the softer, individually-suppressible
`PossiblyNullArgument` (`ArgumentAnalyzer.php:1309-1338`); the same
shape repeats for false/invalid-type/undefined-offset. This is a real
signal-to-noise improvement - separating "this call is definitely
broken" from "narrow this before use" - but it's a cross-cutting
severity policy, not a single new rule: adopting it means threading a
"is the offending member the *entire* type or just *part of* the
union" distinction through most of the existing type-compatibility call
sites (`calls.rs`, `assignment.rs`, `return_type_too_wide.rs`'s sibling
checks, `expr.rs`'s operand checks), each picking a harder/softer
message and (optionally) a distinct suppressible identifier for its own
existing diagnostic.

**Effort: large** (small per call site, but the value is in doing it
uniformly across every existing type-compatibility check, not once).

### Phase 178 — The `Mixed*` family - type-erosion tracking with no PHPStan equivalent at all (§AQ)

#### AQ1. Flag a `mixed`-typed value flowing into a typed context, with provenance

Confirmed real PHPStan has *zero* rules for this
(`find vendor/phpstan-src/src/Rules -iname '*mixed*'` returns nothing).
Psalm has 18 distinct issue types for exactly this shape -
`MixedAssignment`, `MixedArgument`, `MixedArrayAccess`,
`MixedMethodCall`, `MixedPropertyFetch`, and so on - each firing when a
value phpstan-rs's own inference would resolve to `Type::Mixed` reaches
a context expecting something more specific. The production
implementation (`AssignmentAnalyzer.php:356-376`) does more than flag
the symptom: it walks Psalm's dataflow graph to report *where* the
value became `mixed` in the first place (an untyped `json_decode()`
result, an unstubbed third-party call), not just that it currently is.
This is real type-erosion detection - a `mixed` value silently
defeating the type system somewhere upstream - that no PHPStan-based
tool catches today. It's also the largest single item in this part: a
bare "this is mixed" version is a straightforward walk of expressions
phpstan-rs's checker already resolves to `Type::Mixed` at the point
they're consumed by a typed context, but the *useful* version (pointing
at *where* the value became `mixed`) needs the same kind of
provenance-tracking dataflow graph the taint-analysis item just below
would also need - not separable infrastructure, so realistically a
shared prerequisite for both.

**Effort: large** for the provenance-tracking version; **medium** for a
bare presence-only version with no "where did this come from" tracing.

### Investigated and deliberately not pursued (§AR)

Recorded here so a future pass over Psalm doesn't re-spend the same
research budget re-discovering the same "not worth it" verdict:

- **Taint analysis** (`Tainted*`, 17 issue types; `Internal/Codebase/
  TaintFlowGraph.php`, ~1,100 lines just for the graph itself, plus
  source/sink annotations threaded through most of the type-checking
  pipeline). Real, valuable, and completely absent from phpstan-rs and
  PHPStan alike - but architecturally a whole separate subsystem
  (a source-to-sink dataflow graph), not a bolt-on rule. Worth a
  dedicated future scoping pass of its own, not folded into this list.
  AQ1's provenance tracking above would share infrastructure with it.
- **The reference-aliasing family** (`ConflictingReferenceConstraint`,
  `ReferenceConstraintViolation`, `ReferenceReusedFromConfusingScope`,
  `UnsupportedReferenceUsage`, `UnsupportedPropertyReferenceUsage`) -
  read the actual implementation (`IfAnalyzer.php:200-215`,
  `ExpressionAnalyzer.php:378-388`, and siblings); these are Psalm's own
  internal "my by-reference type-tracking couldn't cope with this code"
  escape hatches (one message is literally "This reference cannot be
  analyzed by Psalm"), not a distinct user-facing bug class. Low value
  for the size of subsystem (a full reference-aliasing type model) it'd
  require.
- **`ComplexFunction`/`ComplexMethod`** (a complexity-limit warning) -
  real, but its metric is derived from Psalm's taint/dataflow graph's
  own edge statistics (`StatementsAnalyzer.php:886-910`), not a cheap
  standalone cyclomatic-complexity count. Not separable from the
  taint-analysis prerequisite above.
- **`ImplicitToStringCast`** - real PHPStan already has equivalent
  machinery (`Analyser/ExprHandler/ImplicitToStringCallHelper.php`);
  parity territory, not Psalm-exclusive.
- **`OverriddenFinalConstant`/`AmbiguousConstantInheritance`/
  `OverriddenInterfaceConstant`** (PHP 8.1+ final/interface-constant
  override conflicts) - real PHPStan already has this family
  (`Rules/Constants/OverridingConstantRule.php`); already tracked as a
  parity gap in Phase 133 (Constants), not added here to avoid a duplicate.
- **`UnsafeInstantiation`** (`new static()` without
  `@psalm-consistent-constructor`) - real PHPStan parity territory
  (`Rules/Classes/NewStaticRule.php`), not Psalm-exclusive.
- **`ParamNameMismatch`** (an overriding method renaming a parameter
  that named-argument calls depend on) - phpstan-rs already covers this,
  and more precisely: `checker/named_argument_renames.rs` tracks actual
  call sites project-wide (mirroring real PHPStan's Phase 82), where
  Psalm's version is a purely declaration-time static check.
- **`PrivateFinalMethod`** (redundant `final` on an unoverridable
  private method) - already covered
  (`checker/oop.rs:874-879`, `method.finalPrivate`, matching real
  PHPStan's identical rule).
- **`MissingImmutableAnnotation`/`MutableDependency`** (subclass/trait
  immutability-annotation consistency) - doesn't apply to this project's
  design: `registry::class_is_immutable` (`hierarchy.rs:80-96`) already
  walks `extends` transitively, so immutability is automatically
  inherited rather than needing to be redundantly restated: closer to a
  design improvement over Psalm's requirement than a gap.
- **`PropertyNotSetInConstructor`/`DuplicateEnumCaseValue`** - already
  covered (`checker/uninitialized.rs`, `checker/oop/enums.rs:39-59`).
- **`ReservedWord`** (a user class named `int`/`string`/etc.) - real PHP
  already rejects this at compile time; a static pre-check's practical
  value is marginal.
- **`RedundantFlag`** - narrow, single-function-scoped (`filter_var()`'s
  `flags`/`options` array keys specifically), not a generalizable
  pattern.

The rest of this list was added on a second, genuinely exhaustive pass
after the first pass turned out to have sampled rather than read every
one of the 325 Issue classes individually (the family-level bulk
verification described in this part's own intro above). These are the
additional items that pass turned up and ruled out - not because they
were skipped, but because each was individually read and found to
already be covered, out of scope, or too narrow to be worth the size of
change:

- **`InvalidOverride`** - turned out to be the exact mirror of
  phpstan-rs's own existing check: `#[\Override]` present with no real
  overridden method, or present on `__construct`
  (`FunctionLikeAnalyzer.php:2017-2023`) is precisely `own_sig.has_
  override_attr && !has_real_ancestor` at `method_overrides.rs:273-282`.
  Same check, different issue-class name.
- **`NoInterfaceProperties`** (assigning to a property through an
  interface-typed variable) - interfaces have no properties to find in
  the registry in the first place, so this already falls through to the
  general undefined-property-assignment path every other "property
  doesn't exist" case already goes through.
- **`NonInvariantPropertyType`/`NonInvariantDocblockPropertyType`**
  (property-type invariance on override) - already covered,
  `checker/oop/property_overrides.rs:29-53`'s `check_one_property_
  override`, mirroring real PHPStan's `OverridingPropertyRule`.
- **`ConstantDeclarationInTrait`** (PHP 8.2+ gate on trait constants) -
  already covered, `classConstant.inTrait`
  (`checker/oop.rs:710-731`), matching real PHPStan's
  `ConstantsInTraitsRule` exactly.
- **`TraitMethodSignatureMismatch`** - same visibility-on-override check
  already confirmed for `OverriddenMethodAccess` (`method.visibility`),
  just a trait-specific message variant in Psalm, not a distinct check.
- **`UninitializedProperty`** (reading a property before it's
  definitely initialized) - already covered by
  `checker/uninitialized_dataflow.rs`'s per-constructor "was this
  definitely assigned by this exact point" walk, a deliberately-scoped
  design choice (documented in that module's own header) rather than an
  oversight.
- **`LessSpecificReturnStatement`** - the same general declared-vs-actual
  return-type mismatch phpstan-rs's `return.type` already checks, just
  phrased "more general than" instead of "incompatible with."
- **`InvalidClassConstantType`/`LessSpecificClassConstantType`**
  (typed-class-constant override compatibility, PHP 8.3+) - real but
  narrow: typed class constants are rare, and overriding one with an
  incompatible/wider type rarer still. Read both (`ClassConstAnalyzer.php:
  787-810`), judged too low-incidence to be worth chasing further here.
- **`MoreSpecificReturnType`**, **`IncompatibleTypeParameters`** - both
  tied tightly to Psalm-internal inference mechanics (explicit-`never`
  union tracking; `TypeVariableTracker`'s own bidirectional bound
  resolution) with no clean general pattern to port.
- **`ArgumentTypeCoercion`/`PropertyTypeCoercion`/`MixedArgumentTypeCoercion`/
  `MixedPropertyTypeCoercion`/`MixedArrayTypeCoercion`/`MixedReturnTypeCoercion`**
  - conceptually the same "definite vs. possibly" severity split already
  proposed as its own systemic item (AP1) below, not a separate check.
- **`EmptyArrayAccess`** (indexing a provably-empty array) - an empty
  array shape has zero valid keys, so this is already a degenerate case
  of the general shape-key-existence check (`offsetAccess.notFound`)
  rather than needing its own special case.
- **`RedundantPropertyInitializationCheck`** (an `isset()` check on an
  already-non-nullable property) - same "redundant condition given a
  known type" territory L8 above already covers.
- **`CircularReference`** (a class constant whose own value expression
  references itself, e.g. `const A = self::A;`) - real (PHP treats this
  as a fatal error at runtime) but confirmed low-risk in phpstan-rs's own
  architecture: `infer_const_type` (`registry/function_sig.rs:165-198`)
  never recursively resolves a cross-constant reference at all - it
  falls straight to `Mixed` for anything beyond a literal - so there's no
  infinite-recursion hazard, just an undetected rare-in-practice bug
  class. Low enough incidence not to be worth its own item here.
- **`ParentNotFound`** (`parent::$prop` fetch in a class with no parent)
  - real but low-incidence; the `parent`-as-*type* case is already
  covered (`return.noParent`/`parameter.noParent`), just not this
  narrower `parent::`-as-property-fetch-prefix variant.
- **`CheckType`/`Trace`/`UndefinedTrace`** - Psalm's own `@psalm-check-type`/
  `@psalm-trace` debugging annotations, the same category as real
  PHPStan's own `\PHPStan\dumpType()`/`debugScope()` developer-debug
  helpers (out of scope: the hover-type lookup and Phase 110's T1/T2
  are phpstan-rs's own answer to that need) - not a user-facing code check at all.
- **`UnrecognizedExpression`/`UnrecognizedStatement`/`PsalmInternalError`/
  `ParseError`/`UnresolvableConstant`/`UnresolvableInclude`** - Psalm's
  own "I can't make sense of this" internal fallbacks and giving-up
  notices, not a portable diagnostic concept.
- **`IfThisIsMismatch`** - tied to Psalm's own `@psalm-if-this-is`
  conditional-`$this`-typing template mechanism, a Psalm-specific
  generics feature with no clean separation from Psalm's whole template
  system.
- **`InvalidGlobal`/`InvalidStaticInvocation`/`InvalidParent`/
  `InvalidAttribute`/`NoValue`** - each read individually; real but
  either tied to Psalm's own script-execution-scope model (`InvalidGlobal`),
  already covered by an existing check (`InvalidStaticInvocation`,
  same `method.static` family as `NonStaticSelfCall`), or narrow enough
  (a literal `: parent` return type with no parent class; a trait used
  as an attribute class; Psalm's own type-narrowing producing `Never`
  for an argument) that the size of a dedicated check isn't justified
  by how rarely it would fire.
- **`DuplicateParam`** (a function declaring the same parameter name
  twice) - real (confirmed via real PHPStan's own
  `Rules/Functions/RedefinedParametersRule.php`) but already documented
  as a parity gap in Phase 126 (bundled into its
  `InvalidParameterNameRule`/`RedefinedParametersRule` entry), not added here
  to avoid a duplicate.
- **`InaccessibleProperty`/`InaccessibleMethod`/`InaccessibleClassConstant`**,
  the rest of the **`Undefined*`/`Invalid*`/`Missing*Type`/`Deprecated*`/
  `Duplicate*`/`Impure*`(except L14)/generics/enum/interface/abstract**
  families, and **`UndefinedInterface`** - all individually read this
  pass (not just identifier-census-matched as the first pass did) and
  confirmed already covered by phpstan-rs's existing, comprehensive
  type-checking, visibility, and symbol-resolution machinery.

## Part 12: Findings viewer

### Phase 181 — Scan once, then browse and triage in a local web viewer (§AS)

PHPStan Pro, PHPStan's paid add-on, is essentially a web UI for browsing
an analysis's findings. phpstan-rs can offer the same for free, and go
further. It scans once, at the highest level, keeping findings that
inline ignores, `ignore_errors` or the baseline would hide. A local web
page then filters that one result by level, rule, path and ignore state
without rescanning. Each finding has one-click actions that write the
ignore or the fix to disk.

**Why one scan is enough, and the gate that keeps it true.** Today the
level is mostly a filter on finished results
(`project/session.rs`: `diagnostics.retain(|d| d.level <= self.level)`,
applied before inline ignores and `ignore_errors`). It isn't purely a
filter yet: `Registry::configured_level` drives emission-time acceptance
at thresholds 7/8/9/10, and some sites compute their level at runtime.
Levels phase E5 of the crate split turns every computed-level site into a
named parameter with a level. After that, the viewer's scan runs at max
with every parameter on, and tags each finding with the lowest level
that reports it.

Gate: for each level 0–10 and unset, the viewer's filter applied to the
max scan must equal a real run at that level, over the corpus's
per-level snapshots. Some parameter may change a finding's message
instead of adding or removing a finding. If so, the tag records each
variant with its level range, or the viewer rescans for that level;
the gate's first run decides which.

**Shape:**
- `phpstan-rs report`: one analysis through `Session`, kept as the
  unfinalized `AnalysisResult` (crate split F1), served on `127.0.0.1`
  as a single page.
- `report --export <dir>`: a read-only static copy, so a CI run's
  results can be browsed from its artifacts.
- The page's data is the versioned JSON schema (F3), plus each
  finding's minimum level and hidden-by reason. HTML, JS and CSS are
  embedded in the binary and work offline, with no CDN.
- It lives in its own app-layer crate above the session. The finalizer
  it shares with `check-project` (`AnalysisResult::finalize`) is the
  same one `GATE_SPEED_PLAN.md` S9's gate runner uses for its baseline
  and emission-order views. Once `report --export` ships, the corpus
  gate reads its views from the export and `tools/gate-runner` is
  deleted.

**Filters:**
- level (a slider, applied instantly);
- rule identifier, grouped by family;
- path tree;
- free-text search on the message;
- new versus baselined;
- a "hidden" toggle that shows findings suppressed by an inline ignore,
  `ignore_errors` or the baseline, each with the entry that hid it;
- unmatched ignores (`ignore.unmatchedLine`/`ignore.unmatchedIdentifier`).

Each finding shows the source around its span, its message, tips,
identifier and level, and a preview of its fix when it has one.

**One-click actions** (served mode only):
- **Ignore here:** writes `// @phpstan-ignore <identifier>`, using the
  same-line/previous-line semantics `Project::apply_inline_ignores`
  already implements. The file is then re-checked through the Session's
  incremental update, which picks up the shifted line numbers.
- **Ignore this identifier** in this file, or under a path glob: appends
  an `[[ignore_errors]]` entry to `phpstan-rs.toml`, editing it in place
  so the user's formatting and comments survive.
- **Add to baseline:** updates the configured baseline file.
- **Apply fix:** the same `diagnostics::fix::apply_fixes` path as
  `check-project --fix` and the LSP's quick fixes.

Every write first checks that the target file's content hash still
matches the scanned one, and refuses if it doesn't. Every action records
its reverse edit, so it can be undone from the page.

**Security.** The endpoints write files, so the server:
- binds to loopback only;
- requires a random per-run token in the URL;
- rejects requests whose `Origin`/`Host` isn't its own, which blocks
  DNS-rebinding and cross-site writes from other pages in the browser.

**Later:** a watch mode that keeps the page live as files change, on top
of the F2 result cache and the Session VFS.

**Depends on:** crate-split F1 (`AnalysisResult` and the finalizer), E5
(levels), F3 (versioned JSON schema); watch mode also needs F2. It starts
after Stage F.

**Effort: large.**


## Part 13: Templates

### Phase 182 — Latte templates analysed natively, parsed rather than compiled (§AT)

A `.latte` template is PHP expressions inside HTML and `{tags}`. The
existing route to analysing one - efabrica/phpstan-latte,
symplify/latte-phpstan-compiler, and the ~3,300-line pipeline Piwigo
maintains under `tools/phpstan/Latte/` - runs Latte's own PHP compiler
before the analyser, rewrites the generated code until PHPStan can type
it, analyses the generated files, then maps each error back through the
`/* pos L:C */` comments Latte leaves in them. phpstan-rs owns its PHP
lexer, parser and AST, so it skips the compile step: it parses the
template itself and lowers it straight into the analyser's input, every
node spanned in the `.latte` file. Ignores, the baseline, the use-site
index and the language server then see a template the way they see a PHP
file.

Every semantic rule below is read from the installed Latte's own source -
3.1.6 on the corpus (`Compiler/TemplateGenerator.php`,
`Essential/CoreExtension.php`, `Essential/Passes.php`, the
`Essential/Nodes/*` node classes, `Runtime/Template.php`,
`Loaders/FileLoader.php`), cited where each rule is stated.
The grammar and semantics are those of Latte 3.1, the corpus's line;
templates under another minor or major version are still analysed, with
one notice that they're read as 3.1, and a version this phase's rules
aren't re-verified against is added only after the same source reading.

**What Piwigo's pipeline has to do today** (the corpus analyses its
output as plain PHP under `_analysis/phpstan-latte/`):
- compiled filter and function calls, `($this->filters->translate)(...)`
  and `($this->global->fn->x)($this, ...)`, type as bare `callable`; the
  pipeline redirects them to generated shims carrying the real
  signatures, defaults included;
- every compiled method starts with `extract()`, which makes every local
  `mixed`; the pipeline injects `@var` types after each `extract()`, after
  each `{capture}` target, and for `{define}` parameters (Latte parses
  their types and discards them);
- variable types come from `{templateType}` (a View class and its public
  properties), a `{varType}` block the pipeline writes into each template,
  or a scan of the app's render calls;
- a custom error formatter maps findings back to templates, and a
  dead-code usage provider credits `{templateType}` View properties as
  read - no PHP code reads them, Latte extracts them into template locals.

Parsed natively, none of that rewriting exists: filters and functions are
calls with their signatures, declared types are declarations, and
findings are born on template lines.

**Corpus shape** (Piwigo, Latte 3.1.6, 119 templates, 19.5k lines): about
2,000 prints (`{$x}`/`{=...}`), 1,460 `{if}`, 1,050 `{varType}`, 170
`{foreach}`, 117 `{templateType}`, 74 `{spaceless}`, 55 `{block}`, 51
`{var}`, 51 `{include}`, 27 `{layout}`, 15 `{_}`; 289 `n:if`, 148
`n:foreach`, 58 `n:attr`. The engine enables `Feature::Dedent` and
`Feature::ScopedLoopVariables` (`src/Piwigo/Template/LatteEngine.php`).
Most of the 978 `property.unused` findings on `*\Projection\*View`
classes approved in C3 (`gates/approved/c3-provenance-declarations`) are
View properties no PHP code reads; how many of them a template reads is
182g's delta.

**182a — Latte syntax.** A lossless front end for Latte 3 in its own crate
(`latte-syntax`, depending on `syntax` only), following
`TemplateLexer`/`TemplateParser`/`TemplateParserHtml`/`TagLexer`/
`TagParser`:
- an HTML-aware lexer: text, elements and attributes, comments, bogus
  tags (`<!...>`, `<?...>`), raw-text elements (`<script>`, `<style>`,
  `<textarea>`, `<title>`) and attribute-value contexts, with `{...}`
  found in each; `{* comments *}`, `{l}`/`{r}`, `{syntax}`/`n:syntax`, and
  a `{` followed by whitespace staying literal;
- elements as `TemplateParserHtml` builds them: a name that may be an
  expression (`<{$tag}>`, `<h{$level}>`), attributes produced by tags
  (`<span {if $x}class="a"{/if}>`), void elements, a self-closed non-void
  element (`<div/>`) read as an empty one, and an unclosed or
  mis-closed element tolerated - closed at its parent, as Latte collapses
  it - except under `StrictParsing` or when it carries an `n:`
  attribute, has an expression name or is raw text, where Latte rejects
  it;
- tags as `(name, arguments, modifiers)`: paired and void, `{/tag}` and
  `{/}` closers, `n:` attributes including the automatic `n:inner-`/
  `n:tag-` forms of every pair tag (neither on a void element), and
  `n:else`/`n:elseif`/`n:ifcontent`, an `n:` attribute's value written
  quoted, as `{...}`, or as a bare word;
- arguments parsed by `syntax`'s expression parser in a Latte mode (a
  grammar switch in the `syntax` crate itself, so both grammars share one
  parser) whose
  grammar is `TagParserData`'s (Latte's own generated LALR tables), not
  PHP's: Latte's additions - filter chains `|name:arg1, arg2`, `in`, the
  `key: value` argument shorthand, unquoted words where a tag accepts them
  (`{include file.latte}`, `{block name}`) - and none of the PHP syntax
  Latte's grammar leaves out, which is a parse error in a template even
  where PHP would accept it;
- recovery on malformed input: a template mid-edit still yields a tree
  and one diagnostic, like the PHP parser.

Gates, over every corpus template: printing the tree back reproduces the
source byte for byte; and each tag and print Latte's compiler marks with
`/* pos L:C */` in its output has a node at that line and column, and
each tag or print node that compiles to code (not `{varType}`,
`{templateType}`, `{parameters}` or a comment) has its marker - a
structural oracle from the real compiler, needing no semantic agreement.

**182b — Engine configuration.** A template's meaning depends on the
`Latte\Engine` rendering it: the features (`StrictTypes`,
`StrictParsing`, `ScopedLoopVariables`, `Dedent`, `MigrationWarnings`),
the default content type, the loader, sandbox mode and its `Policy`, and
what is registered on it. These are read from the app's own engine setup
where it's written with literals (`setFeature(Feature::X)`,
`setStrictTypes()`, `setContentType()`, `setLoader(new FileLoader(...))`,
`addExtension(new X(...))`, `addFilter('name', ...)`,
`addFunction('name', ...)`, `addFilterLoader(...)`, `addProvider(...)`),
and each is overridable under `[ext.latte]`. `StrictTypes` (on by
default) decides whether the lowered code is checked under
`strict_types=1`, as the compiled template declares it; `Dedent` changes
only whitespace, so nothing models it; the content type
decides the HTML-only behaviour (URL checking, the `<script>` rules),
changed by a `{contentType}` in the header (or, for an HTML type, inside
a `<script>` element - the one place outside the header Latte allows it)
and by each element's own context (`<script>`, `<style>`, an attribute); `MigrationWarnings`
turns the attribute-rendering changes Latte 3.1 warns about at runtime
under it (`HtmlHelpers::triggerMigrationWarning()`: a `null` value now
omitting the attribute, `true`/`false` in `data-*` now rendering as
`"true"`/`"false"`, an `on*` string no longer JSON-encoded, a `bool` in
`aria-*`, a `null` XML attribute) into findings, unless the value carries
`|accept`; and
a sandboxed engine's `Policy`, when written with literals
(`SecurityPolicy::createSafePolicy()` and its `allow*()` calls), is
checked (182f). A project can configure
several engines: Piwigo configures its runtime one in
`src/Piwigo/Template/LatteEngine.php`, and three more in its compile, lint
and shim-generation commands, which register the same extension but none
of the runtime features. Registrations are unioned across engines (an
extra registered filter can only quiet a finding), while features and the
content type must agree; where they don't, the conflict is reported once
and `[ext.latte] engine` names the class whose setup is read.

**182c — Scopes and lowering.** A template becomes analysis units, not a
synthetic PHP class: no class, method or function is declared, so
declaration-level rules never fire on a template, while statement- and
expression-level rules run on its code unchanged. The units and what
each starts with, as `TemplateGenerator` and `Runtime\Template` build
them:
- the template's prepared variables (`prepare()`): its header code run
  over the passed parameters - with `{parameters}`, exactly the declared
  parameters, each defaulted, and nothing else (no `extract()`, a closed
  set); otherwise every passed parameter (an open set) - plus whatever the
  header itself defines;
- the template body (`main()`): the prepared variables;
- a `{block}` or `{define}` body: the template's parameter array
  (`extract($this->params)`) - the prepared variables when the template
  has a layout (`Runtime\Template::render()` replaces `$this->params`
  with them before rendering the parent), the raw passed parameters
  otherwise, so even a `{parameters}` template's blocks see an open set -
  then, over it, the variables where it's rendered (`get_defined_vars()`
  at the block's own position, or at the `{include block}` that renders
  it), or, for a `{define}` that declares parameters, those parameters
  instead of the render site's variables, their declared types kept
  (Latte parses and discards them);
  a dynamic block (a name computed at runtime) gets no parameter array,
  and an embedded block reads the `{embed}`'s layer (`varStack`);
- an included file: its arguments plus the includer's parameter array -
  not the includer's `{var}` locals (`createTemplate($name, $args +
  $this->params)`);
- a layout parent (`{extends}`/`{layout}`, where `auto` asks the loader
  for the layout and `none` has none): its arguments plus the
  child's prepared variables, with the child's blocks rendered in the
  parent's place;
- a block `{import}` brings in: the `{import}`'s arguments plus the
  importing template's parameter array, as the imported template is
  created with them (`createTemplate($name, $args + $this->params,
  'import')`).

A unit rendered from several places starts with the union of what each
place gives it, and a variable some place doesn't give is possibly
undefined there.

Core tags (`CoreExtension::getTags()`) lower to what each node class's
`print()` generates, which isn't always the PHP construct of the same
name:
- control flow: `if`/`elseif`/`else`, `n:else`, `ifset` (narrowing as
  `isset()` does, and `{ifset block name}`/`{ifset #name}` testing a
  block's existence), `ifchanged` (its arguments compared strictly with
  their previous iteration's, or with none its own rendered output, with
  an `{else}`); `switch` comparing strictly (`===`,
  `in_array(..., true)` for a `{case a, b}` list) with no fallthrough,
  so an `if` chain rather than PHP's loose, falling-through `switch`;
  `for`; `while`, including the `{while}...{/while $cond}` form that tests
  after the body; `skipIf`/`breakIf`/`continueIf`/`exitIf` (`skipIf`
  leaving `$iterator->counter` alone); `try` catching every `Throwable`,
  discarding the block's output and rendering its `{else}`, and restoring
  `$iterator` after it, with `rollback` throwing into it; `foreach` with
  `{else}`, `$iterator` typed
  `Latte\Essential\CachingIterator` over the loop's iterable and chained
  to the enclosing loop's (`getParent()`), `{first}`/`{last}`/`{sep}`/
  `{iterateWhile}` read through it - `{iterateWhile}` advancing the
  loop's own iterator and reassigning the loop's key and value variables
  from `$iterator->key()`/`current()`, with a form testing after its
  body - and, under `ScopedLoopVariables`, the
  loop's key and value variables live only inside the loop, the outer
  ones restored after it;
- variables: `var` (a declared type kept, which Latte parses and
  discards), `default` (assigning only a variable that doesn't exist yet,
  `array_key_exists()` on the scope, so its type is the existing one or
  the default's), `capture` (its target, by
  `CaptureNode`'s own code: `Latte\Runtime\Html` or `''` in HTML text,
  a `string` elsewhere, and with filters the last filter's return type,
  wrapped back into `Html` in HTML text - never `false`, the buffer being
  its own), `parameters`, `do`, `php` (with
  `RawPhpExtension`);
- output: `{$x}`/`{=...}`, `n:attr`, `n:class` (its truthy arguments
  joined with `implode()`, so each is printed), `n:tag`, `n:ifcontent`,
  `contentType`,
  `spaceless`, `dump`/`trace`/`debugbreak`; an unquoted attribute value
  `name={expr}` in HTML renders by the attribute's kind
  (`ExpressionAttributeNode`, `HtmlHelpers::classifyAttributeType()`):
  a boolean attribute (`checked`, `disabled`, ...) by truthiness, a
  space-separated one (`class`, ...) from a string or a list, `style` from
  a string or a declaration map, `data-*` from a scalar, a `bool`
  (`"true"`/`"false"`) or JSON for an array or `stdClass`, `aria-*` with
  its own `bool` rendering, any other from `string|int|float|
  Stringable|null` (`null` omitting it); `|toggle` forces the boolean
  kind and a trailing `|json` the JSON one. That's any attribute whose
  whole value is one escaped print, quoted or not (`name={$x}`,
  `name="{$x}"`, as `parseAttribute()` simplifies the value); a value
  mixing text and prints, or printed with `|noescape`, is text
  interpolation;
- structure: `block` (named and unique in its layer, `{block local}` in
  the template's own layer that neither a layout nor an importer sees,
  and the anonymous `{block|filter}` that only filters its content),
  `define`, `include` (file - `with blocks` also bringing its blocks into
  the including template - block, and `parent`/`this` inside a block,
  rendering the overridden or current block), `embed` (of a block or a
  file - explicit, or a bare name meaning a block - its content outside a
  `{block}` becoming the implicit `{block default}`),
  `import`, `extends`/`layout`, `varType`, `templateType`,
  `templatePrint`/`varPrint`.

A print is a template output node carrying its expression, not Latte's
escaping call; its own check (182f) asks whether the value can be
printed. A tag nothing registers is reported (182f), its arguments still
checked as expressions.

**182d — Types.** Beyond the parameters and locals 182c builds, in
precedence order - a declared type wins over an inferred one, and an
inferred one only types a variable nothing declares:
- `{templateType Cls}`: each of `Cls`'s public properties types the
  parameter of that name, as `render()` passes an object
  (`Helpers::resolveParams()`: `get_object_vars()` from outside the class,
  so its public initialized properties, plus PHP 8.4 virtual properties
  with a `get` hook); the tag itself compiles to nothing, so this is the
  app's declaration, trusted like `@var`;
- `{parameters}`/`{define}` parameters: their declared types;
- `{varType Type $x}`: `$x`'s declared type wherever `$x` is in scope in
  that template, trusted the same way;
- `{include}` sites: an included template's parameters are typed by each
  includer's arguments and parameter array, unioned over the includers;
- render call sites found in PHP (`Engine::render()`/`renderToString()`/
  `createTemplate()`, with a template name the loader resolves and a
  parameter array or object): each parameter's type at each site, unioned
  over the sites;
- otherwise `mixed`.

The render sites are also checked against what the template declares:
a PHP render call whose array or object passes a parameter the
template's `{parameters}` or `{templateType}` types differently, or
leaves out a required `{parameters}` entry, is reported like a call
argument (`latte.render.*`).

In the closed set a `{parameters}` template has, an undeclared variable
is undefined for certain: the ordinary `variable.undefined` reports it.
In an open set any passed parameter can exist at runtime, so an
undeclared one is `mixed`, never undefined; an opt-in
`latte.variable.undeclared` reports variables no declaration covers, for
projects that declare every variable. App conventions beyond these
(Piwigo's `assignContext()` bag, its `#[Template]` View attribute) aren't
modelled: the native declarations type 114 of the corpus's 119
templates, and `{varType}` the rest.

**182e — Filters and functions.** Latte's built-ins come from the
installed latte/latte, read as vendor context (`CoreExtension::
getFilters()`/`getFunctions()`, `TranslatorExtension`), so their
signatures always match the installed version. Registrations (182b) are
read statically: an extension's `getFilters()`/`getFunctions()`/
`getTags()` arrays, and `addFilter()`/`addFunction()`. A parameters
object adds its own: `render()` registers the public methods its class
marks `#[Latte\Attributes\TemplateFilter]` or `#[TemplateFunction]`
(`Helpers::resolveParams()`), so they're filters and functions in each
template rendered with that class, `{templateType}` included. Each callable is
resolved to its real signature: a first-class callable
(`strtolower(...)`, `$this->translate(...)`, `self::jsonEncode(...)`,
`$this->template->once(...)` through the property's type), a closure, or
`[$obj, 'method']`. At the call, as the executors do at runtime:
- a filter whose first parameter is `Latte\Runtime\FilterInfo` gets it
  injected (the content type), and a function whose first parameter is
  `Latte\Runtime\Template` gets the template;
- a registered function's name shadows the PHP function of that name
  (`customFunctionsPass`); a PHP function is used only when none is
  registered;
- `noescape`, `nocheck`/`noCheck`, `datastream`, and on an attribute
  value `toggle`, `accept` and a trailing `json`, are modifiers, not
  filters, and a URL attribute's print gets `checkUrl` appended the way
  `checkUrlsPass` appends it.

With a filter loader registered (`addFilterLoader`), a filter name
nothing else registers can't be called unknown; it becomes `mixed` and
the unknown-filter check stays quiet. `{translate}...{/translate}` and `{_ ...}` (`TranslatorExtension`) are the
`translate` filter over their content or argument, typed by the
translator callable the extension is constructed with. A custom tag is
PHP code run at compile time and can't be read: `[ext.latte] tags` maps one to a core
tag's meaning (Piwigo's `{_ ...}` behaves as `TranslatorExtension`'s
`{translate}`). The companion extensions of Latte's own ecosystem are
the exception, as their packages are known: nette/application's
`UIExtension` (`{link}`, `n:href`, `{plink}`, `{control}`,
`{snippet}`/`{snippetArea}`, ...) and nette/forms' `FormsExtension`
(`{form}`, `n:name`, `{input}`, `{label}`, ...) get their semantics when
the package is installed, read from its own node classes - most Latte
templates are Nette applications'. The corpus uses neither, so a
fixture set stands in for its oracle.

**182f — Checks.** Latte's own compile-time errors, each a diagnostic
where Latte would throw a `CompileException`: the validations in the node
classes (`{parameters}`, `{templateType}`, `{extends}`/`{layout}` and
`{contentType}` outside the header, an unclosed or mismatched tag, an
unknown tag, `n:else` without its `n:if`/`n:foreach`, a block name used
twice in one layer, `n:tag` on `<script>`/`<style>`, a `|noescape` where
no filter is expected, `{iterateWhile}` outside a `{foreach}`, `n:class`
beside a `class` attribute, `n:ifcontent` on an empty element,
`n:embed`, loose content beside an explicit `{block default}` in an
`{embed}`, `{spaceless}` where it can't apply, an unknown `n:` attribute
- with Latte's own "did you mean" suggestion - or one given twice, one
inside a `{tag}`, `n:inner-`/`n:tag-` on a void element, an element name
that's a tag rather than an expression, ...) and in the
core passes (`$GLOBALS` and `$ʟ_*` always; `$this` and `$__*` a
deprecation unless `StrictParsing`, an error with it; a registered
function used as a partial `fn(...)`; a print followed by quotes inside
`<script>`; strict-HTML errors under `StrictParsing`); and the
deprecations Latte raises at compile time (`{first}`/`{last}`/`{sep}`
outside a `foreach`, among others), as deprecation findings. Then, with `latte.*` identifiers:
- an unknown filter or function (an unknown tag is Latte's own compile
  error, above);
- a `{foreach}` key or value variable named like one of the template's
  parameters, without `ScopedLoopVariables` - Latte's own
  `overwrittenVariablesPass` makes that a runtime notice
  ("Variable $x overwritten in foreach") when the template is rendered
  directly or as a layout parent;
- a template `{include}`/`{embed}`/`{import}`/`{extends}`/`{layout}`
  names that doesn't exist, and an `{include block}` no template in the
  chain defines;
- a printed value that can't be printed (an array, an object without
  `__toString()`);
- an attribute value of a type its kind doesn't render - Latte's runtime
  warning `HtmlHelpers::triggerInvalidValue()` ("Invalid value for
  attribute ...: array is not allowed"), such as an array, a `bool` or an
  object without `__toString()` for an ordinary attribute;
- a `{varType}` or `{parameters}` type contradicting `{templateType}`'s
  for the same variable;
- arguments against the signature they're passed to, as a call's are:
  an `{include}` of a `{define}` or of a file whose `{parameters}` declare
  types, and an `{extends}`/`{layout}`/`{embed}` of one - wrong types,
  unknown named arguments, missing required parameters;
- a `{parameters}`, `{define}` or `{var}` default its declared type doesn't
  accept, as a PHP parameter default is checked;
- a `{define}` never rendered (opt-in, with dead code);
- a filter applied to content (`{block|name}`, `{include ...|name}`,
  `{capture|name}`) that isn't content-aware (no `FilterInfo` first
  parameter) while the content is HTML - Latte throws at runtime there
  (`FilterExecutor::filterContent()`: "called with incompatible content
  type", suggesting `|stripHtml` first);
- in a sandboxed engine, a tag, `n:` attribute, filter, function, method
  or property its `Policy` doesn't allow, as Latte's
  `SecurityViolationException`.

Each `latte.*` identifier sits at the level of its PHP counterpart (an
unknown filter or function where `function.notFound` does, an
unprintable value where an invalid echo does), so a level filters
template findings the way it filters PHP ones.

Template names resolve as the configured loader resolves them - for
`FileLoader`, `getReferredName()`: a relative name against the referring
template's directory, else against the base directory. A loader the app
defines itself (Piwigo resolves through its theme chain) is configured as
an ordered list of directories, and a name several resolve is checked
against each. A name computed at runtime (`{include $file}`) isn't
resolved.

The existing checks run unchanged on the lowered code: undefined methods
and properties, argument types (filters and functions included),
`foreach` over a non-iterable, possibly-null access, and the rest.

**182g — Whole-program integration.** A template reads through the
use-site index like PHP code: a method called or a property read on a
value in a template is a use. A template variable `{templateType Cls}`
types is `Cls`'s property of that name, so reading `$title` in the
template uses `Cls::$title` - the link the corpus's View classes have to
their templates. The C3-approved View `property.unused` findings for
properties templates read go away, as an approved delta; properties no
template reads stay reported. A template's calls, class references and
template references are dependencies: editing a PHP class re-checks the
templates reading it, and editing a template re-checks those including,
embedding or extending it. `{* @phpstan-ignore <identifier> *}` suppresses
as the PHP comment does, and baseline entries key on the `.latte` path.
The Session discovers `*.latte` files as their own file kind (crate split
F1's VFS), in parallel with PHP files and under the same result cache:
templates under the analysed paths are analysed and reported, and ones
elsewhere that a template includes, embeds, imports or extends (a
vendor package's) load as context, resolved but never reported, the way
vendor PHP loads.

**182h — Language server.** Diagnostics for `.latte` documents; hover,
go-to-definition and completion inside tag expressions, over the same
lowered units; completion of tag names after `{`, filter names after
`|`, and template and block names in `{include}`/`{extends}`/`{block}`;
go-to-definition from those to the template and block they name. The VSCode extension registers the
`latte` language.

**182i — Retiring the compiled output on the corpus.** Once 182a-g match
the oracle, the corpus config drops `_analysis/phpstan-latte` from its
paths and its generated-code `ignore_errors` entries, as one approved
delta.

**Tests.** A `.latte` fixture kind in the fixture harness
(`tests/fixtures/latte/<category>/<name>.latte` with its `.expect`, and a
directory form for includes, layouts and several engines), one category
per sub-phase.

**Oracles.**
- Findings: Piwigo's own pipeline. Compile with `bin/piwigo
  phpstan-latte:compile`, run real PHPStan on the generated files, and
  map each error back with the pipeline's `GeneratedFileErrorMapper`; the
  compiled files' `/** source: ... */` headers name absolute paths in the
  checkout that produced them, so they're resolved against the corpus
  root. Native findings must match per template, each difference either a
  disclosed gap or a finding the pipeline misses (its variable map widens
  to unions where it can't trace a render call), confirmed against the
  template.
- Compile errors: a fixture per `CompileException` 182f lists, each
  compiled by the real installed Latte; the set of templates Latte
  rejects, and where, must equal ours.

**Architecture.** `latte-syntax` sits directly above `syntax` and below
everything else;
everything else lives in an `ext-latte` crate: it registers the `.latte`
file kind, the lowering, the type sources, the filter and function
resolution and the checks through the Session's and the analyser's
registration points, and `app` registers it when it's active. Activation
follows C1: active when latte/latte is installed in the composer root, or
when `[ext.latte] enabled` says so.

**Later, not in this phase:** Twig and Blade on the same front-end,
scope, lowering and types design; escaping-context checks (`|noescape`
on unescaped input, a URL attribute fed a non-URL).

**Depends on:** 182a on nothing; 182b-f on the crate split's Stage E
(rules as registrations and the derived nodes they read); 182g-i on F1
(the Session, its VFS and file kinds) and F2 (the result cache and
dependencies). It starts after Stage F, except 182a.

**Effort: very large.**

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
  pre-loop type for the whole check, misinterpreted as "always
  terminates"/"always true or false" on extremely common idioms. Fixed
  by building fixed-point loop convergence, then rebuilding the family
  in full rather than staying narrowed to bare syntactic literals.
- **`InvalidKeyInArrayDimFetchRule`** (Phase 50; superseded - reattempted
  and shipped successfully, see Done). The original revert's real
  soundness bug: a *used* generic template parameter's own type leaked
  through as a bogus namespace-qualified class reference, indistinguishable
  from a genuine invalid array key. The reattempt found the real root
  cause was narrower than originally framed.
- **Genuinely-empty-concrete-method-body check** (Phase 61; superseded -
  re-shipped opt-in, see Done). Verified individually correct and NOT a
  soundness bug - the corpus regression (+10,623, nearly all in
  `jetbrains/phpstorm-stubs`) was entirely one real, ubiquitous PHP
  convention (an IDE-stub/native-extension class declaring a native
  function's signature with a deliberately empty body) that real
  PHPStan never encounters, since it loads stub files through a
  separate, non-rule-checked mechanism. Re-shipped behind `Registry::
  check_empty_method_bodies` (default off), per
  [[feedback_dont_discard_correct_checks_ship_opt_in]].
- **`Foreach`'s own "may run zero times" pre-loop-state merge**
  (originally Phase 29, re-attempted and re-reverted, then shipped in a
  narrower form - superseded). Both earlier attempts hit a real,
  demonstrated soundness bug: giving `Foreach` the same
  `While`/`For`-style zero-iteration merge breaks the common "a loop
  mutates the very array it iterates via its own loop variable" idiom.
  A later re-attempt with fixed-point loop analysis in place still
  regressed a real corpus file via a narrowing-wrapper interaction. It
  is now live (commit `ce5b411`): `LoopZeroIterations::Possible {
  subject_exempt }` merges every variable with the pre-loop entry except
  the `foreach`'s own bare-`$variable` subject, which keeps its post-body
  value. Exempting only the subject is a narrow, one-variable version of
  Phase 123's O1 asymmetric join. O1 itself, for every other variable,
  is still open.
- **`@phpstan-type`/local type-alias support, first attempt** (Phase
  48). Implemented and reverted after a net-negative corpus regression,
  root-caused to a separate, real, pre-existing gap (`define()`-declared
  global constants resolving to bare `Mixed`). Successfully reattempted
  once that gap closed, now shipped (see Done, Phase 36).
- **Anonymous-class ancestor-aliasing shortcut** (superseded - the real
  fix shipped as commit `701be86`, see Done). Aliasing an anonymous
  class's inferred type to whichever single `extends`/`implements`
  target it names was implemented and corpus-verified, then reverted:
  unsound for the common "spy" test-double pattern (a class implementing
  a real interface but also declaring its own extra instrumentation
  members a test reads directly). The real fix needed registering the
  anonymous class's own full declaration under a stable synthetic name.

---

# Known limitations

Deliberate "this checker doesn't model X" boundaries not tied to any
one still-open phase above (a phase's own gap is described in its own
Pending entry instead).

## Known but not yet scheduled

Phase 140's sub-items (140a-140d) are the first four bullets. Each has
a specific, corpus-checked reason to stay deferred.

- **140a.** `strtotime()`'s real accuracy and a raw SQL/DQL query's own
  result shape (beyond what the Doctrine/SQL-reflection phases above
  already cover) are structurally infeasible in the general case without
  embedding a PHP runtime or a full SQL parser respectively. Real
  PHPStan needs exactly that too.
- **140b.** A long tail of smaller type-system precision gaps, each
  triaged individually rather than as one bucket: no per-location
  type-inference caching architecture (a performance mechanism, not
  correctness - Phase 95's C1/C3 are the actual build items in this
  space; revisit only if profiling calls for it); cosmetic-only nested-
  `Intersection` flattening (zero confirmed false positive/negative);
  `static`/`$this` late binding has no attached calling class (a real
  architectural gap, no concrete corpus false positive traced yet);
  `class-string<T>`'s own `instanceof`/sealed-hierarchy narrowing is
  unmodeled (see Phase 85's A5); no general `TypeCombinator`
  intersect/remove or generic recursive type-tree traverse/map
  combinator (a refactor-timing call, revisit once several bespoke
  traverse/map functions accumulate).
- **Nested writes through dynamic keys.** `$h[$bus][$message] =
  array_merge(...$byPriority)` inside `foreach ($h as $bus => ...)`
  leaves `$h`'s element type at its pre-loop shape instead of the
  written one - symfony/messenger's `MessengerPass` shows it as a
  spurious `Reference::__construct()` `argument.type` (unscoped corpus).
- **140c remainder.** A `foreach ($arr as &$v) { $v['key'] = ...; }`
  by-reference mutation now does update `$arr`'s tracked element type
  (see Done's `CategoryCatsRenderer::render` entry), but only when the
  subject is a bare variable whose element type is already uniform
  (the same scope `array_element_type` binds the loop variable from). A
  per-field `ArrayShape` subject, or a non-variable subject
  (`$this->items`, `$data['rows']`), still isn't updated. Real PHPStan
  tracks both. Phase 123's O1 is the closely related foreach-merge fix;
  this is a distinct, narrower by-ref-mutation-tracking gap.
- **140d.** Dynamic-key `isset($arr[$dynamicKey])`/`array_key_exists(
  $dynamicKey, $arr)` never narrows the specific element's own value
  type - only a *literal* string key does today. A real, buildable
  feature (extending `array_value_type_for_key`'s literal-key-only
  mechanism to a narrowed dynamic key expression), confirmed live at
  four real corpus call sites. Not attempted yet because it's a
  materially larger change (value-keyed, not name-keyed, narrowing) than
  a corpus-cleanup pass should take on, and each finding it produces
  today is sound, not a false positive to chase down.
- **Correlated-narrowing family** (`ConditionalExpressionHolder`): the
  *general*, unbounded mechanism (an arbitrary boolean-formula memory
  over recorded facts) stays declined - no natural stopping point,
  unlike a single named idiom. Every named instance found in this
  project's own corpus work has been individually assessed on its own
  merits instead (most are now built - see Done's own narrowing-engine
  cluster). One real, still-open sub-case: a cross-*function* version
  of the parallel-accumulator-array correlation (the same-function case
  is filed and buildable - Phase 173, see Planned; the cross-function
  generalization splits into two layers - the two-pass collect-then-
  consume checking architecture (Phases 166/174, now built - see Done's
  "Too-wide return/property types" entry), necessary but not sufficient
  on its own, and "soundly answering whether the correlation still holds
  by the time a different function reads it" - a fundamentally harder,
  unbounded invariant-maintenance problem with no known bounded design
  yet, the actual wall). (The compound `RateService::rate`-shaped case,
  discriminant-variable correlation carried across a closure capture, is
  now built - Phase 175, see Done.)
- **A dynamic array read (`isset`/`array_key_exists`-guarded or not)
  built by a single-loop accumulator idiom can read back `T|null`
  instead of `T`.** Root-caused to one level upstream of narrowing
  entirely (not a missing narrowing feature, confirmed via direct A/B
  testing against real `phpstan/phpstan` 2.2.8, which reports zero
  findings for the minimal repro): `LoopZeroIterations::Possible`'s
  symmetric pre-entry/post-body merge for `foreach` keeps a literal
  empty-shape branch alive even once the loop body has run, which
  `index_result_type`'s own separately-correct "empty shape + dynamic
  key -> `Null`" rule then fires on. This is the exact gap already
  scoped as this project's own Phase 123 "O1" (`foreach` needs a real
  *asymmetric*, certainty-downgrading join, not a symmetric union) - a
  real corpus sweep found this exact signature in 33 of 70 (47%)
  `argument.type`/`return.type` findings in one scoped Piwigo run, a
  deliberate, repeated "parallel accumulator map" idiom throughout the
  codebase, not a handful of isolated sites.
- **erp-web corpus round** (a real, closed-source Laravel app): 4 real,
  general engine bugs found and fixed (see their own Done entries -
  `break`/`continue` with a parenthesized level, PHP 8.2+ trait
  constants, mixed-union collapsing, `use (&$x)` closure-capture
  write-back). Two design questions left open, not general-engine bugs:
  `arrayValues.list`'s own scoping (currently fires on defensive coding
  against an unenforced-at-runtime PHPDoc `list<T>` contract - a
  deliberate-scoping decision, not resolved); and a large (~75-80% of
  diagnostics in that corpus) Laravel/Larastan-shaped stub-layer gap
  this project has no equivalent of at all (untyped Eloquent/Command
  base-class properties; fluent facade/Mockery-chain calls resolving to
  a broad interface instead of the concrete runtime type; a few
  variadic-via-`func_get_args()` signatures; nested local `@phpstan-type`
  alias composition; `array_filter()`'s own falsy-member-dropping
  narrowing) - a real, scoped future item (a Larastan-equivalent stub/
  extension layer), not attempted here.
- `token_get_all()` returns bare `array` - confirmed not a gap versus
  real PHPStan specifically (no dynamic-return-type extension exists for
  it there either).

## Permanent design boundaries (not planned, by choice)

- A generic bound checked against a class this project has no real data
  for (a PHAR-bundled dependency with no loose source vendored, or an
  unparsed local type alias) silently skips the check.
- Integer overflow/precision, floating-point rounding, and other
  runtime-value-dependent behavior are out of scope by construction -
  this checker only ever reasons about static types, never concrete
  values.

## Won't-port (real PHPStan features with no applicable Rust target)

An exhaustive 855-file audit against `vendor/phpstan-src/src/Rules/` and
`Type/` classified ~208 files this way. Recurring patterns, not
individually itemized:

- PHPStan's own BC-promise/reflection-migration machinery (`Api/*`,
  `GetTemplateTypeRule`) - gates on code referencing PHPStan's own
  `PHPStan\*` namespace; phpstan-rs has no such namespace for analyzed
  code to misuse. The real, buildable exception this label previously
  mis-bucketed (rules gating on the *analyzed class itself* implementing
  a `PHPStan\*` interface, not on a referenced class) is now built - see
  the PHPStan-API-usage checks entry in Done.
- `Playground/*` - registered only in phpstan.org's own issue-bot
  service, never the CLI analyzer.
- DI/plugin-architecture wrapper classes with zero logic of their own -
  phpstan-rs has no DI container or plugin model and no per-type-kind
  class hierarchy needing default no-op methods; every check is a
  hardcoded function/match arm. One real, narrow gap hides behind this
  label: real PHPStan's `generalize()` (loop-fixpoint type widening) has
  no phpstan-rs counterpart - a candidate for folding into the loop-
  convergence family if a corpus surfaces it.
- PHP-runtime/XML-shell-out techniques - PHPStan validates by literally
  constructing the real object at analysis time, since it runs inside
  PHP itself; phpstan-rs embeds no PHP runtime.
  `DateIntervalConstructorThrowTypeExtension`'s and
  `DateTimeConstructorThrowTypeExtension`'s own contributions are no
  longer in this bucket - both built by reusing the hand-rolled spec-
  grammar validators this project already ships (confirmed against real
  `php -l`/`php -r`, not runtime construction).
- A handful of one-off inapplicable rules: `AllowedSubTypesRule`
  (sealed-hierarchy extension point, inert even in stock PHPStan without
  third-party config); `ClassNameUsageLocationCreateIdentifierDynamicReturnTypeExtension`
  (PHPStan's own dogfooding self-analysis only); `InnerFunctionRule`
  (phpstan-rs intentionally does the opposite - recursing into nested
  named-function declarations, for the common `function_exists()`-guarded
  polyfill idiom); `EnumCaseOutsideEnumRule` (structurally impossible in
  this AST); `ReadWritePropertiesExtension` (third-party DI extension-
  point interface, no plugin architecture to attach it to).

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
