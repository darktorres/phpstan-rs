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

---

# Current status

- Piwigo frozen-corpus diagnostic count (`phpstan-rs.toml`-scoped,
  `/home/torres/piwigo17-frozen`) - the primary false-positive-hunt
  metric - has fallen from 2312 (start of the Piwigo-corpus hunts) to
  **93** (current; was independently verified clean under actual PHPStan
  at level 10 at 90 - the 3 new diagnostics are Phase 15's own newly-
  shipped `while.alwaysTrue`, individually verified correct against real
  PHPStan's own actual documented behavior for the shape found
  (`vendor/phpstan-src` inspection, not a live PHPStan re-run against
  this exact revision - re-verify against real PHPStan before trusting
  this number as "still independently clean" the same way 90 was).
- Full Piwigo+`vendor/` corpus (~20,300 files, unscoped): last measured
  42,840 diagnostics. Re-run before citing - it drifts as vendored
  dependencies change and isn't re-measured every round.
- Test suite: 2091+ unit tests, all passing.

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
(`positive-int`, `int<min,max>`) and a real cast/coercion method surface
(`toInteger`/`toFloat`/`toString` legality). `Type::Array` carries a real
key type, not just a value type. `object{...}` shapes, `value-of<T>`/
`key-of<T>` (registry-free cases only), and conditional PHPDoc return
types (`@return ($x is T ? A : B)`, widened to the union of both
branches) are all parsed and resolved instead of mangling into bogus
class names. Enum cases have their own singleton `Type::EnumCase`
identity, with `::from()`/`::tryFrom()` narrowing to the exact matching
case on a literal argument. GMP/`BcMath\Number` operator-overload
dispatch. PHPDoc-inheritance backfill: a method correctly omitting its
own `@param`/`@return` in favor of an ancestor's/interface's tag
resolves through that tag instead of looking untyped, including through
anonymous classes and promoted-constructor-property inline `@var` tags,
with real explicit-vs-implicit `mixed` provenance surviving the
inheritance hop (mirrors real PHPStan's `MixedType::isExplicitMixed()`).

## Narrowing engine

`instanceof`, equality/identity, `isset`/`??`/`??=`, common predicate
functions (`is_*`, `str_contains`/`str_starts_with`/`str_ends_with`,
`array_is_list`, `method_exists`-adjacent), PHPUnit assertion methods
(`assertIsString`/`assertNotNull`/etc.), De Morgan-aware compound
`&&`/`||` chains (including cross-leaf, not just single-condition),
through parenthesized/cast wrapper expressions, and through a
closure's own `use (&$x)` by-ref write-back. `break`-reached loop state
merges into the post-loop scope; a `finally` block's own unconditional
exit point correctly overwrites the try/catch side's, matching a
well-known PHP gotcha.

Phase 55: fixed-point loop-body convergence (`Checker::
converge_loop_body`, mirroring real PHPStan's own bounded
`LOOP_SCOPE_ITERATIONS`). `While`/`DoWhile`/`For`/`Foreach` bodies are
checked against a candidate entry state, then re-checked with that entry
unioned against the previous pass's exit whenever the two differ,
capped at 3 total passes; every pass but the one finally kept has its
diagnostics discarded. Fixes a variable reassigned inside a loop body
being stuck at its pre-loop type for the whole check - the real root
cause behind Phase 15's reverted `UnreachableStatementRule`/constant-
condition family (see Reverted). `Foreach`'s key/value binding is
re-applied fresh at the start of every pass rather than carried over -
required for a by-reference loop variable (`foreach (... as &$v)`)
specifically, confirmed by a real corpus false positive
(`QNumericRangeScope::parse`) where carrying a previous pass's
body-mutated `$val` forward wrongly widened it, when real PHP rebinds
`&$val` to a *different* array slot each iteration that pass never
touched. Also fixed as a direct prerequisite, found via this same
verification: a fully-qualified `\assert(\is_string($x))` wasn't
recognized as a narrowing statement at all (only bare `assert(...)` was)
- confirmed by amphp/http-client's `FollowRedirects::removeDotSegments`;
`always_terminates`'s own `Block` arm (and `check_function`'s identical,
separate copy for a function's top-level body) only ever looked at a
block's *last* statement to decide whether it always terminates, so a
block ending in dead code after a real terminator wasn't recognized as
terminating at all - now `.any()` over every statement, not just
`.last()`. A `continue`-reached state (`loop_continue_states`, the
`continue` counterpart of the pre-existing `loop_break_states`) now
feeds into the *next* convergence pass alongside the body's own ordinary
fall-through exit - without it, a variable reassigned *only* on a path
ending in `continue` (a real, common shape: `if ($x === null) { ...; $x
= compute(); continue; }`) never looked reassignable at all, silently
"converging" after a single pass against the stale entry - reintroducing
the exact false-positive class Phase 15 was reverted over in the first
place (real corpus finds: sebastian/diff's `Parser::parse`, Piwigo's own
`tools/openapi-generate-input-dto.php`).

## Rule catalog (checker)

Symbol existence (classes/functions/use-imports/typehints, catch
clauses, `extends`/`implements`/`instanceof`/trait-use targets) -
`ExistingNamesInGroupUseRule`'s case-sensitivity half and the full
`RuleLevelHelper`-equivalent pipeline remain open, see Pending.
Call-argument binding (named args, spread/unpack, by-ref legality).
`MissingTypehintCheck`'s full level-6 battery (implicit mixed, bare
iterable/array, non-generic use of a generic class), including the
iterable-value explicit-vs-implicit-mixed distinction. Property/method/
constant/class-constant visibility enforcement end to end (read, write,
call, static access, late-static-binding-unsafe `static::` access,
PHP 8.4 asymmetric write-visibility), plus the whole first-class-callable
family running the same checks an ordinary call gets. Unused-symbol
detection (constructor params, closure captures, private
constants/methods/properties, with dynamic-access escape hatches).
Duplicate-declaration scans within a class/trait/enum body, plus a
same-file case-insensitive collision between a class-like declaration
and a `use`-imported alias (`UsedNamesRule`) - duplicate top-level
*function* declarations remain open, see Pending.
Override-compatibility rewrite: visibility narrowing, missing/extra/
optional/by-ref/variadic parameter mismatches, `#[Override]`'s
unconditional-fatal-error half, all running uniformly for a class's own
methods and a trait-supplied one - property hooks and native-type
property variance remain open, see Pending. Nine declaration-site
modifier-consistency checks (abstract-in-non-abstract-class,
final-private, `$this` reassignment, reserved constant name, etc.) plus
the class-constant `final`/native-type-modifier trio. Enum sanity
(`EnumSanityRule`'s full remaining checklist) and generic-bound checking
wired to enums. `@internal` restricted-usage engine (function/method/
static-method calls, property/constant reads and writes, instantiation,
`instanceof`, `catch`, implicit `__toString()` via cast/echo/print/
interpolation) - off by default (`--check-internal-usage`); `extends`/
`implements`/param-and-return-type-hints/first-class-callables for this
same engine remain open, see Pending. `@deprecated` usage warnings
across the identical access-shape set, off by default
(`--check-internal-usage`'s sibling, `--check-deprecated-usage`).
Attribute existence-checking (`attribute.notFound`) plus target-validity
(`attribute.notAttribute`/`attribute.target`) for property/method/
function/param declarations, including the promoted-property dual
`parameter or property` target - class-level/enum-case/class-constant/
closure attributes still only get existence-checking, see Pending.
PHPStan-API-usage checks (a third-party project's own unmarked-`@api`
references into real PHPStan's `PHPStan\*` namespace). Checked-exception
groundwork: `@throws`-vs-`Throwable` validity, an explicit `@throws void`
contradicted by a real `throw`, dead-`catch`-clause detection, and a
`finally` block's own exit-point-overwrite detection - the full
transitive throw-type model remains open, see Pending. Uninitialized-
property detection (`property.uninitialized`, off by default via
`--check-uninitialized-properties` given its own heuristic's real
false-positive history). Return-statement completeness
(`return.never`/`return.void`/`return.missing`, both explicit and
fall-through). `isset`/`??`/`?->` existence-probe reversal for a bare
variable/property/method receiver (the offset/property-chain half is
gated behind unbuilt dataflow, see Pending). Generator return-type
compatibility (`YieldInGeneratorRule`'s declared-vs-actual check; the
full `Generator<K,V,TSend,TReturn>` shape remains open, see Pending).
Format-string checking for `sprintf`/`printf`/`fprintf`/`vsprintf`,
including positional (`%N$`) specifiers. Regex capture-group inference
(literal enumeration, tagged unions for one optional group/one top-level
alternation, numeric/non-empty-string fallback typing, trailing-vs-
interior optional-group precision). A real Piwigo-corpus-driven batch
of small builtin checks: function-name case-insensitivity,
`define()`'s PHP-8-removed third argument, `random_int()` literal-bound
ordering, useless-return-value builtins (`var_export`/`print_r`/
`highlight_string`), array-literal duplicate-key detection,
`array_values`/`array_filter` no-op conditions, `array_multisort()`'s
sort-flag-constants-vs-by-ref stub fix. Dozens of individual stdlib
array/string dynamic-return-type extensions (`array_fill_keys`,
`array_search`, `array_intersect_key`, `array_flip`, array-pointer
accessors, the `array_diff`/`array_intersect` family, `preg_split`,
`array_column`, `glob`, `explode`, `tempnam`, `version_compare`,
`filter_var`'s `FILTER_VALIDATE_*` family, `curl_getinfo`'s ~73-entry
table, `date()`/`idate()`/`pathinfo()`/`parse_url()`/`sscanf()`/`stat()`
family, `get_called_class`/`get_parent_class`/`gettype`/`get_debug_type`)
- see Pending for what's still bespoke-per-function and unbuilt.
`DateInterval::__construct()`'s own duration-string grammar validation.
`UnreachableStatementRule` (`deadCode.unreachable`, level 4) - reported
once per maximal unreachable run, at its first statement, without
skipping analysis of the rest (real PHPStan still fully analyzes code
after an unreachable point for every other rule, and this project's own
corpus verification confirmed why: stopping early silently dropped real,
unrelated diagnostics that happened to sit in a since-dead branch).

Phase 15's own constant-boolean-condition family, reinstated in full with
real *inferred*-type trust (`Checker::inferred_constant_condition`,
`checker/control_flow.rs`) - not just the bare-syntactic-literal slice
this phase first shipped with. Rebuilt deliberately end-to-end after the
user explicitly rejected a narrower follow-up plan ("why are you keeping
things out of scope? i dont mind churn, i want MOST proper"), which led
to re-investigating - not assuming - the original revert's own root
causes: loop staleness (already fixed, Phase 55's convergence work) and
`widen_coalesce_result`'s own `?? false` collapse, confirmed this session
to be a small, self-contained, independently-fixable bug (`checker/
expr.rs`'s general-case fallback discarded the left side's own type
entirely instead of unioning it in - `_ => r_ty` fixed to `_ => union_
types(l_ty, r_ty)`), not part of the large, genuinely-separate comparison-
outcome/range-API item the ROADMAP's own prose used to conflate it with.

Full family now built: `if`/`elseif` (`if.always*`/`elseif.always*`,
`checker/stmt.rs`'s new `Checker::check_if`, extracted from the old
inline `StmtKind::If` body) - `elseif` gets its own real identifier and
real PHPStan's own "last condition, no trailing else" exemption
(`reportAlwaysTrueInLastCondition: false`, confirmed in `vendor/
phpstan-src/conf/config.neon`; `elseif` already desugars to a nested
`StmtKind::If` in `parser/stmt.rs::parse_if`, confirmed directly, not
assumed); `while`/`do-while` (unchanged identifiers/exclusions from the
original narrower version, now inferred-type-trusting); the ternary
operator (`ternary.always*`); `&&`/`||` (`booleanAnd`/`booleanOr.
leftAlways*`/`rightAlways*`/`always*`, `Checker::check_boolean_and_or`) -
three diagnostics per node (left unconditional, right/whole gated by
`is_first_level` - real PHPStan's own `!isInFirstLevelStatement()`, which
covers *both* a bare `&&`/`||` used directly as an if/while/ternary
condition, via the new `Checker::in_condition_position` field, *and* one
used as its own bare statement, via the pre-existing `in_bare_statement_
position`, confirmed directly against `ExpressionHandler::processStmt`'s
own `ExpressionContext::createTopLevel()`); `xor` (`logicalXor.
leftAlways*`/`rightAlways*`, `Checker::check_logical_xor` - left/right
only, no whole-result, no first-level gating, confirmed `LogicalXor
ConstantConditionRule.php` has neither); and a standalone `!$expr`
check anywhere it appears (`booleanNot.always*`, inverted relative to the
inner value, with its own copy of the last-condition exemption via the
new `Checker::in_exempt_last_condition` field). `should_skip_constant_
condition` (free function) mirrors real PHPStan's own `ConstantCondition
RuleHelper::shouldSkip` + `FunctionCallConstantConditionHelper::
isTypeCheckCandidate` combined, excluding every shape that has its own
dedicated rule (the six above) plus every equality/identity/ordering
comparison (genuinely nothing to catch - `infer_binary`'s own comparison
arm unconditionally returns bare `Type::Bool`, confirmed directly, no
comparison-outcome type inference exists yet - see Pending) and any bare
function/method/static call (deferred to real PHPStan's not-yet-built
Collector-based `FunctionCallConstantConditionRule`, Pending's own Phase
82). `infer_binary`'s `BinOp::And`/`Or`/`LogicalXor` arms gained real
constant folding (mirroring `BooleanAndHandler`/`BooleanOrHandler`/
`BinaryOpHandler::resolveType` exactly, confirmed against `vendor/
phpstan-src`) - without it, `check_boolean_and_or`'s own whole-result
check had nothing but a permanent bare `Type::Bool` to ever match
against.

Four real, independently-confirmed bugs found and fixed while building
and corpus-verifying this (each with its own regression test in
`checker/tests/control_flow.rs`, not just disclosed): (1) `converge_loop_
body`'s own returned `body_locals` only ever reflected a loop body's
fallthrough exit, silently dropping the same pass's own `continue`-
reached states - a variable only ever assigned on a non-`continue` path
looked unconditionally assigned afterward instead of "maybe, maybe not"
(`Piwigo\Db\BatchWriter::updateRow`); (2) `While`/`For`'s own condition
check ran exactly once against the pre-loop entry, entirely bypassing
Phase 55's fixed-point convergence - moved inside `converge_loop_body`'s
own closure so it runs against each pass's converging state, exactly
matching real PHPStan's own `WhileHandler::processStmt` (`CoreUpdate
Service::upgradeTo`, `QMultiToken::parseExpression`); (3) that same move
broke a variable *the condition itself* assigns (`while (($x = f()) !==
null)`) surviving the "loop may run zero times" merge afterward, fixed
with a quiet, diagnostics-discarded priming evaluation against the real
pre-loop `locals` first, mirroring real PHPStan's own separate quiet
`$condResult`/`$condScope` pass (`Symfony\Component\DependencyInjection\
Compiler\AbstractRecursivePass::getConstructor`); (4) `check_boolean_and_
or`'s left-side check was evaluated against the caller's already-left-
narrowed scope, making a bare-variable left operand look constantly true
by pure circularity (`Piwigo\Controller\Admin\ConfigurationSubController
::processSizes`) - fixed by narrowing internally, only for the right/
whole checks, never the left one; (5) `is_first_level` only ever
consulted `in_condition_position` (an if/while/ternary's own direct
condition), missing real PHPStan's *other* `isInFirstLevelStatement()`
case - a bare `&&`/`||` used directly as its own statement
(`ExpressionContext::createTopLevel()`, confirmed against `ExpressionHandler
::processStmt`) - fixed by also consulting the pre-existing `in_bare_
statement_position` flag (`Piwigo\Url\UrlService::makeSectionInUrl`'s
`isset($x) or trigger_error(...);`, whose own stub return type is exactly
`true`).

Two further real bugs found and fixed while individually reviewing every
line of the initial corpus diff (not just disclosing them as residual
imprecision): (6) `infer_binary`'s `Add`/`Sub` arms collapsed straight to
bare `Int` for any `int<min,max>`-typed operand, discarding the range
entirely - `strpos()`'s own `$offset` parameter (typed `int<0, max>`)
lost its refinement the moment it was reassigned via `$offset = $pos +
1;` inside a loop, a false positive only visible once fix (2) above made
`strpos()`'s own later, real re-check of `$offset` reachable at all
(`Tools\p58\assign.php`). Fixed with real, narrow `int<min,max>`
arithmetic (confirmed directly against `vendor/phpstan-src`'s own
`InitializerExprTypeResolver::integerRangeMath`) - deliberately not the
full range-arithmetic surface (`Mul`/`Div`'s own negative/positive-
interval splitting isn't replicated, and `Sub` only handles a constant
right operand), see Pending. (7) A much larger, previously-unsuspected
gap: `check_expr`'s and `infer_expr`'s own `PropertyAccess` arms matched
`Object`/`EnumCase`/`ObjectShape` directly with no `Union`/`Intersection`
arm at all, silently collapsing to `Type::Mixed` whenever a property's
own receiver type was a `Union`/`Intersection` - a real, common shape
this project's *own* `narrow_instanceof_true` deliberately produces for
an "unrelated sibling" `instanceof` check (see that function's own doc
comment), and just as common as an ordinary `A|B`-typed parameter.
Factored the existing `Object`/`EnumCase`/`ObjectShape` logic (previously
duplicated near-verbatim between the two arms) into one shared
`property_access_type` method that also recurses through `Union`/
`Intersection` members - the identical "try each member, keep the first
real match" idea `MethodCall`'s own receiver-class extraction already
uses a few lines above it. Real corpus false positive this fixes:
`PhpParser\Node\Stmt\ClassMethod::$stmts` (a real, docblock-typed
`Node\Stmt[]|null` property) resolving to bare `Mixed` the moment its
receiver was narrowed via `instanceof` from the `PhpParser\Node`
interface - confirmed via direct debugging (not guessed) that PhpParser's
own AST routes every concrete node type through an interface this way,
not a common base *class*, so `narrow_instanceof_true`'s non-hierarchy-
aware `is_subtype` check (deliberate - see its own doc comment) can never
recognize the relationship and always falls to building a real
`Intersection`. This fix has real reach beyond the one corpus line that
surfaced it: any property access on a `narrow_instanceof_true`-produced
intersection, or on an ordinarily union-typed value, was silently
returning `Mixed` project-wide - confirmed via the full vendor-included
corpus diff, which picked up a batch of genuine, previously-silently-
discarded `property.notFound`/`method.notFound` findings once their own
receiver's real type was finally computed instead of `Mixed` (`member_
confirmed_missing`, the pre-existing diagnostic-deciding logic, already
had correct `Union`/`Intersection` semantics - it just never had a real
type to work with before).

After fix (7) shipped, a deliberate sweep for the identical anti-pattern
elsewhere in the checker (every `let Type::Object(...) = ...` match with
no `Union`/`Intersection` case) found and fixed five more real instances
of the same class of bug, each with its own regression test: (8)
`ExprKind::PropertyAccess`'s own visibility/`@internal`/`@deprecated`
checks (`checker/expr.rs`) had the identical gap *right next to* the
type-resolution one fix (7) already covered, silently skipping all three
for a `Union`/`Intersection` receiver - fixed with a new `find_property_
owner`, deliberately searching every member for the one that actually
declares the property (not just the first found, the shortcut `MethodCall`'s
own pre-existing `recv_class` extraction settles for - a bare-visibility
check on the *wrong* intersection member would silently pass); (9)
`checker::assignment::check_property_assignment` had the same gap on the
*write* side, skipping readonly/visibility/`@internal`/`@deprecated`
*and* the assigned value's own type check entirely for a `Union`/
`Intersection` target - fixed with `find_property_receiver`, `find_
property_owner`'s richer sibling that also carries the matching member's
own type-args for template substitution; (10) `checker::calls::check_no_
discard_statement`'s own `#[\NoDiscard]` method-call check had the
identical gap - fixed with a new `find_method_owner`; (11) the bare,
unconditional `@phpstan-assert` lookup for an instance `MethodCall`
(`checker/stmt.rs`) had it too, silently skipping a real narrowing
opportunity for a `Union`/`Intersection` receiver - fixed by reusing the
same `find_method_owner`; (12) both `StmtKind::Throw` and `ExprKind::
Throw`'s own "must be Throwable" check required the thrown value's type
to be literally `Type::Object` too - fixed with a new, shared `thrown_
value_not_throwable` (a `Union` fails if *any* member isn't Throwable, an
`Intersection` only if *every* member isn't, mirroring `member_confirmed_
missing`'s own identical asymmetry). Fixing (12) surfaced a *second*,
independent, genuinely pre-existing bug via a real corpus multi-catch
`catch (A|B $e) { throw $e; }` (real PHP binds a union type there) that
the Union fix newly made reachable: `is_subclass_of` silently treats a
class name the registry has no entry for as "not a subclass of
Throwable," indistinguishable from a real, known, non-Throwable one -
`thrown_value_not_throwable` now checks `class_name_exists` first,
mirroring `StmtKind::Try`'s own multi-catch handling, which already
guards this exact case (confirmed directly: the class in question,
`RectorPrefix202608\Symfony\...\UnrecoverableExceptionInterface`, is a
PHP-Scoper-prefixed name this project's vendor scan never actually
indexes a declaration for, even though the real, unprefixed interface
genuinely does extend `Throwable`). This one guard alone removed eleven
more pre-existing false positives across the full vendor corpus, none of
them Union/Intersection-shaped at all - a real, independent bug this
session's own Union/Intersection sweep happened to surface, not
something the sweep was looking for.

Checking whether that first sweep (grep for `let Type::Object(...) = ...
else`) had actually been exhaustive found two more real instances of the
identical anti-pattern, missed because they're plain `match` arms instead
of `let-else` - (13) `AssertTarget::ThisProp`'s own "not already
narrowed" fallback (`assert_target_key_and_current`, `checker/
narrowing.rs`) matched the receiver's own type against `Type::Object`
directly, silently discarding the property's real declared type for a
`Union`/`Intersection` receiver - reachable even through the already-
correct `ThisSelf`/branch-conditional call site, since this fallback
re-infers the receiver's own type independently; fixed by reusing
`property_access_type`. (14) `ExprKind::FirstClassCallable`'s own
`MethodCall` arm (resolving a first-class callable used as, e.g.,
`array_map`'s own callback) had the identical gap - fixed with a new
`find_method_receiver_class`, `find_method_owner`'s sibling that returns
the concrete receiver class actually queried (not the declaring owner) -
late static binding needs the real matched member for `self`/`static`
pseudo-return resolution, the same distinction the pre-existing single-
`Object` code already relied on. Given a second miss on the very first
attempt at "exhaustive," this project's own confidence that *this*
sweep is now complete is deliberately not claimed either - only that
every instance found by every search attempted so far is fixed.

A third, broader search (every `Type::Object(...) =>` match across
`src/checker/*.rs`, not just the two narrower `let-else` patterns before
it) found the single highest-impact instance yet: (15) `infer_expr`'s own
`ExprKind::MethodCall` arm - a *separate*, parallel implementation from
`check_expr`'s own `MethodCall` handling (which already extracted a
representative class from `Union`/`Intersection` for its own diagnostic
purposes) - had the identical `Object`/`EnumCase`-only match, silently
collapsing *any* method call's own return type to `Type::Mixed` for a
`Union`/`Intersection` receiver. Since `infer_expr` is the core type-
*inference* path (exercised whenever a method call's result type needs
computing without also running full diagnostics), this was reachable far
more often than any other fix in this family. Extracted into a new
`method_call_return_type`, which - like `property_access_type` - also
picked up a real refinement while being rewritten: `Intersection` keeps
"first member whose declaration resolves wins" (the value genuinely is
every member simultaneously), while `Union` now unions every resolving
member's own return type together instead of just using whichever came
first (`A|B` where `A::get(): int` and `B::get(): string` is really
`int|string`) - the same, more-correct distinction now also applied
retroactively to `property_access_type` itself.

Reviewing *that* refinement's own corpus diff surfaced a fourth, more
subtle bug in the "first member wins" `Intersection` logic itself (16):
`narrow_instanceof_true` always builds `Intersection([original_type,
target_type])` - the broader, pre-existing type first, the just-
`instanceof`-checked, narrower type last. Every `find_map`-based
Intersection lookup this session (`find_property_owner`, `find_property_
receiver`, `find_method_owner`, `find_method_receiver_class`,
`property_access_type`, `method_call_return_type`) searched members in
that same forward order - so a method/property name declared on *both*
the broader original type and the narrower target resolved to the
broader type's own (possibly less-specific, covariant-overridden)
declaration first. Real corpus false positive this produced: `Rector\
Php54\...\RemoveZeroBreakContinueRector`'s own `$staticType instanceof
ConstantIntegerType`, where a shared `PHPStan\Type\Type`-family method
the narrower `ConstantIntegerType` itself overrides more precisely was
found on the broader interface first, producing a bogus argument-type
mismatch - confirmed directly via `assertType()` debug tracing, not
guessed. Fixed by searching every one of those six helpers' own
`Intersection` arm in reverse - provably never wrong (a class's own
`lookup_*_with_owner` already walks its full ancestor chain, so checking
the narrower type first can only ever find an equally or more specific
answer, falling through to the broader type exactly as before when the
narrower one genuinely doesn't have it).

The same broader search also found two much narrower, lower-severity
instances - real, but false-negative-only (a missed diagnostic, never a
wrong one) and low real-world likelihood: (17) `operators.rs`'s
`type_is_instance_of`/`is_valid_overload_operand` (GMP/BcMath operator-
overload legality) and `is_object_or_null`/`is_array_or_null` (nullsafe/
comparison validity) each handled `Union` but not `Intersection` - fixed
with the correct, sometimes-opposite quantifier per function (`any`
member proves an instanceof/overload-legal fact true, matching `Union`'s
own "any" shape for a different reason; `any` member being array-shaped
proves `is_array_or_null`, unlike `Union`'s own "all" requirement for
definiteness) - `is_array_or_null`'s own gap meant a real, common shape
(`list<T>`/`non-empty-array<T>`, both `Intersection([Array(_),
Accessory(_))])`, Phase 14) was never recognized as array-shaped for
comparison-validity purposes at all. (18) `coercion.rs`'s `type_to_
integer`/`type_to_float`/`type_to_string` handled `Union` (via the
existing `union_coercion` helper) but not `Intersection`, falling to the
permissive "always legal" default - fixed by reusing `union_coercion` for
`Intersection` too (the identical "if any member offers a legal cast
path, don't flag" reasoning already applied to `Union`).

Checking whether fix (16)'s own reverse-order correction should have
reached further found one more real instance (19): `check_expr`'s own
*separate* `MethodCall` arm (distinct from `infer_expr`'s, fixed by (15))
had an analogous but differently-shaped gap in its pre-existing (not
newly-written) `recv_class` extraction - a plain "first Object/EnumCase
member, regardless of whether it actually declares the method" pick,
correctly kept as the gate for the separate `member_confirmed_missing`-
based existence check, but *also* reused to resolve the method's real
signature/owner for argument-type checking, visibility/`@internal`/
`@deprecated`, the Doctrine query special case, and the final return
type - all of which need a member that actually declares the method, in
the correct (`Intersection` reversed) order. Fixed with a new
`find_method_receiver` (`find_property_receiver`'s own method
counterpart), confirmed via direct testing (not assumed) to have been a
real, live bug: a same-named public method on a broader interface
silently let a call through to a narrower, *private* override that
should have been rejected.

Confirmed against the real Piwigo corpus (both the app-only scope and
the full vendor-included one) after every fix above: zero remaining
false positives traced to this family's own new checks - the only
surviving diff lines are `widen_coalesce_result`'s own genuine, expected
precision ripples (reviewed individually), real, correct findings newly
unlocked by fixes (7)-(19) above, and real, correct findings unrelated to
any of this session's own changes (including in vendor/example code,
where a docblock-declared literal param type like `@param false $x`
makes `if ($x)` genuinely always-false, confirmed against `vendor/
jetbrains/phpstorm-stubs`). Fix (16)'s own reverse-order correction alone
removed a further net nine false positives across the full vendor
corpus once applied on top of (15) - most of (15)'s own newly-surfaced
findings turned out to be this same ordering bug, not genuine new
precision. Fixes (13)/(14)/(17)/(18) had no corpus-verifiable effect
either way (confirmed via a full before/after diff) - all are confirmed
correct only by their own dedicated regression tests.

Fix (19)'s own full-corpus diff was later reviewed exhaustively, line by
line (it had initially only been spot-checked - see above). Of its ~57
changed lines, the overwhelming majority (every `argument.type`/
`return.type` finding whose "got" type was the broad five-member union
`int|float|string|bool|null`, plus several differently-shaped ones
sampled individually - Doctrine ORM's own cache layer, PHPUnit, Rector's
bundled Symfony console/messenger copies, Symfony Config/Console/
Messenger/Mailer/Serializer, thecodingmachine/safe) traced to one single,
deeper, pre-existing root bug this review uncovered while chasing the
`int|float|string|bool|null` cluster to its source: `Registry::
flatten_phpdoc_inheritance_one_pass` (Phase 87) backfilled an ancestor's
own docblock-only return/param type onto a child override **verbatim**,
never reconciling it against the child's *own* real native type hint the
way an *own* (non-inherited) docblock already does via `FunctionSig::
resolve_type`/`reconcile_with_native`. Concretely: `PHPStan\Type\
Constant\ConstantStringType::getValue(): string` (a concrete override)
sits under `ConstantScalarType::getValue()`'s own `@return int|float|
string|bool|null` doc tag - the ancestor's far broader union was cloned
onto the child wholesale, discarding its own, already-correct, narrower
native `string` for something far less precise (real PHPStan's own
`TypehintHelper::decideType` never does this: an inherited doc type only
wins when the native hint is itself a supertype of it). Fixed (20) by
running the exact same `reconcile_with_native` an own docblock already
goes through on the inherited one too, in both `flatten_phpdoc_
inheritance_one_pass`'s param and return backfills (`registry.rs`) -
this alone accounted for the entire `int|float|string|bool|null` cluster
(confirmed via a mini repro isolating the exact shape, and via the real
corpus diff, both before/after). This is a real, independent, more
severe bug than any single fix in the (6)-(19) family: unlike a wrong-
member Intersection/Union arm (which only ever *loses* precision to
`Mixed`), this one *actively supplied a wrong, misleadingly-specific*
type wherever it fired, both losing real diagnostics on the affected
override's own parameters (a native `string $v` silently accepting real
`int`/`float`/`bool`/`null` call-site arguments, confirmed live) and
fabricating false ones on its return type.

Reviewing fix (20) itself immediately surfaced a second, narrower gap
(21) in `reconcile_with_native`'s own `member_possible_given_native`
helper: its deliberate `Type::Object => true`/`Type::Callable => true`
leniency (justified for reconciling an *own* docblock, where this
project has no class hierarchy yet to disprove a claimed subclass) was
never scoped away from a native `void` return hint, which admits no
value at all, not even a class or callable. Real corpus false positive:
`PhpParser\NodeVisitor::enterNode()`'s own `@return null|int|Node|
Node[]` doc backfilling straight through a native `void` override two
ancestors down, wrongly demanding a real `Node` return from a function
whose own signature is `void` (22 real corpus false positives this
one fix alone removed, including the entire `PhpBench\...::configure():
OptionsResolver` family). Fixing this the obvious way (native `void`
excludes every doc member outright) broke a real, correct case in turn:
`Type::Never` needed its own carve-out, since an explicit `@return
never` genuinely means "always diverges" and is honestly compatible with
*any* native return hint including `void` (real PHPStan's own
`TypehintHelper::decideType` special-cases exactly this, checked before
its general rule) - confirmed via `Nette\Utils\ObjectHelpers::
strictStaticCall(): void`'s own `@return never`, whose loss wrongly
flagged every `__callStatic` ending with a call to it as "should never
return, but a return statement was reached." Both (20) and (21) have
their own dedicated regression tests, each explicitly reverted and
confirmed to fail without their fix.

With (20) and (21) applied, the review of fix (19)'s remaining, non-
`int|float|string|bool|null` lines completed - first as a pattern-sample
across categories (disclosed as such when asked "did you leave anything
deferred or partial?"), then, on request, individually: every single one
of the ~20 remaining distinct call sites (Doctrine ORM's cache layer -
an interface member missing on a `Region::get(): CacheEntry|null` result
where only the concrete `EntityCacheEntry` declares it, and a `Persistence
\Mapping\ClassMetadataFactory::getMetadataFor()`/`CollectionCacheEntry::
$identifiers`'s own broader-than-used docblock shape; PHPUnit's
`addToAssertionCount(non-negative-int)`/`Util\Filter`'s sealed array-
shape mismatch; Rector's bundled Symfony console copies and the real
`symfony/console` alongside them - an `arguments.count` mismatch calling
a 2-arg concrete override through its 1-arg interface type,
`ConsoleSectionOutput::clear()`'s `int|null` against a `float`-tainted
`$lineCount`, `OutputInterface::warning()` genuinely undeclared on the
interface; Symfony Config's `ArrayShapeGenerator`'s own `NodeInterface`-
vs-`BaseNode` argument narrowing and `FileLoader::doImport()`'s
`?string`-into-`string`/interface-segregation gap; Console's
`Interaction::interact()` against `Closure::getClosureThis(): ?object`;
DependencyInjection's `ValidateEnvPlaceholdersPass::process()` against a
`string[][]` docblock less precise than its own declared use; Mailer's
`SentMessage::__construct()` against `Headers::get(): ?HeaderInterface`;
PropertyInfo's `PhpDocTypeHelper::getType()` against `AggregatedType::
get(): ?Type`; Serializer's `TraceableSerializer`'s own two-argument
calls through the one-argument `EncoderInterface`/`DecoderInterface`;
thecodingmachine/safe's `DateTimeImmutable::createFromInterface()`
against its own three-way `instanceof` fallthrough) is confirmed, each
by reading its real source directly, to be a genuine, correct finding
fix (19) correctly unmasked, not a new bug of its own.

Two real, pre-existing bugs did surface this way, both fixed immediately
on request rather than left disclosed. (22): `Symfony\Component\
Messenger\MessageBus::dispatch()`'s
`while ($x instanceof IteratorAggregate) { $x = $x->getIterator(); }`
loop-exit type of `$x` stayed bare `Traversable` (missing `rewind()`/
`valid()`/`current()`) instead of narrowing to `Iterator`. Real PHP's
`Traversable` is a marker interface userland code can never implement
directly - only through one of its exactly two direct subinterfaces,
`Iterator` and `IteratorAggregate` - so excluding one structurally
proves the other; real PHPStan special-cases this exact relationship.
This project's existing negated-`instanceof` narrowing (`narrow_type_
excluding`, already reached via `ExprKind::Instanceof`'s false branch
and via `While`'s own loop-exit `apply_narrowing` call - there was no
missing dispatch mechanism after all, just a missing case in the
function both already fed into) turned out to be exactly the right,
narrow place to add this: a new arm matching `Type::Object("Traversable",
_)` excluding `Object("IteratorAggregate", _)`/`Object("Iterator", _)`
by exact name (deliberately *not* subclass-aware - the structural
guarantee only holds for these two specific interfaces, not an arbitrary
subtype of either), resolving to the other by name.

(23): Monolog's own `MongoDBHandler::write()`'s `isset($this->manager,
$this->namespace)` left `$this->namespace` (a real `string|null`
property) un-narrowed inside the guard - real PHP's `isset()` is
genuinely variadic (`isset($a, $b)` means `isset($a) && isset($b)`,
every argument must be set), but the narrowing dispatch's own single-
argument `[single_arg]` slice pattern never matched a multi-argument
call at all, silently narrowing *none* of its arguments, not just the
ones past the first. Fixed with a new, dedicated arm for `isset` with
more than one argument, narrowing each independently the same way the
existing single-argument case already does (only the true branch, same
asymmetry).

Both (22) and (23) have their own dedicated regression test reproducing
the real corpus shape, each explicitly reverted and confirmed to fail
without its fix, and each confirmed via a full corpus diff removing
exactly its own targeted false positive(s) - 3 for (22), 1 for (23) -
with zero other ripples. This closes the fix (19) diff review started
above: every line in it is now individually accounted for as either a
genuine finding or a fixed, tested bug.

Asked to re-examine this family's own remaining Pending entry (below) to
see whether any of its "blocked on a bigger feature" justifications
still held up rather than trusting the prior write-up, one turned out to
be understated rather than accurate: (24) `while.alwaysTrue`/`doWhile.
alwaysTrue`'s own breakless-detection was described as reusing `stmt_
contains_break`'s "any break anywhere" conservatism instead of real
PHPStan's more precise per-loop-level `continue N` analysis - true, but
not the whole gap. Reading `WhileLoopAlwaysTrueConditionRule`'s real
source directly (`BreaklessWhileLoopNode::getExitPoints()`) showed its
actual suppression logic treats *any* exit point that isn't a `break` or
a bare/level-1 `continue` as disqualifying - a `return`/`throw`
reachable inside the loop body disqualifies it too, which this project
checked for nowhere at all (`DoWhileLoopConstantConditionRule`'s
identical logic confirms `alwaysFalse` is correctly *un*touched by this -
only `break` suppresses that case). This made the diagnostic
substantially more broken than the "narrow, disclosed simplification"
framing suggested: every one of the 53 `while.alwaysTrue`/`doWhile.
alwaysTrue` findings in the full corpus turned out to have a genuine
internal return/throw/escaping-continue - the common, idiomatic "`while
(true)` as a retry-until-done loop" shape - meaning the diagnostic was
misfiring on essentially every real-world use of the pattern, only ever
correctly catching a loop with no internal exit at all. Fixed with a new
`stmt_contains_escaping_exit` (`control_flow.rs`), the same maximally-
conservative "anywhere in the subtree" traversal `stmt_contains_break`
already uses, wired into both `While`'s `skip_always_true` and
`DoWhile`'s `alwaysTrue` arm only. Verified via five dedicated
regression tests (escaping `continue`, same-level `continue` still
flagging, `return` inside for both loop kinds), each explicitly reverted
and confirmed to fail without the fix, and via a full corpus diff
removing exactly these 53 false positives with zero other ripples.

Continuing the same re-examination into this family's `goto`/label
disclosure (below) found one more concrete, live false positive rather
than only a theoretical gap (25): real, non-trivial `goto` usage exists
in the corpus (hand-written state-machine parsers - `Amp\Http\Client\
Connection\Internal\Http1Parser::parse()`'s own HTTP/1.1 response
parser, 14 `goto`s across its states), and `deadCode.unreachable`'s own
`check_block` loop had no way to know a labeled statement immediately
following an always-terminating one (`return $response; body_identity:
{ ... }`) is a real jump target (`goto body_identity;`, elsewhere in the
same function), not dead code. Fixed with a new `collect_goto_targets`
(`control_flow.rs`) - the same maximally-conservative "anywhere in the
subtree, not scope-aware" traversal `stmt_contains_break` already uses,
since recovering *which* `goto` can legally reach *which* label would
mean building the full goto/label CFG this project still doesn't have
(genuinely out of scope, per the disclosure above) - computed once per
function/closure/method body into a new `Checker::current_function_goto_
targets` field (the same save/set/restore shape `current_function_is_
generator` already has at every one of its three entry points) and
consulted by `check_block`'s own report site. Verified via two dedicated
regression tests (a targeted label survives, an untargeted one still
flags), the fix explicitly reverted and confirmed to fail without it,
and via a full corpus diff removing exactly this one false positive with
zero other ripples.

Asked directly to close the family's remaining "narrow, disclosed
simplification, no corpus evidence needs it" items anyway - "do all
those narrow [items], our corpus not having them is not a reason not to
do them" - three more landed together (26)-(28), none corpus-motivated,
all verified the same way regardless.

(26): a `for` loop's non-last comma-separated condition expressions
(`for ($stop = false; !$stop, $i < $n; $i++)` - only the *last*
expression's truthiness gates the loop, but real PHP re-evaluates
*every* one of them on each pass) were checked exactly once, up front
against the pre-loop entry, instead of every pass - the same Phase 15
continuation bug the *last* condition's own fix already addressed,
just left unfixed for the others at the time. Moved into the same
per-pass convergence closure. Verified via a constructed regression
test (mirroring the existing `QMultiToken`-style one immediately above
it in `control_flow.rs`, moved into a non-last slot instead), explicitly
reverted (confirmed to reproduce a real `booleanNot.alwaysTrue` false
positive) and restored.

(27): real PHP's own well-known `continue`-inside-`switch` gotcha (a
`switch` counts as one level for `break`/`continue` purposes, so a bare
`continue;`/`continue 1;` written directly in a case is exactly
`break`-equivalent) was recognized nowhere at all. Turned out to be two
distinct, real bugs sharing this one root cause, both found by
constructing and directly verifying real test cases rather than trusting
the fix in isolation: `always_terminates`'s own `Switch` arm treated
*any* `Continue` the same as `Return`/`Throw`, wrongly marking code
after a switch as dead once every case ends in either `return` or a
break-equivalent `continue`; separately, `reaches_after_switch`'s own
first, unconditional arm only special-cased a literal `Break` this way,
leaving a *non-last* case (or one whose break-equivalent `continue` is
reached through a nested `if`, not directly) still gated behind
`is_last`. New shared `is_switch_break_equivalent` (`control_flow.rs`),
recursing through `Block`/`If` the same way `always_terminates`'s own
recursion already does - a case ending in `if ($c) { continue; } else {
continue; }` is break-equivalent on every branch too, found only after
constructing a test for exactly that shape and watching it still fail.
A genuinely *mixed* branch (one side break-equivalent, the other
diverging a different way entirely) remains a disclosed residual -
`is_switch_break_equivalent`'s own doc comment has the full reasoning
for why threading that distinction through `always_terminates`'s much
more broadly-used recursion isn't attempted here. Three dedicated
regression tests (a non-last case, the last/only case, and the nested-if
shape), each explicitly reverted and confirmed to fail without its
piece of the fix.

(28): `infer_binary`'s `int<min,max>` arithmetic, previously `Add`/`Sub`
(the latter only for a constant right operand) only, gained the rest of
real PHPStan's own range-arithmetic surface, each transcribed directly
from `vendor/phpstan-src`'s own `InitializerExprTypeResolver`: `Mul`
(`mul_int_range` - the real source's own four-corner-product formula,
substituting signed infinity for an unbounded end, with a `0`-corner
special case to avoid computing `0 * INF`); `Div` (`div_int_range` -
real division isn't guaranteed to itself produce an int, so the result
is always a refined-int-range-or-`Float` union, never a pure int range;
the recursive sign-splitting when either operand's own range spans zero
is transcribed as real recursion, each half already returning a
complete, tail-processed `Type` unioned directly); `Mod` (`mod_int_range` -
a literal-pair exact fold first, matching real PHPStan's own two-step
structure, since the general magnitude-based range fallback alone would
answer `7 % 3` with the far coarser `int<0, 2>` instead of the exact `1`);
`Pow` (`pow_int_range` - a literal base's own degenerate `min == max`
bounds happen to make the same single formula real PHPStan splits across
two separate call sites work correctly for both a literal and a genuine
range base). `Sub` gained the full range-minus-range case
(`sub_int_range_full`, real PHPStan's own `Minus` arm - including its
own sequential `min`/`max` mutation, preserved exactly rather than
"cleaned up," since correctness depends on matching the real source's
execution order, not just its inputs and outputs in isolation).
`normalize_int_range` gained real PHPStan's own `IntegerRangeType::
fromInterval`'s "`min > max` collapses to `Never`" guard, needed once
`Div`'s own rounding step (ceiling the min, flooring the max
independently) can produce exactly that shape for a division with no
exact int result at all (`10 / 4` is always exactly `2.5`, never any
int - its own int "half" collapses away, correctly leaving bare
`float`). Every function has its own dedicated regression tests
(literal-pair folding, bounded-range computation, and each function's
own particular edge - the zero-corner special case, the inverted-range-
collapses-to-float case, the second-block-overwrites-`min` sub-case),
each explicitly reverted and confirmed to fail without it. Confirmed via
a full corpus diff: one further real false positive removed (`Nette\
Utils\Paginator::getOffset()`'s own `@return int<0, max>` now correctly
resolves through a real `getPageIndex() * itemsPerPage` multiplication,
both operands provably non-negative), plus several message-precision
refinements on already-firing diagnostics, zero new false positives,
Piwigo's own app-only scope completely unchanged.

Phase 43: PHP 8.4 property-hook checker validation, ported directly from
`vendor/phpstan-src/src/Rules/Properties/*.php` and `Generics/
PropertyVarianceRule.php` - real, function-like checking for hook bodies
that were previously only parsed, never analyzed at all. Picked as the
first of the roadmap's own "phases that unblock others" (see
`PHASE_DEPENDENCIES.md`, built the same session): its own prerequisite
(Phase 57's `readonly`/`private(set)` metadata on `PropertySig`) had
already shipped, and closing it unblocks Phase 48's `ExistingClassesIn
PropertyHookTypehintsRule`.

The "virtual property" algorithm (does a hooked property have real
backing storage at all) has no vendored source to transcribe - it's
computed by the Zend engine/BetterReflection, neither present in this
project's vendored copies. Verified empirically instead, the same "confirm
against real, live `php`, not recalled knowledge" methodology this whole
document uses elsewhere: real PHP 8.5's own `ReflectionProperty::
isVirtual()` run against ~15 constructed shapes. The confirmed rule - a
hooked property is virtual iff no hook body contains, anywhere in its own
function scope (recursing through ordinary statements/expressions,
including an unreachable branch like `if (false) {...}` - not
reachability-based, purely syntactic), a `$this->propName` property-fetch
for that exact property (a read, a bare unused statement, `isset()`, or
`unset()` all count identically; a *nested* closure/arrow-fn/anonymous-
class body does not, since it's a separate function scope; neither does a
dynamic `$this->$name` access or an indirect copy like `$self = $this;
$self->propName`) - is computed by a new `hooks_reference_backing_storage`/
`hook_body_references_property` pair (`registry/class_info.rs`), a plain
generic tree walk reusing the existing `expr_children`/`stmt_children`
helpers (`ast_walk.rs`) exactly as they already stand - their own,
pre-existing "stop at a nested closure/arrow-fn/anonymous-class boundary"
behavior turned out to already match real PHP's own scope boundary here,
confirmed by the same empirical probe rather than assumed. New `PropertySig::
is_virtual: bool` field, gating `propertyGetHook.noRead`/`propertySetHook.
noAssign` (`checker/oop.rs::check_property_hooks`) the same way real
PHPStan's own `GetNonVirtualPropertyHookReadRule`/`SetNonVirtualProperty
HookAssignRule` do - a disclosed simplification of the *assign* half:
real PHPStan runs actual dataflow there (does every execution path end
with the property initialized), which this project has no primitive for;
a syntactic "does the body reference it at all" is coarser but zero-false-
positive on the common case.

`SetPropertyHookParameterRule`'s native-type-contravariance half
(`propertySetHook.nativeParameterType`) is ported; its docblock-merged
sibling (`propertySetHook.parameterType`) deliberately isn't, mirroring
this project's own established default for override compatibility
generally - real PHPStan's own `checkPhpDocMethodSignatures` flag defaults
off, confirmed neither Piwigo's own `phpstan.neon` nor this project's
corpus config enables it (`FunctionSig::return_type_native`'s own doc
comment already documents the identical reasoning for method overrides).

Each hook body is now checked as a real function-like scope - modeled
directly on the existing `ExprKind::Closure`/`ExprKind::ArrowFn` handling
in `checker/expr.rs` (fresh `locals`, explicit `$this` binding,
generator-flag/goto-targets save-restore), deliberately *not* on
`check_function`'s registry-`FunctionSig`-lookup path: that path runs a
lot of method-specific machinery (attribute-target validation for
*methods*, `#[Override]`/`#[NoDiscard]`, first-class-callable
eligibility) that would be actively wrong for a hook, which - unlike a
real method - is never itself callable as `$obj->get()`. This closes the
actual core gap ROADMAP's own prior entry named ("no function-like
registry entry exists to check hook bodies against"): an undefined
variable inside a hook body, a wrong return type, a missing return, or a
generator-return-type mismatch are all caught now, the same way they
would be inside any ordinary method body - confirmed live via `php -l`
that real PHP itself applies the identical Generator-return-type
compile check to a hook body that contains `yield`, validating that
reusing `check_generator_return_type` here (declared return = the
property's own type) is the right call, not an assumption.

One real, live bug found and fixed while building this, caught by a
constructed mini-repro rather than assumed safe: `check_get_hook`/
`check_set_hook` initially never saved/set/restored `Checker::
current_class` the way `check_function` already does - an entirely
legal same-class private property access from inside a hook body
(`$this->celsius` read from within `$this->fahrenheit`'s own `get`/`set`
hooks, both declared on the same class) was wrongly flagged
`property.private`, since the visibility check reads `current_class` to
allow it. Has its own dedicated regression test, explicitly reverted and
confirmed to fail without the fix.

`PropertyVarianceRule` reuses the existing `walk_variance` primitive
directly (`checker/generics.rs`) rather than going through `check_variance`'s
own param/return-specific loop - a property has one type, not a pair -
with real PHPStan's own `hasRestrictedWriteAccess` mapped onto this
project's existing `Position` enum: `Position::Out` (covariant-safe) for
`readonly`, private-set (`write_visibility == Private`, only reachable
for a property whose *read* visibility isn't itself private - already
filtered, so this can only mean a genuine asymmetric `private(set)`), or
a get-only hooked property (hooks present, none of them `set`);
`Position::Inv` otherwise. One real, disclosed wording imprecision: this
reuses `walk_variance`'s own hardcoded, method-parameter-flavored
violation strings verbatim ("cannot be used in an input (parameter)
position") rather than real PHPStan's property-specific phrasing - fires
on exactly the right condition, same identifier/level, just a text
nuance judged not worth splitting the shared strings by call site for how
rarely a property is both generic-template-typed and hits this specific
violation. `check_variance`'s own doc comment used to say properties
weren't checked "because this project doesn't track `readonly` at all
yet" - stale since Phase 57 shipped `PropertySig::is_readonly`/
`write_visibility`; corrected as part of building this, rather than left
contradicted by the current code (the same kind of stale-prior-write-up
gap this whole document's Phase 15 re-examination found repeatedly).

Confirmed via a full corpus diff: Piwigo's own app-only scope is byte-
identical (zero PHP 8.4 property hooks or generic-template properties
anywhere in Piwigo's own code); the full vendor-included corpus gained
exactly 5 new diagnostics, all `generics.variance`, all individually
confirmed genuine by reading the real source (plain, read-write public
properties on a `@template-covariant`-declared class - Doctrine ORM's
`ClassMetadata::$reflClass`, `Doctrine\Persistence\ObjectManagerDecorator`,
Symfony Config's `NodeDefinition`, and phpstorm-stubs' `Ds\Pair::$key`/
`$value` - each a real, correct finding real PHPStan's own identical rule
would also produce against the same source) - zero property-hook-specific
findings anywhere in the ~20,300-file corpus, confirming zero real-world
PHP 8.4 property hook usage there at all, and zero false positives from
either family.

Phase 37 follow-up: attribute constructor-argument validation
(`attribute.abstract`/`.noConstructor`/`.constructorNotPublic`, plus
arity/named-argument/by-ref/type checking), the second phase picked from
`PHASE_DEPENDENCIES.md`'s own "phases that unblock others" this session.
Its own prior write-up called this "genuinely needs Phase 49's call-
argument-binding generalization" - re-checked rather than trusted (the
same standing practice this whole document's Phase 15 entry started):
`resolve_call_args`/`check_args` (`checker/calls.rs`) already take a bare
`args: &[Arg]` + `sig: &FunctionSig`, no expr-node coupling at all - the
exact same pair `ExprKind::New`'s own constructor-call handling already
uses, directly reusable for an attribute's own constructor call with zero
further generalization needed. `Checker::check_attributes` (the single
choke point every attribute-bearing declaration already routes through)
now also visits each argument expression via `check_expr` against a
fresh, empty `Locals::default()` - correct, not a shortcut, since a real
PHP attribute argument is a constant-expression and can never reference a
local variable at all - which newly catches things like an undefined
class-constant reference inside an attribute argument, previously
invisible to this checker entirely.

One real, live bug found via this session's own corpus diff, not assumed
safe: the constructor lookup initially used a bare `info.methods.get
("__construct")` (this class's own declared methods only) instead of the
ancestor-chain-aware `Registry::lookup_method` `ExprKind::New` already
uses - PHPBench's own `#[BeforeMethods]` (`class BeforeMethods extends
AbstractMethodsAttribute {}`, no `__construct` of its own, inheriting its
parent's) was wrongly flagged `attribute.noConstructor` on every real,
legal use in Piwigo's own test suite. Fixed, with its own dedicated
regression test, explicitly reverted and confirmed to fail without the
fix.

Confirmed via a full corpus diff: Piwigo's own app-only scope is byte-
identical once the ancestor-lookup fix above landed. The full vendor-
included corpus gained exactly 2 new diagnostics, both `argument.type`,
both the identical real (if very minor) finding: JetBrains' own
`#[Deprecated(since: 8.3)]` convention in two phpstorm-stubs files passes
a bare float literal where the attribute's own constructor docblock
explicitly declares `@param string $since` - a real type mismatch by this
project's own established docblock-is-the-checked-bar convention (the
parameter has no *native* type at all, so real PHP itself raises no
runtime error either way), not a new false-positive class - individually
correct, and far too narrow (2 vendor-only occurrences, functionally
inert attribute metadata) to be worth chasing further.

Phase 76 follow-up: `filter_var()`'s own literal `['options' => [...]]`
array narrowing, the third phase picked from `PHASE_DEPENDENCIES.md`'s
own "phases that unblock others" this session, and the first one where
the investigation itself (not just the fix) was the point - the user
asked to check what this actually needed before committing to it, since
the 51→76 edge looked speculative. Confirmed: `FilterFunctionReturnTypeHelper::
applyRangeOptions` (`vendor/phpstan-src`) only needs a plain "build an
int range from two literal/range bounds" primitive - exactly
`normalize_int_range`/`int_range_bounds` (`checker/expr.rs`), already
built this same session for Phase 15's own range-arithmetic work, and
entirely unrelated to Phase 51's own actually-remaining item (range-
preserving `**`/shift arithmetic). The 51→76 edge in `PHASE_DEPENDENCIES.md`
was speculative, not a real blocker - `PHASE_DEPENDENCIES.md` updated to
reflect this rather than left stale.

New `filter_var_options_fields`/`filter_var_option` (`checker/expr/
builtin_calls.rs`) extract a real, literal `['options' => [...]]` sub-
array's own `default`/`min_range`/`max_range` sub-keys - a non-constant
`options` value (e.g. `['options' => $dynamic]`) is left completely
unrecognized, matching real PHPStan's own `getOptions()` "not a constant
array" leniency rather than guessing. `min_range`/`max_range` only apply
to `FILTER_VALIDATE_INT` (real PHPStan's own `getFilterTypeOptions()` has
`FILTER_VALIDATE_FLOAT`'s own entry commented out - "PHPStan does not yet
support FloatRangeType" - and this project doesn't either); every other
`FILTER_VALIDATE_*` constant still gets its own base type from
`filter_validate_type_for_constant`, just unioned with an explicit
`default` option when present instead of the implicit `false`/`null`
fallback. Confirmed via a full corpus diff on both scopes: byte-identical
- no real corpus code (Piwigo's own or the full vendor tree) currently
passes an `options` array to `filter_var()` at all, matching this phase's
own long-standing "not corpus-evidenced" note; built anyway per the
user's own standing directive that corpus silence isn't a reason not to.

Phase 80 follow-up: `value-of<T>` naming a real, registered enum, the
fourth phase picked from `PHASE_DEPENDENCIES.md`'s own "phases that
unblock others" this session. Real PHPStan represents `value-of<T>`/
`key-of<T>` as `LateResolvableType`s that resolve themselves lazily,
wherever a type is actually consulted anywhere in the program - this
project has no general lazy-type mechanism, and building one would touch
every one of the ~19 files with an exhaustive `match` over `Type` for a
rarely-used feature, so a lower-risk design was chosen instead: resolve
**once**, in a new one-time `Registry::resolve_deferred_value_of` pass
that runs after every class/enum is indexed, the same architectural shape
`flatten_phpdoc_inheritance` already uses for the closely analogous
problem. Reading `KeyOfType::getResult()` directly confirmed a real scope
narrowing before any code was written: `key-of<T>` naming a bare class/
enum reference has no special case in real PHPStan at all (it just tries
`getIterableKeyType()`, `Mixed` for an ordinary class) - only
`value-of<T>` special-cases an enum, so that's the only real slice worth
building. A second simplification found by reading `registry/class_info.rs`
directly: a backed enum's own `properties["value"]` (already built for
its real `->value` property) is already the exact union-of-case-value
type `value-of<EnumName>` needs - reused directly, not recomputed.

`types::parse_type_keyword_or_class`'s `"value-of"` arm now produces a
sentinel (`Type::Object("$$valueof", [Type::Object(name, [])])`) for a
bare class/enum reference instead of falling to `Mixed` - the same
"encode a special case via an existing variant, guarded by an impossible
name" idiom `"resource"`/`"object"` already use (`$` can never appear in
a real PHP identifier). `resolve_deferred_value_of` resolves it once
every class/enum is indexed: not found, or not an enum, stays `Mixed`
(can't verify, don't guess); an unbacked enum (no real `->value`
property) is `Never`, matching `ValueOfType::getResult()`'s own
`count($valueTypes) === 0 -> NeverType` exactly; a backed enum reuses its
already-built `properties["value"]` type directly.

Two real, live bugs found via this session's own corpus diff, neither
caught by the unit test suite alone (both fixed, each with its own
dedicated regression test, explicitly reverted and confirmed to fail
without its fix):

(1) `registry/name_context.rs::resolve_names_in_type`'s generic `Object`
arm namespace-qualifies every name it doesn't special-case - inside a
real `namespace` block, the sentinel's own name (not yet its inner
argument) got mangled into `App\$$valueof` before the resolution pass
ever got a chance to recognize it, producing a bogus `class.notFound`
instead of the real resolved type. A namespace-less test fixture never
exercises this at all (`ctx.resolve` on a bare name with an empty
namespace is a no-op) - caught only once a namespaced regression test was
deliberately added. Fixed with a fourth exact-name guard alongside
`"object"`/`"Traversable"`/`"resource"`'s existing ones, which - unlike
those three - still recurses into (and resolves) the sentinel's own inner
argument, the real class/enum name it wraps.

(2) A real, live false positive in the full vendor corpus
(`shipmonk/dead-code-detector`'s own `CollectedUsage::deserialize()`,
`/** @var array{..., t: value-of<MemberType>, ...} $result */`) leaked
the raw, unresolved `$$valueof<...>` sentinel straight into a real
diagnostic message. `resolve_deferred_value_of` only runs once, at
registry-build time - an inline `@var` annotation (and an anonymous
class's own param/return docblock fallback) parses its own raw text
independently of that pipeline, at check time, when every class/enum is
already fully indexed anyway. New `Registry::resolve_deferred_value_of_now`
(the `&self`, immediate-resolution twin of the `&mut self` one-time pass)
wired into `Checker::resolve_type_hint` (used by the inline-`@var` path)
and `checker.rs`'s anonymous-class param/return fallback.

Fixing (2) surfaced a third, more subtle bug while writing its own
regression test: `php-standard-library`'s own `BackedEnumValueType::coerce():
string|int`, `/** @return value-of<T> */` for an unresolvable `@template
T`, started returning bare `Mixed` instead of the correct native
`string|int` fallback - a real regression, not just a missing
improvement, traced to `function_sig::member_possible_given_native`'s own
deliberate `Type::Object(_, _) => true` leniency (built for a genuine, if
unverifiable, class reference) blanket-trusting the still-unresolved
sentinel too early in the pipeline, which made `reconcile_with_native`
keep it instead of correctly falling back to the native type when nothing
else survived - a real, live regression this feature would otherwise have
introduced into existing, previously-correct behavior. Fixed with a
native-fallback-aware resolution path (`resolve_deferred_value_of_with_native_fallback`
and its `resolve_deferred_value_of_now_with_native_fallback` check-time
twin): once a sentinel resolves to bare `Mixed`, falls back to the
position's own already-tracked native-only type (`ParamSig::ty_native`/
`FunctionSig::return_type_native`) instead, restoring the exact behavior
`value-of<T>` support would otherwise have silently regressed. A
disclosed, narrower gap remains for `PropertySig`/`ConstSig`/
`ClassInfo::template_bounds`, none of which persist a native-type
counterpart at all (Phase 64's own pre-existing, disclosed limitation) -
an unresolvable `value-of<T>` there stays plain `Mixed` rather than
falling back to a native hint, with no current corpus evidence it matters.

Confirmed via a full corpus diff on both scopes, after all three fixes:
Piwigo's own app-only scope byte-identical; the full vendor-included
corpus also byte-identical against its own pre-this-phase baseline (the
two real false positives fixes (1)/(2)/(3) found were all introduced and
removed within this same phase, confirmed via the corpus diff at each
intermediate step before finally landing net-zero against the true
starting point) - no known real corpus usage of `value-of<EnumName>`
exists in either corpus at all, matching the user's own standing
directive that corpus silence isn't a reason not to build a real,
disclosed feature.

## Performance & tooling

CI-friendly output formats (`json`, `checkstyle`). Baseline suppression
(`--generate-baseline`/`--baseline`) and inline `@phpstan-ignore-line`/
`-next-line`/`<identifier list>` suppression, matching real PHPStan's
own trailing-vs-standalone placement rule exactly. `ignoreErrors:`
config support (`[[ignore_errors]]` TOML, identifier/path-glob/message-
regex/count fields). `--level`/`--php-version`-gated portability
checks. Incremental multi-file `Project` with cross-file dependency
invalidation; a real Content-Length-framed JSON-RPC LSP server, now with
hover/go-to-definition (Phase 17, both stages plus its own follow-up -
see Pending for what's still disclosed as out of scope). An 8MiB worker
stack (fixing a real
stack-overflow crash on
deeply-nested expressions), persistent structurally-shared `Locals`
(O(1) clone), `FxHashMap` for hot build-once maps, a criterion
regression-guard benchmark, and `hotpath`, an opt-in in-process
instrumentation profiler that catches effects system-wide `perf`
sampling is too coarse to see.

Phase 17 Stage A: a new `Project::decl_locations` index (name ->
declaring `(path, Span)`, gated behind `Project::set_retain_source` so a
batch `check-project` run pays nothing for it) plus a new `lsp::
navigation`/`lsp::text_split` pair - the former walks a file's AST for
the innermost reference at a cursor position, the latter recovers a real
per-name sub-span from an AST span whose own source text holds more than
one name (`Foo::CONST`, `?Foo|Bar`) by re-lexing just that span's text.
Needed a small, purely additive AST change first: every name reference
the parser previously flattened into a bare `String` with no span of its
own (`extends`/`implements`/`uses` lists, `InstanceofTarget::Name`,
`CatchClause.types`, every declaration's own name) now also carries a
real `Span`, populated by the parser at the exact point it already
parses each one - no existing field's value changed, so every existing
consumer (checker, registry, hundreds of tests) needed no change at all.

Phase 17 Stage B: an instance receiver's own type (`$x->method()`/
`$x->prop`, `$x` not `$this`/`self`/`static`/`parent`) resolves via a
new, additive `Checker::probe_span`/`probe_hit` pair, checked exactly
where `check_expr`'s own `MethodCall`/`PropertyAccess` arms already
compute the receiver's real type as part of ordinary checking - matched
by *exact span equality* against the one specific receiver `lsp::
navigation` already identified, which sidesteps any inner-vs-outer
ambiguity a chained call (`$x->foo()->bar()`) could otherwise raise,
entirely. The LSP side builds one throwaway `Checker` per request and
re-runs `check_function` over just the enclosing function/method (a
clone of it, carried on `navigation::Resolved::Instance` - no lifetime
entanglement with `Project`, at the cost of one clone per request) -
real, bounded per-request cost, no caching, matching any real language
server's own "hover" cost profile.

Phase 17 follow-up (closing out all three gaps the original two-stage
plan disclosed as out of scope):

- **Closure/arrow-fn body probing** turned out to already work with no
  code change at all - the original disclosure was overly conservative.
  `lsp::navigation`'s `current_function` was already left pointing at the
  *enclosing named* function/method through a `Closure`/`ArrowFn` (never
  reset - it isn't a separate `Checker` entry point), so `probe_receiver_
  type`'s re-run of `check_function` on that named function already
  reaches a receiver nested inside a closure/arrow-fn body the same way
  ordinary `check_expr` recursion does during a real `check-project` pass.
  Confirmed empirically (both a `use (...)`-captured closure variable and
  an arrow-fn's own implicit capture resolve correctly) before writing it
  up as fixed - only the stale doc comment and a real regression test
  needed to change.
- **A `Union`/`Intersection` receiver** now resolves every class that
  actually declares the member being navigated to, not just the first
  (`lsp.rs`'s new `type_to_classes`/`resolve_member_owners`, deduped by
  the member's real *declaring* owner via the existing `lookup_*_with_
  owner` hierarchy walk). Hover joins every match with `" | "`;
  definition returns `GotoDefinitionResponse::Array` when more than one
  real owner is found (LSP's own way to offer a picker) and stays a plain
  `Scalar` for the overwhelming majority-case single match, so every
  existing single-owner test kept its original assertions unchanged.
- **Local-variable-declaration navigation** (a read resolving to its own
  declaration) shipped twice: first as `lsp::navigation`'s own hand-rolled
  arm-merging heuristic (a `var_decls`/`conditional_depth` walk local to
  the LSP module), then - once asked "is this the *most* proper approach,
  or are you still avoiding churn?" - replaced entirely with real,
  checker-driven reaching-definitions, riding along the exact same
  flow-sensitive traversal `check-project` itself already trusts (branch
  merging via `merge_scopes`, loop fixed-point convergence via
  `converge_loop_body`, termination-pruning via `always_terminates`, real
  `break`/`continue` handling via `loop_break_states`/`loop_continue_
  states`) rather than a second, independent approximation of it that
  could silently drift from the real one. The old heuristic's own code is
  gone from `lsp::navigation` entirely.
  - `Locals` (`checker.rs`) changed from a bare `imbl::HashMap` type alias
    to a small wrapper struct: `types` (unchanged) plus an opt-in,
    parallel `provenance: imbl::HashMap<String, imbl::HashSet<Span>>`
    side-channel, gated by a `track: bool` field carried alongside every
    clone (so it turns on/off atomically with the scope it belongs to,
    including a closure's own fresh scope via the new `fresh_scope`
    constructor). `types`/`provenance` are two separate fields, not one
    `(Type, ProvenanceSet)` pair, specifically so the ~40 existing call
    sites that only ever touch `Type` (type-narrowing: `instanceof`/null/
    array-shape checks, a by-ref call's own type-widening effect) needed
    zero changes - plain `Locals::insert` leaves `provenance` for that key
    completely untouched, correct for a refinement that isn't a new
    declaration. Two new primitives cover the two ways provenance
    *does* need to change: `Locals::declare` (a fresh, single-span
    provenance, replacing whatever was there - used at every genuine
    syntactic binding site: an assignment's left-hand side, including
    each leaf of a list-destructuring target; a parameter; a `foreach`/
    `catch`/`static`/`global` binding; `$x++`/`$x--`/`$x += 1`) and
    `Locals::insert_with_provenance` (copies an *existing* provenance set
    across unchanged - used where a value moves between two `Locals`
    without being redeclared: `merge_scopes`'s own union, a closure's
    `use (...)` capture, `&&`/`||`'s own short-circuit new-definition
    propagation). `PartialEq` for `Locals` deliberately compares `types`
    only, never `provenance` - `converge_loop_body`'s own fixed-point
    check keeps converging purely on type stability, unchanged.
  - `merge_scopes` (`checker/control_flow.rs`) is the one function every
    multi-way join in the checker already routes through (`If`'s own
    then/else selection, `Switch`'s `exit_states` fold, `Try`'s `end_
    states` fold, and `converge_loop_body`'s own fixed-point loop, which
    is built entirely out of `merge_scopes` calls) - fixing provenance-
    merging here once (union both sides' known spans for a name present
    in both) is what makes every one of those inherit real reaching-
    definitions precision with **no other call site needing any change at
    all**, including this project's own already-correct termination-
    pruning and break/continue handling.
  - `Checker::probe_var_provenance: Option<HashSet<Span>>` mirrors
    `probe_hit` exactly (Stage B's own precedent): checked in `check_expr`'s
    `ExprKind::Variable` arm the same way `probe_hit` is checked in
    `MethodCall`/`PropertyAccess`. `lsp.rs`'s new `probe_variable_
    provenance` builds one throwaway `Checker` per request and reads it
    back, the direct sibling of `probe_receiver_type`.
  - `lsp::navigation::Resolved` gained `VariableSelf(Span)` (a declaration
    clicked directly - a parameter, a `foreach`/`catch`/`static`/`global`
    binding's own name - resolves to itself, no probe needed) and
    `VariableRead { span, function, is_static, class }` (a genuine read,
    resolved via the probe above, mirroring `Instance`'s own shape).
  - Two small additive AST spans made `VariableSelf` possible without a
    text-split hack: `Param.name_span` and `CatchClause.var_span`,
    populated by the parser at the same point each is already consumed.
  - `global $x;` deliberately does **not** try to link to a specific
    top-level assignment (the earlier heuristic's own order-independent
    pre-scan did, and was removed) - the real `Checker` itself already
    treats `global $x;` as binding `Mixed` with no attempt to track real
    PHP's own global scope (`checker/stmt.rs`'s own doc comment: a
    principled scoping decision, not an oversight), so a read now
    resolves to the `global $x;` statement itself, matching that same
    honest boundary instead of reintroducing a separate, less-principled
    LSP-only guess alongside a now-much-more-rigorous system.
  - Real, corpus-verified checker nuances a hand-rolled LSP-side
    heuristic could never have known about are now inherited for free:
    `foreach` deliberately does *not* merge with its own pre-loop state
    (Phase 29's own corpus-verified decision - a common idiom relies on
    the loop variable's post-loop type being trusted as if the loop body
    definitely ran), so a post-`foreach` read correctly stays a single
    candidate, unlike `while`/`for` (which do merge, so a post-loop read
    there correctly offers the pre-loop value as a live alternative). A
    guard clause (`if ($cond) { $x = 1; } else { $x = 2; return; }`)
    correctly excludes the terminated `else` branch's own `$x = 2` from
    ever becoming a candidate at all.
  - Zero corpus impact confirmed directly, not assumed: the full
    checker test suite (1449 `checker::` tests) passes unchanged, and a
    before/after diff of `check-project` on both the Piwigo-scoped and
    full (`vendor/`-included) corpus is byte-identical - `Locals::track`
    defaults to `false` everywhere except the one throwaway, per-request
    `Checker` an LSP variable-navigation probe builds.
- Two more follow-ups closed the last real gaps this design still had:
  - **Top-level script-scope support**: `lsp::navigation::Resolved::
    Instance`/`VariableRead` gained a `ProbeScope` (`Function(Box<
    FunctionDecl>)` or `TopLevel(Vec<Stmt>)`, replacing the plain
    `function: Box<FunctionDecl>` field both used to carry) - a receiver/
    read at top-level script scope (no enclosing named function at all)
    now re-checks the whole file's own top-level statements via `Checker::
    check_program` instead of `check_function`, the real entry point
    `check-project` itself already uses there. `check_program`'s own
    `top_locals` needed the identical one-line `set_track` wiring `check_
    function` already had.
  - **Auto-vivification**: a by-ref call argument, an array index-write
    (`$x['key'] = 1;`), or `preg_match`'s own `$matches` out-parameter is
    real PHP's own well-known "this variable is silently created here if
    it didn't already exist" behavior (already documented and relied on
    elsewhere in this checker, e.g. `bind_assignment_target`'s own
    existing comment on array auto-vivification) - when the target name
    is genuinely undefined beforehand, these three call-effected sites
    now `declare` (real, useful provenance: the call/write site itself,
    since there's nothing truer to point to) instead of leaving
    provenance untouched (which would mean *no* provenance, since there
    was never anything there to leave). When the name *already* has a
    real declaration, they still leave it alone exactly as before - a
    real corpus example construct (`f($items)` widening an already-`$items
    = []`-declared array by reference) keeps resolving to its own actual
    declaration, not the incidental call site, confirmed by a dedicated
    regression test.
  - Same verification bar as the first round: full checker test suite and
    a before/after `check-project` diff on both corpora, byte-identical.

---

# Pending / partial

Each entry: what's missing, and the root cause blocking it where one's
known. Numbered by the original phase for cross-reference; the number
is not a priority order. See `PHASE_DEPENDENCIES.md` for a graph of
which of these phases actually block each other (most don't).

**15 - Unreachable-code / constant-condition rule catalog (PARTIAL).**
`UnreachableStatementRule` (`deadCode.unreachable`) and the full,
inferred-type-trusting constant-boolean-condition family (`if`/`elseif`/
`while`/`do-while`/ternary/`&&`/`||`/`xor`/`!`, see Done) are both done.
What real PHPStan's own family still has that this doesn't, each for a
real, freshly-verified technical reason (not proportionality):
`NumberComparisonOperatorsConstantConditionRule` (`</>/<=/>=` whose own
inferred type happens to be a constant boolean) - confirmed directly that
`infer_binary`'s own comparison arm unconditionally returns bare
`Type::Bool`; genuinely nothing to catch until this project builds real
comparison-outcome type inference (real PHPStan's own `Type::isSmallerThan`/
`isSmallerThanOrEqual` equivalent) - a different half of "the range API"
than the one `Add`/`Sub` now have (see Done): those two arms gained real,
narrow `int<min,max>` *arithmetic* this session, but nothing yet computes
a *comparison*'s own boolean result from two ranges, which is what this
rule actually needs. Still Phase 55's own separate, larger item. `FunctionCallConstantConditionRule`'s own cross-
file aggregation (`is_string($x)` constant at every call site, reported
once as "this check is dead everywhere") needs Collector-style whole-
codebase aggregation - Pending's own Phase 82. `assert(false)`/
`assert(0)` as a sub-expression is exactly this same call-shaped-
condition case, not a separate gap. No match/switch dead-arm/
exhaustiveness checking; no impossible-`instanceof`/redundant-
`is_string()`-etc. detection on a *non-constant* narrowed type (this
phase's own family only ever catches a condition whose inferred type is
a `ConstantBooleanType`, never a broader "this instanceof check can never
succeed given the value's declared type" analysis - a different, larger
feature, not owned by any phase). Full `goto`/label control-flow edges
(e.g., narrowing a value correctly across an arbitrary jump) remain
unmodeled and out of scope, but the one concrete false positive this
produced (`deadCode.unreachable` flagging a labeled statement a real
`goto` elsewhere in the same function jumps straight to) is fixed - see
Done, fix (25). `exits_function_or_loop`'s own recursion through a
`Try`/nested `Switch` inside a switch case is unmodeled the same way
`always_terminates`'s own doesn't recurse into those either - a real,
disclosed residual, narrower than the shapes already fixed (see Done,
fixes (26)-(28)). A genuinely *mixed* case-ending branch (one side
break-equivalent, the other diverging a different way entirely - a
`: never`-returning call, `return`, `throw`) is a real, disclosed
residual gap too: `exits_function_or_loop`'s own recursion correctly
judges that shape, but `always_terminates`'s own separate, much more
broadly-used recursion has no way to thread "this specific `Continue` is
switch-scoped" through an arbitrary nesting depth without risking wrongly
loosening its own unrelated, function-level "does this exit for real"
question elsewhere (guard-clause detection, `deadCode.unreachable`'s own
non-switch callers) - not attempted, narrower and more corner-case than
what's fixed. `for`'s own non-last comma-separated condition
expressions, `continue N`'s own level argument for switch-narrowing
purposes, and `infer_binary`'s full `Mul`/`Div`/`Mod`/`Pow`/`Sub` range-
arithmetic surface (all three previously disclosed here as "narrow, no
corpus evidence needs it") are now Done - see fixes (26)-(28): closed on
request ("do all those narrow [items], our corpus not having them is not
a reason not to do them"), not because corpus evidence later emerged.

**17 - LSP hover/go-to-definition (PARTIAL).** Both stages plus five
follow-ups are done (see Done): every name reference resolvable without
receiver-type inference; an instance receiver's own type (`$x->method()`/
`$x->prop`, including inside a closure/arrow-fn body, and now at
top-level script scope too via `ProbeScope::TopLevel`/`check_program`); a
`Union`/`Intersection` receiver resolving to every declaring class; and
local-variable-declaration navigation, backed by real, checker-driven
reaching-definitions (not a hand-rolled approximation of it) - an
ambiguous read across mutually exclusive `if`/`switch`/`try` arms or a
`while`/`for` loop offers every real candidate as a `GotoDefinitionResponse
::Array`; a guard clause correctly excludes a branch that always
`return`s/`throw`s from ever becoming a candidate; `break`/`continue` are
handled precisely; a `foreach` correctly does *not* offer its own
pre-loop value as an alternative (matching Phase 29's own corpus-verified
checker design); list-destructuring targets are tracked; `global $x;`
resolves to the `global` statement itself (a deliberate, principled
boundary, not a gap); and a genuinely first-appearance by-ref call
argument/`preg_match` `$matches`/array auto-vivification write resolves
to that real site, while one that widens an already-declared variable
still resolves to its own true declaration. What's left is genuinely
narrow, both disclosed and deliberate rather than unattempted: a
`Closure`'s own `use (&$x)`/`use ($x)` capture resolves only when the
outer scope already has tracked provenance for that name at the closure
literal's own point in the file (garbage in, garbage out - nothing to
copy across when there's nothing there). Only *lexically visible* binding
syntax and the three now-handled auto-vivification shapes update
provenance - an ordinary call's own incidental effect on an *already-
declared* variable's type (`sort($arr)`, `array_push($arr, ...)`, a
by-ref widen of a pre-existing variable) never does, on purpose: a read
afterward should resolve to wherever the variable was *really* declared,
not every call that happened to touch it since (see `Locals::insert`'s
own doc comment in `checker.rs`).

**18 - Extension/plugin API (PLANNED).** Deliberately last - premature
before the rule/type-extension catalog is rich enough for a third party
to want to extend.

**19 - Fuzz testing (NOT STARTED).** `arbitrary`+`cargo-fuzz` for the
lexer/parser, `proptest` for `types.rs`'s subtyping logic. Dev-only.

**36 - PHPDoc tag ecosystem expansion (PARTIAL).** `@phpstan-*`-prefixed
tag spellings, `@property`-family validation (the `class.notFound`
slice), and `@phpstan-assert`/`-if-true`/`-if-false` (including the
narrowing they drive at call sites, not just tag parsing) are done.
Still unbuilt: `@method`/`@mixin` synthesis into `ClassInfo`;
`@phpstan-type`/`@phpstan-import-type` are done for a *local* alias
(the define()-registry-backed reattempt, see Done) but cross-class
`@phpstan-import-type` isn't; `@phpstan-require-extends`/`-implements`/
`-sealed`; `@phpstan-consistent-constructor`; `@readonly`
(documentation-only convention); inline `@var`'s own error-reporting
half (`VarTagChangedExpressionTypeRule`/`WrongVariableNameInVarTagRule`
- looked much larger up close than originally scoped: 7+ distinct
statement shapes - assign, foreach, static, global, return, throw,
class/function-level "no effect" - plus a paired type-checking helper,
not a quick win); an enumerated `@phpstan-*` typo-detection whitelist;
`@param-immediately-invoked-callable`/`-later-invoked-callable`/
`-closure-this`.

**37 - Attribute target-validity, remaining kinds (PARTIAL).** Existence
checking, target-validity for property/method/function/param, and
constructor-argument validation (`attribute.abstract`/`.noConstructor`/
`.constructorNotPublic`/arity/type checks, see Done) are all done.
`ClassAttributesRule`/`ClassConstantAttributesRule`/
`TraitAttributeClassRule`/`TraitAttributesRule`/
`PropertyHookAttributesRule`/`EnumCaseAttributesRule`/
`ClosureAttributesRule`/`ArrowFunctionAttributesRule`/
`ConstantAttributesRule` still only get existence-checking, not target
validity. `attribute.nonRepeatable` (needs a same-declaration-list
accumulator, a different shape of check than `check_attributes`'s own
per-attribute loop) and `attribute.deprecated` (would duplicate this
project's own existing `@deprecated` family rather than add real
coverage) aren't ported - a real, deliberate scope trim, not corpus-
evidenced as needed.

**39 - `@deprecated` usage warnings (PARTIAL).** Full access-shape
coverage is done (see Done). Whether a class-level `@deprecated` tag
should imply deprecation of its members is unclaimed - real PHPStan's
own answer lives in a NOT-vendored Composer package
(`phpstan/phpstan-deprecation-rules`), so it can't be confirmed here.

**40 - PHPStan-API-usage checks (PARTIAL).** Done for instantiation,
static calls, class-const fetches, `instanceof`, `extends`/`implements`/
trait-use, and a syntactically-typed slice of method calls (a directly-
typed parameter/closure-capture receiver). Full receiver-type inference
for method calls (e.g. through a constructor-promoted service property)
isn't attempted - a deliberate, disclosed scope trim, not a gap found
later.

**41 - Purity / side-effect model (PARTIAL).** `NoopRule` (a discarded
bare `&&`/`||`/`xor`/ternary result) is done. Everything else is
unbuilt: impure-point classification (~20 operation kinds, `certain` vs
`maybe`), `@phpstan-pure`/`-impure`/`-immutable` tag validation, the
four statement-level `*WithoutSideEffectsRule`s, and transitive purity
across the call graph (needs Phase 82's Collector architecture). XL-size
architectural work, not incremental.

**42 - Checked-exception / throw-type model, remaining (PARTIAL).**
`@throws`-vs-`Throwable` validity, `throws.void`, dead-catch detection,
and `finally`'s own exit-point overwrite are done (see Done). Still
open: transitive `throws` union across the call graph,
`MethodThrowTypeCovarianceRule` (deliberately declined - an implicit
"void when untagged" default would false-positive heavily given how
inconsistently this corpus tags `@throws`), `TooWideFunctionThrowTypeRule`/
`MethodThrowTypeRule`, and the ~16-item per-callee dynamic-throw-type
extension cluster (`Type/Php/*ThrowTypeExtension`).

**43 - Property-hook checker validation (PARTIAL).** Virtual-property
detection, `propertyGetHook.noRead`/`propertySetHook.noAssign`,
`propertySetHook.nativeParameterType` contravariance, existence-checking
for a `set` hook's own parameter type, real function-like body checking,
and `PropertyVarianceRule` are all done (see Done). Only plain property
declarations are covered - a promoted constructor property's own inline
hooks (`Param::hooks`) aren't, since nothing in the registry resolves
`set_ty`/`is_virtual` for that shape either (`property_hook_set_type`
takes a `&PropertyDecl`, structurally unable to accept a `&Param`) - a
real, disclosed residual, not corpus-evidenced (property hooks on a
promoted property is a rare combination even within an already-rare PHP
8.4 feature).

**45 - Declared-vs-actual-narrowest-type inference (PLANNED).** No
mechanism compares a declared type against the narrowest type a body
could actually produce. Splits into a return/property variant (unions
every actual return/assignment and compares against the declared type)
and a rarer, harder `@param-out` variant (this project has no
`@param-out` tag concept at all yet).

**46 - PCRE pattern-validity checking (PLANNED).** No malformed-regex
detection (`regex_groups.rs` only extracts structure, never validates).
Needs a real `pcre2` binding - this project's first C dependency - since
Rust's own `regex` crate is a different-enough flavor to give wrong
answers on real PCRE patterns.

**48 - Symbol-existence pipeline, remaining stretch goals (PARTIAL).**
The existence-check MVP, typed-class-constant validation, and the
multi-namespace-per-file positional-tracking fix are all done (see
Done). Still open: `ExistingNamesInGroupUseRule`'s case-sensitivity/
forbidden-name folding; `ExistingClassesInPropertyHookTypehintsRule`'s
existence-checking half is done for a `set` hook's own parameter type
(Phase 43, no longer blocked - `checker::oop::check_set_hook` reuses the
same `check_type_class_names_exist` every other param/return position
already goes through), but its other sub-checks (a template type not
referenced in a parameter, an unresolvable native type, a native union
type pre-PHP-8.0) aren't ported - real PHPStan's own get hook return-type
half doesn't apply here either, since this project's AST has no explicit
declared return type for a `get` hook to check (implicitly the
property's own type, never separately written down); and
`ClassNameCheck`/`ClassCaseSensitivityCheck`/
`ClassForbiddenNameCheck` - the last needs "name as literally written in
source" tracked through resolution, which today's checker discards at
the point of resolving to a class's own FQN; real PHPStan's own default
(`checkImportedClassNameCase: false`) means most of the practically
common case (a `use`-imported reference) is out of scope anyway even if
built. A full `RuleLevelHelper`-equivalent pipeline (`ClassNameNodePair`/
`ClassNameUsageLocation`/`FoundTypeResult`, ~27 usage-location kinds)
remains a size-XL stretch goal on top of the now-done MVP.

**49 - Call-argument binding, remaining (PARTIAL).** Named-argument
legality, spread/unpack boundary-aware arity checking, and by-ref
argument legality are done (see Done). Full constant-array-spread
expansion (type-checking individual unpacked elements against their own
target parameter) is a deliberate scope trim, not attempted.

**50 - `Type::Array` key-type / offset engine, remaining (PARTIAL).**
The key-type parameter itself and `InvalidKeyInArrayItemRule` are done
(see Done). `InvalidKeyInArrayDimFetchRule` (the offset read/write-site
counterpart) was built and corpus-verified, then reverted - see
**Reverted**, blocked on Phase 54's generics-substitution rewrite.
`ArrayShape`'s own int-vs-string key precision, unsealed-shape
(`array{...}`) tracking, and `HasOffsetType`/`HasOffsetValueType`
accessory facts remain unbuilt.

**51 - `Type` coercion surface, remaining (PARTIAL).** Cast legality,
`IntegerRangeType`'s `abs()`/`~` arithmetic, `print`/`InvalidPartOfEncapsedStringRule`,
and four more `IntegerRangeType`-consumer extensions are done (see
Done). Range-preserving `**`/shift arithmetic on `IntegerRangeType`
remains unbuilt - not corpus-evidenced as a real gap.

**52 - Invalid-operand-combination (`ErrorType`), remaining (PARTIAL).**
The operand-validity matrix (binary/comparison/unary/inc-dec) is done
(see Done), deliberately not a full per-operator `ErrorType`-value
resolver - division/modulo by a literal zero isn't modeled, and a
`Union`'s own legality is "at least one member is legal" rather than
real PHPStan's stricter "every member," a deliberate leniency choice
for this project's coarser unions.

**53 - Structural Callable/Closure type, remaining (PARTIAL).**
Contextual closure-body checking for `array_map`'s own callback is done
(see Done). `Type::Callable` itself is still a bare marker with no
parameter list/return type/purity/template map; contextual injection
for `array_filter`/`usort`/`array_walk`/`preg_replace_callback` is
unbuilt (`array_map` only, for now).

**54 - Generic template richness (PARTIAL).** `TemplateParam` carries a
real bound/variance/scope/default (not a bare name) and the bound-aware
subtype/assignability rules are done (see Done). `substitute_template`/
`class_template_bindings`/`check_variance` still read the old flat
class-level `covariant_templates`/`contravariant_templates` name-sets
rather than each `TemplateParam`'s own field, so variance doesn't
compose through *nested* generics - the single largest remaining
architectural gap in the whole type system (PHPStan splits "template
bound to X" into ~19 concrete subclasses this project still represents
as one variant). Size: XL, genuine algorithmic rewrite. Blocks Phase
50's reverted `InvalidKeyInArrayDimFetchRule` reattempt.

**55 - Narrowing-engine generalization, remaining (PARTIAL).** `count()`
truthiness, `array_is_list`, `str_contains`-family, a real
`instanceof`-intersection bug, and - this batch - fixed-point loop-body
convergence are fixed (see Done). Still missing: a comparison-outcome/
range API for `<`/`>`/`<=`/`>=` generally (only the `count()` special case
exists - cross-check against Phase 51's own `IntegerRangeType` work
before building, so the two don't each reinvent range arithmetic); a
loose `==`/`!=` truth table between differing scalar kinds; a
subtracted/negated-type (`T&~U`) representation (so `!$x instanceof Foo`
on a non-union type is dropped instead of recorded - confirmed still
live by this batch's own full-corpus verification, e.g. Latte's
`CachingIterator::__construct`'s `do { $iterator =
$iterator->getIterator(); } while (!$iterator instanceof \Iterator);`,
where the missing negation means the loop's own exit-narrowing can't
strip `Iterator` back out of the converged type); a bare `assert(...)`
used as a sub-expression rather than a top-level statement; arithmetic on
an `IntRange` operand doesn't preserve the range (`IntRange(0,max) + 1`
widens to plain `Int` instead of staying a range) - newly found by this
batch's full-corpus convergence verification (php-standard-library's
`TemplateParser::parse`/`decode()`, both `strpos()`-offset-plus-length
idioms), not fixed here to keep this batch scoped to loop convergence
itself. `is_a()`/`class_exists()`-family narrowing were evaluated and
declined (near-zero corpus incidence, or - for `class_exists()`
specifically - an unobservable no-op given how this project already
represents a bare `class-string`). Was the prerequisite for Phase 15's
own reverted diagnostics - now unblocked, see Done.

**56 - Grammar-level PHPDoc parser rewrite (PLANNED).** `docblock.rs` is
a lenient regex tokenizer that silently drops what it doesn't recognize,
unlike real PHPStan's recoverable-error grammar parser. Architectural,
not a hard blocker for any one Phase 36 sub-item, but every one of them
would ideally build on it. Size: XL.

**60 - Isset/empty/nullsafe reversal, remaining (PARTIAL).** The bare-
variable/property/method-receiver half is done (see Done). `empty()`'s
own falsiness-aware variant needs machinery this pass didn't build
(both nullability and falsiness need to be unambiguous, which keeps
real PHPStan's own equivalent silent on almost every ordinary shape
too). The offset/property-chain half (`isset($x['key'])`) is gated
behind real property-initialization/offset-existence dataflow this
project doesn't model (Phase 62). One narrow residual: a function
bundling both a legacy and modern native-typed variant behind a runtime
`PHP_VERSION_ID` branch (not just `function_exists()`) can still share
one registry slot with the wrong nullability assumption (4 corpus
occurrences).

**61 - Return-statement completeness, remaining (PARTIAL).** The
`return.never`/`.void`/`.missing` split is done (see Done). The
genuinely-empty-concrete-method-body case (distinguishing it from an
abstract/interface bodiless declaration) was built and corpus-verified,
then reverted - see **Reverted**. Generator `TReturn` completeness
remains unmodeled, matching this project's existing generator
simplification.

**62 - Uninitialized-property dataflow, remaining (PARTIAL).** The core
"assigned anywhere in the class body" heuristic is done, off by default
(see Done). Real per-constructor dataflow (the parent-constructor
chain, `readonly`-specific uninitialized-at-end-of-constructor
detection, `__unserialize`/`__clone` recognition, double-assignment-in-
clone detection) remains unbuilt - a considerably larger feature than
this project's own cheaper heuristic, deliberately not attempted after
three rounds of real corpus false-positive fixes already went into the
simpler version.

**63 - Duplicate-declaration scans, remaining (PARTIAL).** The
within-body scan and `UsedNamesRule` are done (see Done). Duplicate
top-level *function* declarations remain open: real PHPStan's own
version is a whole-project, cross-file scan, and this project's own
registry already has an established *legitimate* multi-declaration
pattern for top-level functions (phpstorm-stubs' own real-overload
representation) that a naive "second declaration = bug" scan would
conflict with, needing the same structural-signature-difference
treatment the class-member case needed, plus real cross-file tracking.

**64 - Override-compatibility rewrite, remaining (PARTIAL).** Method-
and most of the property-override halves are done (see Done). Still
open: property hooks (`notReadable`/`notWritable`/
`abstractOverridingNonAbstractHook`); native-type property variance
(PHP requires *invariant* property types on override) - blocked on
giving `PropertySig` a `ty`/`ty_native` split the way `FunctionSig`
already has, to avoid the same false-positive class that split was
built to fix for methods; PHP 8.4 `final`/`abstract` property modifiers
(new parser work, not currently tracked); and interface-vs-trait-
abstract-method ancestor-lookup *priority* for signature-compatibility
checks specifically (the `#[Override]` check's own trait-aware helpers
are deliberately scoped narrower than this).

**67 - `@internal` restricted-usage engine, remaining (PARTIAL).** Calls,
property/constant access, instantiation, `instanceof`, and `catch` are
all covered (see Done). Still open: `extends`/`implements`/param-and-
return-type-hints/first-class-callables for the same engine, and the
separate, *user-configured* restricted-usage mechanism
(`RestrictedClassConstantUsageRule` and five siblings - unrelated to
`@internal`, lets a project declare its own arbitrary restricted-symbol
list).

**68 - Array no-op diagnostics, remaining (PARTIAL).** Duplicate-key
detection and the two no-op `array_values`/`array_filter`-empty checks
are done (see Done). `arrayFilter.same`/`arrayFilter.alwaysEmpty`
(gated on the array's own element type being provably always-truthy/
-falsy) were dropped after corpus verification caught two live false
positives from a variable-type-tracking gap unrelated to this phase
itself (a dynamic-key assignment inside a branch not widening correctly
across it, and a `try`/`finally` reassignment not accounting for the
`try` body possibly never running).

**69 - printf/sprintf/scanf family, remaining (PARTIAL).** Positional
specifiers and `fprintf`/`vsprintf`/`vprintf` are done (see Done).
Width/precision `*` sub-placeholders, a strict-vs-castable matching
mode, and `scanf`-style by-ref-output-variable counting for
`sscanf`/`fscanf` remain unbuilt - zero corpus incidence found for the
last one.

**70 - Generator/yield type modeling, remaining (PARTIAL).**
Declared-return-type-compatibility checking is done (see Done). No real
`Generator<K, V, TSend, TReturn>` shape exists - `yield`/`yield from`
still always infer `Mixed`; checking each `yield`'s own key/value
against `K`/`V`, a sent-in value against `TSend`, and a `return`
statement against `TReturn` are all unbuilt. Low real-world `yield`
incidence in this project's own Piwigo corpus (2 first-party call
sites) doesn't yet justify the full architecture.

**71 - Small builtin checks batch, remaining (PARTIAL).**
Function-name case-insensitivity, `define()`'s removed third argument,
`random_int()` ordering, and useless-return-value builtins are done
(see Done). Deliberately deferred: `call_user_func`/`call_user_func_array`
argument-reordering-and-recheck (needs callable-string/array-to-
signature resolution this project doesn't have); `filter_var()`'s
`FILTER_NULL_ON_FAILURE`/`FILTER_THROW_ON_FAILURE` flag-conflict (1
corpus call site, using neither flag); `preg_quote()` delimiter
cross-checking (every real corpus call site is already correctly
delimited, nothing to verify a fix against); `function_exists()`-guard
suppression for the undefined-function check (no evidence it's
currently needed).

**73/74 - Array/string stdlib coverage, remaining (PARTIAL).** Key-type-
dependent functions (`array_fill_keys`, `array_search`,
`array_intersect_key`, `array_flip`) and the pointer/`array_diff`/
`array_intersect` family are done (see Done). Still unbuilt, all zero
or near-zero corpus incidence: `array_change_key_case`, `array_rand`,
`array_count_values`, PHP 8.4's `array_find`/`array_find_key`/
`array_any`/`array_all` (not even stub-registered yet); `array_key_exists`
narrowing (needs `HasOffsetType`/`HasOffsetValueType` accessory facts,
Phase 50); `array_pop`/`array_shift`'s own expression-result type;
`array_splice`/`array_replace`/`array_pad`; a known stub imprecision in
`array_udiff`/`array_uintersect_assoc`'s variadic-tail typing (zero
corpus incidence).

**75 - String accessory case-tracking (PLANNED).** No `Lowercase`/
`Uppercase`/`DecimalIntegerString` accessory kinds; `strtolower()`/
`strtoupper()` and friends (`str_pad`/`str_repeat`/`substr`/`trim`/
`ltrim`/`strrev`/`parse_str`) don't produce case-tracked types.
Separately: literal-string is never inferred from concatenation, which
undermines the feature's main real-world use (SQL-injection-safety
checking). Size: M.

**76 - `filter_var`/`filter_input`, remaining (PARTIAL).** The literal-
`FILTER_VALIDATE_*`-constant-with-plain-flags case and a literal
`['options' => [...]]` array's own `default`/`min_range`/`max_range`
sub-keys (see Done) are done. `filter_input()` itself (zero corpus
incidence), `FILTER_REQUIRE_ARRAY`/`FILTER_FORCE_ARRAY`'s per-element
return type, and PHP 8.4's `FILTER_THROW_ON_FAILURE` remain unbuilt.
`FILTER_SANITIZE_*` needs no work - every one already maps to plain
`string`, matching the generic stub.

**77 - Runtime-capability modeling, remaining (PARTIAL).** Most of the
originally-scoped functions are done (see Done). Reflection-class
modeling in the registry and `SimpleXMLElement` magic-property
resolution remain unbuilt - both need genuinely new infrastructure this
project doesn't have (no Reflection-specific registry surface, no
magic-property-resolution mechanism), and no corpus evidence of heavy
usage has surfaced to justify building it speculatively.

**78 - DateTime/DOM/SimpleXML, remaining (PARTIAL).** `DateInterval`'s
own duration-string grammar is done (see Done). `DOMDocument`/
`SimpleXMLElement` XML-content validation and `DateTime`'s relative-
date-string parsing (`"+1 day"`, `"next monday"`) both need real
PHP-runtime/XML-parser simulation this project has no way to do -
flagged low-priority/possibly-permanent-boundary, not concretely
scheduled. Size: L remaining, low value.

**80 - Object-shape/key-of/conditional-type syntax, remaining
(PARTIAL).** `object{...}` shapes, `value-of<T>`/`key-of<T>` (the
registry-free cases), conditional-return-type syntax, and (see Done)
`value-of<T>` naming a real, registered enum (a deferred-resolution
mechanism, `Registry::resolve_deferred_value_of`) are all done. Still
open: `OffsetAccessType` (a literal-key array access's own type extracted
from a docblock type-string context); `key-of<T>` naming a class/enum
reference (confirmed real PHPStan has no special case there either -
`KeyOfType::getResult()` just falls to `getIterableKeyType()`, `Mixed`
for an ordinary class - so this is a deliberate non-gap, not deferred
work); letting a conditional return type pick a specific call site's real
branch instead of always widening to the union of both. Phase 54's own
`TemplateKeyOfType`/`TemplateObjectShapeType` bound kinds can now build on
an already-resolved `ClassInfo.template_bounds` (the specific piece Phase
54 needed from here), though Phase 54 itself remains untouched. Size: M
remaining (down further after the `value-of`/enum slice above).

**82 - Collector / cross-file aggregation architecture (PLANNED, do not
schedule standalone).** No two-phase Collector-then-aggregate pass
exists. Blocks a handful of low-value dedup rules and is a soft
prerequisite for Phase 41's transitive-purity resolution. Build only
when Phase 41's transitive half is actually tackled, and let that phase
absorb the cost. Size: XL, low standalone value.

**83 - Whole-project duplicate class-name detection (PLANNED, low
priority).** Re-evaluated and confirmed a genuine architectural
conflict, not just low-priority: the registry's own "second
unconditional declaration merges as a targeted stub override" behavior
(needed for real, observed corpus correctness - Piwigo's own
`Imagick.stub.php`) is indistinguishable, in this project's current
design, from an accidental duplicate. Real PHPStan tells them apart via
a `stubFiles` loading mechanism this project has no equivalent of.
Needs file-provenance tracking as a genuine new prerequisite, not a
self-contained addition. Size: XL, low value.

**84 - `RuleLevelHelper`-equivalent tri-state type-acceptance
architecture (PLANNED, low priority).** `is_assignable`/`is_subtype`
return plain `bool`, no tri-state (`Yes`/`No`/`Maybe`)-with-reasons
result. Re-evaluated and confirmed low-value on a second look, not just
a confidence gap: every "maybe" already resolves to a concrete answer
somewhere in the match arms today, so the rewrite would touch every
`is_subtype`/`is_assignable` call site without changing any diagnostic's
actual outcome - a real cost with no corpus-verifiable behavior change
to confirm it was done correctly. Size: XL, low-medium value.

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
  reinstated with real inferred-type trust, see Done). A real, demonstrated
  soundness bug, not a corpus-representativeness call, caused the original
  revert: this checker had no fixed-point loop analysis, so a variable
  reassigned inside a loop body kept its pre-loop type for the whole
  check, which both diagnostics misinterpreted as "always terminates"/
  "always true or false" on extremely common idioms (a loop-scoped flag
  variable, a deferred reassignment). ~900 unverifiable false positives.
  Two real root causes, both since fixed: loop staleness (Phase 55's
  fixed-point convergence, `converge_loop_body`/`loop_continue_states`)
  and `widen_coalesce_result`'s own `?? false` collapse (confirmed a
  small, self-contained, independently-fixable bug this session - not
  the same thing as the large, genuinely-separate comparison-outcome/
  range-API item this entry used to conflate it with). A narrower,
  syntactic-literal-only version shipped as an intermediate step while
  only the first cause was fixed; once both were, the *original* design -
  trusting the condition's own full inferred type - was rebuilt in full
  rather than staying narrowed, per explicit direction ("i dont mind
  churn, i want MOST proper"). Building it surfaced and fixed four more
  real bugs of the identical shape (a condition/its own dependencies
  checked against a stale or wrongly-narrowed scope) - see Done for the
  full list.
- **`InvalidKeyInArrayDimFetchRule`** (Phase 50). Also a real soundness
  bug: a *used* (not merely declared) generic template parameter's own
  type leaks through as a bogus namespace-qualified class reference
  instead of staying `Type::TemplateParam`, which this check couldn't
  tell apart from a genuine invalid array key without registry/template
  context it doesn't have as a free function. +31 diagnostics, all
  traced to this one gap. Real fix belongs to Phase 54's generics
  rewrite.
- **Genuinely-empty-concrete-method-body check** (Phase 61). Verified
  individually correct (confirmed via `php -l`: a real `TypeError` at
  the call site) and NOT a soundness bug - the corpus regression
  (+10,623, nearly all in `jetbrains/phpstorm-stubs`) was entirely one
  real, ubiquitous PHP convention (an IDE-stub/native-extension class
  declaring a native function's signature with a deliberately empty
  body) that real PHPStan itself never encounters, because it loads
  stub files through a separate, non-rule-checked mechanism this
  project's own broader verification corpus doesn't mirror. Zero
  confirmed genuine bugs among the new findings, but also zero evidence
  the check is *wrong* - **a strong candidate to ship opt-in
  (`--check-...`) instead of staying fully discarded**, per
  [[feedback_dont_discard_correct_checks_ship_opt_in]]; not yet
  revisited.
- **`Foreach`'s own "may run zero times" pre-loop-state merge**
  (originally Phase 29, re-attempted and re-reverted since). A real,
  demonstrated soundness bug both times: giving `Foreach` the same
  `While`/`For`-style zero-iteration merge breaks the common "a loop
  mutates the very array it iterates via its own loop variable" idiom
  in general, and a later, more careful re-attempt still regressed a
  real corpus file (`CalendarMonthly.php`) via an `if ($items !== [])`
  narrowing-wrapper interaction the fix didn't account for. Needs real
  fixed-point loop analysis (Phase 55) to do soundly, not a targeted
  patch.
- **`@phpstan-type`/local type-alias support, first attempt** (Phase
  48). Implemented and reverted after a net-negative corpus regression
  (a `class.notFound` win more than offset by a new `argument.type`
  false-positive class, once a precisely-resolved alias type stopped
  benefiting from an existing unresolvable-type leniency) - root-caused
  to a separate, real, pre-existing gap (`define()`-declared global
  constants resolving to bare `Mixed`). Historical only: successfully
  reattempted once that gap was closed with a real global-constant
  registry, and is now shipped (see **Done**, Phase 36).
- **Anonymous-class ancestor-aliasing shortcut** (an ad hoc round
  during the Phase 14.5 corpus-cleanup series). Aliasing an anonymous
  class's inferred type to whichever single `extends`/`implements`
  target it names was implemented and corpus-verified, then reverted:
  unsound for the common "spy" test-double pattern (a class implementing
  a real interface but also declaring its own extra instrumentation
  properties/methods a test reads directly) - the aliased interface-only
  type doesn't have those, turning every read into a new false positive
  (+11, worse than the 4 real fixes it found). A real fix needs
  registering the anonymous class's own full declaration under some
  stable synthetic name, the same way a named class already is - not a
  type-inference tweak.

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
  beyond the dozen or so this project special-cases remains untouched.
  Two narrow, low-value narrowing gaps: reversed `0 < count($x)` (only
  `count($x) > 0`/`>= 1` narrow), and `!empty($x)` not upgrading an
  already-`non-empty-string` value to `non-falsy-string` on a redundant
  nested guard.
- An overriding method's own parameter doesn't inherit a stricter
  ancestor/interface docblock refinement it doesn't redeclare itself
  when there's no *own* docblock inheritance case to trigger the
  already-shipped PHPDoc-inheritance backfill (real shape: Doctrine's
  `EntityManager::getReference()`'s generic signature lives only on its
  interface). Needs real interface-to-implementation PHPDoc inheritance
  for method signatures generally, params and returns, generic or not -
  a sizable feature on its own, not attempted piecemeal per shape.
- No mechanism for narrowing correlated across two *separate* variables/
  statements (real PHPStan's `ConditionalExpressionHolder`) - a boolean
  flag assigned exactly a condition expression then checked bare/negated
  later; two variables assigned together in the same branch where
  narrowing one should imply the other's nullability; a repeated
  identical condition on a never-reassigned variable, checked twice.
  Each real and evidenced (9-ish diagnostics across a handful of files)
  but genuinely hard to thread through `Locals`'s own O(1)-clone,
  structurally-shared design without a parallel mutable side-channel
  duplicating its branch-merge/invalidation semantics from scratch.
- `strtotime()`'s real accuracy and a raw SQL/DQL query's own result
  shape are both structurally infeasible without embedding a PHP
  runtime or a SQL parser respectively - confirmed real PHPStan needs
  exactly that (literally calling `strtotime()` at analysis time; ORM-
  extension-specific DQL introspection this project doesn't replicate).
- phpstorm-stubs' interleaved-multiple-parameter-list shape (two full
  parameter lists under the same names across PHP-version eras, e.g.
  `pg_query_params`) isn't recognized - ~384 residual "expects at least
  N arguments" diagnostics, not fixed.
- A trait declared twice with genuinely different method signatures per
  conditional branch checks both bodies against whichever declaration
  won the registry slot, not the specific branch being checked (4
  diagnostics, one real file) - fixing it means `check_function`
  trusting the AST node's own declared return type instead of
  re-resolving through the registry for this one check, a change to a
  heavily-used core path not attempted for this narrow an incidence.
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
- Variance checking doesn't cover properties (this project doesn't
  track `readonly`, and a *readonly* property is actually a legitimate
  covariant-safe position) or flip variance through a nested generic
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
