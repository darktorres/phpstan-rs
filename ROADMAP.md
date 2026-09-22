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
- Test suite: 4471 unit tests, all passing (build/test/clippy clean).

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
`object{...}` shapes (including a genuinely unsealed `array{..., ...}`),
`value-of<T>`/`key-of<T>`, `OffsetAccessType` (`SomeShape['key']`), and
conditional PHPDoc return types are all parsed and resolved. A
conditional return type now resolves to a specific call site's real
branch (`Type::Conditional`, `types::resolve_conditional`) rather than
always widening to the union of both branches, with a safe union
fallback when no bound argument exists to resolve against.
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

PHPDoc-inheritance backfill: a method correctly omitting its own
`@param`/`@return` in favor of an ancestor's/interface's tag resolves
through that tag, including through anonymous classes and promoted-
constructor-property inline `@var` tags, with real explicit-vs-implicit
`mixed` provenance surviving the hop and the backfilled type reconciled
against the child's own native type hint (not cloned verbatim) - this
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
against every real `new MixedType(true)` site in `vendor/phpstan-src`.
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
guards `function.notFound`. `is_a()` narrowing (PHASE_BACKLOG Phase 167)
reuses `instanceof`'s own primitives directly.

**Fixed-point loop-body convergence** (Phase 55, `Checker::
converge_loop_body`, mirroring real PHPStan's own bounded
`LOOP_SCOPE_ITERATIONS`): a
`While`/`DoWhile`/`For`/`Foreach` body is checked against a candidate
entry state, then re-checked with that entry unioned against the
previous pass's exit whenever the two differ, capped at 3 total passes,
with only the final pass's diagnostics kept. This is the foundation the
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
whatever the annotation preamble had set); by-ref closure-
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
visibility property's own narrower dead-write check. Duplicate-
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
checking wired to enums.

Invalid-operand-combination detection (`ErrorType`): the operand-
validity matrix across binary/comparison/unary/inc-dec operators, plus
division/modulo-by-literal-zero detection.

