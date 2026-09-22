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
it's a parity gap, a beyond-parity idea from `BEYOND_PHPSTAN.md`, or a
top-100-extension survey finding), **Reverted** (implemented,
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
guards `function.notFound`. `is_a()` narrowing (Phase 167)
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
target resolution** (`$obj::method()`, Phase 158):
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

# Planned (phases 85+)

Every phase from 85 up that isn't built yet. Each one is **PLANNED**
unless its entry says otherwise. A phase (or sub-item) that ships moves
into **Done** and its entry is deleted here, not left in place with a
flipped status tag. Numbers are sequential by source, not by priority.
Pick items the same way any other Pending/partial phase gets picked up.

**Numbering and sources:**
- **85-124** come from `BEYOND_PHPSTAN.md`'s lettered sections, one
  phase per section, in that document's own Part 1-9 order. Sub-items
  keep their original letter and number (85's own "A1"-"A6"). Each
  entry here is condensed to what's needed to schedule and scope it.
  Before implementing, go back to `BEYOND_PHPSTAN.md`'s numbered finding
  (e.g. "A1", "U3") for the full file:line citations into
  `vendor/phpstan-src`, `vendor/phpstan-strict-rules`, and this
  project's own `src/`.
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
  follow-ups. Everything in it has shipped (see Done) except 173 and
  175, which are listed at the end of this section. 165 was a duplicate
  of already-shipped work and was removed.

A number between 85 and 175 with no entry here (86, 92, 93, 99,
102-104, 106, 108, 135, 158-172, 174) belongs to a phase that has fully
shipped, and Done records it.

---

## Part 1 (BEYOND_PHPSTAN): Precision, whole-program analysis, closed-world inference

phpstan-rs analyzes one closed-world project+vendor universe in a single
process with one frozen `Registry`; real PHPStan must stay sound for
arbitrary downstream consumers of a library it's analyzing, and its
per-file reflection is fixed before a function's body is even walked.
Every phase in this Part is either PHPStan approximating for that
architectural reason where phpstan-rs doesn't have to, or a
whole-program fact PHPStan's own 14-collector `CollectedDataNode`
machinery makes expensive that phpstan-rs's single live `Registry` gets
nearly for free.

**Shared prerequisite for every whole-program *diagnostic* phase below
(86-94, and Part 8's composer-hygiene phases 113-120) - now built,
found and closed while implementing Phase 92 (folded into Done,
"Whole-program diagnostic-emission pipeline" - see there for the full
design).** This project used to have no whole-project,
non-per-file diagnostic-emission path at all, and `Project::decl_
locations` used to be gated behind `retain_source` (off in batch
`check-project` mode) - both fixed. Any of these phases can now build
directly on `Project::check_whole_program` (gate a new opt-in `Registry`
bool, resolve findings through `decl_locations` exactly like Phase 92's
own `checker::whole_program::dead_enum_cases` does) instead of
re-deriving this infrastructure.

### Phase 85 — More precise than real PHPStan itself (BEYOND_PHPSTAN §A)

Real PHPStan approximates in these spots for its own architectural
reasons; phpstan-rs doesn't share the reason.

- **A1 (small).** Raise `converge_loop_body`'s `MAX_PASSES = 3` cap
  (`checker/control_flow.rs`) — copied from PHPStan's own
  `LOOP_SCOPE_ITERATIONS = 3`, but without copying the *reason* 3 is
  tolerable there: PHPStan widens literal types to their base type after
  just 1 pass (`GENERALIZE_AFTER_ITERATION = 1`) specifically because
  each pass re-walks the whole Scope/Rule-registry/PHPDoc machinery.
  phpstan-rs's own convergence does plain union merging with no early
  widening — already more precise per pass, just capped for no real
  reason. Raise to 5-8, or adaptive on non-convergence.
- **A2 (large, new feature).** Unroll `foreach` over small compile-time
  arrays with a higher ceiling than PHPStan's 16/8
  (`FOREACH_UNROLL_LIMIT`/`_NESTED_LIMIT`) — `check_foreach`
  (`checker/stmt/loops.rs`) has no unrolling at all today, always
  unioning every `ArrayShape` field into one coarse element type.
- **A3 (medium, folds into whichever phase builds `offsetAccess.notFound`
  — see Phase 125 Arrays).** When that check is built, its power-set
  key-presence expansion should use a bailout tuned to phpstan-rs's own
  (cheap `Vec` clone) per-variant cost, not a reflexive import of
  PHPStan's 16384 (`TypeUtils::flattenTypes`, object-allocation-costed
  there).
- **A4 (medium).** Check `BenevolentUnionType` results by default instead
  of leaving them permanently unchecked. Real PHPStan computes a real
  narrowed result for `mixed + mixed`/`mixed & mixed`/etc.
  (`InitializerExprTypeResolver::getBitwiseAndType`/etc.) then
  *systematically declines to check it* (`checkBenevolentUnionTypes`
  hardcoded `false` in every shipped level, protecting an installed base
  phpstan-rs doesn't have). phpstan-rs's own `check_binary_operand_validity`
  bails on `Mixed` operands entirely — could compute and enforce the
  same narrowed union by default.
- **A5 (large).** Auto-infer closed (sealed) hierarchies from
  whole-program subclass enumeration, not just an explicit
  `@phpstan-sealed` tag. Real PHPStan's `ObjectType::isInstanceOf`
  always returns `Maybe` for an untagged class — deliberate, since a
  library's downstream consumers can add subclasses it never sees.
  phpstan-rs's closed-world model makes "zero subclasses found anywhere
  in the analyzed set" sound. Needs a new reverse (parent-to-children)
  index on top of `hierarchy.rs`'s forward-only `all_ancestor_names`
  (reused directly by Phase 88/94 below - Phase 93, which cited this
  item too, turned out not to need the reverse index at all: an inverted
  `all_ancestor_names` sufficed for its own narrower "any concrete
  descendant" existence query - see Done).
- **A6 remainder (small).** `Add`, `Sub`'s constant-shift fast path, and
  `Mul` now union `Float` in on genuine `i64` overflow (see Done) -
  `sub_int_range_full`'s own general range-minus-range case (right side
  not a single constant) is a faithful transcription of real PHPStan's
  own multi-branch logic and was left untouched to
  avoid risking a real bug in already-correct, carefully-verified code;
  still silently discards a would-be float bound as unbounded there.

### Phase 87 — Whole-program value-flow diagnostics (§H)

Same whole-project registry-fixed-point architecture as Phase 86, for
questions about *values* flowing through the program instead of
existence.

- **H1 (large).** "Dead flexibility" parameter detection — a parameter
  with 2+ call sites that always resolves to the exact same literal
  value project-wide is a hardcode/drop candidate. No PHPStan collector
  aggregates argument *values* across call sites (only named-argument
  *names*, `NamedArgumentParameterMethodCallsCollector`).
  `resolve_call_args` already resolves the per-site literal `Type`
  (`checker/calls.rs`) — just needs a project-wide accumulator.
- **H2 (medium).** Whole-program inferred "effectively NoDiscard"
  detection — a non-void callee whose return value is discarded at
  *every* recorded call site infers the same fact a real
  `#[\NoDiscard]` attribute would assert. `FunctionSig.no_discard` is
  exactly the bit to set automatically; `check_useless_return_value_call`
  already answers "was this call's result unused" per site.
- **H3 (medium).** Whole-program property-type narrowing from every
  assignment site — if every `$this->prop = rhs` across the whole
  subclass-aware hierarchy is a strict subtype of the declared type,
  suggest narrowing to a literal union/backed enum. No PHPStan rule or
  collector intersects RHS types across assignment sites.

### Phase 88 — Closed-world inference replacing PHPStan's doc-tag-only gaps (§U)

Six cases with an identical shape: PHPStan resolves a fact purely from
an author-written PHPDoc tag on the immediate declaration, with zero
fallback when it's missing, because its per-file reflection is fixed
before the body is walked. phpstan-rs's Phase 41 transitive-purity
fixed point already proves this exact shape works for one fact; these
extend it to five more.

- **U1 (large).** Resolve `method_exists()`/`property_exists()`
  narrowing to real closed-world signatures, not PHPStan's
  `DummyMethodReflection` stub (zero params, everything `Mixed` —
  architecturally forced open-world leniency). Invert `Registry.classes`
  into a member-name → declaring-classes reverse index so a literal
  `method_exists($x,'foo')` narrows `$x` to the union of every real
  class anywhere in the closed universe declaring `foo`.
- **U2 (large).** Registry-level transitive `@throws` inference,
  replacing PHPStan's docblock-only per-hop requirement — extend
  `registry/purity.rs`'s fixed point with a sibling computing each
  function's *real* union of thrown checked-exception types from its
  own body, independent of whether `@throws` exists. This is also what
  directly unblocks the `MethodThrowTypeCovarianceRule` port Phase 45 already
  declined for exactly this "untagged means unknown, not nothing"
  reason. **A concrete future consumer** (top-100
  Packagist survey): once U2 exists, `nette`'s own `RethrowException
  Rule` idea (flag a `try` block containing a call that can throw a
  specific exception subtype when the only `catch` present is a bare
  `\Exception`/`\Throwable` with no more-specific clause and no re-
  `throw`) becomes a strictly more powerful, framework-independent
  check — works for *any* throwing call project-wide instead of
  Nette's own hardcoded per-method exception table. Fold in as a
  concrete U2 consumer, not a separate framework-specific item.
  Separately, `pepakriz/phpstan-exception-rules` confirms an
  `UnreachableCatchRule`/`DeadCatchUnionRule` pair is real, buildable,
  and previously uncatalogued: a `catch (Specific $e)` clause (or one
  member of a `catch (A|B $e)` union) matching nothing the `try` body
  can actually throw, once U2's aggregate throw-type is known — a real
  dead-code family specific to exception handling, worth its own small
  sub-item (U2a) once U2 lands.
- **U3 (large).** Registry-level by-ref parameter out-type inference,
  the by-ref analogue of U2 — real `ParameterOutExecutionEndTypeRule`
  literally short-circuits when no `@param-out` tag exists. Recurse
  through a nested by-ref call's own computed out-type via the same
  fixed point instead of requiring a hand-written tag on it.
- **U4 (large).** Closed-world inference of `@phpstan-self-out` for the
  straight-line case (a void/self/static-returning body of direct,
  unconditional `$this->prop = <expr>;` with no branches) — real
  PHPStan never attempts inference here either direction, only trusts
  an existing tag unconditionally. (Depends on `@phpstan-self-out`
  itself gaining real tag support first — currently only in the
  typo-detection whitelist.)
- **U5 (large).** Cross-check `@phpstan-require-implements`/`-extends`
  against actual trait usage (flag a tag broader than the trait body
  demonstrably needs), and use the required type's real member set to
  widen `$this` in phpstan-rs's own existing orphan-trait fallback
  (`check_trait_methods`) — real PHPStan structurally never walks a
  trait body with zero using classes at all, so it can't do either half.
- **U6 (medium).** Infer an implicit `@phpstan-assert` fact when a
  function/method's entire body is exactly one
  `if (!<condition>) { throw ...; }` guard clause — a single-function,
  non-recursive syntactic pattern match feeding the existing
  `FunctionSig::assert` field every consumer already reads uniformly.
  An explicit tag always wins when present.

### Phase 89 — Whole-program structural duplication (§V)

Extends single-function duplication checks (Phase 96's D3/D7) to
duplication *across* declarations project-wide.

- **V1 (medium).** Whole-program duplicate function/method body
  detection — canonical structural fingerprint (postorder hash,
  alpha-renamed locals) bucketed by hash, confirmed via an
  alpha-equivalence `Stmt`-list comparator (generalizing
  `is_switch_break_equivalent`). Skip related pairs
  (`is_subclass_of`/`classes_using_trait`) and trivial bodies. PHPStan's
  `Rules/` has no body-content comparator at all.
- **V2 (large).** Divergent-clone detection — near-duplicate bodies
  differing by exactly one leaf (a differing literal/operator), via a
  coarser shape-only pre-filter over V1's machinery plus a bounded
  leaf-divergence count. A correctness signal (a fix applied to one
  copy, forgotten in its sibling), not style.

### Phase 90 — Whole-program redundant-override detection (§AI)

V1's (Phase 89) mirror image: reuses the same alpha-equivalence
machinery but fires *only* when the override relationship holds and the
overriding method's body is alpha-equivalent to the ancestor it
overrides — dead code or a forgotten divergent edit. Skip trivial
0-1-statement bodies (bare `parent::foo()` forwarders). Real PHPStan's
408-line `OverridingMethodRule` never once reads body content from
either side. **Effort: small.**

### Phase 91 — Whole-program parameter-type narrowing from call-site arguments (§X)

- **X1 (large).** Narrow a "wide" parameter (an interface/non-final
  abstract class, or a 2+-class Union) to the one concrete class
  actually passed, when 2+ real call sites all agree — retaining and
  aggregating `arg_ty`, a value `check_args` already computes and
  currently discards. No PHPStan collector aggregates per-call argument
  *types*.
- **X2 (medium).** Same aggregation, second dimension: narrow a plain
  `array`-typed parameter to `non-empty-array` when every real call site
  provably passes a non-empty value (`is_known_non_empty_array` already
  exists for this exact test).

**Confirmed already shipping in a real, popular tool** (top-100
Packagist survey): `rector/type-perfect`'s own `PublicClassMethodParam
TypesCollector`/`NarrowPublicClassMethodParamTypeRule` is literally X1's
own proposed mechanism, already live using real PHPStan's Collector
architecture (which core PHPStan itself has no first-party equivalent
of) — cite as concrete "this exact idea already ships" evidence when X1
is picked up. The same package's `NarrowReturnObjectTypeRule` is the
return-type sibling, closer in spirit to the already-built too-wide-
return-type check (Phase 111's I3/Phase 45's 45A) than to X1 itself.

### Phase 94 — Closed-world precision unlocking Pending techniques further (§P)

Two cases where finishing/extending an existing mechanism with
phpstan-rs's own closed-world registry resolves something real PHPStan
is *structurally* unable to resolve, not just unbuilt.

- **P1 (large).** Resolve provably-closed virtual dispatch inside the
  transitive-purity fixed point (`registry/purity.rs`) — a `$this->
  method()` call through a non-final overridable method isn't
  unconditionally disqualified anymore if the same reverse index (A5)
  proves zero real overrides exist anywhere in the closed universe.
  Real PHPStan's `PossiblyPureCallTransitivePurityResolver` unconditionally
  disqualifies here, by deliberate open-world policy, not a cost cap.
- **P2 (medium).** Resolve real Composer package boundaries for `@internal`
  enforcement (Phase 67), replacing the root-namespace-segment heuristic
  both projects currently share, using `composer::
  find_composer_package_name` (already built for Part 8's composer
  hygiene work below) — closes both false negatives (two same-vendor
  packages sharing one namespace root) and false positives (one package
  spanning several namespaces). **Reinforced by two independent real
  packages** (top-100 Packagist survey): `dave-liddament/phpstan-php-
  language-extensions`' `Package{Static,New,Method}CallRule` family
  directly overlaps this item — real Composer-package-boundary
  visibility enforcement via attributes, worth reading as a second
  reference implementation. `pb30/phpstan-composer-analysis` (wrapping
  the standalone `shipmonk/composer-dependency-analyser` tool)
  independently needs the identical class-FQN → owning-package index
  P2 needs for its own, differently-shaped Phase 142 checks below — the
  two items should share one `composer::find_composer_package_name`-
  style implementation, not duplicate it.

---

## Part 2 (BEYOND_PHPSTAN): Native-performance wins real PHPStan needs a separate extension for

Real PHPStan's own team ships (or is building) these as opt-in native
C++ ("Turbo") because stock PHP can't afford them. phpstan-rs is already
native for every user — it needs the data structure, not the extension
architecture.

### Phase 95 — Native-performance (§C)

- **C1 (large).** Type interning + memoized union/intersect/remove
  (`TypeCombinator` caching), on by default. Real PHPStan's own
  `TypeCombinatorCache` documents "~91% of calls repeat an argument
  tuple already computed" — shipped only behind Turbo, so most installs
  never get it. phpstan-rs hit the identical problem shape already
  (`checker/type_union.rs`'s own doc comment records a real `gdb`-caught
  hang inside `Type::eq`/`union_types`/`merge_scopes`, worked around with
  an ad hoc `MAX_UNION_MEMBERS = 12` cap rather than the root fix).
- **C3 (medium).** Memoize class-hierarchy member lookups
  (`Registry::lookup_in_hierarchy_with_owner`, re-walks the full
  ancestor BFS from scratch on every call) — real PHPStan's own
  July-2026 profiling names the identical lookup cost as a still-open
  target, unsolved even by its native extension. A plain per-(class,
  member) memo table is immediately buildable once the class graph
  freezes at `finish_loading()`.
---

## Part 3 (BEYOND_PHPSTAN): New rule categories and design-pattern suggestions

Not approximation gaps — PHPStan's entire `Rules/` catalog simply has no
rule of these shapes at all, confirmed by grep.

### Phase 96 — New rule categories with zero PHPStan precedent (§D)

- **D1 (medium).** Function/method complexity/maintainability metrics
  (cyclomatic complexity, max nesting, parameter count) — zero PHPStan
  rule is keyed on body *shape* at all. Additive to the existing generic
  `stmt_children`/`expr_children` walk every function already passes
  through. **Algorithm recommendation from a real, popular tool**
  (top-100 Packagist survey): `tomasvotruba/cognitive-complexity`
  implements Campbell/SonarSource's "Cognitive Complexity" metric
  specifically (weighs nested control flow more heavily than a flat
  sequence of branches, unlike plain cyclomatic complexity) — build
  against that named algorithm, not an unspecified "some complexity
  metric." The same package's `ClassDependencyTreeRule` (sum a class's
  own constructor-injected dependencies' complexity *transitively* —
  this class's complexity plus everything `__construct()` pulls in,
  recursively — flag when the total exceeds a threshold) is a distinct,
  novel "god object with a deep, complex dependency graph" smell worth
  a D1b sub-item; needs only recursive constructor-param-type
  resolution through the already-live closed-world registry.
- **D3 (medium).** Identical-branch / structurally-duplicate conditional-
  body detection — two branches with satisfiable-but-different
  conditions and identical bodies; PHPStan's `Rules/Comparison`/
  `DeadCode` families only ever reason about condition *values*, never
  body equality. Reuses `is_switch_break_equivalent`'s technique one
  level up.
- **D4 (medium).** Narrow, single-function-scoped taint check —
  superglobal input reaching `eval`/`exec`/`unserialize`/a non-literal
  `include` within one function body only, no interprocedural tracking.
  PHPStan has zero taint infrastructure. Ship with an explicit
  false-positive-risk caveat; stay single-function-scoped.
- **D5 remainder (small).** `@see`'s own validation is now built (see
  Done) — the one piece still open is
  `brainbits/phpstan-rules`' `CoversClassExistsRule` sibling (validates
  a PHPUnit `@covers`/`#[CoversClass]` tag's target class exists),
  originally scoped to fold into the same D5 item rather than a
  separate check.

### Phase 97 — Whole-program design-pattern suggestion rules (§AA)

Real PHPStan's machinery in this space only ever *validates* an
existing stricter annotation, never *suggests* adding one — a
directional asymmetry every sub-item below exploits.

- **AA1 (medium).** "Readonly candidate" suggestion — a non-readonly,
  non-static, non-promoted property assigned exactly once project-wide,
  as a plain (non-compound, non-by-ref) `Assign` inside the declaring
  class's own constructor, is a mechanically safe readonly candidate.
  Distinct from Phase 87's H3 (site *count*, not RHS type diversity).
  **Third independent confirmation** (top-100 Packagist survey):
  `shipmonk/phpstan-rules`' `EnforceReadonlyPublicPropertyRule` ships
  the *enforcement*-mode sibling of this same idea (hard requirement,
  not a suggestion) — real, valued in production, worth citing as
  evidence when AA1 is picked up, and worth offering both a suggestion
  mode (AA1 as scoped) and an opt-in hard-enforcement mode matching
  shipmonk's own stricter default.
- **AA2 (medium).** Suggest narrowing a public constructor to
  private/protected when `new ClassName(...)` is only ever called from
  exactly one static factory method project-wide — excluding classes
  extended elsewhere or escaping into dynamic instantiation.
- **AA4 (small).** Class-level `readonly class` shorthand once every
  property independently qualifies under AA1 — one aggregation step on
  top of AA1, suppressing the per-property suggestions for that class.
- **AA6 (medium).** Suggest `@template-covariant`/`-contravariant` when
  actual usage already proves it safe — `walk_variance`'s existing
  per-class traversal just needs a second, always-run pass that
  *records* positions (In/Out/Inv) across every occurrence of an
  undeclared/invariant template instead of comparing them against a
  pre-existing declared variance. No PHPStan code path infers variance
  from usage anywhere.

### Phase 98 — Two PHP 8.4 rough edges (§AB)

- **AB1 (medium).** Purity/impure-point enforcement for property hooks.
  Real PHPStan's own `PropertyHooksProcessor` gathers `ImpurePoint`s from
  a hook body but no `Rule`/`Collector` anywhere ever reads them —
  gathered and never consumed, confirmed in PHPStan's own source. Wire
  phpstan-rs's existing `check_declared_purity_tag` into
  `check_get_hook`/`check_set_hook` (`checker/oop/property_hooks.rs`),
  currently wired only for ordinary function/method declarations.

### Phase 100 — Opt-in algorithmic-complexity heuristic (§AF)

Flag a linear `in_array()`/`array_search()`/`array_key_exists()` lookup
into a pre-built, unbounded array from inside an unrelated loop
(`loop.quadraticLookup`) — explicitly heuristic and opt-in, with a
disclosed false-positive caveat for small-but-unbounded-at-runtime
haystacks. PHPStan's entire `Rules/` catalog reasons about types/
existence/soundness, never runtime execution cost — no rule concept
like this exists anywhere. **Effort: medium.**

---

## Part 4 (BEYOND_PHPSTAN): Ecosystem — `phpstan-strict-rules` ports and Won't-port corrections

Grounded directly against the vendored `phpstan-strict-rules` submodule.

### Phase 101 — Ecosystem checks ported from `phpstan-strict-rules` (§E)

- **E2 (medium, gate on revisiting two prior false positives first).**
  `array_filter()` without an explicit callback for an ambiguous
  element type — phpstan-rs already declined two siblings
  (`arrayFilter.same`/`.alwaysEmpty`) after corpus false positives
  traced to this project's own flow-sensitive variable-type-tracking
  gaps, not the checks themselves; this one risks the identical
  truthy/falsy union-splitting judgment call.

---

## Part 5 (BEYOND_PHPSTAN): LSP / editor-native capabilities

Real PHPStan ships no first-party LSP or interactive query API at all —
`FileAnalyser::analyseFile` is its sole entry point, always a fresh
whole-file CLI invocation. phpstan-rs's LSP server already keeps a
resident, incrementally-updated `Project`/`Registry` — every phase below
is a gap in already-built plumbing. (The LSP server's own trustworthiness/
concurrency/protocol hardening, plus the VSCode extension client itself,
were a separate concern, formerly tracked in `VSCODE_EXTENSION_PLAN.md` —
that document is now retired, fully done; see Done's own "Performance
& tooling" section.)

### Phase 105 — Finer-grained incremental re-check (§G)

- **G1 (large).** Sub-file (per-function) real-time re-check on
  `didChange` — every edit today unconditionally re-parses the whole
  file and re-runs the full registry-rebuild sequence, even for an edit
  entirely inside one function body touching no declaration.
  `run_probe_scope`'s existing `ProbeScope::Function` arm already
  proves the isolated-function-check mechanism works for hover; detect
  a declaration-shape-preserving edit and reuse it, falling back to full
  `update_file` on save or a declaration-shaped edit.
- **G2 (large).** Per-symbol signature-shape fingerprinting to skip
  re-checking unaffected dependents — `Project::update_file` seeds
  `changed_symbols` with every name a file declares regardless of
  whether its externally-visible shape actually changed; fingerprint
  each symbol's signature (excluding body) and seed only genuinely-
  changed ones, narrowing `transitive_affected_files`'s existing BFS.
  Real PHPStan's own invalidation edge is stored per-*file*, not
  per-declaration, so it structurally cannot reach this granularity
  without a rewrite — this project's own code comment already names it
  as future work.

### Phase 107 — Whole-file/whole-project symbol lists (§AH)

- **AH3 (medium).** `textDocument/codeLens` reference-count annotations
  above declarations — aggregating the already-built Tier-1/Tier-2
  use-site index (`project::use_sites`, built for `textDocument/
  references`/rename) into per-declaration counts, plus new `CodeLens`
  response wiring.

### Phase 109 — `willRenameFiles` (§AO)

- **AO1 (large, depends on Phase 114's R1).** `workspace/
  willRenameFiles` — PSR-4-consistent rename-file refactoring on a
  file-explorer move: derive the old FQN (reverse `DeclLocations`
  lookup) and the new FQN the new path implies (mirror-image
  arithmetic of R1's path-to-FQN), rewrite the declaration plus every
  project-wide use-site via the already-built `project::use_sites`
  index (no longer a second dependency — it's the same index
  `textDocument/references`/rename already consume).

### Phase 110 — Diagnostic explainability: derivation traces (§T)

Real PHPStan's only introspection tool, `PHPStan\dumpType()`, computes
one final type per call with no accumulated reasoning kept and requires
editing source + a full CLI re-run.

- **T1 (large).** Interactive branch-narrowing derivation trace — an
  opt-in trace mode on `apply_narrowing` pushing (condition, branch,
  mechanism, before/after type) records into a new side-channel modeled
  on the existing `provenance: ProvenanceMap` field; `merge_scopes`
  already shows how such a channel composes across branch joins.
  Surfaced via the same one-shot instrumented-recheck pattern
  `probe_span`/`run_probe_scope` already proves for hover.
- **T2 (medium).** Per-pass loop-convergence widening trace — each
  pre-final pass of `converge_loop_body` is fully materialized in
  memory immediately before being discarded purely to keep iteration
  bounded; capture a before/after diff per pass under the same trace
  mode instead of truncating it silently. Needs its own step kind
  (iteration-indexed, no controlling condition).

---

## Part 6 (BEYOND_PHPSTAN): Codemods

Four fixes attaching to an existing diagnostic via the already-built
`textDocument/codeAction` wiring
(`Diagnostic.fix: Option<DiagnosticFix>`, `Server::code_actions`) — the
wiring itself has zero production call sites populating a real `fix`
yet, which is exactly what each item below does. Real PHPStan's entire
autofix surface is used by only 4 rule files project-wide.

### Phase 111 — Codemods attached to diagnostics phpstan-rs already ships (§I)

- **I1 (small).** Byte-exact same-length splice for every `*.nameCase`
  mismatch (`class.nameCase`, and the now-built `method.nameCase`,
  Phase 101's former E4) — identifier casing changes never change byte
  count, the safest codemod shape possible.
- **I2 (small).** Delete-attribute-group codemod for the already-
  shipped invalid-`#[Override]` diagnostic — a pure span deletion,
  strictly smaller/safer than real PHPStan's own whole-node-reprint
  approach to the identical rule.
- **I3 (medium).** Whole-declaration deletion for the zero-usage subset
  of unused-private-member diagnostics (`*.unused`, not the
  `neverRead`/`onlyWritten` siblings, which must NOT be auto-deleted).
  Must walk backward past any preceding doc-comment before deleting.
- **I4 (small).** `constructor.missingParentCall` (Phase 101's E3, now
  built) is ready to consume. Conditional single-line
  `parent::__construct();` insertion — only when the resolved parent
  constructor has zero required params; decline rather than guess
  otherwise.

---

## Part 7 (BEYOND_PHPSTAN): Config/strictness composability

Real PHPStan's parameters resolve exactly once into one frozen DI
container for the whole run; a rule is on-or-off for literally every
file. phpstan-rs has no such container and no parallel-worker model to
keep consistent.

### Phase 112 — Config/strictness composability (§J)

- **J1 (medium).** Per-directory additive opt-in checks — resolve a
  small path-pattern list once at load time into a per-file-path
  override table, consulted before falling back to the project-wide
  bool. No new dispatch mechanism needed.
- **J2 (large).** Per-directory strictness tiers in one run, with
  cross-directory calls still checked consistently — real PHPStan can
  only do this via two entirely separate invocations with no shared
  registry and no cross-directory soundness at all. Generalizes J1's
  override table from on/off to named tiers.

---

## Part 8 (BEYOND_PHPSTAN): Composer/dependency hygiene

Eight distinct axes, each keyed off a different `composer.json` section.
PHPStan's `Rules/` catalog has no package-declaration-aware check of any
kind — structural, since it has no persistent whole-program "which
package provides this symbol" index. phpstan-rs already has most
underlying pieces built for other reasons (`composer::
find_composer_package_name`, `detect_phpstan_dependency`,
`deps::referenced_names`, `DeclLocations` — several items below
originally noted `DeclLocations` needing to be opened up for batch
`check-project` runs, not just its LSP-only `retain_source` gate; that,
plus the whole-program diagnostic-emission pipeline every phase below
also needs, are both now built — see Part 1's own shared-prerequisite
note above Phase 85).

### Phase 113 — Composer declared-vs-used dependency hygiene (§K)

Three diagnostics keyed off resolving "which composer package owns this
symbol": `unused.composerDependency` (a `require` entry contributing
zero referenced symbols), `undeclared.composerDependency` (first-party
code resolves into a vendor package that's only present transitively,
not actually required — an unrelated `composer update` can silently
break the build), `misplaced.composerDependency` (a `require-dev`-only
package referenced from non-test application code, breaking
`composer install --no-dev`). **Effort: large.**

### Phase 114 — PSR-4 filesystem consistency (§R)

Real PHPStan's only PSR-4 awareness is consuming the mapping to *locate*
classes for reflection — it never validates location matches namespace.

- **R1 (medium).** `psr4.pathMismatch` — resolve each declared FQN's
  expected PSR-4 path and compare against its real declared file
  (`DeclLocations.classes`, now populated in batch runs too - see
  Part 1's own shared-prerequisite note above Phase 85). A file
  loading fine today only because Composer's dev autoloader falls back
  to directory scanning silently breaks under `composer dump-autoload
  --optimize`. (Phase 109's AO1 reuses this same prefix-map data in
  reverse.)
- **R2 (small, needs R1 first).** `psr4.caseMismatch` — case-
  insensitive-but-not-exact path match, via real on-disk `read_dir`
  casing (never `canonicalize`, which normalizes case and hides the
  bug). The classic "works on the author's case-insensitive laptop,
  breaks in Linux/Docker production" bug class — invisible to PHPStan
  since its own contributors/CI overwhelmingly run case-insensitive
  platforms too.

### Phase 115 — `misplaced.autoloadDevSymbol` (§AC)

Flag production code (resolving under `autoload.psr-4`) reaching a
symbol only declared under `autoload-dev.psr-4` — `composer
dump-autoload --no-dev` drops the dev root entirely, so the reference
fatals at runtime with zero warning today even though every dev-mode
install (what CI and every local run actually uses) resolves it fine.
Needs extending `composer.rs`'s existing parse to also read
`autoload-dev.psr-4` - `DeclLocations`'s own batch-mode prerequisite
(same as Phase 114's R1) is already built. **Effort: medium.**

### Phase 116 — `provide.unimplemented` (§AG)

For each `composer.json` `"provide"` entry matching one of the ~7
formally-specified PSR virtual packages (`psr/log-implementation` →
`Psr\Log\LoggerInterface`, etc.), verify via `Registry::is_subclass_of`
that some project-declared class actually implements the target
interface — a false claim any downstream `require`-ing package would
silently trust. Deliberately excludes the two PSR packages mapping to
several interfaces at once (ambiguous, v2 material). **Effort:
medium.**

### Phase 117 — `ext.notRequired` (§AK)

Flag a call to a stdlib function/class sourced from a specific
`vendor/phpstorm-stubs/<extension>/` directory when the project's own
`composer.json` declares neither `ext-<extension>` nor a covering
polyfill — the "works because the extension happens to be installed
locally" bug. The one real gap: no per-function "which extension" table
exists yet, though `build.rs`'s stub-collection pipeline already knows
and discards each file's extension-named parent directory at exactly
the point that data is needed. Must exclude always-compiled-in
extensions (`Core`, `SPL`, `Reflection`, ...). **Effort: large.**

### Phase 118 — `composerScript.notFound`/`notPublicStatic` (§AL)

Parse `composer.json`'s `"scripts"` map; for every `Class::method`-
shaped callable-string value (deliberately excluding shell-command
strings), resolve against the registry and flag a missing symbol or a
non-public/non-static target — Composer calls it via reflection-free
`call_user_func`, so either throws or silently no-ops at real
invocation time with zero static warning today. One honest gap:
`FunctionSig` has no `is_static` field yet (the raw `MethodDecl::
is_static` bool is already read and discarded one line away from where
`visibility` is threaded through, mirroring the already-completed
Phase 57 visibility rollout). **Effort: medium.**

### Phase 119 — `composer.platformPhpInconsistentWithRequire` (§AN)

Cross-check `config.platform.php` (a pinned PHP version overriding
Composer's own dependency-resolution assumption) against the project's
own `require.php` semver constraint for internal self-contradiction —
real PHPStan reads the platform field only as a fallback *input* to its
own analysis version, never validates it against anything else in the
same file. Needs a new small hand-rolled evaluator for Composer's
common constraint forms (`^X.Y`, `~X.Y`, `>=X.Y`, simple `||` groups) —
phpstan-rs has none today. **Effort: medium.**

### Phase 120 — Composer path-repository / monorepo workspace support (§S)

- **S1 (large).** Fix the symlink-escaping vendor-context filter (a
  real bug: `canonicalize`-then-`starts_with(vendor_dir)` always fails
  for a path-repository's `vendor/<pkg>` symlink, silently dropping the
  whole package from context), then read `composer.json`'s
  `repositories` array to auto-promote local `type: path` siblings from
  context-only into first-party analyzed files.
- **S2 (medium).** `check-project`'s vendor-context discovery is
  hardcoded to a single `cwd` root, even though the underlying
  multi-`dirs` capability already exists — for a real multi-package
  workspace, only whichever package happens to be `cwd` gets its
  vendor context discovered at all. Walk upward per entry in `dirs`
  (reusing `find_composer_package_name`'s pattern) and merge+dedup
  contexts.

---

## Part 9 (BEYOND_PHPSTAN): Type-system policy, baseline robustness, reverted-feature fixes, API diffing

### Phase 121 — Type-system representation policy lessons (§M)

Not missing features — four places real PHPStan's own source admits, in
its own comments, permanent BC debt from an early design decision.
phpstan-rs has no installed userbase to protect; each item is a
forward-looking policy to keep following, not a build task. **Effort:
small each, standing policy.**

- **M1.** Keep `ArrayShape`'s one in-band unsealed-marker representation
  — never add a nullable "ambiguous/legacy" tier alongside an explicit
  marker the way PHPStan's `ConstantArrayType::$unsealed` had to,
  forcing permanent parallel `legacyIsKeysSupersetOf`/`legacyMergeWith`
  code paths. Relevant once Phase 85's A3 territory (unsealed-shape
  superset/merge reasoning) is actually built.
- **M2.** Keep the `Type` layer a closed Rust `enum`, never a public
  trait third parties implement — PHPStan's `Type` interface still
  carries 7 permanently-`@deprecated`-but-unremovable methods because
  any implementor (including third-party extensions) must fully
  implement the interface forever.
- **M3.** Keep one canonical member order for `Union`/`Intersection`,
  never a `describe()`-only sorted cache alongside the canonical order —
  PHPStan's own comments admit an earlier version corrupted `getTypes()`
  by sorting in place inside `describe()`.
- **M4.** Never split builtin function signatures into a legacy-loose
  stub plus an opt-in stricter delta (real PHPStan ships parallel
  loose/strict stub files gated by `stricterFunctionMap`, defaulting
  off, purely for BC) — always encode the most precise known signature
  directly.

### Phase 122 — Baseline robustness (§N)

Real PHPStan's baseline is not actually unbounded; phpstan-rs's
currently is. Both concrete and fixable inside `src/baseline.rs`'s
existing data flow.

- **N1 (medium).** Bound baseline matches by recorded occurrence count
  — give `baseline::Entry` a `count` field so an occurrence beyond the
  originally-baselined count falls through as new. The count
  information already flows through `generate()` and is only discarded
  when `load()` collapses into a `HashSet`.
- **N2 (medium).** Opt-in identifier-anchored baseline match mode,
  surviving cosmetic message-text changes — `Diagnostic.identifier` is
  already stable-by-design; `baseline.rs`'s `Entry` just doesn't
  consult it yet as an alternate match key.
- **N4 (small, new from top-100 Packagist survey).** Per-identifier
  baseline file splitting — `shipmonk/phpstan-baseline-per-identifier`
  splits one monolithic baseline into separate files per diagnostic
  identifier (`baseline/argument.type.neon`, `baseline/return.type.neon`,
  etc.), making git diffs on baseline changes far smaller/more
  reviewable and letting a team pay down baseline debt one error
  category at a time. A pure file-organization/generation feature on
  top of `baseline.rs`'s existing data, no new matching logic needed.

### Phase 123 — Reattempting reverted features with real PHPStan's actual technique (§O)

Both items found the original *revert* (see Reverted) was correct given what
phpstan-rs had available, but that real PHPStan solves the identical case with a specific technique phpstan-rs's
attempt never used.

- **O1 (medium, narrowed and partly shipped - see below).** Foreach's
  zero-iteration merge needs real PHPStan's *asymmetric*, certainty-
  downgrading join (`processAlwaysIterableForeachScopeWithoutPollute` —
  keeps the post-loop type, only downgrades certainty, never re-unions
  the pre-loop type), not `merge_scopes`'s symmetric `union_types` — the
  exact self-mutating-iteratee idiom (`foreach ($items as $day => $data)
  { $items[$day][...] = ...; }`) both of phpstan-rs's prior attempts
  regressed on.

  A corpus-driven investigation found the *specific* false-positive
  pattern this item was scoped to fix (a loop-accumulator array read
  back as `T|null` instead of `T`) had a narrower real cause one level
  below the merge algorithm itself: `union_types` never simplified away
  a genuinely-empty `ArrayShape` (`array{}`) once a compatible general
  `Array(key, val)` member was already present in the same union - a
  plain redundant-subtype absorption real PHPStan's own
  `TypeCombinator::union()` already does. Fixed directly in
  `union_types` (`checker/type_union.rs`); `converge_loop_body`'s own
  merge algorithm was untouched, so the documented self-mutating-
  iteratee regression this item's own asymmetric-join proposal exists to
  avoid stays unaffected. 16 of the pattern's 70 known scoped-corpus
  findings were fully removed by this fix alone; O1's own general
  asymmetric-join proposal above remains open for whatever the
  narrower fix didn't reach. Along the way, this also root-caused a
  separate `int|string` array-key-type-imprecision cluster to a missing
  `BenevolentUnionType` marker on `Type[]`-shorthand-derived key types -
  not built here, folds into Phase 85's A4/`checkBenevolentUnionTypes`
  item instead of getting its own. Full corpus citations and the
  by-ref-foreach-mutation trade-off this fix's own verification
  surfaced: Known limitations' "Known but not yet scheduled" entry.

(O2 — registering each anonymous class under a real, stable synthetic
name instead of the discarded ancestor-aliasing shortcut — is now built;
see Done, "Anonymous class stable synthetic naming."
The former Phase 165 described the identical shipped work
independently; removed for the same reason.)

### Phase 124 — Whole-program public-API backward-compatibility diffing (§Q)

A new capability with no PHPStan-core or strict-rules equivalent at
all — its `analyseFile` entry point has no notion of comparing two
points in time; `Project`/`Registry` can trivially coexist twice in one
process since both are frozen, in-memory structures.

- **Q1 (large).** `phpstan-rs api-diff <old-ref> <new-ref>` — load two
  independent `Project`/`Registry` snapshots and structurally diff every
  public symbol's already-existing struct fields (removed member,
  narrowed parameter/widened return via `is_subtype`, narrowed
  visibility, newly `@internal`/`final`, removed interface, a
  non-final class gaining an abstract method). No reconstruction-via-
  reflection needed — every field this needs already exists as a plain
  struct field.
- **Q2 (medium, extends Q1).** PHPDoc-level generic/template-bound
  narrowing as its own break category — a tightened `@template T of
  Bound` or changed `@extends`/`@implements` type argument silently
  rejects existing callers even though the native PHP declaration (and
  therefore Reflection-visible signature) never changed at all,
  invisible to any tool working from native signatures alone.
  `ClassInfo::template_bounds`/`extends_args`/`implements_args` are
  already first-class, already-resolved fields on the same `ClassInfo`
  Q1 walks.

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

### Phase 125 — Classes (real-PHPStan `Rules/Classes/` gaps)

- `ImpossibleInstanceOfRule` — `instanceof.alwaysTrue`/`.alwaysFalse`/
  `.invalidExprType` when an `instanceof`'s own type resolves to a
  literal boolean or a non-string/object dynamic class-name expr;
  phpstan-rs's unreachable-code family deliberately excludes
  `instanceof` today.
- `NewStaticRule`/`NewStaticInAbstractClassStaticMethodRule` — "Unsafe
  usage of `new static()`" in a non-final class without a
  consistent-constructor guarantee, and the abstract-static-method
  variant.
- `InstantiationCallableRule` — `new Foo(...)` (first-class-callable
  syntax on `new`) is syntactically legal but a real semantic error;
  `callable.notSupported`.
- `InstantiationRule`'s remaining sub-checks — phpstan-rs's `new`
  checking only covers `new.interface`/`new.abstract`, missing several
  siblings.
- `ExistingClassesInClassImplementsRule` — phpstan-rs's `extends`/
  `implements`/trait-use check is existence-only, never confirms the
  target is the *right kind* of symbol (a class implementing an
  interface that's actually a trait, etc.).
- `InvalidPromotedPropertiesRule` — promotion legality (only inside
  `__construct`, never abstract, never variadic, PHP-8.0-gated, `final`
  promotion additionally gated).
- `ReadOnlyClassRule` — the PHP-version-gate data already exists
  (`PhpFeature::ReadOnlyClasses`/`ReadOnlyAnonymousClasses`,
  `php_version.rs`) but is never consumed by any checker call site.
- `UnusedConstructorParametersRule` — an unused constructor parameter
  is already flagged, but always under the generic `parameter.unused`
  identifier instead of real PHPStan's own `constructor.unusedParameter`.

### Phase 126 — Functions (`Rules/Functions/` gaps)

- `CallCallablesRule` remainder — `callable.nonCallable` for a dynamic-
  callee call is now built (receiver-type-checks phase, see Done);
  `callable.inaccessibleMethod` and full arity/parameter-type checking
  against a resolved `[$obj, 'method']`/`'Foo::bar'`-shaped callable's
  own signature are still open.
- `IncompatibleDefaultParameterTypeRule` closure/arrow-fn remainder — the
  function/method halves are now built (`parameter.defaultValue`, see
  Done); only the closure/arrow-fn literal variants remain.
- `return.missing` imprecision — `FunctionReturnTypeCheck`'s bare-
  `return;` check (`return.empty`) is done (see Done), but its sibling
  `return.missing` (falling off the end with no `return` at all) still
  has the same imprecision `return.empty` had before that fix.
- `ReturnNullsafeByRefRule` family — "nullsafe cannot be returned by
  reference" when a by-ref function/arrow-fn's return passes through
  `?->`; the existing `contains_nullsafe_access` helper is unused for
  this.
- `InvalidParameterNameRule`/`RedefinedParametersRule`/
  `VariadicParametersDeclarationRule` — a small family of level-0
  parameter-list sanity checks (superglobal/`$this` param name, a
  duplicate parameter name, a non-last variadic parameter) with zero
  phpstan-rs equivalent.
- `ParameterCastableToStringRule`/`...ToNumberRule` family — array-
  argument values passed to `implode`/`array_combine`/`sort`/etc.
  actually castable to string/number, since these implicitly cast at
  runtime.

### Phase 127 — Methods (`Rules/Methods/` gaps)

- `MissingMagicSerializationMethodsRule` — a non-abstract `Serializable`
  class lacking `__serialize()`/`__unserialize()`.
- `AbstractMethodInNonAbstractClassRule`'s second half — a non-abstract,
  non-interface method with no body at all (real PHP's parser accepts
  this invalid syntax; PHPStan must catch it).
- `MethodCallableRule` — gates first-class-callable syntax
  (`$obj->method(...)`) behind a PHP-8.1 version check; phpstan-rs's own
  extensive `PhpFeature`/`php_version.rs` framework covers roughly a
  dozen similarly narrow gates but not this one.
- `StaticMethodCallCheck`'s static/instance-context mismatch half —
  `method.staticCall` for calling a non-static method through
  `Foo::method()` static-call syntax (or the reverse). The casing-
  mismatch check in the same file is already built.
- `OverridingMethodRule`'s tentative-return-type sub-check —
  `method.tentativeReturnType`. Niche: it only matters when extending
  certain SPL/core classes.

### Phase 128 — Properties (`Rules/Properties/` gaps)

- `ExistingClassesInPropertiesRule`'s `property.trait`/two further
  identifiers — the `class.notFound` half is built (Phase 48); the
  "wrong kind of symbol" half isn't.
- `PropertiesInInterfaceRule`/`PropertyInClassRule` — PHP 8.4
  property-hook *declaration-site* legality (hooked/public/non-
  readonly/non-static/non-abstract/non-final for interfaces; the
  matching modifier-combination matrix for classes).
- `ReadOnlyPropertyRule`/`ReadOnlyByPhpDocPropertyRule` — `readonly`
  declaration legality (native type required, no default, not static,
  PHP-version-gated) for both the native and `@phpstan-readonly`/
  `@readonly` doc-tag forms.
- `InvalidCallablePropertyTypeRule` — `callable` as a property's native
  type is a real PHP fatal error; unchecked today.
- Five-rule cluster around PHP 8.4 write-only/read-only/asymmetric-
  visibility properties: assign-by-ref (plain/readonly-native/
  readonly-by-phpdoc), reading a write-only property, writing a
  not-writable one.

### Phase 129 — Comparison (`Rules/Comparison/` gaps)

- `ImpossibleInArrayHaystackFiniteTypesRule` — `in_array()`/
  `array_search()`/`array_keys()` where a needle's finite type can never
  match any constant-array-haystack value (level 4).
- `UsageOfVoidMatchExpressionRule` — using the result of a `match`
  whose every arm is a void-returning call; phpstan-rs's `Match`
  handling has no void-result-usage check at all.

### Phase 130 — PhpDoc (`Rules/PhpDoc/` gaps)

- `InvalidPhpDocTagValueRule` — `phpDoc.parseError` for a
  syntactically malformed PHPDoc tag value. The grammar-level PHPDoc
  parser already produces `DocBlock.parse_errors` (see Done), but no
  rule reads them yet. This one has a high corpus count (155 hits on
  piwigo16), spread across Piwigo's own code rather than one vendored
  file, so it's worth a closer look.
- `IncompatiblePhpDocTypeRule` family (param/property/property-hook/
  class-constant) — a PHPDoc type incompatible with its own native type
  hint; phpstan-rs silently reconciles/coerces instead of diagnosing.
  **Quick-win sub-slice from the top-100 Packagist survey**:
  `shipmonk/phpstan-rules`' `ForbidPhpDocNullabilityMismatchWithNative
  TypehintRule` covers just the *nullability* disagreement between a
  PHPDoc type and its native type hint — sharper-scoped and easier to
  implement than the full family; worth building first as an early
  win before tackling the general case.
- `IncompatibleSelfOutTypeRule` — `@phpstan-self-out` is entirely
  unimplemented (only in the typo-detection whitelist). (Phase 88's U4
  is the *inference* half of this same tag family, once real tag
  support exists.)
- `FunctionAssertRule`/`MethodAssertRule`'s deeper assertion-validity
  half — Phase 48.7 closed the plain class-existence slice
  (`class.notFound`); `assert.impossibleType`/`.alreadyNarrowedType`/
  `.unresolvableType`/`.trait`/`.unknownExpr` and generic/missing-
  typehint checks on the asserted type remain.
- `FunctionConditionalReturnTypeRule`/`MethodConditionalReturnTypeRule`
  — validating a conditional PHPDoc return type's own declaration
  (`conditionalType.subjectNotFound`/`.alwaysTrue`/`.alwaysFalse`).
- `InvalidPhpDocVarTagTypeRule`/`WrongVariableNameInVarTagRule`/
  `VarTagChangedExpressionTypeRule` — phpstan-rs's inline-`@var`
  checking is deliberately scoped to the plain-assignment case only;
  the much larger name-matching surface (foreach/static/global/return/
  misplaced-declaration variants) is unbuilt.

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

### Phase 132 — Arrays (`Rules/Arrays/` gaps)

- `DeadForeachRule` — `foreach` over a provably always-empty array,
  level 4. (`IterableInForeachRule`'s own non-iterable-subject half,
  `foreach.nonIterable`, is now built - receiver-type-checks phase, see
  Done.)
- `ArrayDestructuringRule` remainder — the RHS-is-array-like half
  (`offsetAccess.nonArray`) is now built (receiver-type-checks phase);
  the per-key-existence half is still open.
- `ArrayUnpackingRule`'s `arrayUnpacking.stringOffset` remainder — the
  non-iterable-spread half (`arrayUnpacking.nonIterable`/`argument.
  unpackNonIterable`) is now built (receiver-type-checks phase); only
  the PHP-8.1-string-key-support gating remains.
- `OffsetAccessWithoutDimForReadingRule` — `$x[]` used in read position
  (a real PHP compile-time fatal), non-ignorable.
- `NonexistentOffsetInArrayDimFetchRule` remainder — the "isn't offset-
  accessible at all" half is now built (`shapeAccess.notFound` for the
  shape case, receiver-type-checks phase); general-array/constant-array
  key-existence checking beyond shapes is still open. (Ties into Phase
  85's A3 sizing note.)
- `OffsetAccessAssignmentRule`/`OffsetAccessAssignOpRule` — an illegal
  key type for `$arr[$key] = $v`/compound-assign.
- `OffsetAccessValueAssignmentRule` — assigning a wrong-typed value into
  a typed array/object offset; `write_index_into` always unions the new
  value in rather than checking it against a pre-existing declared
  value type.

### Phase 133 — Constants (`Rules/Constants/` gaps)

- `ConstantRule` (level 1) — a bare undefined global-constant reference;
  phpstan-rs's `global_constant_type` falls back to `Mixed` for anything
  not in the table.
- `DynamicClassConstantFetchRule` — PHP 8.3's `Foo::{$expr}` dynamic
  class-constant fetch; `StaticAccess`'s AST has no way to represent a
  dynamic member name at all.
- `MagicConstantContextRule` — `__CLASS__`/`__TRAIT__`/`__FUNCTION__`/
  `__METHOD__`/`__NAMESPACE__` used outside their own context; blocked
  on phpstan-rs having no dedicated magic-constant `Expr` kind at all
  (already documented in `registry.rs`).
- `OverridingConstantRule` — class-constant override compatibility
  against a parent/interface (final override, visibility narrowing,
  native-type narrowing/dropping, PHPDoc-value-type covariance).
- `ValueAssignedToDefineRule`/`ValueAssignedToGlobalConstantRule` — a
  `define()`'d/`const`-declared global constant's value checked against
  a configured `dynamicConstantNames` type.
- `ValueAssignedToClassConstantRule`'s PHPDoc-`@var`-type half — the
  native-typed-constant half is built; an untyped `const` whose inline
  `@var` doesn't accept the literal value (`classConstant.phpDocType`)
  is explicitly skipped.

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
- `NullCoalesceRule`'s dead-`??`-operand check — this project's
  `nullCoalesce.variable`/`.property`/`.offset` only fire when the left
  side is *definitely undefined*. Real PHPStan's `nullCoalesce.expr`
  also flags the opposite case, where the left side is defined and
  already excludes `null`, so `??`'s right side can never run (5 corpus
  hits).

(`VariableCloningRule` — `clone.nonObject` — is now built; see Done,
receiver-type-checks phase.)

### Phase 136 — Traits+InternalTag (`Rules/Traits/` gaps)

- `ConflictingTraitConstantsRule` — a class constant overriding one
  supplied by a used trait must keep the same visibility/final-ness/
  native type (and, for immediately-used traits, the same value).

### Phase 137 — Keywords+Debug (`Rules/Keywords/`, `Rules/Debug/` gaps)

- `ContinueBreakInLoopRule` — a `break`/`continue`'s numeric level
  resolving outside enough loop/switch nesting, or crossing a closure
  boundary first.
- `DeclareStrictTypesRule` — `declare(strict_types=...)`'s value must
  be literally `0`/`1`, and the declare must be the file's first
  statement.
- `GotoUndefinedLabelRule` — a `goto` targeting a label that doesn't
  exist in the function; `collect_goto_targets` today only feeds
  dead-code reachability, never validates target existence.
- `UnusedLabelRule` — a `label:` statement never targeted by any
  `goto` — the inverse of the above, same underlying data.
- `RequireFileExistsRule` — resolving `require`/`include`(`_once`) path
  expressions (including the `__DIR__ . 'x'` idiom) against cwd/
  include_path/the file's own directory.
- `{Dump,DumpNative,DumpPhpDoc}TypeRule`/`DebugScopeRule` — PHPStan's
  own ad hoc developer-debugging hooks (`\PHPStan\dumpType()` and
  siblings). (Phase 110's T1/T2 and the already-shipped hover-type
  lookup (Phase 17/108-AM2, see Done) are phpstan-rs's own,
  materially more capable, interactive answer to this same need — this
  entry stays here only as the parity-audit's own finding, not a
  separate build.)

### Phase 138 — Generators+Pure (`Rules/Generators/` gaps)

- `YieldFromTypeRule` — `generator.nonIterable` when the expression
  after `yield from` is definitely (or, with `reportMaybes`, possibly)
  not iterable. A standalone check needing only `exprType.isIterable()`,
  independent of the broader `Generator<K,V,TSend,TReturn>` generic-
  shape work.

### Phase 139 — DeadCode+EnumCases+Namespaces+Regexp+Ignore+Missing+Names+Types+Whitespace (mixed small gaps)

- `Rules/DeadCode/NoopRule`'s generic fallback — `noop.rs` covers a set
  of specific, named side-effect-free-statement cases but has no
  generic catch-all (`expr.resultUnused`) for every *other* provably
  side-effect-free expression statement (a bare comparison, a bare
  arithmetic expression, a bare property/variable read).
- `Rules/Whitespace/FileWhitespaceRule` — a leading UTF-8 BOM
  (`whitespace.bom`) and a whitespace-only closing-tag remnant as the
  file's last statement (`whitespace.fileEnd`).
- `Rules/Types/InvalidTypesInUnionRule` — rejecting `mixed`/`never`/
  `void` inside a union or nullable type on a param/return/property.
- `Rules/Ignore/IgnoreParseErrorRule` — a non-ignorable diagnostic for
  malformed `@phpstan-ignore` identifier-list syntax; phpstan-rs's own
  `ignore_comments.rs` deliberately swallows this with no diagnostic at
  all today (confirmed by its own test name).


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

### Phase 141 — Declarative architecture/dependency-direction testing (new capability)

A whole new, well-established category with no precedent anywhere in
BEYOND_PHPSTAN.md or this file, confirmed by `carlosas/phpat` (1276
favers — among the highest real-developer-approval counts in the entire
top 100, not just a transitively-pulled-in download count). A fluent
DSL over composable selectors:

```
PHPat::rule()
  ->classes(Selector::inNamespace('App\Domain'))
  ->shouldNotDependOn()
  ->classes(Selector::inNamespace('App\Infrastructure'))
```

- **Selectors** compose: `inNamespace`, `classname`, `extends`,
  `implements`, `isAbstract`/`isFinal`/`isInterface`/`isEnum`/
  `isReadonly`/`isException`/`isTrait`/`isAttribute`/
  `appliesAttribute`/`isStandardClass`, with `->excluding(...)`
  exemptions.
- **Relations**: `shouldDependOn`/`shouldNotDependOn`/`shouldImplement`/
  `shouldExtend`/`beFinal`/`beReadonly`/etc.
- **Effort: large** (a new rule-DSL layer), but the underlying
  mechanism — resolving every class's own `use`-import/type-hint/
  extends/implements references against a small set of configured
  namespace/selector rules, flagging a forbidden dependency direction —
  is a project-wide pass phpstan-rs's own closed-world, single-process
  Registry is arguably *better* suited to than real PHPStan itself (the
  same "whole-program fact PHPStan's architecture can't cheaply compute"
  theme Part 1 already establishes repeatedly). Real corpus relevance is
  plausible for Piwigo's own layered namespace structure — worth
  checking at implementation time.
- Genuinely distinct from `restricted_symbols` (per-symbol banning) and
  from the `@internal` engine (package-boundary enforcement) — namespace
  *layering direction* is a third, different axis.

### Phase 142 — Composer-manifest dependency-declaration hygiene (new capability)

Two independent real packages converge on the same underlying need:
validate that every class a file uses is actually reachable through
that *specific module's own* declared Composer dependencies, not merely
resolvable via Composer's flat, whole-project autoloader.

- **Shared prerequisite (medium).** Parse the project's own
  `composer.json` (`require`/`require-dev`) plus `vendor/composer/
  installed.json` (every installed package's own PSR-4 namespace
  mapping) into a class-FQN → owning-package index. (Phase 94's P2
  above needs the identical index for real Composer package-boundary
  `@internal` enforcement — share one implementation, don't duplicate.)
- **142a (medium).** Shadow-dependency detection: a class is used, but
  its owning package is declared in neither `require` nor
  `require-dev` — only resolvable because some *other* direct
  dependency happens to also require it. Breaks the moment that
  incidental transitive path is removed. (`gosuperscript/phpstan-rules`'
  `RestrictImplicitDependencyUsage`, independently confirmed by
  `pb30/phpstan-composer-analysis`'s wrapped `shipmonk/composer-
  dependency-analyser` tool — real, two-package-validated demand.)
- **142b (small, needs a prod-path/dev-path file classification).**
  Dev-dependency-used-in-production-code: a class from a
  `require-dev`-only package is referenced from a file classified as
  "prod" — would break a `composer install --no-dev` deployment.
- **142c (small).** The inverse of 142b: a `require`-declared package
  is only ever referenced from dev/test paths — should be
  `require-dev` instead.
- **142d (small).** Unused-dependency detection: a declared package
  (either section) has zero real usage anywhere in scanned paths —
  dependency-bloat, safe to remove.
- 142b-142d all come from `pb30/phpstan-composer-analysis` alone;
  142a is the one both packages independently confirm.

### Phase 143 — Generalized "ban this construct/keyword/type" mechanism

A small, self-contained AST-shape-match mechanism — deliberately
simpler than the `spaze` allow/disallow-in DSL
(Phase 67), because none of these need call-site-scope-path/attribute-
context matching, only "does this bare AST-node-kind/type appear at
all, opt-in per rule." Real, independently popular (`ergebnis/
phpstan-rules`, republished/forked as `localheinz/phpstan-rules` —
one of the richest single packages surveyed):

- **143a.** Ban a bare language construct outright, config/opt-in per
  rule: `eval`, `isset()`, `compact()`, the `@` error-suppression
  operator, `switch`, PHP 8 named arguments, assignment-by-reference
  (`=&`) and by-ref function/method/closure parameters/returns, the
  short ternary `?:`, loose `==`/`!=` comparison, implicit
  array-append-key creation (`$arr[] = x`), `empty()`, backticks,
  `@phpstan-ignore`/`@phpstan-ignore-line` comments (forces fixing real
  issues instead of suppressing — an "eat your own dog food" policy
  check), and direct `$_GET`/`$_POST`/etc. superglobal access (forces a
  request-wrapper abstraction). Sources: `ergebnis/phpstan-rules`,
  `phpstan/phpstan-strict-rules`' `Disallowed*`/`NoSuperglobalsRule`
  family (`thecodingmachine/phpstan-strict-rules`).
- **143b.** Ban a bare *type* outright: `roave/no-floaters`' float/
  double ban (property types, param/return signatures, variable
  assignments) — avoid floating-point in money/precision-critical
  domains. A distinct "ban this native type" sub-family from 143a's
  "ban this construct."
- **143c.** Ban dynamic member-name-expression access entirely (opt-in
  strictness, not a resolution problem): `$$foo` variable-variables,
  `$obj->$memberNameExpr`/`$class::$memberNameExpr` where the member
  name is a runtime expression rather than a literal — these already
  collapse to `Mixed` in phpstan-rs's checker; this rule only needs to
  flag the *syntactic shape*, not resolve anything
  (`phpstan/phpstan-strict-rules`' `VariableVariablesRule`/`Variable*`
  family, duplicated independently in `symplify/phpstan-rules`'
  `NoDynamicNameRule`).
- **143d (curated preset, not new architecture).** A ready-made config
  preset banning 12 byte-oriented string functions (`strlen`/`substr`/
  `strtolower`/`strpos`/`stripos`/`strstr`/etc.) in favor of their
  `mb_*`-prefixed multibyte-safe counterparts — a real, common
  UTF-8-correctness bug class (`korbeil/phpstan-generic-rules`). Ships
  as a documented example config for `restricted_symbols`
  (Phase 67), not a new mechanism.
- **Effort: medium** overall — one AST-node-kind/type-kind matcher plus
  a config table, reused across all four sub-families.

### Phase 144 — "Closed by default" class shape + generalized parent-hook-call requirement

- **144a (small).** Opinionated "closed by default" class-shape bundle:
  every class `final` unless `abstract`; every member `private` unless
  the class is `final` (no subclass exists to protect from); concrete
  methods inside an `abstract` class themselves `final` (only
  overridable via the class's own abstract methods) — `ergebnis/
  phpstan-rules`' `FinalRule`/`PrivateInFinalClassRule`/
  `FinalInAbstractClassRule`. Each a trivial class/member-shape check.
- **144b (medium, generalizes and supersedes Phase 101's E3 scope).**
  A fully generic "must invoke `parent::method()`" mechanism —
  config-driven `{className, methodName, invocation: first|last|any|
  never, hasContent: yes|no|maybe}` entries, walking up to the real
  declaring parent class via reflection (not requiring an exact
  class-name match). Ship with sensible defaults for PHPUnit's own
  `setUp`/`tearDown`/`setUpBeforeClass`/`tearDownAfterClass`/
  `assertPreConditions`/`assertPostConditions` and Codeception's
  equivalent hooks — real, concrete value even with zero project
  config. This is the canonical, best-designed version of the same
  underlying idea independently reinvented three more times in the
  survey: `phpstan/phpstan-phpunit`'s `ShouldCallParentMethodsRule`
  (PHPUnit-only), `cambis/silverstan`'s
  `RequireParentCallInOverridenMethodRule` (SilverStripe-only,
  first/anywhere only, no last/never), and BEYOND_PHPSTAN's own E3
  (`parent::__construct()` only). Build against `ergebnis`'s shape
  directly — it strictly subsumes all three narrower versions,
  including E3 itself (a `{className: null (any), methodName:
  '__construct', invocation: any}`-style entry, or keep E3 as its own
  cheap special case if the generic form's config surface isn't worth
  it yet).

### Phase 145 — Extended access-control primitives (alternative to Phase 67's declined DSL)

`dave-liddament/phpstan-php-language-extensions` ships a materially
*simpler*, attribute-based design for the same underlying need Phase
67's declined `spaze` allow/disallow-in DSL was trying to solve
generically — worth building this instead when that need resurfaces,
since it needs no path/context-matching engine at all:

- **145a.** `#[Friend(AllowedCaller::class)]` on a method/class —
  explicit whitelist of which *other* specific classes may call it (the
  classic C++ `friend` concept via attributes). The whitelist lives on
  the target itself, not in a separate global config file.
- **145b.** Namespace-level sibling of 145a — declare a whole namespace/
  class visible only from specific other namespaces via attribute.
- **145c.** `#[TestOnly]`-style marker on a method, flagging *production*
  code that calls it — the inverse framing of Phase 115's autoload-
  dev-symbol-reachability idea, keyed on an explicit author-written
  marker instead of `composer.json`'s PSR-4 split.
- **145d.** Restrict a trait to only be `use`d by a whitelisted set of
  classes — a novel, narrow access-control primitive with no existing
  phpstan-rs equivalent.
- **145e.** Composer-package-boundary visibility enforcement — directly
  overlaps Phase 94's P2 above; share one implementation.
- **Effort: medium** — an attribute-reading pass plus a caller-identity
  check at each of the relevant call/instantiate/extend sites, reusing
  the same call-site inventory the `@internal` engine (Phase 67) and
  `restricted_symbols` already walk.

### Phase 146 — Pluggable assertion/mock-narrowing tables + PHPUnit native mock-builder hygiene

- **146a (medium).** Generalize phpstan-rs's existing
  `apply_phpunit_assertion_narrowing` mechanism into a **pluggable
  table design** rather than one hardcoded library. Confirmed by real,
  independent demand across at least 6 separate assertion/mock
  libraries surveyed, each needing only a new table entry once the
  mechanism is pluggable: `webmozart/assert` (~64 static methods —
  `string`/`nullOr`-prefixed/`all`-prefixed variants of each basic
  type/shape check; genuinely likely in Piwigo's own transitive
  dependency tree, worth checking first), `beberlei/assert` (same
  fluent shape), `azjezz/psl` (PHP Standard Library), Mockery,
  Prophecy, Nette Tester's `Assert::*`, and `eloquent/phony`'s
  MockBuilder/InstanceHandle fluent API.
- **146b (medium, new family, entirely generic, no new mechanism
  needed).** PHPUnit-native mock-builder-API misuse detection —
  distinct from 146a (this catches mock-*configuration* mistakes, not
  assertion-type narrowing), confirmed by `rector/mockstan`'s rich,
  fully generic rule set (needs only phpstan-rs's existing test-class
  detection + method-name matching):
  - `$this->expects($this->any())` is ambiguous — require an explicit
    count or a stub instead.
  - `->method('x')->with(...)` without a preceding `->expects(...)`
    call is a stub, and PHPUnit's own `with()` on a stub is silently a
    no-op — a real, easy-to-miss test bug (the assertion looks like
    it's checking arguments but never does).
  - Combining `willReturnOnConsecutiveCalls()` and
    `willReturnCallback()` on the same mock is redundant/confusing.
  - A property typed as a union/intersection mixing `MockObject` with a
    real class is an ambiguous smell.
  - A mocked method configured via `->method(...)` needs an explicit
    `->expects(...)` count.
  - `$this->atLeast(0)` is meaningless (matches any call count
    including zero) — require ≥ 1.
  - A test consisting only of mocked properties with no real property
    is suspicious — it isn't testing anything real.
  - Config-driven ban on mocking specific classes — same shape as
    `restricted_symbols`, applied to mock-target class names.

### Phase 147 — Exception-hygiene family (consolidates 3 independent confirmations)

Entirely generic, zero framework dependency, confirmed independently by
`nette/nette` (RethrowExceptionRule), `thecodingmachine/phpstan-strict-
rules`, and `shipmonk/phpstan-rules` — three separate real tools
converging on the same small set of checks is a strong signal this is
real, widely-valued, and cheap:

- **147a (small, easy, high value).** Inside a `catch` block, a newly
  thrown *wrapping* exception must pass the caught variable as its own
  `$previous` constructor argument somewhere — otherwise the original
  stack trace/cause is silently lost. Walk a catch body's own
  `throw new Exception(...)` sites, check the caught variable appears
  in the constructor call's own args.
- **147b (small).** Catching the base `\Exception`/`\RuntimeException`/
  `\Throwable` requires an unconditional rethrow somewhere in the catch
  body, unless deliberately building an exception handler — the
  unconditional sibling of Phase 88 U2's Nette-rethrow consumer (which
  only fires when a *more specific* exception was thrown and
  swallowed).
### Phase 148 — "No mixed member access" + null-coercion-safety + enum-exhaustiveness-enforcement families

Confirmed independently by `rector/type-perfect` and `shipmonk/
phpstan-rules` (same "no mixed" idea reinvented twice) — entirely
generic, zero framework dependency:

- **148a (small, 2x confirmed).** Opt-in ban on calling a method or
  accessing a property on a `mixed`-typed value at all, forcing
  explicit narrowing before any member access.
- **148b (small).** Ban `isset()`/`empty()`/array-index syntax
  targeting a plain object (each behaves oddly/silently on a plain
  object without `ArrayAccess`/magic methods).
- **148c (novel family, small each).** Ban a possibly-null value from
  silently participating in string interpolation (renders as `""`), a
  binary operator, or a compound assignment (`+=` etc.) — PHP's silent
  null-to-empty-string/zero coercion is a real, common bug source.
- **148d (novel, 3x-confirmed-independently-valuable, medium).** A
  `match` over an enum subject with a `default` arm defeats PHPStan's
  own exhaustiveness benefit — when a new case is added later, the
  default arm silently swallows it instead of surfacing a missing-arm
  error. Pair with requiring `match` (not `switch`/if-chains) when
  branching over an enum value at all, so exhaustiveness checking
  actually applies.

### Phase 149 — Native-typehint-preference + arithmetic/comparison-operand strictness families

- **149a (small each, easy).** Require a *native* type hint (not just a
  PHPDoc-only one) wherever PHP's own grammar could already express it,
  for return types and closure params — closes real precision gaps
  PHPDoc-only typing leaves open to drift.
- **149b (medium, real 3rd-independent-confirmation family, from
  `phpstan/phpstan-strict-rules` §5 + `shipmonk/phpstan-rules` +
  `voku/phpstan-rules`).** Ban PHP's automatic numeric-string coercion
  in arithmetic/binary operators entirely when strict — requires
  genuinely numeric operands for `+`/`-`/`*`/`/`/`%`/`**` and unary
  `+`/`-`/`++`/`--` (phpstan-strict-rules' 12-operator
  `OperatorRuleHelper` family); restrict `<`/`>`/`<=`/`>=` to genuinely
  comparable type pairs (`shipmonk`'s
  `AllowComparingOnlyComparableTypesRule`); ban a string operand
  combined with a non-string/non-mixed operand, and an array operand
  combined with a non-array/non-mixed operand, across most binary
  operators (`voku/phpstan-rules`' `ExtendedBinaryOpRule`/
  `ExtendedAssignOpRule`). One shared mechanism (a type-kind check on
  both operands of a `BinOp`/`AssignOp`/unary-op node), three
  independently-converged-on operand-mismatch policies.
- **149c (small, real, version-relevant).** `in_array()`/
  `array_search()` called without an explicit strict `true` third
  argument, where the needle/haystack-element types are a non-numeric-
  string vs. int/float pair — the exact PHP-7-to-PHP-8 loose-comparison
  hazard (`0 == "foo"` flipped from true to false in PHP 8). Fits
  naturally alongside phpstan-rs's existing `PhpFeature`-gated
  version-behavior-change checks (`voku/phpstan-rules`'
  `InArrayLooseComparisonRule`).
- **149d (small).** `ini_get`/`ini_set`/`ini_alter`/`ini_restore`/
  `get_cfg_var` called with a literal string naming a PHP ini directive
  deprecated as of a specific PHP version (hardcoded table:
  `mbstring.func_overload` since 7.2, `assert.active` since 8.3,
  `session.sid_length` since 8.4, etc.), checked against the project's
  configured PHP version — a tiny addition to the existing
  `PhpFeature`/`php_version.rs` framework (`phpstan/phpstan-
  deprecation-rules`).
- **149e (small each).** Sharp-edge gotcha checks: `iterator_to_array()`
  called without `$preserve_keys` explicit (silently loses/collides
  keys iterating a Generator with non-sequential/string keys);
  identical-class `===` comparison; calling `__construct()` directly as
  an ordinary method/static call (`$obj->__construct()` re-runs the
  constructor on an already-built object); a non-static closure
  explicitly capturing `$this` via `use ($this)` when it's already
  implicitly bound (pure redundancy); a `foreach`/`for` loop's own
  loop-variable shadowing a variable already holding a value from
  outside the loop (data-loss footgun); calling a statically-declared
  method via `->` instead of `::`; a `switch` case value whose type
  differs from the subject's (switch uses loose `==` internally).
- **149f (small, easy, generic).** `$a === $b` (`Identical`/
  `NotIdentical`) where `$a`'s static type is a class defining a native
  `equals($other)` method whose parameter type accepts `$b`'s type —
  the class author clearly intended value-equality semantics via
  `->equals()`, so `===` (reference identity) is almost always a bug
  (two logically-equal but distinct instances won't be `===`-equal).
  Fully generic, no framework dependency (`gosuperscript/phpstan-
  rules`).

### Phase 150 — "No magic numbers" + typed-class-constants families

- **150a (medium, novel, comprehensive, zero existing equivalent).** Ban
  a bare numeric literal (int/float, configurable exempt-numbers list
  e.g. `[0, 1, -1]`, optional numeric-string variant) across ~12
  distinct AST positions: assignment, ternary, switch case, return,
  match, logical/comparison/bitwise/arithmetic operators, default
  parameter value, property assignment, function argument. One shared
  base-class detection predicate (handling unary `+`/`-`, string-casts,
  optional numeric-string literals) reused across all position-specific
  checks — one shared helper plus N small call-site checks
  (`sidz/phpstan-rules`).
- **150b (small, easy).** Require every class constant carry an
  explicit PHP 8.3 native type declaration (`const int FOO = 5;`, not
  bare `const FOO = 5;`) — a natural sibling of phpstan-rs's own
  `missingType.*` family if class constants aren't already covered
  (`hiperdk/phpstan-rules`).

### Phase 151 — PSR-3 structured-logging hygiene family

Confirmed and deepened independently by two real packages
(`staabm/phpstan-psr3`, `struggle-for-php/sfp-phpstan-psr-log`) — same
shape as phpstan-rs's own already-built printf/sprintf placeholder-type
checking (`format_string.rs`), just PSR-3's `{key}`-brace syntax against
a literal context array's own keys instead of positional arguments.
Genuinely good real-corpus-value potential given how near-universal
PSR-3 logging is in any DI-framework-based PHP app:

- **151a (medium).** For any `Psr\Log\LoggerInterface` method call
  (`log`/`debug`/`info`/`notice`/`warning`/`error`/`critical`/`alert`/
  `emergency`), cross-check the message string's `{placeholder}` tokens
  against the literal keys of the passed context array — flag a
  placeholder with no matching key, and vice versa — plus checking
  context values used for interpolation are stringable.
- **151b (small).** Require the log message itself be a static/literal
  string, not a dynamically-concatenated one — dynamic values belong in
  the context array's own placeholders for structured-logging/log-
  aggregation friendliness.
- **151c (small).** Validate a log-level argument is a recognized PSR-3
  level constant.
- **151d (medium).** Validate the *structure* of the context array
  itself — e.g. requiring a `context['exception']` key actually be a
  `Throwable`.

### Phase 152 — "No implicit current-time dependence" family

A genuinely novel, valuable family: real, simple AST-shape/literal-
argument checks with zero framework dependency, useful for testable,
deterministic code (a function silently depending on wall-clock "now"
is a classic source of flaky tests and hard-to-reproduce bugs)
(`slam/phpstan-extensions`):

- Bans `time()`.
- Bans `strtotime('now'/'+1 day'/etc.)` — anything relative to the
  current instant rather than a fixed, explicit reference.
- Bans `new DateTime()`/`new DateTimeImmutable()` with no explicit
  fixed date/reference.
- Bans `date($format)` without its optional second `$timestamp`
  argument, which implicitly uses the current time.
- **Effort: small** overall — four independent literal-argument/
  call-shape checks sharing no state.

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

### Phase 154 — Output-format additions

Two small, self-contained `output::OutputFormat` additions, neither
touching any diagnostic logic — reads the already-resolved file's own
source text plus the diagnostic's already-known span/message:

- **154a (small).** A JUnit-XML `ErrorFormatter` — each diagnostic
  becomes a `<testcase>` (failure) in a `<testsuite>`, consumable
  natively by Jenkins/GitLab-CI/GitHub Actions test-report UI widgets.
  Add `Junit` as a 4th `OutputFormat` variant alongside the existing
  `Text`/`Json`/`Checkstyle` (`mavimo/phpstan-junit`).
- **154b (small).** A "friendly"/codeframe terminal formatter —
  renders each diagnostic with a syntax-highlighted source-code frame
  around the offending line, the same UX convention modern compilers
  use (rustc, ESLint's codeframe formatter) versus the terse default
  "file:line: message" one-liner (`yamadashy/phpstan-friendly-
  formatter`).

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
Deployer stubs); Blade/Twig/Latte template-compiler packages (needs a
template-DSL-to-PHP compiler plus a source-map — architecturally
interesting but no live successor found for two of the three, and the
third is Laravel-Blade-specific); Pest's own plugin (entirely built
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

## Narrowing-engine follow-ups (phases 173, 175)

The two phases left over from the 158-175 correlated-narrowing batch.
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

### Phase 175 — Closure capturing an outer branch's co-assignment fact

`RateService::rate`'s own closure - `$userAnonymous && $anonymousId
!== $savedAnonymousId` implying `$savedAnonymousId` was set to a real,
non-null value by the *outer* scope's own `if ($userAnonymous) {
$savedAnonymousId = ... ?? ...; }` before the closure even runs.

Phase 169's own mechanism (`Locals::branch_implied_facts`,
`checker.rs`/`checker/narrowing.rs`) is now built and its exact shape
settled, so this is independently assessable - and needs real, further
work beyond what 169 shipped, not just closure-capture plumbing on top
of it: `record_branch_implied_facts` only recognizes an equality
discriminant (`$subject === value`/`$subject == value`), matching the
`AuthService::authKeyLogin` shape 169 was built for. `RateService::
rate`'s own discriminant is a bare boolean truthiness test
(`if ($userAnonymous) { $savedAnonymousId = ...; }`), a different
recognized-shape gap on top of the closure-capture question - the
discriminant key would need to cover a bare `narrowing_key`-able
truthiness condition too, not just an equality comparison, before the
closure-capture half (surviving into a closure body reading the
captured variable, seeded from the outer scope's own `Locals` at the
closure literal's declaration point - see `checker/expr.rs`'s
`closure_locals`/`arrow_locals` seeding) is even reachable.

**Effort: medium** — one more discriminant-shape arm in
`record_branch_implied_facts`/its replay hook (bare truthiness, not
just equality), plus verifying the closure-capture seeding path
actually carries `branch_implied_facts` forward (`Locals::fresh_scope`
currently resets it to empty for every closure literal, matching
`condition_aliases`'s own precedent - may need to *not* reset it here
specifically, unlike the boolean-flag-scoped `condition_aliases`,
since a fact here is meant to survive scope boundaries).


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
- **140c.** A `foreach ($arr as &$v) { $v['key'] = ...; }` by-reference
  mutation never updates `$arr`'s own tracked element shape. A real,
  buildable gap (real PHPStan does track this), surfaced only once a
  separate fix stopped masking it behind `Mixed`. Phase 123's O1 is the
  closely related foreach-merge fix; this is a distinct, narrower
  by-ref-mutation-tracking gap, not the same item.
- **140d.** Dynamic-key `isset($arr[$dynamicKey])`/`array_key_exists(
  $dynamicKey, $arr)` never narrows the specific element's own value
  type - only a *literal* string key does today. A real, buildable
  feature (extending `array_value_type_for_key`'s literal-key-only
  mechanism to a narrowed dynamic key expression), confirmed live at
  four real corpus call sites. Not attempted yet because it's a
  materially larger change (value-keyed, not name-keyed, narrowing) than
  a corpus-cleanup pass should take on, and each finding it produces
  today is sound, not a false positive to chase down.
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
  is filed and buildable - Phase 173, see Planned; the cross-function
  generalization splits into two layers - the two-pass collect-then-
  consume checking architecture (Phases 166/174, now built - see Done's
  "Too-wide return/property types" entry), necessary but not sufficient
  on its own, and "soundly answering whether the correlation still holds
  by the time a different function reads it" - a fundamentally harder,
  unbounded invariant-maintenance problem with no known bounded design
  yet, the actual wall); a compound `RateService::rate`-shaped case
  compounding discriminant-variable correlation with closure-capture
  timing (Phase 175, see Planned, blocked on Phase 169 - now built, so
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
