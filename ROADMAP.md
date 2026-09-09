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

This document has four sections after Current status: **Done**,
**Pending / partial**, **Reverted** (implemented, corpus-verified, then
undone - kept so the real root cause isn't re-discovered from scratch
next time), and **Known limitations** (deliberate boundaries not tied to
any one phase). See **Verification approach** at the end for the
methodology behind every corpus number cited above.

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
  metric - has fallen from 2312 (start of the Piwigo-corpus hunts) to
  low double digits, independently verified clean under real PHPStan at
  level 10 as of the last full re-run. Re-verify against real PHPStan
  before citing an exact number - it drifts slightly release to release
  as new checks ship.
- Full Piwigo+`vendor/` corpus (~20,300 files, unscoped): tens of
  thousands of diagnostics, the overwhelming majority genuine findings
  in third-party code this project doesn't control. Re-run before
  citing a number - it drifts as vendored dependencies and this
  project's own precision both change.
- Test suite: 2400+ unit tests, all passing.

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
through `Intersection`). `Type::Array` carries a real key type, not
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
`GenericObjectTypeCheck`).

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
`count()`/`array_is_list`/`str_contains`-family truthiness narrowing,
including the mirrored/negated `count($x) < 1` shape. Comparison-outcome
narrowing for `<`/`>`/`<=`/`>=` between known-numeric operands, and a
loose (`==`/`!=`) truth table between differing scalar literal kinds,
transcribed from real PHPStan's own `LooseComparisonHelper`.

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
Call-argument binding (named args, spread/unpack, by-ref legality).
`MissingTypehintCheck`'s full level-6 battery. Property/method/constant/
class-constant visibility enforcement end to end (read, write, call,
static access, late-static-binding-unsafe `static::` access, PHP 8.4
asymmetric write-visibility), plus the whole first-class-callable family
running the same checks an ordinary call gets. Unused-symbol detection.
Duplicate-declaration scans within a class/trait/enum body, plus a
same-file case-insensitive class-vs-`use`-alias collision. Override-
compatibility (visibility narrowing, missing/extra/optional/by-ref/
variadic parameter mismatches, `#[Override]`'s unconditional-fatal-error
half, running uniformly for a class's own methods and a trait-supplied
one). Nine declaration-site modifier-consistency checks plus the
class-constant `final`/native-type-modifier trio. Enum sanity and
generic-bound checking wired to enums.

Property-hook checker validation (PHP 8.4): virtual-property detection
via an empirically-confirmed rule (a hooked property is virtual iff no
hook body references `$this->propName` anywhere in its own function
scope - verified against real `ReflectionProperty::isVirtual()`, since
neither vendored corpus documents the algorithm), `propertyGetHook.
noRead`/`propertySetHook.noAssign`, `SetPropertyHookParameterRule`'s
native-type-contravariance half, real function-like body checking for
hook bodies (undefined variables, return-type/generator-return
mismatches), and `PropertyVarianceRule` reusing the existing
variance-walk primitive. Promoted-constructor-property inline hooks
aren't covered - the registry has no path to resolve `set_ty`/
`is_virtual` for that shape.

`@internal` restricted-usage engine (function/method/static-method
calls, property/constant reads and writes, instantiation, `instanceof`,
`catch`, implicit `__toString()`), off by default. `@deprecated` usage
warnings across the identical access-shape set, off by default.
Attribute existence-checking plus target-validity for every declaration
kind the parser captures an attribute list for (property/method/
function/param/class/trait/enum/enum-case/class-const/closure/arrow-fn),
plus constructor-argument validation (abstract/no-constructor/non-public-
constructor checks, arity/named-argument/by-ref/type checking,
ancestor-chain-aware). `PropertyHookAttributesRule`/`ConstantAttributesRule`
are blocked on a real parser gap (the AST doesn't capture a property
hook's or a top-level `const`'s own attribute list at all);
`attribute.nonRepeatable`/`.deprecated` are a deliberate scope trim.
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
Return-statement completeness (`return.never`/`.void`/`.missing`, both
explicit and fall-through). Generator declared-vs-actual return-type
compatibility - the full `Generator<K,V,TSend,TReturn>` shape itself
remains unmodeled.