**Purity/side-effect model**: a registry-build-time fixed point decides
which functions/methods/constructors are provably effect-free, flagging
a discarded statement-level call to one - a call only resolves as a
dependency when its target is knowable without type inference. Same-
family bare-statement `NoopRule` (`xor`/`&&`/`||`/`and`/`or`/ternary
result-unused, the word-spelling pair honoring real PHPStan's own
`logicalAnd`/`logicalOr` identifiers - see the `and`/`or` word-operator
entry below). `@phpstan-impure` tag validation (one-directional: flags a
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
object inferring real key/value types.

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
target resolution** (`$obj::method()`, PHASE_BACKLOG Phase 158):
`ExprKind::StaticCall`'s receiver is now a real `StaticTarget` enum
(`Name`/`Expr`) instead of discarding a non-literal receiver into a
`"?"` sentinel - existence, visibility, arg-type checking, return-type
inference, and LSP hover/go-to-definition all now work through
`$obj::method()`, not just `Foo::method()`.

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

**`Type::Array` key-type/offset engine** (Phase 50):
`InvalidKeyInArrayDimFetchRule` (reverted once, reattempted successfully
- see Reverted); the `array_key_exists()`-narrows-a-specific-element
equivalent of `HasOffsetValueType`; unsealed-shape (`array{...}`)
tracking with a real `...` sentinel field gating both narrowing and
`shapeAccess.notFound`; `ArrayShape`'s own int-vs-string key precision.

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
missing-`default:`-arm detection (PHASE_BACKLOG Phase 147); missing-exception-message detection;
base-Exception/Error-class-thrown detection; discarded-`new`-exception
detection; assertion-less ("risky") PHPUnit/Pest test detection.

**Whole-program diagnostic-emission pipeline**, built as the
prerequisite for every whole-program check below (PHASE_BACKLOG Phase
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

**Doctrine ORM support** (PHASE_BACKLOG Phase 168, all sub-items a-k
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
`InvalidAssignVarRule` (PHASE_BACKLOG Phase 135), all three cases (`nullsafe.assign`/`.byRef`/
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
`@template`-typed parameter to its own bound first; the property check
replicates real PHPStan's own untyped-property `null`-default exemption
exactly. **`identical.alwaysFalse`/`.alwaysTrue`, `notIdentical.
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

CI-friendly output formats (`json`, `checkstyle`). Baseline suppression
(`--generate-baseline`/`--baseline`, on by default reporting an
unmatched baseline entry) and inline `@phpstan-ignore-line`/`-next-line`/
`<identifier list>` suppression. `ignoreErrors:` config support.
`--level`/`--php-version`-gated portability checks (`PhpFeature::
NoncapturingCatch`/`ThrowExpression` are declared with the correct
threshold but not yet wired to a diagnostic - a real, small residual).
Incremental multi-file `Project` with cross-file dependency invalidation.
An 8MiB worker stack, persistent structurally-shared `Locals` (O(1)
clone), `FxHashMap` for hot build-once maps, a criterion regression-
guard benchmark, and `hotpath`, an opt-in in-process instrumentation
profiler. `check-project`'s own parse and check phases both run on a
shared rayon thread pool, files pre-sorted largest-first to shrink the
straggler tail.

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

**LSP completeness**: `textDocument/codeAction` fix wiring (the
production `Diagnostic.fix` population itself remains a separate, open
job); whole-project `textDocument/references`/`rename` backed by a real
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

---

# Newly identified gaps (parity audit against real PHPStan Rules/)

A 20-agent workflow cross-checked every real Rule class under
`vendor/phpstan-src/src/Rules/` (~420 classes) against this project's
actual coverage and this file's own claims - a verification pass, not a
re-derivation from scratch. Most rules were confirmed either already
covered-and-documented or correctly excluded; real mismatches already
found their way into the phase entries above. What follows are the
genuinely new gaps it found: real, verified-against-source feature-
parity gaps with no existing phase, Pending entry, or Won't-port/Known-
limitations line covering them at all - not defects, just undocumented
scope. None of these have been triaged into a numbered phase yet. Full
evidence (file:line in both `vendor/phpstan-src` and this project's own
`src/`) for every item below lives in the audit workflow's own journal
(`wf_495e5a37-b39/journal.jsonl`) if a future scoping pass needs more
than the one-line summary here. A later, direct `vendor/bin/phpstan
analyse` (level 10) vs. `phpstan-rs check-project` identifier diff
against `~/piwigo16` cross-checked every real identifier this project's
source has no matching literal for at all against the list below before
folding in a residue of real, previously-undocumented gaps (marked
inline).

Several items below are now done - each marked inline, pointing at its
own Done entry, rather than removed outright, since some carry a real
residual scope note alongside the closure.

## DeadCode

- **`Rules/DeadCode/NoopRule.php`'s generic fallback**: this project's
  `noop.rs` covers a set of specific named side-effect-free-statement
  cases but has no generic catch-all (`expr.resultUnused`) for every
  *other* provably-side-effect-free expression statement (a bare
  comparison, a bare arithmetic expression, a bare property/variable
  read).

## Classes

- **`Rules/Classes/UnusedConstructorParametersRule.php`**: an unused
  constructor parameter is already flagged, just always under the
  generic `parameter.unused` identifier rather than real PHPStan's own
  distinct `constructor.unusedParameter`.
- **`Rules/Classes/ImpossibleInstanceOfRule.php`**: `instanceof.
  alwaysTrue`/`.alwaysFalse`/`.invalidExprType` - this project's
  unreachable-code family deliberately excludes `instanceof` from its
  generic constant-condition check.
- **`Rules/Classes/NewStaticRule.php`**: `new.static`/`.
  staticInAbstractClassStaticMethod` - `new static()` in a non-final
  class without a consistent-constructor guarantee.
- **`Rules/Classes/InstantiationCallableRule.php`**: `new Foo(...)`
  (first-class-callable syntax on `new`) parses but is real, illegal PHP
  (`callable.notSupported`).
- **`Rules/Classes/InstantiationRule.php`**: only two of several real
  `new`-expression sub-checks are built (`new.interface`/`.abstract`).
- **`Rules/Classes/ExistingClassesInClassImplementsRule.php`**: this
  project's own existence check is deliberately existence-only, never
  confirming an `extends`/`implements`/trait-use target is the *right
  kind* of symbol.
- **`Rules/Classes/InvalidPromotedPropertiesRule.php`**: several real
  legality constraints on constructor property promotion (constructor-
  only, not-in-an-abstract-constructor, never-variadic, PHP-8.0-gated,
  `final`-promotion-gated) aren't checked.
- **`Rules/Classes/ReadOnlyClassRule.php`**: the PHP-version-gate data
  (`PhpFeature::ReadOnlyClasses`/`ReadOnlyAnonymousClasses`) exists but
  is never consumed by any checker call site.

## Functions

- **`FunctionReturnTypeCheck.php`'s bare-`return;` validity check is now
  done**, see Done. Found along the way: this project's own sibling
  `return.missing` check (falling off the end with no `return` at all)
  has the identical pre-existing imprecision `return.empty` itself had
  before its own fix - a real, disclosed follow-up, not fixed here.
- **`Functions/CallCallablesRule.php`**: partially closed -
  `callable.nonCallable` for a dynamic-callee call is done (see Done,
  receiver-type-checks phase). Still open: `callable.inaccessibleMethod`
  and full arity/parameter-type checking against a resolved
  `[$obj, 'method']`/`'Foo::bar'`-shaped callable's own signature.
- **`Functions/IncompatibleDefaultParameterTypeRule.php` (+ the
  identical `Rules/Methods/` variant) is now done**, see Done. Still
  open: the closure/arrow-fn variants.
- **`Functions/ReturnNullsafeByRefRule.php`**: `nullsafe.byRef` for a
  by-ref function/arrow-function whose return expression passes through
  a nullsafe operator - this project has the underlying
  `contains_nullsafe_access` helper (used for the analogous by-ref-
  argument check) but nothing calls it here.
- **A family of small level-0 parameter/closure-use-list sanity checks**
  (`InvalidParameterNameRule`/`InvalidLexicalVariablesInClosureUseRule`/
  `RedefinedParametersRule`/`VariadicParametersDeclarationRule`):
  `parameter.superglobal`/`.this`, `closure.useThis`, a duplicate
  parameter name, a non-last variadic parameter - none built.
- **`Functions/ParameterCastableToStringRule.php` + 3 siblings**:
  real PHPStan checks that array-argument values passed to specific
  stdlib functions (`implode`, `array_combine`/`natsort`/etc.,
  `array_intersect`/`array_diff` family, `sort`/`asort`/etc.) are
  actually castable to string/number.

## Methods

- **`Rules/Methods/MissingMagicSerializationMethodsRule.php`**:
  `class.serializable` for a non-abstract `Serializable` class lacking
  `__serialize()`/`__unserialize()`, PHP-version-gated.
- **`Rules/Methods/AbstractMethodInNonAbstractClassRule.php`**:
  `method.nonAbstract` for a non-abstract, non-interface method with no
  body at all (real PHP's parser accepts this syntactically).
- **`Rules/Methods/MethodCallableRule.php`**: first-class-callable
  syntax (`$obj->method(...)`) isn't PHP-8.1-version-gated
  (`callable.notSupported`), unlike this project's dozen other
  `PhpFeature`-gated syntax checks.
- **`Rules/Methods/StaticMethodCallCheck.php`'s static/instance-context
  mismatch half**: `method.staticCall` for calling a genuinely
  non-static method through `Foo::method()` static-call syntax (or the
  reverse) - distinct from the already-built casing-mismatch check in
  the same file.
- **`Rules/Methods/OverridingMethodRule.php`'s tentative-return-type
  sub-check**: `method.tentativeReturnType`, niche (only relevant
  extending certain SPL/core classes).

## Properties

- **`Rules/Properties/DefaultValueTypesAssignedToPropertiesRule.php` is
  now done**, see Done.
- **`Rules/Properties/ExistingClassesInPropertiesRule.php`**: the
  `class.notFound` existence half is done (Phase 48); `property.trait`/
  other non-existence identifiers aren't.
- **`Rules/Properties/PropertiesInInterfaceRule.php`/`PropertyInClassRule.php`**:
  PHP 8.4 property-hook *declaration-site* legality (an interface
  property must be hooked/public/non-readonly/etc.; abstract/final/
  private/static/readonly/hooksNotSupported combinations on a class's
  own hooked property).
- **`Rules/Properties/ReadOnlyPropertyRule.php`/`ReadOnlyByPhpDocPropertyRule.php`**:
  a `readonly` property declaration's own legality (native type, no
  default, not static, PHP-version-gated), native-keyword and docblock-
  tag forms alike.
- **`Rules/Properties/InvalidCallablePropertyTypeRule.php`**:
  `property.callableType` - real PHP forbids `callable` as a property's
  native type.
- **A cluster of five PHP 8.4 write-only/read-only/asymmetric-
  visibility property rules**: assigning by reference, reading a
  write-only property, writing a non-writable property.

## Comparison

- **`Rules/Comparison/StrictComparisonOfDifferentTypesRule.php` is now
  done**, see Done.
- **`Rules/Comparison/ImpossibleInArrayHaystackFiniteTypesRule.php`**:
  `function.impossibleHaystackValue` - a needle's finite type that can
  never match any value in a constant-array haystack, for `in_array()`/
  `array_search()`/`array_keys()`.
- **`Rules/Comparison/UsageOfVoidMatchExpressionRule.php`**: `match.void`
  - using the result of a `match` whose every arm's result is a
  void-returning call.
- **`Rules/Comparison/MatchExpressionRule.php` is now fully done, both
  halves** (`match.unhandled` and `match.alwaysTrue`/`.alwaysFalse`),
  see Done.

## PhpDoc

- **`Rules/PhpDoc/InvalidPhpDocTagValueRule.php`**: `phpDoc.parseError`
  for a syntactically malformed PHPDoc tag value - a high corpus count
  (155 hits on piwigo16), spread across Piwigo's own code rather than
  one vendored file, worth a closer look.
- **`Rules/PhpDoc/IncompatiblePhpDocTypeRule.php` + 3 siblings**: a
  `@param`/`@return`/`@var`/property/class-constant PHPDoc type
  incompatible with its own native type hint - this project silently
  reconciles/coerces rather than flagging the mismatch.
- **`Rules/PhpDoc/IncompatibleSelfOutTypeRule.php`**: `@phpstan-self-out`
  is entirely unimplemented - only whitelisted so it isn't flagged as an
  unknown tag.
- **`Rules/PhpDoc/FunctionAssertRule.php`/`MethodAssertRule.php`'s own
  assertion-*validity* half**: `assert.impossibleType`/
  `.alreadyNarrowedType`/`.unresolvableType`/`.trait`/`.unknownExpr` -
  distinct from the already-built class-existence slice.
- **`Rules/PhpDoc/FunctionConditionalReturnTypeRule.php`/
  `MethodConditionalReturnTypeRule.php`**: a conditional PHPDoc return
  type's own declaration-validity (`conditionalType.subjectNotFound`/
  `.alwaysTrue`/`.alwaysFalse`).
- **`Rules/PhpDoc/InvalidPhpDocVarTagTypeRule.php` + 2 siblings**: this
  project's inline-`@var` checking is deliberately scoped to a plain
  assignment statement only; real PHPStan's much larger `@var`-tag-
  validity surface (foreach/static/global/return/misplaced-declaration
  name-matching) is unbuilt.

## Generics

- **`Rules/Generics/MethodTagTemplateTypeCheck.php`**: a `@method` tag's
  own inline template parameters (distinct from a real method's
  `@template`) aren't modeled at all, including its own shadow-check
  (`methodTag.shadowTemplate`).

## Arrays

- **`Rules/Arrays/DeadForeachRule.php`**: `foreach.emptyArray` - a
  `foreach` whose subject is provably always-empty.
- **`Rules/Arrays/ArrayDestructuringRule.php`**: the `offsetAccess.
  nonArray` half is closed (Done, receiver-type-checks phase); the
  each-key-exists-on-the-RHS half (`NonexistentOffsetInArrayDimFetchCheck`)
  is still open.
- **`Rules/Arrays/ArrayUnpackingRule.php`**: `arrayUnpacking.
  stringOffset` - `[...$arr]` when `$arr`'s key type could be string and
  the target PHP version predates 8.1.
- **`Rules/Arrays/OffsetAccessWithoutDimForReadingRule.php`**:
  `offsetAccess.noDim` - `$x[]` used in a read position, a real PHP
  compile-time fatal.
- **`Rules/Arrays/NonexistentOffsetInArrayDimFetchRule.php`**: the
  "isn't offset-accessible at all" half is closed (Done, receiver-type-
  checks phase, ships as `shapeAccess.notFound` for the shape case);
  general-array/constant-array key-existence checking beyond shapes is
  still open.
- **`Rules/Arrays/OffsetAccessAssignmentRule.php`**/**`OffsetAccessValueAssignmentRule.php`**:
  `offsetAssign.dimType`/`.valueType` - a write's own key/value type
  checked against the container's declared offset type; `write_index_
  into` always unions the new value's type in rather than checking it.

## Constants

- **`Rules/Constants/ConstantRule.php`**: `constant.notFound` - a bare
  global-constant reference not found anywhere falls back to `Mixed`
  rather than being flagged.
- **`Rules/Constants/DynamicClassConstantFetchRule.php`**: PHP 8.3's
  `Foo::{$expr}` dynamic class-constant fetch - this project's
  `StaticAccess` AST node has no way to represent a dynamic member name
  at all.
- **`Rules/Constants/MagicConstantContextRule.php`**: `__CLASS__`/
  `__TRAIT__`/`__FUNCTION__`/`__METHOD__`/`__NAMESPACE__` used outside
  their respective context - this project has no dedicated magic-
  constant `Expr` kind of its own.
- **`Rules/Constants/OverridingConstantRule.php`**: class-constant
  override compatibility (final, visibility, native type, PHPDoc
  covariance) against a parent/interface prototype.
- **`Rules/Constants/ValueAssignedToDefineRule.php`/
  `ValueAssignedToGlobalConstantRule.php`**: a `define()`'d/`const`-
  declared global constant's value checked against a project's own
  `dynamicConstantNames` config.
- **`Rules/Constants/ValueAssignedToClassConstantRule.php`**: only the
  native-typed-constant half (PHP 8.3 `const TYPE NAME = value;`) is
  covered; the PHPDoc-`@var`-type half (`classConstant.phpDocType`) is
  explicitly skipped.

## Variables

- **`Rules/Variables/AssignToByRefExprFromForeachRule.php`**:
  `assign.byRefForeachExpr` - assigning to (or property-fetching
  through) the same expression just bound as a `foreach ($x as &$expr)`
  reference variable, a well-known PHP gotcha.
- **`Rules/Variables/CompactVariablesRule.php`**: `variable.undefined`
  for a `compact()` argument naming an undefined variable - this
  project's own `variable.undefined` machinery only fires on a direct
  `$var` read.
- **`Rules/Variables/UnsetRule.php`**: five distinct `unset.*`
  diagnostics (undefined variable, illegal offset, readonly/hooked
  property) - none built.
- **`Rules/Variables/VariableCloningRule.php`'s own `clone.nonObject` is
  now closed** (Done, receiver-type-checks phase).
- **`Rules/Variables/NullCoalesceRule.php`'s own dead-`??`-operand
  check**: this project's `nullCoalesce.variable`/`.property`/`.offset`
  only fire when the left side is *definitely undefined*; real PHPStan's
  `nullCoalesce.expr` additionally flags the opposite - the left side is
  defined and already excludes `null`, so `??`'s right side can never
  run (5 corpus hits).

## Operators+Cast

- **`Rules/Operators/InvalidIncDecOperationRule.php`**: a whole
  unimplemented rule family - `postInc.type`/`postDec.type`/`preInc.type`/
  `preDec.type` for incrementing/decrementing a type PHP can't
  meaningfully handle (an array, a bool, a plain object). Non-trivial
  corpus incidence for legacy PHP (postInc.type 41, postDec.type 14,
  preInc.type 3, preDec.type 2).

## Traits+InternalTag

- **`Rules/Traits/ConflictingTraitConstantsRule.php`**: a class constant
  overriding one supplied by a used trait must keep the same visibility/
  finality/native type/(for immediately-used traits) value.

## Keywords+Debug

- **`Rules/Keywords/ContinueBreakInLoopRule.php`**: `continue.outOfLoop`/
  `break.outOfLoop` - a break/continue's own numeric level not resolving
  inside enough loop/switch nesting, or crossing a closure boundary.
- **`Rules/Keywords/DeclareStrictTypesRule.php`**: `declare(strict_types=...)`'s
  value must be literally 0/1, and the declare must be the file's first
  statement.
- **`Rules/Keywords/GotoUndefinedLabelRule.php`**: `goto.labelUndefined`
  - this project tracks goto targets only for dead-code reachability,
  never validates the target label actually exists.
- **`Rules/Keywords/UnusedLabelRule.php`**: `label.unused` - a `label:`
  never targeted by any `goto` in the function.
- **`Rules/Keywords/RequireFileExistsRule.php`**: `require`/`include`
  path resolution against the working directory/`include_path`/the
  analyzed file's own directory.
- **`Rules/Debug/{DumpTypeRule,DumpNativeTypeRule,DumpPhpDocTypeRule,DebugScopeRule}.php`**:
  real PHPStan's own ad-hoc developer-debugging hooks
  (`\PHPStan\dumpType()`/`dumpNativeType()`/`dumpPhpDocType()`/
  `debugScope()`).

## Generators+Pure

- **`Rules/Generators/YieldFromTypeRule.php`**: `generator.nonIterable`
  as a standalone check needing only `isIterable()`, independent of the
  full `Generator<K,V,TSend,TReturn>` shape work (already done).

## EnumCases+Namespaces+Regexp+Ignore+Missing+Names+Types+Whitespace

- **`Rules/Whitespace/FileWhitespaceRule.php`**: `whitespace.bom`/
  `.fileEnd` - a UTF-8 BOM at file start, or a whitespace-only
  closing-tag remnant as the last statement.
- **`Rules/Types/InvalidTypesInUnionRule.php`**: `mixed`/`never`/`void`
  appearing inside a union or nullable type declaration
  (`unionType.*`/`nullableType.*`).
- **`Rules/Ignore/IgnoreParseErrorRule.php`**: `ignore.parseError`, a
  non-ignorable diagnostic for a malformed `@phpstan-ignore` identifier-
  list - this project's own `ignore_comments.rs` deliberately swallows
  this case with no diagnostic at all.

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
  (originally Phase 29, re-attempted and re-reverted since). A real,
  demonstrated soundness bug both times: giving `Foreach` the same
  `While`/`For`-style zero-iteration merge breaks the common "a loop
  mutates the very array it iterates via its own loop variable" idiom -
  a later re-attempt with fixed-point loop analysis in place still
  regressed a real corpus file via a narrowing-wrapper interaction.
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

- `strtotime()`'s real accuracy and a raw SQL/DQL query's own result
  shape (beyond what the Doctrine/SQL-reflection phases above already
  cover) are structurally infeasible in the general case without
  embedding a PHP runtime or a full SQL parser respectively.
- A long tail of smaller type-system precision gaps, each triaged
  individually rather than as one bucket: no per-location type-inference
  caching architecture (a performance mechanism, not correctness -
  revisit only if profiling calls for it); cosmetic nested-`Intersection`
  flattening; `static`/`$this` late binding has no attached calling
  class (no concrete corpus false positive traced yet); `class-string<T>`'s
  own `instanceof`/sealed-hierarchy narrowing is unmodeled; no general
  `TypeCombinator` intersect/remove or generic recursive type-tree
  traverse/map combinator (a refactor-timing call).
- **A closure literal's own `Type::Closure`/`Callable` signature uses
  its bare declared return-type hint, not its more precise inferred body
  return type** - unlike an ordinary named function/method, which
  already gets careful native/PHPDoc reconciliation. Confirmed via
  `\PHPStan\dumpType()`: real PHPStan uses the body's own more precise
  inferred return type for a closure/arrow-fn literal's own signature. A
  real, well-scoped gap at the specific closure-literal signature-
  construction call site(s), not inventoried this round.
- **Correlated-narrowing family** (`ConditionalExpressionHolder`): the
  *general*, unbounded mechanism (an arbitrary boolean-formula memory
  over recorded facts) stays declined - no natural stopping point,
  unlike a single named idiom. Every named instance found in this
  project's own corpus work has been individually assessed on its own
  merits instead (most are now built - see Done's own narrowing-engine
  cluster). Two real, still-open sub-cases: a cross-*function* version
  of the parallel-accumulator-array correlation (the same-function case
  is filed and buildable - PHASE_BACKLOG Phase 173; the cross-function
  generalization splits into two layers - the two-pass collect-then-
  consume checking architecture (Phases 166/174, now built - see Done's
  "Too-wide return/property types" entry), necessary but not sufficient
  on its own, and "soundly answering whether the correlation still holds
  by the time a different function reads it" - a fundamentally harder,
  unbounded invariant-maintenance problem with no known bounded design
  yet, the actual wall); a compound `RateService::rate`-shaped case
  compounding discriminant-variable correlation with closure-capture
  timing (PHASE_BACKLOG Phase 175, blocked on Phase 169 - now built, so
  this specific compound case is a real, narrow, buildable follow-up).
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
