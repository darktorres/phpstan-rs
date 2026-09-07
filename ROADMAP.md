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
whole checks, never the left one.

Confirmed against the real Piwigo corpus (both the app-only scope and
the full vendor-included one) after every fix above: zero remaining
false positives traced to this family's own new checks - the only
surviving diff lines are `widen_coalesce_result`'s own genuine, expected
precision ripples (reviewed individually) and real, correct findings
(including in vendor/example code, where a docblock-declared literal
param type like `@param false $x` makes `if ($x)` genuinely always-false,
confirmed against `vendor/jetbrains/phpstorm-stubs`).

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
is not a priority order.

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
comparison-outcome/range type inference (Phase 55's own separate, larger
item - not the same thing `widen_coalesce_result`'s bug was, that's now
fixed and unblocked). `FunctionCallConstantConditionRule`'s own cross-
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
feature, not owned by any phase). `while.alwaysTrue`/`doWhile.alwaysTrue`'s
own breakless-detection reuses `stmt_contains_break`'s existing "any
break anywhere" conservatism rather than real PHPStan's own more precise
per-loop-level `continue N` analysis - a narrow, disclosed simplification
(a `continue N > 1` that truly does escape the loop isn't recognized as
disqualifying the diagnostic, unlike real PHPStan). A `for`'s own non-
last comma-separated condition expression(s) are checked once up front
rather than on every iteration (real PHP re-evaluates all of them, only
the last one's truthiness gates the loop) - a narrow, disclosed
simplification, since every real corpus use seen is a side-effect-only
scratch expression there. `continue N`'s own level argument is parsed
but never consulted for *narrowing* purposes (Phase 55's own `loop_
continue_states` now consults it for convergence), and `goto`/label CFG
edges are unmodeled - neither owned by any phase.

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
checking and target-validity for property/method/function/param are
done (see Done). `ClassAttributesRule`/`ClassConstantAttributesRule`/
`TraitAttributeClassRule`/`TraitAttributesRule`/
`PropertyHookAttributesRule`/`EnumCaseAttributesRule`/
`ClosureAttributesRule`/`ArrowFunctionAttributesRule`/
`ConstantAttributesRule` still only get existence-checking, not target
validity. Constructor-argument validation
(`attribute.noConstructor`/parameter-count/type checks) needs Phase 49's
call-argument-binding generalization first.

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

**43 - Property-hook checker validation (PLANNED).** Hook bodies are
parsed but never type-checked - no function-like registry entry exists
for one to check against. Needs "virtual property" detection (no hook
body ever references the property's own backing storage) and, as a
prerequisite, `readonly`/`private(set)` metadata on `PropertySig`
(tracked at Phase 57, done) before `PropertyVarianceRule` can run.

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
forbidden-name folding, `ExistingClassesInPropertyHookTypehintsRule`
(blocked on Phase 43), and `ClassNameCheck`/`ClassCaseSensitivityCheck`/
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
`FILTER_VALIDATE_*`-constant-with-plain-flags case is done (see Done).
`filter_input()` itself (zero corpus incidence), `FILTER_REQUIRE_ARRAY`/
`FILTER_FORCE_ARRAY`'s per-element return type, an `options` array's
`default`/`min_range`/`max_range` (needs Phase 51's `IntegerRangeType`
completion), and PHP 8.4's `FILTER_THROW_ON_FAILURE` remain unbuilt.
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
registry-free cases), and conditional-return-type syntax are all done
(see Done). Still open: `OffsetAccessType` (a literal-key array access's
own type extracted from a docblock type-string context); a genuine
deferred-resolution mechanism that would let `value-of`/`key-of`
resolve a class/enum reference (needs registry access unavailable at
parse time) and let a conditional return type pick a specific call
site's real branch instead of always widening to the union of both.
Also a prerequisite for Phase 54's `TemplateKeyOfType`/
`TemplateObjectShapeType` bound kinds. Size: M remaining (down from XL).

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