Format-string checking for `sprintf`/`printf`/`fprintf`/`vsprintf`,
including positional (`%N$`) specifiers. Regex capture-group inference
(literal enumeration, tagged unions for one optional group/one
top-level alternation, numeric/non-empty-string fallback typing).
A Piwigo-corpus-driven batch of small builtin checks: function-name
case-insensitivity, `define()`'s PHP-8-removed third argument,
`random_int()` literal-bound ordering, useless-return-value builtins,
array-literal duplicate-key detection, `array_values`/`array_filter`
no-op conditions, `array_multisort()`'s stub fix. Dozens of individual
stdlib array/string dynamic-return-type extensions: `array_fill_keys`,
`array_search`, `array_intersect_key`, `array_flip`, `array_change_key_case`,
`array_rand`, `array_count_values`, `array_splice`, `array_replace(_recursive)`,
`array_pad`, array-pointer accessors (`array_pop`/`array_shift` included),
the `array_diff`/`array_intersect` family (a disclosed residual: the
vendored stub itself, not this checker, types the PHP8+ variadic tail
of `array_udiff`/`array_uintersect_assoc` as bare `Mixed`), `preg_split`,
`array_column`, `glob`, `explode`, `tempnam`, `version_compare`,
`filter_var`'s `FILTER_VALIDATE_*` family (including a literal
`['options' => [...]]` array's own `default`/`min_range`/`max_range`
sub-keys), `curl_getinfo`'s ~73-entry table, the `date()`/`idate()`/
`pathinfo()`/`parse_url()`/`sscanf()`/`stat()` family,
`get_called_class`/`get_parent_class`/`gettype`/`get_debug_type`.
`DateInterval::__construct()`'s own duration-string grammar validation.
`UnreachableStatementRule` (`deadCode.unreachable`, level 4), reported
once per maximal unreachable run without skipping analysis of the rest.

Contextual closure-body checking (a callback's own inferred parameter
types, not a bare unhinted check) for `array_map`, `array_filter`
(2-arg default mode), `usort`/`uasort`, `array_walk` (2-param closures),
and `preg_replace_callback` (a generic `array<int|string, string>`
`$matches` shape).

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
target as reachable rather than dead. This family was reverted once
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
one.

**PHPDoc tag ecosystem** (Phase 36). `@phpstan-*`-prefixed tag
spellings; `@property`-family validation; `@phpstan-assert`/`-if-true`/
`-if-false` (including the narrowing they drive at call sites);
`@phpstan-type` and cross-class `@phpstan-import-type`;
`@phpstan-consistent-constructor`; `@phpstan-require-extends`/
`-implements`/`@phpstan-sealed`; `@method`/`@mixin` synthesis into
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
tag parsers already establish. Still unbuilt:
`@param-immediately-invoked-callable`/`-later-invoked-callable`/
`-closure-this` (needs live per-call-site `Scope`-threading this
project's checker has no equivalent for - a genuine architectural
mismatch, not a scope trim).

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
config support. `--level`/`--php-version`-gated portability checks.
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

**15 - Unreachable-code / constant-condition rule catalog (DONE,
disclosed residuals only).** See Done for the full family. What's left,
each a real, verified reason rather than an oversight: match/switch
dead-arm/exhaustiveness checking and impossible-`instanceof`/redundant-
`is_string()`-style detection on a *non-constant* narrowed type are a
different, larger feature this family was never scoped to cover (it
only ever catches a condition whose inferred type is a constant
boolean); a nested `switch` as a case's own last statement isn't
recognized as exiting the enclosing function/loop (unlike a `try`,
which is, since it introduces no new `break`/`continue` scope of its
own) - a nested switch's own `continue N` levels would need reinterpreting
one level shallower first, real complexity rather than an oversight; a
genuinely mixed case-ending branch (one side break-equivalent, the
other diverging a different way) has the identical real complexity;
full `goto`/label control-flow modeling (narrowing a value correctly
across an arbitrary jump) remains out of scope.

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

**36 - PHPDoc tag ecosystem expansion (PARTIAL).** See Done for what
shipped. Still unbuilt: `@param-immediately-invoked-callable`/
`-later-invoked-callable`/`-closure-this` - a genuine architectural
mismatch (needs live per-call-site `Scope`-threading), not a scope trim.

**37 - Attribute target-validity, remaining kinds (PARTIAL, most kinds
done).** See Done for what shipped. `PropertyHookAttributesRule`
remains blocked on a real parser gap (a property hook's own attribute
list is never captured in the AST at all). `ConstantAttributesRule` is
NOT a parser gap - a top-level `const`'s own `#[Attr]` groups are
already captured on `ClassConst::attrs` via the same leading-attributes
path every other declaration uses - but no checker call site feeds
`StmtKind::GlobalConst`'s attrs into `check_attribute_targets` yet, and
no `ATTRIBUTE_TARGET_CONSTANT` flag (real PHP 8.5's
`Attribute::TARGET_CONSTANT`) exists in the target-flag list. `attribute.nonRepeatable` (needs a
same-declaration-list accumulator, a different shape of check than the
existing per-attribute loop) and `attribute.deprecated` (would
duplicate this project's own existing `@deprecated` family) are a
deliberate scope trim.

**39 - `@deprecated` usage warnings (PARTIAL).** Full access-shape
coverage is done (see Done). Whether a class-level `@deprecated` tag
should imply deprecation of its members is unclaimed - real PHPStan's
own answer lives in a not-vendored Composer package
(`phpstan/phpstan-deprecation-rules`), so it can't be confirmed here.

**40 - PHPStan-API-usage checks (PARTIAL).** Done for instantiation,
static calls, class-const fetches, `instanceof`, `extends`/`implements`/
trait-use, and a syntactically-typed slice of method calls (a directly-
typed parameter/closure-capture receiver). Full receiver-type inference
for method calls (e.g. through a constructor-promoted service property)
is a deliberate, disclosed scope trim.

**41 - Purity / side-effect model, transitive slice (DONE); remaining
(PARTIAL).** The transitive purity slice - deciding which functions/
methods/constructors are provably effect-free, and flagging a discarded
statement-level call to one - is built (`registry/purity.rs`, a
registry-build-time fixed point, not a Collector - see Phase 82). A
call is only ever resolved as a dependency when its target is knowable
without type inference (`$this->`/`self::`/`static::`/`parent::`/`new
self()`/`new static()`/a literal class name/a plain function call by
name); any other receiver shape disqualifies the whole declaration,
matching real PHPStan's own identical "unknown callee" restriction.
`@phpstan-impure` tag validation is built too, one-directional only
(flags a declaration tagged impure that this project's classifier can
*prove* pure; the opposite direction would conflate "genuinely impure"
with "unknown, conservatively disqualified" in this project's binary
model - a real false-positive risk). Unbuilt: the fuller ~20-kind
impure-point classification with `certain`-vs-`maybe` grading, and
`@phpstan-immutable` tag validation. One disclosed limitation inherited
from real PHPStan's own actual algorithm, not a bug of this project's
making: neither project counts constructor-promoted-property
initialization as an impure point, so a `parent::__construct(...)` call
chaining into a promoted-only ancestor constructor can look like a
discarded no-op even though it genuinely initializes state.

**42 - Checked-exception / throw-type model, remaining (PARTIAL).**
`@throws`-vs-`Throwable` validity, `throws.void`, dead-catch detection,
and `finally`'s own exit-point overwrite are done (see Done). Still
open: transitive `throws` union across the call graph,
`MethodThrowTypeCovarianceRule` (declined - an implicit "void when
untagged" default would false-positive heavily given how inconsistently
this corpus tags `@throws`), `TooWideFunctionThrowTypeRule`/
`MethodThrowTypeRule`, and the ~16-item per-callee dynamic-throw-type
extension cluster.

**43 - Property-hook checker validation (PARTIAL).** See Done for what
shipped. Only plain property declarations are covered - a promoted
constructor property's own inline hooks aren't, since nothing in the
registry resolves `set_ty`/`is_virtual` for that shape (a real,
disclosed residual; property hooks on a promoted property is a rare
combination even within an already-rare PHP 8.4 feature).

**45 - Declared-vs-actual-narrowest-type inference (PLANNED).** No
mechanism compares a declared type against the narrowest type a body
could actually produce. Splits into a return/property variant (unions
every actual return/assignment against the declared type) and a rarer,
harder `@param-out` variant (this project has no `@param-out` tag
concept at all yet).

**46 - PCRE pattern-validity checking (PLANNED).** No malformed-regex
detection (`regex_groups.rs` only extracts structure, never validates).
Needs a real `pcre2` binding - this project's first C dependency - since
Rust's own `regex` crate is a different-enough flavor to give wrong
answers on real PCRE patterns.

**48 - Symbol-existence pipeline, remaining stretch goals (PARTIAL).**
The existence-check MVP, typed-class-constant validation, multi-
namespace-per-file positional tracking, and a `use`-imported build-
tool-mangled-prefix check (`class.prefixed`) are all done (see Done).
`ClassCaseSensitivityCheck`'s own class-name-case half was investigated
but deliberately not built: whether this project's own name resolution
is already exact-case-only, making the check pure noise, or has a
case-insensitive fallback somewhere this reading didn't find, is a
genuine, disclosed uncertainty. `ExistingClassesInPropertyHookTypehintsRule`'s
existence-checking half is done (Phase 43 unblocked it); its other
sub-checks (a template type not referenced in a parameter, an
unresolvable native type, a native union type pre-PHP-8.0) turned out
to be generic `FunctionDefinitionCheck` sub-pieces this project has
never built for *any* function/method, not property-hook-specific - the
native-union-pre-8.0 case is additionally blocked by a real parser gap
(native and PHPDoc typehints collapse into the same `Type`
representation, with no way to tell which was written natively). A full
`RuleLevelHelper`-equivalent pipeline (~27 usage-location kinds) remains
a size-XL stretch goal on top of the now-done MVP.

**49 - Call-argument binding, remaining (PARTIAL).** Named-argument
legality, spread/unpack boundary-aware arity checking, and by-ref
argument legality are done (see Done). Full constant-array-spread
expansion (type-checking individual unpacked elements against their own
target parameter) is a deliberate scope trim.

**50 - `Type::Array` key-type / offset engine, remaining (PARTIAL,
almost everything done).** See Done for what shipped.
`ArrayShape`'s own int-vs-string key precision remains unbuilt,
genuinely separate from what shipped so far.

**51 - `Type` coercion surface (DONE).** Cast legality, `IntegerRangeType`
arithmetic across every operator including range-preserving shifts,
`print`/`InvalidPartOfEncapsedStringRule`, and the stdlib
`IntegerRangeType`-consumer extensions are all done. Nothing left
unbuilt in this phase's own scope.

**52 - Invalid-operand-combination (`ErrorType`), remaining (DONE).**
The operand-validity matrix (binary/comparison/unary/inc-dec) and
division/modulo-by-literal-zero detection are done. A `Union`'s own
legality staying "at least one member is legal" rather than real
PHPStan's stricter "every member" remains a deliberate leniency choice
for this project's coarser unions.

**53 - Structural Callable/Closure type, remaining (PARTIAL, contextual
injection generalized).** Contextual closure-body checking is done for
`array_map`/`array_filter`/`usort`/`uasort`/`array_walk`/
`preg_replace_callback` (see Done). `Type::Callable` itself is still a
bare marker with no parameter list/return type/purity/template map of
its own - a `Closure(...)`/`callable(...)` PHPDoc shape's own
per-parameter signature degrades to it rather than being modeled
precisely.

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
built-in).

**55 - Narrowing-engine generalization, remaining (DONE).** See Done.
`is_a()`/`class_exists()`-family narrowing were evaluated and declined
(near-zero corpus incidence, or - for `class_exists()` specifically - an
unobservable no-op given how this project already represents a bare
`class-string`).

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

**60 - Isset/empty/nullsafe reversal, remaining (DONE, one disclosed
residual).** See Done. One narrow residual, left disclosed rather than
chased further: a function bundling both a legacy and modern
native-typed variant behind a runtime `PHP_VERSION_ID` branch (not just
`function_exists()`) can still share one registry slot with the wrong
nullability assumption (4 corpus occurrences) - investigated in depth,
by all available static reasoning it should already resolve correctly,
but couldn't be conclusively confirmed either way without an isolated
corpus run.

**61 - Return-statement completeness, remaining (PARTIAL).** The
`return.never`/`.void`/`.missing` split is done (see Done). The
genuinely-empty-concrete-method-body case was built and corpus-verified,
then reverted (see **Reverted** - a real, ubiquitous IDE-stub convention
this project's broader verification corpus doesn't mirror the way real
PHPStan's own stub-loading does, not a soundness bug; a candidate to
ship opt-in). Generator `TReturn` completeness remains unmodeled.

**62 - Uninitialized-property dataflow (DONE, both halves).** The
"assigned anywhere in the class body" heuristic and real per-constructor
premature-access dataflow (including transitively-called-method
absorption) are both built, off by default. `readonly`-specific
handling and `__unserialize`/`__clone`-as-constructor recognition
turned out, on direct verification against real PHPStan's own source,
to never have been this rule's own scope at all (readonly belongs to
the separate, already-built `ReadOnlyPropertyAssignRule`; real PHPStan
only treats `__unserialize`/`__clone` as constructors when a project's
own config opts in, which Piwigo's doesn't). One genuine, disclosed
limitation remains: a called method's own logic gated behind an
*unrelated* property's runtime value is invisible to this module, since
it tracks property initialization only, never arbitrary values -
closing this would mean re-implementing the main checker's narrowing
engine inside this deliberately standalone walker. Piwigo's own
`phpstan.neon` never enables this check at all (matching real PHPStan's
own off-by-default), so verification came from a full unscoped run
against the vendored `vendor/` tree with the flag force-enabled, not
the usual scoped diff.

**63 - Duplicate-declaration scans, remaining (PARTIAL).** The
within-body scan and same-file class-vs-alias collision check are done
(see Done). Duplicate top-level *function* declarations remain open:
real PHPStan's own version is a whole-project, cross-file scan, and
this project's registry already has an established legitimate
multi-declaration pattern (phpstorm-stubs' own real-overload
representation) a naive "second declaration = bug" scan would conflict
with.

**64 - Override-compatibility rewrite, remaining (PARTIAL).** Method-
and most of the property-override halves are done (see Done). Still
open: property hooks (`notReadable`/`notWritable`/
`abstractOverridingNonAbstractHook`); native-type property variance -
blocked on giving `PropertySig` a `ty`/`ty_native` split the way
`FunctionSig` already has; PHP 8.4 `final`/`abstract` property modifiers
(new parser work); interface-vs-trait-abstract-method ancestor-lookup
priority for signature-compatibility checks specifically.

**67 - `@internal` restricted-usage engine, remaining (PARTIAL).**
Calls, property/constant access, instantiation, `instanceof`, and
`catch` are all covered (see Done). Still open: `extends`/`implements`/
param-and-return-type-hints/first-class-callables for the same engine,
and the separate, user-configured restricted-usage mechanism
(`RestrictedClassConstantUsageRule` and five siblings - unrelated to
`@internal`, lets a project declare its own arbitrary restricted-symbol
list).

**68 - Array no-op diagnostics, remaining (PARTIAL).** Duplicate-key
detection and the two no-op `array_values`/`array_filter`-empty checks
are done (see Done). `arrayFilter.same`/`arrayFilter.alwaysEmpty` were
built and dropped after corpus verification caught two live false
positives traced to a variable-type-tracking gap unrelated to this
phase itself.

**69 - printf/sprintf/scanf family, remaining (PARTIAL).** Positional
specifiers and `fprintf`/`vsprintf`/`vprintf` are done (see Done).
Width/precision `*` sub-placeholders, a strict-vs-castable matching
mode, and `scanf`-style by-ref-output-variable counting for
`sscanf`/`fscanf` remain unbuilt - zero corpus incidence for the last
one.

**70 - Generator/yield type modeling, remaining (PARTIAL).**
Declared-return-type-compatibility checking is done (see Done). No real
`Generator<K, V, TSend, TReturn>` shape exists - `yield`/`yield from`
still always infer `Mixed`. Low real-world `yield` incidence in this
project's own Piwigo corpus doesn't yet justify the full architecture.

**71 - Small builtin checks batch, remaining (PARTIAL).**
Function-name case-insensitivity, `define()`'s removed third argument,
`random_int()` ordering, and useless-return-value builtins are done
(see Done). Deliberately deferred: `call_user_func(_array)`'s
argument-reordering-and-recheck (needs callable-string/array-to-
signature resolution this project doesn't have); `filter_var()`'s
`FILTER_NULL_ON_FAILURE`/`FILTER_THROW_ON_FAILURE` flag-conflict;
`preg_quote()` delimiter cross-checking; `function_exists()`-guard
suppression for the undefined-function check - all zero or
near-zero corpus incidence.

**73/74 - Array/string stdlib coverage, remaining (DONE, one disclosed
upstream-stub residual).** See Done for the full list shipped this
round. PHP 8.4's `array_find`/`array_find_key`/`array_any`/`array_all`
turned out to already work with no code change - the vendored stub
already declares them with real generic docblocks. One genuine residual:
`array_udiff`/`array_uintersect_assoc`'s own variadic-tail argument
typing is imprecise in the vendored stub itself (types the whole PHP8+
variadic tail as bare `Mixed`) - a third-party stub's own declared
imprecision, not a bug in this project's checker, and confirmed zero
corpus incidence.

**75 - String accessory case-tracking (PLANNED).** No `Lowercase`/
`Uppercase`/`DecimalIntegerString` accessory kinds; `strtolower()`/
`strtoupper()` and friends don't produce case-tracked types. Literal-
string is never inferred from concatenation, undermining the feature's
main real-world use (SQL-injection-safety checking). Size: M.

**76 - `filter_var`/`filter_input`, remaining (PARTIAL).** The literal-
`FILTER_VALIDATE_*`-constant case and a literal `['options' => [...]]`
array's `default`/`min_range`/`max_range` sub-keys are done (see Done).
`filter_input()` itself, `FILTER_REQUIRE_ARRAY`/`FILTER_FORCE_ARRAY`'s
per-element return type, and PHP 8.4's `FILTER_THROW_ON_FAILURE` remain
unbuilt - all zero corpus incidence.

**77 - Runtime-capability modeling, remaining (PARTIAL).** Most of the
originally-scoped functions are done (see Done). Reflection-class
modeling in the registry and `SimpleXMLElement` magic-property
resolution remain unbuilt - both need genuinely new infrastructure this
project doesn't have, with no corpus evidence of heavy usage to justify
building it speculatively.

**78 - DateTime/DOM/SimpleXML, remaining (PARTIAL).** `DateInterval`'s
own duration-string grammar is done (see Done). `DOMDocument`/
`SimpleXMLElement` XML-content validation and `DateTime`'s relative-
date-string parsing both need real PHP-runtime/XML-parser simulation
this project has no way to do - low-priority, possibly a permanent
boundary rather than concretely scheduled.

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
- **Genuinely-empty-concrete-method-body check** (Phase 61). Verified
  individually correct and NOT a soundness bug - the corpus regression
  (+10,623, nearly all in `jetbrains/phpstorm-stubs`) was entirely one
  real, ubiquitous PHP convention (an IDE-stub/native-extension class
  declaring a native function's signature with a deliberately empty
  body) that real PHPStan itself never encounters, because it loads
  stub files through a separate, non-rule-checked mechanism this
  project's own broader verification corpus doesn't mirror. A strong
  candidate to ship opt-in (`--check-...`) rather than stay fully
  discarded, per [[feedback_dont_discard_correct_checks_ship_opt_in]];
  not yet revisited.
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
  mirror-image feature, and is scheduled).
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
- PHP-runtime/XML-shell-out techniques (`DateIntervalConstructorThrowTypeExtension`,
  `SimpleXMLElementConstructorThrowTypeExtension`) - PHPStan validates
  by literally constructing the real object at analysis time, since it
  runs inside PHP itself; phpstan-rs embeds no PHP runtime. The
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
