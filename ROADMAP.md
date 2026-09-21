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
  metric - is 342 as of `c8555bd`, down from a 343 pre-batch baseline
  (`6412b33`) after
  a 14-item batch (Phases 17, 18, 19, 40, 45, 46, 48, 54, 67, 75, 77, 78,
  83, 84 - see Done for each). The net *decrease* despite several brand-
  new checks shipping is real: this batch's own corpus-diff pass found
  and fixed six genuine, previously-latent bugs exposed by verifying the
  new work at scale, several pre-existing (not introduced by this
  batch) - a nullsafe-operator postfix chain (`$a?->b->c`) losing its
  own inherited nullability past the first non-nullsafe link; `resolve_
  pseudo` never recursing into a generic class reference's own type
  arguments; `match`'s own result-type computation building a raw,
  un-flattened `Type::Union` instead of reducing through `union_types`;
  a multi-condition `match(true) { $a, $b => X }` arm getting zero
  narrowing applied to its own body; real PCRE's own leading/trailing-
  whitespace-tolerant delimiter detection; and `check_too_wide_return_
  type`'s own boolean (not real PHPStan's trinary) actual-vs-declared
  comparison flagging every declared member whenever the actual type
  widened to `Mixed`. Phase 56 (the full grammar-level PHPDoc parser
  rewrite) remains the one deliberately unattempted item from this
  batch's own source plan - its own dedicated future session, given its
  XL size and the whole plan's own explicit decision to sequence it
  last. Every run is saved to `.corpus-runs/{scoped,unscoped}/
  <commit-hash>.txt` (gitignored, see that directory's own README) -
  check there before re-running an old commit to get a comparison
  snapshot. Re-verify against real PHPStan before citing an exact
  number either way - it drifts release to release even without new
  checks shipping.
- Full Piwigo+`vendor/` corpus (~20,200 files, unscoped): 40,459
  diagnostics as of `c8555bd`, the overwhelming majority genuine findings
  in third-party
  code this project doesn't control - not re-verified line-by-line at
  this scale (best-effort secondary coverage, per **Verification
  approach**), only spot-checked per new-finding bucket. Re-run before
  citing a number - it drifts as vendored dependencies and this
  project's own precision both change.
- Test suite: 3248 unit tests, all passing (build/test/clippy clean).

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
resolves literal-shaped constant values project-wide, now including
every real built-in/extension constant the vendored phpstorm-stubs
corpus itself declares (`php_stubs::generated_stub_constants`, merged
the same way `generated_stubs`/`generated_stub_classes` already are) -
`use const`'s own existence check (`constant.notFound`) is unconditional
(constants are case-sensitive in PHP, so there's no name-case
equivalent to build); a `define()` nested inside a function/method body
stays a disclosed, pinned residual (invisible to this registry either
way, not a regression). `PHP_INT_MIN` - `define_call_constant`'s own
pre-existing, deliberate exclusion for its overflowing raw literal -
used to mean it was never registered at all (safe for ordinary
constant-value inference, which already falls back to `Mixed` on a
miss, but indistinguishable from "doesn't exist" for the new existence
check); now registered with `Type::Int` directly instead. Corpus-
verified: the scoped Piwigo run is clean (one genuine false positive
fixed, `DIRECTORY_SEPARATOR` now resolving to its real value); the
unscoped vendor-included run surfaces a disclosed, narrow residual -
~250 new `argument.type`/`return.type` findings concentrated entirely
in php-cs-fixer's own bundled `Token`-array-literal handling (never in
Piwigo's own code), reproducible only at full ~20k-file corpus scale
and not root-caused further past confirming the underlying constants
themselves resolve correctly in isolation - a real, if imprecise,
downstream interaction with this project's own array-shape/control-flow
analysis at that scale, not a wrong constant value.

A class/interface/trait declaring the same method name twice in one
body (phpstorm-stubs' own version-split/overload idiom, e.g. real
`DatePeriod::__construct`'s three distinct forms) now merges into one
positional-union signature the same way two top-level free-function
declarations already did (Phase 32 Finding C) - `ClassInfo::from_decl`/
`from_enum_decl` previously built the method table with a bare
`.collect()`, silently keeping only the textually-last declaration and
discarding every earlier alternate parameter list. A trait declared
twice under a conditional with genuinely different method signatures
(real corpus shape: doctrine-orm's own
`PersistentCollectionImplementation`) now checks each branch's own body
against its own literal signature (`check_trait_methods` builds it
directly via `ClassInfo::from_decl` and passes it down), not whichever
declaration happened to win the registry's single per-name slot -
`registry/index.rs`'s own tiebreaking policy is unchanged, the fix is
confined to the checker's own signature-selection step.

**Symbol-existence pipeline: name-case checking, full coverage** (Phase
48). `ClassCaseSensitivityCheck`'s own class-name-case half
(`Registry::canonicalize_class_name_case`, a one-time O(classes)
rewrite pass run right after indexing, before any ancestor-graph walk
reads `extends`/`implements`/`uses`, plus `Registry::lookup_class`
mirroring `lookup_function`'s case-insensitive-fallback shape) now
covers every native-syntax class-name reference (`new`, `instanceof`, a
static method/property/constant access, a `catch` clause, a class/enum
declaration's own `extends`/`implements`/trait-`use`) and every PHPDoc-
typed position too: function/method param and return types, property
positions (including `@property`), `@method`/`@mixin`/`@phpstan-type`
tag targets, property hooks' own `set()` parameter, `@phpstan-assert`-
family tag targets, `@phpstan-require-extends`/`-implements`/`-sealed`
RHS, and closure/arrow-fn literal param/return hints - the last four via
a parallel traversal (`registry::name_context::collect_class_name_case_
refs`, structurally identical to `resolve_names_in_type` but collecting
`(raw, resolved)` pairs into a side list rather than rewriting the
`Type` in place) run one step earlier in the parse pipeline, since this
project's own type-parsing collapses each reference straight into a
resolved `Type` with no per-reference raw spelling retained by the time
any checker call site sees it. A *backfilled* (inherited-docblock)
position deliberately never gets `case_refs` propagated - the raw text
was written under an ancestor file's own `use` imports, not this file's
`name_ctx`, so re-collecting against the descendant's own context would
be actively wrong (different imports) and double-report; the ancestor's
own declaration already ran the check once at its true owning file.
Also fixed, the same corpus round: `Parser::take_name_segment`'s own
keyword-as-identifier recovery reconstructed an identifier from the
keyword's own canonical lowercase spelling rather than the original
source bytes, so `class Enum { ... }` (a real, valid class literally
named the same as the `enum` keyword, confirmed against `vendor/
marc-mabe/php-enum`) silently corrupted its own declared name to `enum`
well before any registry/case-checking logic ran. Fixed the way this
project already fixes exactly this class of "stash a value out of band
during lexing, thread it through a parallel channel" problem elsewhere
(`Lexer::pending_return_type_override`, `Parser::spans`): a new `Lexer::
pending_keyword_raw` captured at the keyword-token site, threaded
through `tokenize_with_spans`'s widened return tuple into a new
`Parser::keyword_raw` side channel `take_name_segment` consults instead
of reconstructing from `Keyword::as_str()` - `Token`'s own shape and its
167 existing match/construct sites across the codebase stay completely
untouched.

**`function.duplicate` corrected from always-on to opt-in** (Phase 63
follow-up, found via a Piwigo/real-PHPStan corpus diff). Real PHPStan's
own `DuplicateFunctionDeclarationRule` is `#[ValidatesStubFiles]` -
exactly like `DuplicateClassDeclarationRule` below - and never runs
against ordinary scanned project files at all, only a project's own
configured `stubFiles:` list in an isolated pass. Phase 63 originally
shipped this project's own equivalent as an unconditional, always-on
check on the mistaken premise that the real rule ran the same way
project-wide; confirmed live via a real corpus false positive:
`C13yInternalTest`'s own `Unit`/`Integration` suites each unconditionally
declare an identically-named top-level function, never actually loaded
together in the same PHP process - a case this checker (like real
PHPStan) has no general way to rule out. Now gated behind `Registry::
check_duplicate_function_declarations` (opt-in, default `false`, full
config/CLI/`Project`-setter wiring chain mirroring `check_duplicate_
class_declarations` below exactly), correcting the doc-comment
mischaracterization in both `checker::duplicate_functions` and this
entry. Still adds its own real, deliberate divergence from the real
rule's own body once opted in: an added signature-difference check (the
phpstorm-stubs `strtr` two-declaration overload split) - unaffected by
this change.

**Whole-project duplicate class-name detection** (Phase 83). New opt-in
`check_duplicate_class_declarations` (off by default - see below) flags
the same class/interface/trait/enum FQN declared unconditionally more
than once project-wide, going beyond real PHPStan (whose own `Duplicate
ClassDeclarationRule` only ever runs against a project's own configured
`stubFiles:` list in an isolated validation pass, never against ordinary
scanned files - this is `phpstan-rs` going stricter than real PHPStan by
design, not porting an existing whole-project behavior, per this
project's own "stricter than PHPStan is fine" convention). New
`Registry::stub_paths`/`--stub-path` config-and-CLI concept (mirroring
`scan_paths`/`--scan-dir`) designates a file/directory's class-like
declarations as deliberate overrides (a vendored `Imagick.stub.php`,
say) rather than genuine accidental duplicates - still loaded for type-
resolution context exactly like `scan_paths`, just excluded from the new
check's own counting. `src/project/duplicate_class_sites.rs`'s
`DuplicateClassSites` is a straight port of the existing Phase 63
`DuplicateFunctionSites` precedent to class-likes (same statement-only,
skip-conditional-declarations walk; an anonymous class is structurally
excluded for free, since `new class {...}` never reaches a `StmtKind::
ClassDecl` this walker descends into), plus the one new `is_stub` field
the function-side precedent didn't need. Ships opt-in, not unconditional
like its function-side sibling: the one known corpus false-positive
shape (a vendored built-in stub colliding with the real extension's own
class) can only be defused once `stub_paths` is correctly populated,
which every existing `phpstan-rs.toml` predates - flip the default once
a project's own config migrates its stub-override paths there.

**Anonymous class stable synthetic naming** (commit `701be86`). A
`new class { ... }` instance is now registered into the registry under a
real, stable synthetic name (`"AnonymousClass" + md5("{file}:{line}
[:{line_index}]")`, mirroring real PHPStan's own `AnonymousClassNameHelper`/
`AnonymousClassVisitor` algorithm exactly, `line_index` present only when
more than one anonymous class starts on the same line) instead of
inferring bare `Type::Mixed` - a new `registry::collect_anonymous_classes`
whole-file walk (descending into function/method/closure bodies, unlike
`build_declarations`'s top-level-only walk) registers each one's full
`ClassInfo`, not just an alias to whichever interface it implements (see
this same real fix's own Reverted-section entry above for why a type-
inference-only shortcut is unsound). The checker's own inference site
never recomputes the name - it looks its own expression's span up in a
`Span -> name` map computed once at parse time, so there's only one
computation site to keep in sync. Fixes 3 `*MiddlewareTest.php` findings
(`ApiIdempotencyMiddlewareTest`, `ExceptionHandlerMiddlewareTest`,
`SentryMiddlewareTest`) where an anonymous-class instance is
passed/assigned somewhere its declared interface should satisfy.

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
`+`/`-`'s constant-shift fast path/`*` now union `Float` into their
result when a bound genuinely overflows `i64` (`checked_bound_op`,
`checker/expr/ranges.rs`), matching real PHP's own silent int-to-float
promotion on overflow - distinguished from an operand that was already
unbounded (which correctly stays plain unbounded, no `Float` needed).
`sub_int_range_full`'s own general range-minus-range case still doesn't
- left as a faithful, untouched transcription of real PHPStan's own
logic there.
`Type::Array` carries a real key type, not
just a value type. `object{...}` shapes (including a genuinely
unsealed one, `array{..., ...}`), `value-of<T>`/`key-of<T>` (a
`value-of<T>` naming a real registered enum resolves via a one-time
deferred-resolution registry pass; other cases stay `Mixed`, matching
real PHPStan's own lack of a special case there), `OffsetAccessType`
(`SomeShape['key']`), and conditional PHPDoc return types (`@return
($x is T ? A : B)`) are all parsed and resolved instead of mangling
into bogus class names. `enum-string`/`enum-string<T>` route through
the existing `Type::ClassString` bound to `UnitEnum`, mirroring real
PHPStan's own `GenericClassStringType(UnitEnum)` resolution for free
(every enum already synthesizes `implements UnitEnum`); `callable-
string` is `Intersection([String, Callable])`, mirroring real PHPStan's
own resolution and this project's existing `literal-string`/`numeric-
string` pattern. `interface-string`/`trait-string` get real content
verification too (checked against `ClassInfo::kind` via two internal
`$$interface`/`$$trait` sentinel markers) even though real PHPStan
itself draws no distinction between these and bare `class-string` -
this project is allowed to check more strictly than real PHPStan does.
Enum
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

Phase 80 follow-up: a conditional PHPDoc return type now resolves to a
specific call site's real branch (`Type::Conditional`, `types::
resolve_conditional`, wired into `Checker::infer_call_return_type`,
the single choke point all ~9 real call-return-type sites already
use), instead of always widening to the union of both branches - real
PHPStan's own actual per-call-site behavior. A position with no bound
argument to resolve the subject against (a `@param`/`@var` tag, or a
`return` statement's own compliance check against its declaring
function's signature) falls back to the same safe union comparison
this project always did, reusing the real `Union`-vs-`Union`
decomposition rule rather than a hand-rolled, coarser check (a real
corpus false positive caught before landing: a compound `sub`
straddling both branches piecewise, e.g. `list<string>|list<X>`,
was wrongly rejected unless it fit entirely inside just one branch).
Two real, disclosed, pre-existing gaps this newly exercised (both in
adjacent-but-separate subsystems, not this resolution mechanism
itself, confirmed via isolated repros with zero conditional-type
involvement) at the time: this project's own return-statement type
inference didn't correlate a value accumulated across several separate
`if`/`else` branches/loops with an outer condition made much earlier in
the same function (a real corpus find, `NotificationByMailSender::
sendMailNotifications()`, where the returned list's element type
depended on the same `$action` a conditional return type's own subject
also names) - **now fixed**, see Done's own `discriminant_array_facts`
entry (a per-branch accumulator side-channel, consulted as a return-
check fallback, rather than a change to this resolution mechanism
itself). The second gap remains open: ternary/if-else branches don't
track a `bool`/string literal narrowly enough for a declaration-level
conditional-return-type self-check to match precisely (`return $x ===
[] ;`-shaped code widens to plain `bool` rather than `true`/`false`
per branch - several real instances across `vendor/php-standard-
library`'s own heavily-conditional-typed functions).
PHPDoc-inheritance backfill: a method correctly omitting its own
`@param`/`@return` in favor of an ancestor's/interface's tag resolves
through that tag instead of looking untyped, including through
anonymous classes and promoted-constructor-property inline `@var` tags,
with real explicit-vs-implicit `mixed` provenance surviving the
inheritance hop and the backfilled type reconciled against the child's
own native type hint the same way an *own* docblock already is (not
cloned in verbatim, which would let an ancestor's broader doc type
override a child's own correct, narrower native signature). This
backfill now also fires for an ancestor type mentioning a `@template`
declared directly on the ancestor *method itself* (as opposed to its
enclosing class) - `TemplateParam::scope` already distinguishes the two
cases structurally, and a method-scoped template is safe to clone
verbatim since call-site binding is purely name-keyed and blind to
which class's `FunctionSig` it came from (verified against real
PHPStan's own inheritance-merge source: it draws the identical line,
via `getActiveTemplateTypeMap()` holding only class-level bindings).
Real corpus shape this fixes: Doctrine's `EntityManagerInterface::
getReference()` (`@template T of object`) backfilling onto
`EntityManager::getReference()`'s own wholly undocumented native
signature. The class-scoped half (real cross-inheritance `@extends
Base<Concrete>` template binding) stays exactly as declined as before -
see `an_ancestors_template_parameterized_type_is_never_inherited`.

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
dropped `arrayFilter.same`/`arrayFilter.alwaysEmpty` attempt (since
fixed and shipped unconditionally - see "Array no-op conditions"
above). Corpus-verified project-wide: fixes the
identical redundant-union-member pattern across dozens of unrelated
findings (cleaner, correctly-merged displayed types), and surfaces
several new, plausible true positives from a real key type that was
previously under-reported (a dynamic array key genuinely built from
more than one source type, e.g. `int|string`, is now visible as such
project-wide instead of only the first-seen branch's own narrower key
type).

**String accessory case-tracking** (Phase 75). New `AccessoryKind`
variants `LowercaseString`/`UppercaseString`/`DecimalIntegerString`
(the latter implying `NumericString`/`LowercaseString`/`UppercaseString`/
`NonEmptyString`, mirroring `AccessoryDecimalIntegerStringType`'s own
real implications), produced by `strtolower`/`mb_strtolower`/
`strtoupper`/`mb_strtoupper`/`lcfirst`/`ucfirst`/`mb_convert_case` (whose
own force/keep-lowercase/uppercase composition, resolved from a
constant `MB_CASE_*` mode argument, is real accessory-composition
behavior, not a no-op the way `ucwords`/`mb_convert_kana` genuinely are).
A new `resolve_concat_type` (a full port of real PHPStan's own
`InitializerExprTypeResolver::resolveConcatType`) closes the feature's
main real-world gap: concatenation previously never inferred `literal-
string`/`non-falsy-string` at all, collapsing straight to plain `string`
and silently undermining `literal-string`'s own SQL-injection-safety
use case. `lowercase-string`/`uppercase-string`/`non-empty-lowercase-
string`/`non-empty-uppercase-string` PHPDoc pseudo-types now route
through the same real `Intersection([String, Accessory(_)])` machinery
as `non-empty-string`/`numeric-string`, reversing an old downgrade-to-
plain-`string` policy that predates having any producer to verify them
against. A new `format_php_float` (`src/php_float_format.rs`) gives
concat's own float-to-string coercion PHP's real, empirically-verified
(live PHP 8.5) fixed-14-significant-digit `%G`-style algorithm, not a
shortest-round-trip encoder (`(string)(0.1+0.2)` is `"0.3"`, not
`"0.30000000000000004"` - that's `var_export()`'s own, different, code
path). Declined, needing a wholly separate dynamic-array-key-narrowing
consumer mechanism this codebase doesn't have: `AccessoryDecimalInteger
StringType`'s own `inverse`/array-key-casting variant - its own future
item, the same shape as the declined Mixed-provenance item below.

**Level-aware type-acceptance** (Phase 84). `Registry::configured_level:
Option<u8>` (the project's own `level`, `None` preserving today's
always-strict behavior exactly) plus `Registry::accepts_at_configured_
level` give `project.level` real teeth over argument/return/property-
type strictness at 8 call sites, mirroring real PHPStan's own
`RuleLevelHelper::accepts()`: below level 8, `Null` is stripped from an
accepted Union before the check; below level 7, a Union comparison uses
`.any()` instead of `.all()`. A real, severe bug caught before shipping:
an earlier draft special-cased "`sup` is also a Union" as a bail-to-
plain-`is_assignable` fast path, which would have silently disabled the
whole feature for its single most common real-world trigger - every
nullable declared type (`?string`, `?Foo`, ...) is represented as
`Union([X, Null])` in this project, so `sup` being a Union is the
*typical* case, not an edge one; fixed by giving the new method its own
level-aware recursive counterpart to the existing Union-vs-Union
cascade instead of bailing. Declined, needing infrastructure large and
disconnected enough to be its own future phase: `checkExplicitMixed`/
`checkImplicitMixed`/`checkBenevolentUnionTypes` (real level 9/10) -
`Type::Mixed` carries no explicit-vs-implicit provenance to key them off.

**`array + array`'s key/value precision, found via a Piwigo/real-PHPStan
corpus diff.** `BinOp::Add`'s array-union arm used to collapse straight
to `Array(unknown_array_key(), Mixed)` unconditionally, discarding both
operands' own key/value types entirely. Real impact traced directly:
`$merged = $filters + $functions;` on two real `array<string,
callable>`-typed values (Latte's own `Engine::getFilters()`/
`getFunctions()`) made every `foreach ($merged as $name => ...)`'s
`$name` infer as `int|string`, cascading into real `strtolower($name)`/
`strtoupper($name)`/`renderMethod($name)` false positives in `tools/
phpstan/Latte/ShimClassGenerator.php`; the same root cause separately
cost `tests/Browser/FeedControllerTest.php`'s `[$headers, $body] =
explode(...) + ['', ''];` its own `$body`'s precision, collapsing it to
`mixed`. Fixed to mirror real PHPStan's own `InitializerExprTypeResolver
::getPlusType`'s general (non-constant-array) case: key type is the
union of both operands' iterable key types (via the existing
`array_key_type`/`array_element_type` helpers this file's other array-
consuming call sites already use), value type is the union of both
operands' iterable value types, with `ArrayList`/`NonEmptyArray`
accessories preserved matching real PHPStan's own `isList()->yes() &&
isList()->yes()`/`isIterableAtLeastOnce()->yes()` rules. Scoped Piwigo
diff: 5 findings removed cleanly, zero new false positives. Deliberately
not ported: real PHPStan's own sibling *constant-array* branch (a
`ConstantArrayTypeBuilder`-based per-offset merge preserving an exact
literal `array{...}` shape when both operands are literal shapes) - a
structurally different, genuinely intricate feature (an optional left-
hand key's value is unioned with the right side's only when the key is
*not* actually optional) left as a disclosed, sound-but-imprecise gap:
`array{a: int} + array{b: string}` now falls back to the general
`Array(key, val)` union case rather than a precise merged shape - still
a real, provable supertype, just not maximally precise. Pinned by its
own regression test (`adding_two_literal_shapes_loses_precise_keys_a_
known_disclosed_gap`).

**Two more small stdlib precision gaps, same corpus-diff round.**
`array_keys()`'s own key-type-propagation arm used to gate on `args.len
() == 1` only, so the real, common 2-/3-arg `$search_value`/`$strict`
form fell through to the bare stub's own useless `int[]|string[]`
answer - `UserService::searchCaseUsername`'s own `array_keys($byLower,
$usernameLower, true)[0]` against a declared `string` return type.
`$search_value`/`$strict` only ever narrow *which* keys survive, never
the key type itself (confirmed via `dumpType()`: real PHPStan returns
the identical `list<string>` for the 3-arg form that it does for the
bare 1-arg form) - fixed by widening the same arm's guard to `(1..=3).
contains(&args.len())`. `array_filter()`'s own recognized-callback-shape
set was missing a real, common third case alongside its existing
null-check and `instanceof`-check recognizers: a native `is_string`/
`is_int`/`is_float`/`is_bool`/`is_null` type-check predicate -
`TagService::buildTagList`'s own `array_filter($altNamesEvent->value,
is_string(...))` (the first-class-callable form) lost its own precision
down to `mixed`. Confirmed via `dumpType()` that real PHPStan narrows
the retained element type to exactly that type; new `is_type_check_
filter_callback` recognizes the first-class-callable form (the actual
corpus shape) alongside the same arrow-fn/closure wrapping shapes the
`instanceof` recognizer already handles. Deliberately scoped to the
handful of `is_*` predicates with an exact, unambiguous 1:1 `Type`
mapping already in this project (`is_array`/`is_object`/`is_callable`/
`is_numeric`/`is_scalar`/`is_iterable` all narrow to something wider
than a single `Type` variant - a real, disclosed gap, not attempted
here).

**`usleep()`'s stub over-refined past what real PHPStan actually checks**
(same corpus-diff family, a genuinely different mechanism from the
others above). `usleep()`'s own phpstorm-stubs PHPDoc refines its native
`int` parameter to `int<0,max>`, but real PHPStan's own `resources/
functionMap.php` declares it as plain `int` - and confirmed directly
(`dumpType()`/a real `vendor/bin/phpstan analyse` run at level 10 with
`bleedingEdge.neon`) that neither an unconstrained `int` variable nor a
literal `-5` argument is flagged at all. Root cause, traced directly in
`vendor/phpstan-src`: `NativeFunctionReflectionProvider::
findFunctionReflection` never consults phpstorm-stubs' own PHPDoc for
any function `functionMap.php` already covers (the overwhelming
majority of internal functions) - only PHPStan's own separate, much
smaller internal `stubs/` bundle can refine such a signature further,
and it has no entry for `usleep`. New `php_stubs::patch_usleep`
(mirroring `patch_array_multisort_by_ref`'s own narrow, one-function,
post-generation-patch shape exactly) resets the parameter back to plain
`int`. Real corpus false positive fixed: `RegisterControllerTest::
registerMaturedKey`'s own `usleep((int) ($remainingSeconds *
1_000_000))` inside an `if ($remainingSeconds > 0)` guard - flagged
because this project has no float-range-narrowing mechanism to prove the
cast's own result is non-negative, a gap that's moot once `usleep`'s own
parameter matches what real PHPStan actually checks against.

**Confirmed, not just hypothesized: a whole real resource this project
doesn't ingest at all.** A follow-up corpus finding (`proc_open()`'s own
by-ref `$pipes` out-parameter, below) traced the `usleep` gap's own
"broader, unconfirmed pattern" note above to a real, concrete cause:
`vendor/phpstan-src/stubs/` (note: not `phpstorm-stubs` - a completely
separate, much smaller bundle PHPStan itself maintains, `core.stub`
included) holds refined signatures for exactly this class of internal
function, and `build.rs`'s own `PHPSTAN_SRC_SOURCES` collector
deliberately excludes it (`stubs/` named directly in that exclusion's own
comment, alongside `tests/`/`apigen/`/etc., on the reasoning that this
project only needed the `PHPStan\Type\*`/`PHPStan\Rules\Rule`-style
runtime classes from `vendor/phpstan-src/src`, not this differently-
purposed `stubs/` directory). Ingesting it generally - a real, well-
scoped future item, not attempted here - would need genuine new
machinery beyond just adding a third source array: `core.stub` uses
`@template P of int` / `@param-out array<P, resource> $pipes` to
correlate a by-ref out-parameter's own key type to a *different*
parameter's own literal keys at the call site, a template-binding-
through-`@param-out` shape this project has no consumer for at all.
Two narrow, one-function patches shipped instead this round (`usleep`,
`proc_open` below), matching `array_multisort`'s own established
discipline - not a decision to leave the general gap unaddressed
indefinitely, but a real, disclosed, appropriately-scoped future item
(would need: parsing `.stub` files as a real third stub source with
precedence *over* phpstorm-stubs for overlapping functions; general
template-binding through `@param-out`; and a caller-side "update this
local's type after calling a function whose matched parameter has an
`out_ty`" mechanism, since today's `@param-out` consumer only checks a
function's *own* body against its own declared `out_ty`, never updates a
caller's locals after an external call the way `preg_match`'s own
special-cased `$matches` handling does).

**`proc_open()`'s by-ref `$pipes` output, the corpus finding that traced
the gap above.** Left completely unspecialized, falling to the generic
`Mixed` leniency every unrecognized by-ref output gets. New special case
in `checker/expr.rs`'s `ExprKind::Call` arm (mirroring `preg_match`'s own
by-ref-`$matches`-shape special case immediately above it) sets `$pipes`
to `array<int, resource>` after a `proc_open()` call - real PHPStan's own
`array<P, resource>` (`P` bound from `$descriptor_spec`'s own literal
keys, confirmed via `dumpType()`: `array<1|2, resource>` for a `[1 =>
..., 2 => ...]` spec) is more precise, but a plain `array<int, resource>`
already satisfies the real corpus shape (`ImageStdParamsTest::
spawnBackgroundLockHolder`/`UploadServiceTest`'s identical helper, both
declaring a plain `@return array{0: resource, 1: array<int, resource>}`)
- the template-correlation precision gap is disclosed above, not chased
here.

**`__serialize()`/`__debugInfo()`'s own real, unconditional return-type
override.** Real PHPStan's own reflection layer (`PhpMethodFromParser
NodeReflection`'s constructor, confirmed by direct read) hardcodes these
two magic methods' real return type to `array<mixed, mixed>` (`__debug
Info()`: `array<mixed, mixed>|null`) with *explicit*, not implicit,
`mixed` key/value types - unconditionally, even when the source itself
writes an explicit `: array` hint with no `@return` tag at all, unlike
`MAGIC_METHODS_WITH_IMPLICIT_RETURN_TYPE`'s own existing exemption
(which only ever applies when *no* hint is written at all). An explicit-
mixed value type is exactly what `missingType.iterableValue` exists to
require, so it's unconditionally satisfied for these two - confirmed
directly: an ordinary, non-magic method with the identical bare `:
array` hint and body *is* flagged, `__serialize()` never is. Real corpus
false positive fixed: `DerivativeParams::__serialize(): array { return
[...]; }` (no `@return` tag) flagged `missingType.iterableValue`. Fixed
with a second, narrower magic-method exemption in `checker/missing_
typehint.rs`, scoped to only the iterable-value/generics checks (not the
whole function, unlike the existing implicit-return-type exemption) -
`missingType.return`'s own bare-Mixed check was never actually at risk
either way, since neither method's real type is bare `Type::Mixed`.

**A whole, previously-unexamined class of stub over-refinement: phpstorm-
stubs' own `#[ArrayShape(["key" => "type", ...])]` attribute, never read
by real PHPStan at all.** A different, much broader mechanism than the
`usleep()`/`proc_open()` cases above (both real, narrow phpstorm-stubs-
vs-`functionMap.php` divergences) - this one's own premise ("real PHPStan
reads this attribute directly to recover a precise array shape") was
never actually true, for *any* function. Confirmed by a direct grep of
`vendor/phpstan-src/src/`: zero occurrences of `"ArrayShape"` anywhere in
it. This project's stub-generation pipeline used to graft that
attribute's own shape onto any bare `array`/`array|X` return type
unconditionally - for a function or method carrying an attribute
literally *named* `ArrayShape` at all, own project code included, not
just the vendored stub corpus - across ~85 real occurrences in 37
vendored phpstorm-stubs files. Two real Piwigo corpus false positives
this caused, both traced directly to `functionMap.php` (the real source
`NativeFunctionReflectionProvider` uses for a `functionMap.php`-covered
function's native signature, before phpstorm-stubs' own attributes are
ever reachable) giving the function a bare, unshaped type while the
attribute gave phpstan-rs a precise one: `ImageBackend::
getGraphicsLibrary`'s own `gd_info()['GD Version'] ?? null`
(`nullCoalesce.offset` - `functionMap.php`'s own `gd_info` is bare
`array`) and `ImageExtImagick::write`'s own `isset(pathinfo($path)
['dirname'])` (`isset.offset`).

Fixed by removing the general mechanism entirely (`registry::
function_sig::from_decl` no longer consults this attribute at all,
matching real PHPStan exactly) and restoring real precision only where
`functionMap.php` genuinely gives one - `php_stubs::patch_functionmap_
array_shapes`, transcribed directly from that file's own type text
(several disagree with the attribute in the details - e.g.
`imagecolorsforindex`'s real shape uses `int<0, 255>`/`int<0, 127>`
ranges, not the attribute's plain `int`), for every function this
investigation actually checked (`imagecolorsforindex`, `getdate`,
`date_parse`/`date_parse_from_format`, `date_get_last_errors`,
`timezone_location_get`, `session_get_cookie_params`, `posix_getpwnam`/
`getpwuid`, `localeconv`, `getimagesize`/`getimagesizefromstring`,
`stream_context_get_params`, `stream_get_meta_data`, `error_get_last`,
`proc_get_status`, `time_nanosleep`, `pathinfo`) - the same narrow,
one-function-at-a-time, `functionMap.php`-verified discipline
`patch_usleep`/`patch_debug_backtrace` already established, just
covering every function this pass actually checked rather than the
single one direct corpus evidence first surfaced. Not ported:
`date_sun_info` (`functionMap.php` wraps its shape in a
`__benevolent<...>` marker this project's type-string parser doesn't
model) and `opcache_get_configuration` (a real but very large nested
shape, left bare rather than risk a hand-transcription typo for no real
corpus payoff); every method-only occurrence (`Imagick`/`PDOStatement`/
`Phar`/`SQLite3`, ~22 sites) needs no patch at all - `functionMap.php`
only ever covers top-level functions, and none of those classes have a
real PHPStan-bundled dynamic-return-type extension either, so bare is
already the correct answer.

`pathinfo()` needed a second, genuinely distinct fix on top of the
above: its return type is never actually read from the registry
signature at all at an ordinary 1-/2-arg call site - a dedicated Phase
77 call-site special case (`checker::expr::builtin_calls::
pathinfo_array_shape`, mirroring real PHPStan's own
`PathinfoFunctionDynamicReturnTypeExtension`) unconditionally overrides
it. That real extension's own source (confirmed by direct read) only
marks `dirname` unconditionally present when `$path`'s own type is
provably `non-empty-string` (`!$pathType->isNonEmptyString()->yes()` as
the field's own optionality) - this project's port hardcoded it present
regardless, contradicting its own doc comment's stated intent. Fixed by
threading `$path`'s inferred type through and checking
`string_type_has_accessory(_, AccessoryKind::NonEmptyString)`, matching
real PHPStan exactly: a literal non-empty path string still (correctly)
gets `dirname` flagged as always-present, but a plain, unrefined
`string`-typed variable no longer does.

Verification: scoped Piwigo diff 50 -> 48, exactly the 2 target findings
removed, zero new. Unscoped diff: 83 removed (a broad, genuine
precision win well beyond Piwigo itself - `guzzlehttp/psr7`'s own
`Stream::__construct`, `monolog/monolog`'s `RotatingFileHandler` [both
the same `pathinfo()`-`dirname` shape], `php-standard-library`'s
`ResourceHandle`/`DKIM\Signer`/`TLS\parse_peer_certificate`, `phpunit`'s
own `SystemGarbageCollectorStatusProvider`, and more - all real
`functionMap.php`-divergence false positives this same mechanism was
causing project-wide), 84 added - all but one are either the vendored
phpstorm-stubs files' own bare-`array` declarations now correctly
triggering `missingType.iterableValue` under the unscoped run's
"scan literally everything, including `vendor/`" mode (real, correct
precision - these functions genuinely have no shape per `functionMap.php`
- not a false positive), or summary-line/tooltip noise. The one
substantive addition, `tests/Browser/ImageDerivativeControllerTest.php`'s
own `idcPixel` (`return type mismatch: ... got array{red: mixed, green:
mixed, blue: mixed}`), does **not** appear in the scoped run at all -
confirmed root cause: the unscoped mode's own "scan `vendor/` as regular
project source" quirk causes `imagecolorsforindex()` to be *redeclared*
by literally parsing `vendor/jetbrains/phpstorm-stubs/gd/gd.php` as a
project file, and that redeclaration (which gets none of `patch_
functionmap_array_shapes`'s patching, since that only patches the
built-in `generated_stubs()` cache) merges with the patched built-in
signature via `FunctionSig::merge`'s `union_types`, degrading the merged
shape's own field values to `Mixed`. A pre-existing architectural
quirk of scanning vendor stub source as if it were project code, not a
new regression in the actual check logic - previously invisible only
because both the built-in and the redeclared copy happened to agree
(both driven by the same now-removed attribute), not because the
underlying double-declaration wasn't already happening.

**A `foreach` by-ref value variable never widened its own subject array's
tracked element type.** `StmtKind::Foreach`'s own `by_ref: bool` field was
destructured in `stmt.rs`'s dispatch match and then silently dropped -
never even passed into `check_foreach` at all, so `foreach ($items as
&$item) { $item['x'] = ...; }` was checked identically to a plain by-
value `foreach ($items as $item)`: writing a new field into the by-ref
alias had zero effect on what this project believed `$items`'s own
elements looked like afterward. Real corpus false positive:
`CategoryCatsRenderer::render`'s own `foreach ($infosOfImage as &$info)
{ $info['src_image'] = new SrcImage(...); }`, unconditionally adding
`src_image` to every element - a later `$infosOfImage[$representative
PictureId]['src_image']` read (through a fresh `??`-defaulted dynamic-key
lookup, past the loop) was flagged `shapeAccess.notFound` because
`$infosOfImage`'s own tracked element type never picked the field up.

Fixed by threading `by_ref` through to `check_foreach`, and - once the
loop's body has converged - unioning the by-ref value variable's own
final type into the subject's tracked element type, when the subject is
a bare variable with an already-homogeneous element type (the same scope
`array_element_type` already commits to for binding the loop variable in
the first place). The value's final type has to be captured *from
inside* the last accepted convergence pass, not read back from the
locals `converge_loop_body` itself returns - by the time that returns,
its own "loop may run zero times" merge has already dropped the value
variable entirely (the same treatment it correctly gives any other name
introduced only inside the loop body, confirmed live: reading the value
variable itself right after the loop, unguarded, correctly still reports
`variable.undefined`). Deliberately unions with the pre-loop element
type rather than replacing it outright - sound even if some other,
not-yet-seen caller shape turns out to add the field conditionally
rather than unconditionally.

Verification: scoped Piwigo diff 48 -> 47, the 1 target finding removed
cleanly, zero new. Unscoped diff: 5 removed/4 added, net -1 - the same
Piwigo finding is the only real fix; the other 3 removed/3 added pairs
are the identical pre-existing `return.type`/`argument.type` findings in
`symfony/string`/`rector`/`sabberworm` vendor code, now with a correctly
by-ref-aware (wider, more precise) "got" type in the message - the same
diagnostic at the same line, still flagged either way, neither a new nor
a fixed finding.

**A real generics-substitution gap, fully diagnosed and fixed.** Calling
a generic interface's own templated method through a concrete class that
*itself directly re-declares/implements* that method (not merely
inherits it unedited) never substituted the interface's own `@template`,
even when the concrete class's own `@implements Interface<Concrete>` tag
correctly binds it. Confirmed live - `MessengerFactory::transport`'s own
`(new DoctrineTransportFactory($registry))->createTransport(...)` call:
`DoctrineTransportFactory`'s own `@implements TransportFactoryInterface
<DoctrineTransport>` correctly binds `TransportFactoryInterface`'s own
`@template-covariant TTransport of TransportInterface`/`@return
TTransport` to `DoctrineTransport` in real PHPStan (confirmed via
`dumpType()` against a minimal, structurally-identical repro) - this
project inferred the bare interface-level `TransportInterface` instead,
losing the fact that `DoctrineTransport` also implements
`MessageCountAwareInterface`.

Two real, distinct bugs, both needed for the fix to actually take
effect (confirmed by testing each independently - fixing only one still
left the corpus case broken) - **plus a real regression in the first
attempt at fixing bug 1, caught by the unscoped corpus diff and
corrected in the same round**, all landed together as the final shape:

1. `checker/generics/substitution.rs::substitute_class_templates`'s own
   fast path bailed whenever `owner` (from `lookup_method_with_owner` -
   the class that *lexically* declares the resolved method) had an empty
   `templates` list itself - correct when a method is purely *inherited*
   unedited, but wrong when `owner` directly re-declares an interface
   method (required, since it's implementing one) while the method's own
   effective type still carries a `TemplateParam` scoped to that
   interface, not to `owner`. A `signature_references_any_template`
   cheap pre-check (gating a walk of `owner`'s own ancestors for a real
   concrete binding, so the overwhelmingly common non-generic case stays
   exactly as fast as before) now covers this case too, kept as a
   defense-in-depth safety net for *external call-site* resolution -
   but, per the regression below, it is not where the real fix ended up
   living for the corpus case that motivated all this.
2. Separately, `registry/resolve.rs`'s own docblock-inheritance backfill
   pass never even let a class-scoped ancestor template reach `owner`'s
   signature in the first place - `type_mentions_foreign_scoped_
   template_param`'s own guard unconditionally refused backfilling any
   template not scoped to the method itself, a deliberate, historically-
   corpus-justified fix (this same file's own doc comment cites a real,
   previously-confirmed Doctrine `ClassMetadataFactory`/`getAllMetadata()`
   false positive from backfilling such a template before this whole
   round, when nothing could ever resolve it afterward and it leaked out
   unresolved). Replaced with `template_param_scopes_all_safe_to_
   backfill`: allows the backfill exactly when the class being backfilled
   onto has a real, *non-empty* concrete binding for that scope (computed
   once per class via `class_template_bindings`, not merely "is this
   ancestor name reachable at all" - a real, existing regression test
   caught a laxer version letting a still-genuinely-unresolvable *erased*
   generic reference, no `<Concrete>` argument anywhere, through). A
   bare, erased `implements Container` with no type argument still
   correctly refuses backfill and stays `Mixed`, unchanged.

**The regression, and its fix - the part that actually closes the
corpus case.** The first shipped version resolved the now-permitted
class-scoped template only at fix 1's own *external call-site*
resolution, leaving a raw, still-templated `TemplateParam` sitting in
the backfilled signature itself. That's fine for a call from elsewhere,
but a signature is also read directly to check its *own declaring
class's own method body* (seeding `locals` for its own parameters) - a
path with no "external call site" to resolve anything through at all.
Caught by the *unscoped* corpus diff, not the scoped one (Piwigo has no
matching shape): `Composer\Pcre\PHPStan\InvalidRegexPatternRule`
implementing real PHPStan's own `Rule<TNodeType>` (`@param TNodeType
$node` on the interface's own `processNode()`) left `$node`'s own local
type literally named `"TNodeType"` the instant its own body called
`$this->extractPatterns($node, ...)`. Fixed properly by resolving a
class-scoped ancestor template *eagerly*, at backfill time (new
`Registry::substitute_class_scoped_ancestor_templates` in `registry/
generics.rs` - `registry` cannot depend on `checker`, so this couldn't
reuse fix 1's own substitution helper) - matching real PHPStan's own
mechanism exactly (`ResolvedPhpDocBlock::resolveTemplateTypeInTag`
substitutes through `getActiveTemplateTypeMap()` immediately, at
docblock-merge time, confirmed by direct read), not merely checking a
binding is *available* and deferring substitution to whichever call
path happens to read the signature next.

Verified against 4 repros: the original `MessengerFactory`-shaped case,
a `new X()->method()`-vs-variable-mediated-call pair (both work
identically), the exact historical Doctrine `ClassMetadataFactory` shape
the backfill guard's own doc comment cites, and the `InvalidRegexPattern
Rule`-shaped own-body-self-check case the regression needed. Full test
suite green including the pre-existing regression test for the erased
case. Unscoped corpus diff before the regression fix: 507 removed/58
added; after: 445 removed/43 added, with the specific `TNodeType`-shaped
leak pattern completely gone from the remaining 43 - spot-checked, not
chased to zero (some are genuine new precision; one confirmed sample is
a pre-existing, unrelated Psalm-`@psalm-type`-alias gap this project has
no reason to resolve, newly reached rather than newly broken).

**Subtyping properties via proptest, and two real soundness bugs found
by it** (Phase 19). `src/types/tests/subtyping_properties.rs`: a
bounded-depth `Type` generator (`proptest::prop_recursive`) backing five
properties - reflexivity, `Mixed` is top, `Never` is bottom, union
injection, intersection projection. Transitivity is deliberately *not*
asserted as a blanket property - a real, already-documented non-
transitive chain (`ClassString(Foo) <: String <: ClassString(Bar)` while
`ClassString(Foo) <: ClassString(Bar)` is false, a deliberate false-
negative-avoiding design choice with its own existing test) would make a
blanket property fail on a case this file's own author already reasoned
through. Two real bugs the property suite surfaced, both fixed in the
same pass their own regression landed (fails-then-fixed, not deferred):
`Type::FloatLiteral(f64::NAN)` failed reflexivity (Rust's own `f64:
PartialEq` makes `NAN != NAN`; real PHPStan's `ConstantFloatType::
equals()` explicitly special-cases exactly this); and `is_subtype`'s
existing arm ordering put a generic `(_, Type::Union(members))` arm
*before* the generic `(Type::Intersection(members), _)` arm, so an
`Intersection` containing a `Union` as one of its own members (e.g.
`Intersection([Void, Union([Null, Bool])])`) checked against that same
`Union` wrongly returned `false` instead of the mathematically-required
`true` (the basic intersection-type law `A & B <: B`) - fixed by adding
a new, more specific `(Intersection, Union)` arm ahead of both generic
ones.

**`BenevolentUnionType` marker** (commit `eee86cb`). A new
`AccessoryKind::Benevolent`, used the same `Intersection`-wrapper way as
`ArrayList`/`NonEmptyArray`, mirrors real PHPStan's own
`TypeNodeResolver::resolveArrayTypeNode`: a bare PHPDoc `Type[]`-shorthand
array's key type (and the `+` operator's own same-key-type fallback) is
now benevolent, accepted wherever *any* member matches rather than
requiring *every* member to. Fixed `BlockManager::apply`,
`CategoryAvailableListController::__invoke`, `SpecStructureTest`,
`openapi-generate-input-dto.php`, and (reached via `array_flip()` moving
the marker from key to value position) all 4 `SiteUpdateSubController`
`int|string`-vs-`int`/`list<int>` findings.

**`chronologyDate` list-degradation through an in-place index-rewrite
loop** (commit `ed2ecb7`). `write_index_into`'s dynamic-key-write
handling now accepts a "this write is a provably in-range index rewrite,
not a growth" signal - computed by recognizing the syntactic idiom
`for ($i = 0; $i < count($container); $i++) { $container[$i] = ...; }`
via a new `Checker::in_range_index_writes` fact - so an existing
`ArrayList` marker survives instead of being unconditionally dropped on
every dynamic-key write. Fixes `CalendarRenderer::render` and
`SectionPopulator`, both rewriting a `list<int|string>` parameter in
place via this exact loop shape.

**Degraded spread-array-literal list-safety check** (commit `dafaed4`).
`infer_spread_array_literal_type`'s "degraded" (non-`ArrayShape`) branch
now keys off the spread source's own key type (`array_key_type`, only a
possible *string* key breaks list-ness) instead of whether the source
was already `ArrayList`-marked - real PHP's spread semantics always
renumber a source's *integer* keys sequentially regardless of whether
they were already sequential. Fixes `SearchService::
getQuickSearchResultsNoCache`'s `$params = [...$params, ...$ids]`.

**`reconcile_with_native` native-union-member preservation** (commit
`8a10fc2`). When the native type is itself a `Union`, `reconcile_with_
native` now adds back whichever native alternatives the docblock never
mentioned (a native member with no `kept` subtype covering it survives
verbatim), mirroring real PHPStan's `TypehintHelper::decideType`'s own
per-member keep-or-refine logic instead of only ever filtering the
docblock's own member list down. Fixes `PictureModifyInteractionTest`'s
own call into `Pest\Browser\Api\Concerns\InteractsWithElements::keys()`,
whose `@param array<int, string> $keys` narrows only the `array` half of
a native `array|string` parameter.

**`Registry::is_assignable`'s missing `Type::Conditional` arm** (commit
`da6df83`). Unlike `types::is_subtype` (which already flattened a
`Type::Conditional` sup into `Union([if_true, if_false])`),
`is_assignable` had no arm for it at all, so a `sub` that's itself an
`Intersection` (any `list<T>`/`non-empty-array<T>` value) hit the
generic `(Intersection, _) => any(...)` rule first, decomposing `sub`'s
own members and checking each in isolation - none of which, stripped
from its `Intersection` wrapper, still represents a real list - before
`sup`'s own shape was ever inspected. Added both directions (`sub`/`sup`
being the `Conditional`), placed before every arm that could decompose
the other side first, mirroring `is_subtype`'s own placement exactly.
Confirmed via the full unscoped corpus: 21 real false positives
corrected across the vendored tree (`Composer\Pcre`, Symfony
`EventDispatcher`/`Messenger`, PHP-CS-Fixer, Doctrine,
`php-standard-library`, Nette `Utils`), independent of the narrowing
mechanism below.

**`token_get_all()` stub precision** (commit `9e8f13e`). Added to
`php_stubs.rs`'s existing `patch_functionmap_array_shapes` table
(`list<array{0: int, 1: string, 2: int}|string>`, PHP's own real
documented per-token tuple shape) - the `functionMap.php`-derived stub
gave no per-element shape at all, so a token's own line-number index
(`$tok[2]`) inferred as `mixed` instead of `int`. Fixes `StructuralTest::
findBareCallSites` and, incidentally, two PHP_CodeSniffer findings in the
wider unscoped corpus.

**`strtotime()` literal relative-offset narrowing** (commit `7f1e8cf`).
Mirrors real PHPStan's own `StrtotimeFunctionReturnTypeExtension`
(confirmed by direct extraction from the vendored `phpstan.phar`): a
constant-string first argument matching the bounded "N unit(s) [ago]"
relative-time grammar (`'-1 hour'`, `'-90 days'`, ...) narrows the return
type from the stub's blanket `int|false` to plain `int`, ignoring a
second `$now` argument even when one is passed - matching real PHPStan
exactly, which evaluates only the first argument in isolation. Fixes 3
`AdminShellTest`/`PictureControllerTest` findings and, incidentally, one
in `node_modules/plupload`.

**A dynamic-key write's unconfident key type is now Benevolent-marked**
(commit `23f1243`). `dynamic_write_key_type`'s own fallback for a write
whose key expression can't be confidently proven `int` or `string` now
returns `Type::benevolent_array_key()` (Fix 1's own existing marker, two
rounds ago) instead of a plain `int|string` union - mirrors real
PHPStan's `MixedType`/`UnionType::toArrayKey()` exactly (confirmed
reading `vendor/phpstan-src` directly), which returns a
`BenevolentUnionType` for the identical "I don't know" case.
`Registry::is_assignable`'s own Benevolent leniency (also from Fix 1)
already did the right thing once a value actually carries the marker -
no change needed there. A narrower, safer fix than first drafted:
changing the far more widely-used `Type::unknown_array_key()` itself (83
call sites across 18 files) was the original proposal, but reusing the
already-existing, already-proven `Type::benevolent_array_key()` at this
one canonical fallback site achieves the identical result with a
fraction of the blast radius. Fixes `openapi-generate-input-dto.php`'s
own two-loop dynamic-key chain (`array_keys()` → `foreach` → another
dynamic-key write → another `foreach`) and, confirmed via the full
unscoped corpus (zero new finding locations, 23 disappeared), the
identical false-positive class across Symfony, Doctrine, Rector, and
PHP-CS-Fixer.

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
`class_exists()` narrowing was evaluated and declined - an
unobservable no-op given how this project already represents a bare
`class-string`. `is_a()` narrowing was reevaluated (its own prior
decline cited near-zero corpus incidence, which isn't a valid decline
reason on its own - see PHASE_BACKLOG Phase 167, filed as real,
near-zero-effort work reusing `instanceof`'s own narrowing primitives).
One narrow `isset`/`empty`/nullsafe residual,
left disclosed rather than chased further: a function bundling both a
legacy and modern native-typed variant behind a runtime
`PHP_VERSION_ID` branch (not just `function_exists()`) can still share
one registry slot with the wrong nullability assumption (4 corpus
occurrences) - investigated in depth, by all available static
reasoning it should already resolve correctly, but couldn't be
conclusively confirmed either way without an isolated corpus run.

Five narrowing precision gaps, all corpus-verified: `count()`'s own
comparison narrowing now recognizes the reversed operand order too
(`0 < count($x)`, not just `count($x) > 0`); `!empty($x)` now upgrades
an already-refined `non-empty-string` to the strictly narrower
`non-falsy-string`, not just a bare `string`; a dynamic-key array
write's own key type is now narrowed by a preceding `is_int()`/
`is_string()`-family guard on the key expression itself (real corpus
find: Piwigo's own `CategoryAvailableListController.php`); `array_filter()`'s
no-callback default filtering now narrows a provably-null/falsy *field*
of an `ArrayShape` input the same way it already narrows a homogeneous
`Array(K, V)`'s value type (real corpus false positive fixed:
`tests/Integration/GroupServiceTest.php`); `array_merge($shape,
$plainStringKeyedArray)` stays field-aware (keeps known field names)
instead of degrading to a generic array, scoped to the no-canonical-
int-key case (full `HasOffsetType`-equivalent fidelity stays a
separate, larger prerequisite - see "Known limitations").

A narrow, corpus-justified slice of real PHPStan's own
`ConditionalExpressionHolder`: `$ok = $x instanceof Foo; if ($ok) { ...
}` (or the plain-alias form `$y = $x; if ($y) { ... }`) now correlates
`$x`'s own narrowing to `$ok`'s truthiness, by storing the boolean's own
RHS condition (`Locals::condition_aliases`, the same O(1)-clone/
intersect-on-merge/purge-on-rebind design `guarded_functions` already
established) and replaying it through the existing `apply_narrowing`
recursion at the point of use - no new type-narrowing logic, only
replay. The general mechanism (compound `&&`/`||` decomposition,
`isset()`/array-dim correlation, `match`-arm correlation) stays
declined - see "Known limitations".

`method_exists($recv, 'name')`/`property_exists($recv, 'name')` true
branches now guard `method.notFound`/`property.notFound` the same way
`function_exists()` already guards `function.notFound` (mirroring real
PHPStan's own `MethodExistsTypeSpecifyingExtension`/
`PropertyExistsTypeSpecifyingExtension`, `Locals::guarded_methods`/
`guarded_properties`) - real corpus false positive fixed: Piwigo's own
`ImageBackend::destroy()`, guarded by `method_exists($image,
'destroy')` where `$image`'s declared `ImageInterface` type never
declares `destroy()` itself.

Fixed-point loop-body convergence (`Checker::converge_loop_body`,
mirroring real PHPStan's own bounded `LOOP_SCOPE_ITERATIONS`): a
`While`/`DoWhile`/`For`/`Foreach` body is checked against a candidate
entry state, then re-checked with that entry unioned against the
previous pass's exit whenever the two differ, capped at 3 total passes,
with only the final pass's diagnostics kept (every earlier pass's own
diagnostics are discarded, not just its state). This is the foundation
the unreachable-code/constant-condition family below depends on - see
that entry and **Reverted** for the real soundness bug this fixed.

Same-branch discriminant-variable correlation (Phase 169): a new
`Locals::branch_implied_facts` side-table (`checker.rs`), mirroring
`condition_aliases`'s own O(1)-clone `Rc`-wrapped-value design - when an
`if`'s own `$subject === value`/`$subject == value` true branch causes
some *other* variable to become provably non-null,
`record_branch_implied_facts` (`checker/narrowing.rs`) stores that fact
keyed by `(subject, value)`; `apply_equality_narrowing`'s own replay
hook (`checker/narrowing/assertions.rs`) applies it the next time the
identical discriminant is tested true anywhere else in the program,
with no lexical relationship to the original `if` at all. Unlike
`condition_aliases` (purged the moment its own target is rebound, and
intersected on merge), a fact here is a durable structural fact about
the code at one exact discriminant - `merge_scopes` unions both
branches' entries instead. Scoped to a value comparably concrete enough
to make "the identical comparison" reproducible (an enum case, an int/
string literal), for the equality shape specifically; surviving into a
closure capture is disclosed, not attempted - see PHASE_BACKLOG's
Phase 175.

Same-condition-expression-recurrence narrowing (Phase 170): generalizes
the mechanism above to a second discriminant shape,
`truthiness_discriminant_subject` - a bare variable (the ternary/flag-
reuse idiom, `$flag ? $news : null` reusing the identical `$flag`'s own
earlier `if`) or a single-argument `isset(...)` call, keyed by `Type::
True`/`Type::False` as `discriminant_canonical_key`'s own "value" half
instead of a compared literal. Unlike the equality shape (true-branch
only - "not equal to X" isn't one fixed identity to key facts under),
this one records for *both* branches, since truthy/falsy exhaust a
boolean condition's own outcome space; `replay_truthiness_discriminant_
facts` is the shared replay half, called from the bare-`Variable` arm
and the `isset(...)` arm alike.

Disjunctive-syllogism narrowing (Phase 162): `if ($a || $b) { if ($b)
{} else { /* $a must be true */ } }` - a new `Checker::pending_or_
disjunctions` stack (control-flow-position-scoped, not a `Locals`
field, since it tracks *where* checking currently is, not any
variable's value), pushed with `(l, r)` by `check_if` for the duration
of an outer `Binary(Or, l, r))`'s own true-branch check and popped
after, regardless of nesting depth. Any nested `if` proving one
disjunct false (`Checker::apply_disjunctive_syllogism`, matched via
`disjunctive_syllogism_atom_key` - deliberately restricted to a bare
variable or `(bool) $variable`, never an equality comparison, to avoid
two different-literal comparisons wrongly collapsing to "the same
atom") asserts the other disjunct true through the identical
`apply_narrowing` recursion any other proven-true condition already
uses. The mirror `Binary(And, ..)`-false-branch case stays unattempted
- unlike the `||`/true direction, it isn't a sound implication.

`is_a()` narrowing (Phase 167): `is_a($x, SomeClass::class)`/
`is_a($x, 'SomeClass')` reuses `instanceof`'s own
`narrow_instanceof_true`/`narrow_type_excluding` primitives directly,
resolving the class-name argument via either a string literal or a
`Foo::class` constant (`ExprKind::StaticAccess(name, "class")`,
`checker/narrowing.rs`).

Equality-comparison leaves in the OR/AND-chain narrowing mechanism
(Phase 171) and nullsafe-chain-proven-non-null implies its receiver is
non-null (Phase 172): `or_chain_narrowing_target` gained a
`Binary(Identical | Eq, ..)` leaf (`$parentId === null || $parentId ===
0` now narrows directly to the union, the same way an arbitrary-depth
chain of `is_*()`/`instanceof` leaves already did); the flag-variable-
reuse form of the identical shape (`$flag = $parentId === null ||
$parentId === 0; if (! $flag) { use $parentId; }`) needed a different,
more load-bearing fix - `condition_alias_shape`/`condition_alias_
target` now accept a compound `Binary(And | Or, l, r)` RHS at all, when
both sides share one narrowing target, since `apply_narrowing`'s own
existing sequential-recursion arms (`And` true-branch, `Or`
false-branch) already replay it correctly once the alias can be
stored - no new narrowing logic needed for that half. Phase 172's own
nullsafe-chain implication (`$a?->b() !== null` implies `$a` is
non-null) needed one real new `apply_equality_narrowing` check (since a
nullsafe *method* call has no `narrowing_key` of its own to reach
through the ordinary path) for the direct-testing form, plus a new
`condition_alias_target` arm (targeting the *receiver*'s key, not the
chain's) for the assigned-then-tested form
(`$x = $a?->b(); if ($x !== null) { ... }`).

`foreach` header inline `@var` annotation (Phase 161):
`Checker::inline_var_annotation` already ran generically for every
statement kind, `Foreach` included, via `check_stmt`'s own preamble -
the real gap was `check_foreach` itself unconditionally rebinding the
loop variable to the ordinarily-inferred element type on every
convergence pass, overwriting whatever the preamble had set. Threaded
the resolved annotation through as an override instead. The bare
(nameless) `@var Type` form needed one more piece:
`statement_defined_variable_names` gained a `StmtKind::Foreach` arm
resolving to the loop's own value variable, but only when there's no
separate key variable too (`foreach ($x as $k => $v)` binds two names,
the same ambiguity every other unresolvable shape already declines to
guess between).

Phases 169/170/171/175's own real corpus target sites - despite being
recorded as built - were still flagged in a later corpus re-run, each
for a reason narrower than its own filing text anticipated (commits
`2403e36`/`a5224e6`/`f7047fe`): `condition_alias_target` required
every leaf of a compound `&&`/`||` to share one single subject and had
no arm for a `Unary(Not, ...)` leaf at all, so `PasswordController::
processVerificationCode`'s own two-subject, negated-leaf condition
never got stored as an alias in the first place - generalized to
`condition_alias_targets` (collects every distinct subject, `Vec`
instead of one `String`) plus a `Not` arm recursing into the negated
operand. `record_branch_implied_facts` only ever keyed a branch's own
implied facts under the *tested condition itself*, never a *proxy*
variable that condition merely caused to become a literal `true`/
`false` or a concrete `Object` inside the same branch
(`AlbumsPageRenderer::render`'s own flag re-tested bare;
`UploadService::addUploadedFile`'s own connection object re-tested via
`instanceof`, needing a new `replay_object_discriminant_facts`
alongside the existing bare-truthiness replay) - generalized to treat
either as an equally valid subject for the same implied-facts bundle.
`RateService::rate`'s own shape needed a third, unrelated fix: a plain
`Closure`'s `fresh_scope()` starts `branch_implied_facts` empty (unlike
an `ArrowFn`'s full `locals.clone()`), so a fact recorded outside a
`use (...)`-capturing closure never survived into its body at all -
now carried across at closure-construction time, filtered to only the
facts naming an actually-captured variable (an unfiltered carry-over
would otherwise mask a real `variable.undefined` on a genuinely-
uncaptured same-named reference inside the closure).

**By-ref closure-capture callback convergence** (commit `6ecd3f6`). A
closure literal's body (both `checker/expr.rs`'s own `Closure` arm and
`closure_context.rs`'s near-duplicate) now routes through `Checker::
converge_loop_body`/`LoopZeroIterations`, the same zero-to-many-run,
converge-to-fixed-point treatment an ordinary loop body already gets,
instead of a single unconditional pass - applies uniformly to every
closure literal (`preg_replace_callback`/`array_walk`/`usort`/etc. all
genuinely call their closure zero-to-many times), not gated to one
builtin. Fixes `SearchService::positionalCondition`'s own
`arrayValues.list` finding (a by-ref `use (&$values)` capture mutated
across however many regex matches `preg_replace_callback` finds). One
honestly-documented residual gap: a by-ref mutation immediately followed
by `return` inside one branch isn't caught (`check_if`'s own branch-
discarding for post-if code still hides it from the convergence loop's
own by-ref read) - verified via `dumpType()` that real PHPStan gives the
identical answer for that exact shape too.

**If-body-set flag, OR-chain condition** (commit `5f7bebf`). A new
`record_if_body_flag_alias` correlates a flag set as the *sole*
statement of an `if` body with that `if`'s own (negated) condition,
extending the existing `condition_aliases` replay machinery to a new
trigger site (an `if`-body side effect, not just a direct assignment
expression). Fixes `ImageDerivativeController`'s own `$derivative_mtime
= @filemtime(...); if ($derivative_mtime === false or ...) {
$need_generate = true; }`, later `if (! $need_generate) { ...
gmdate(..., $derivative_mtime) ... }` - reaching `!$need_generate`
provably means every disjunct (including `$derivative_mtime === false`)
is false, now recorded.

**Whole-container per-field facts from a by-ref `foreach`** (commit
`2d2567e`). `$row['field'] = X` inside `foreach ($rows as $k => &$row)`
writes into `$rows`'s own storage, but only a single flat per-container
element type was ever tracked. A new `Locals` field,
`container_field_facts`, records "every element's own field X is type
T" (a fact about the container's whole shape, not one key) - harvested
from `bind_assignment_target`'s own pre-existing `narrowing_key`-format
entries at the by-ref loop's exit (a pure harvest of already-computed
state, not a second analysis) and eagerly propagated onto any later
variable bound from the same container via a dynamic-keyed read (a
`foreach` value, or a plain `$x = $container[$dyn];`) - the two
pre-existing `narrowing_key`-consulting Index-read fast paths pick it up
with no further changes. `merge_scopes` intersects a container's own
entry across a branch join (a fact invalidated on one branch must not
survive). Fixes `ConfigurationSubController::processSizes`'s three
`must_square`/`must_enable`/`enabled` findings - `$pderivatives` is a
*uniform* `array<string, array<string, ...>>`, not an `ArrayShape`, so
there was no per-field slot to narrow at all before this.

**Branch-gated array-accumulator tracking** (commit `da6df83`). A new
`Locals` field, `discriminant_array_facts`, separately tracks each
`if`/`else` branch's own array-push accumulator (keyed by a bare-
boolean-variable condition, pushed/popped via a new `Checker::
active_discriminant` around the branch) - `write_index_into`'s generic
`Type::Union` handling was applying an append write to *every* member of
an already-diverged union uniformly, including a member produced by a
*different* loop iteration's *other* branch (confirmed by a minimal
repro: `if ($f) { $x[] = 'a'; } else { $x[] = new Obj(); }` inside a
loop produces a genuinely-impossible `list<string|Obj>` member after
only two convergence passes - not a `merge_scopes` flattening bug,
`union_types` never merges two different `Array`/`Intersection` element
types on its own). Consulted only as a fallback when a `return` against
a `Type::Conditional` declared type would otherwise fail: reconstructing
`Union([true-slot, false-slot])` and re-checking passes through the
`is_assignable` fix above with no per-call-site discriminant resolution
needed. Fixes `NotificationByMailSender::sendMailNotifications`'s own
`return $returnList;` (`$returnList` pushed a `string` under
`$isActionSend` and a `UserMailNotification` otherwise, checked against
a conditional return type keyed on `$action === 'send'`).

**`record_branch_implied_facts`'s missing enum-case proxy discriminant,
plus a general self-referential-implication guard** (commit `21d03c5`).
The proxy-discriminant scan only ever recognized `Type::True`/`Type::False`
and `Type::Object` as a subject a fact could be recorded under - a
variable that became a concrete `Type::EnumCase` specifically within a
branch (`$valid_key = ConnectedWith::ApiKey;`) was never recorded at
all. Adding it exposed a real, pre-existing soundness gap the mechanism
already had for the other two proxy kinds: a candidate "implied"
variable whose own name is directly referenced by the branch's own
condition (a real corpus regression - `League\Uri\QueryString::
composeRecursive`'s `$value`, `instanceof`-narrowed by that exact `if`)
must never be recorded as implied by a *different*, co-occurring
discriminant - replaying that stale fact on a later loop pass overwrote
`$value`'s freshly re-narrowed `instanceof` type with a leftover value
from an earlier pass. Fixed generally (a `cond_references_variable` AST
walk over the branch's own `cond`), not just for the enum-case trigger
that surfaced it. Fixes `AuthService::authKeyLogin`'s own
`$valid_key`/`$secret_key` correlation.

**Dynamic-key array writes resolved when the key is known by type, not
just AST syntax** (commit `b3d6e7c`). Two complementary mechanisms, both
needed for the real target: (1) `write_index_into`'s `ArrayShape` arm
only resolved a literal key syntactically (`literal_string_key`, an
AST-token check) - a non-literal index expression whose own *inferred*
type is already a precise `StringLiteral`/`IntLiteral` (`$key = 'id';
$row[$key] = ...;`) fell through to the generic degrade; (2) even with
(1), a container already widened past `ArrayShape` (no docblock on its
own producing method, e.g. `CategoryAvailableListRow::toArray(): array`)
has no per-field precision left to update at all -
`bind_assignment_target` now also writes each resolved literal key's
own `narrowing_key`-format synthetic entry directly, independent of
`write_index_into`'s own container-type-based update. Fixes
`CategoryAvailableListController::__invoke` and, incidentally, 3 more
findings in the wider unscoped corpus (Latte `Tracer`, PHP-CS-Fixer x2).

## Rule catalog (checker)

Symbol existence (classes/functions/use-imports/typehints, catch
clauses, `extends`/`implements`/`instanceof`/trait-use targets), plus a
`use`-imported build-tool-mangled-prefix check (`class.prefixed`).
`use function`'s own `function.nameCase` check is done too, opt-in
(`Registry::check_function_name_case`, mirroring real PHPStan's own
opt-in `checkFunctionNameCase` parameter, off at every level).
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
`.neverWritten`/`.onlyRead`), plus PHASE_BACKLOG Phase 98's AB2: a PHP
8.4 asymmetric-visibility property (`public`/`protected private(set)`)
gets its own, narrower dead-write check reusing the same `property.
neverWritten` identifier - real PHPStan's own `UnusedPrivate*Rule`
gates on `visibility == Private` and so structurally can't see
`write_visibility` at all. Only the write side is checked (a
same-class-only scan can soundly conclude "never written from inside
the declaring class" the exact same way it does for a fully-private
property), never "unused"/"never read" - the read side is genuinely
reachable from outside the class, so those two would need a real
whole-project scan this check doesn't attempt. Hooked properties are
skipped (a `set` hook's own body is a different write mechanism, not
covered by the same-class `usage.writes` tracking this reuses).
Duplicate-declaration scans within a class/trait/enum body, a
same-file case-insensitive class-vs-`use`-alias collision, and
duplicate top-level *function* declarations (a real, whole-project,
cross-file scan - distinguished from phpstorm-stubs' own established
legitimate multi-declaration/real-overload pattern rather than
conflicting with it). Override-
compatibility (visibility narrowing, missing/extra/optional/by-ref/
variadic parameter mismatches, `#[Override]`'s unconditional-fatal-error
half, running uniformly for a class's own methods and a trait-supplied
one), including the interface-vs-trait-vs-extends ancestor-lookup
priority fix (matching real PHPStan's `MethodPrototypeFinder` exactly,
including its own visibility-check-skip and self-type-resolution
nuances for a trait-sourced prototype) and property `#[Override]`
attribute checking. PHASE_BACKLOG Phase 101's E4: an overriding method
whose own declared casing mismatches its parent/interface's
(`method.nameCase`) - `Registry::find_case_mismatched_ancestor_method`
reads the same-name-different-casing signal straight back out of
`method_owner_by_class`'s existing exact-spelling keys (two case-
insensitively-equal names both survive there as separate entries),
with no separate ancestor walk needed. Property hooks are covered too:
`PropertyHookBody::Abstract` (an AST variant distinguishing "no hook"
from "an abstract hook with no body") plus `property.notReadable`/
`.notWritable`/`.abstractOverridingNonAbstractHook`. PHP 8.4 `final`/
`abstract` property modifiers, including on a promoted constructor
property (`public final int $x` in a constructor parameter list is
real, valid PHP, confirmed live; only `abstract` is genuinely impossible
there) - `property.parentPropertyFinal`. `method.nonStatic`/
`method.static` (an override must match its ancestor's own static-ness
exactly, either direction) and the general, non-`__construct` case of
`method.abstractOverridingNonAbstract`, both `nonIgnorable` matching
real PHPStan's own `OverridingMethodRule` exactly - `ClassInfo::
static_methods` mirrors `abstract_methods`/`final_methods`, with the
same trait-flattening propagation `abstract_methods` already has (a real
bug found and fixed along the way: `static_methods` wasn't trait-
flattening-aware at all, a real corpus false positive on `Symfony\
Component\Cache\Adapter\RedisAdapter`'s trait-supplied
`createConnection()`). Both
the real-keyword and docblock-only (`@final`/`@phpstan-final`, a purely
static-analysis convention real PHP itself never enforces) `final`-
ancestor checks are done, for both methods and properties
(`method.parentMethodFinal`/`.parentMethodFinalByPhpDoc`,
`property.parentPropertyFinal`/`.parentPropertyFinalByPhpDoc` -
`ClassInfo::final_methods_by_phpdoc`/`final_properties_by_phpdoc`,
consulted alongside the native-keyword sets one ancestor at a time
nearest-first, real keyword winning when a single ancestor somehow has
both, matching real PHPStan's own if/elseif order). Native-type
property-override variance is done too (`property.nativeType`/
`.missingNativeType`/`.extraNativeType`, `PropertySig::ty_native`/
`has_native_type` mirroring `ParamSig`'s identical split, checked via
`resolve_pseudo` for `self`/`static`-typed properties and bidirectional
`is_assignable` rather than raw `Type` `PartialEq`, which is union-
order-sensitive). The `__construct`-specific branch of
`abstractOverridingNonAbstract` (triggered only when no normal prototype
resolves at all) stays unbuilt - a distinct code path, not a narrowing
of the general case. Nine declaration-site modifier-consistency checks
plus the class-constant `final`/native-type-modifier trio. Enum sanity
and generic-bound checking wired to enums.

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
transitive slice above. One disclosed limitation inherited from real
PHPStan's own actual algorithm, not a bug of this project's making:
neither project counts constructor-promoted-property initialization as
an impure point, so a `parent::__construct(...)` call chaining into a
promoted-only ancestor constructor can look like a discarded no-op even
though it genuinely initializes state.

Phase 41 follow-up: for a declaration real PHPStan considers pure
(`@phpstan-pure`/`@pure`-tagged), `check_purity_impure_points`
(`checker/noop.rs`) reports every real occurrence its body actually
contains - `impure.<id>`/`possiblyImpure.<id>` (`certain`-vs-`maybe`,
`registry::purity::collect_impure_points`, a second, non-short-
circuiting walk mirroring `stmt_disqualifies`/`expr_disqualifies`'s own
match arms one call site at a time, deliberately not touching those two
functions themselves - regression risk to the already-working fixed
point), plus `pure{Function,Method}.parameterByRef` and `pure{Function,
Method}.void`. The gate is tag-only (`tagged_pure`), not "tagged OR
classifier-proven" - gating on `is_transitively_pure` too made
`pure{Function,Method}.void` fire for every ordinary, untagged,
provably-pure void declaration (extremely common), confirmed by 287
broken tests before landing the narrower gate. A corpus-confirmed
precision gap, left as a disclosed follow-up rather than fixed here:
`possiblyImpure.functionCall`/`.methodCall`/`.new` fire for any call
target this walk's own `resolved` check can't prove - which, reusing
`resolve_static_call`/`resolve_this_method_call`/`resolve_new` above
as-is, only recognizes `self::`/`static::`/`parent::` and `$this->`
receivers, never a literal class name (`Color::ansi256(...)`) or an
arbitrary object receiver, confirmed against real PHPStan's own
`StaticCallHandler.php`/`SimpleImpurePoint::createFromVariant` (which
resolves a literal-class-name static call, and any receiver's method
call, the same way, only emitting an `ImpurePoint` when the *resolved*
target's own reflection says it has side effects). Not a regression -
the same resolution narrowness already silently capped
`is_transitively_pure` before this diagnostic existed; corpus-confirmed
via `vendor/php-standard-library` (heavy `@pure` tag usage - 335 files
flagged out of 467 using the tag, most from this exact gap plus a
separate, unrelated `namespace\foo()`-relative call-name-resolution
bug also confirmed in the corpus). `@phpstan-immutable` (`docblock.rs`)
is recognized (previously in `KNOWN_PHPSTAN_TAGS` but silently
discarded, no `apply_tag` arm at all) and treated identically to
`@phpstan-readonly` (`ClassReflection::isImmutable()`'s own OR),
`Registry::class_is_immutable` walking `extends` only at query time
(not baked into `is_readonly`/`is_immutable` at index time, avoiding an
ancestor-not-yet-indexed ordering hazard) - `property_assignment.rs`'s
existing readonly-outside-constructor gate now also fires for a
property of an immutable class (or subclass).

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
syntactically-typed slice of method calls). Checked-exception/throw-type
model: `@throws`-vs-`Throwable` validity, `throws.void`, dead-catch
detection, `finally`'s own exit-point-overwrite detection, transitive
throw-point collection across the call graph (`Checker::current_
function_throw_points`/`current_function_has_unannotated_call`, recorded
at every real `throw` and every already-resolved-signature call choke
point, with a curated, disclosed subset of per-callee dynamic-throw-type
extensions - `intdiv`, `array_combine`, `assert`, `json_encode`/
`json_decode`, `filter_var`, `new DateInterval(...)`, `DateTime[Immutable]
::sub()`, the four `Reflection*` constructors; `version_compare()` and
the DOM/SimpleXML/DateTime-relative-string extensions excluded, each for
its own disclosed reason), `TooWideFunctionThrowTypeRule`/
`TooWideMethodThrowTypeRule` (`throws.unusedType` - the method version
gated on visibility/finality/first-declaration, `Registry::check_too_
wide_throw_type_in_protected_and_public_methods`, mirroring real
PHPStan's own default-off `checkTooWideThrowTypesInProtectedAndPublicMethods`,
an ordinary overridable method's unused `@throws` tag staying silent by
default), the same family ported onto property hooks (`InvalidThrowsPhpDoc
ValueRule`/`ThrowsVoidPropertyHookWithExplicitThrowPointRule`/
`TooWidePropertyHookThrowTypeRule`/`MissingCheckedExceptionInPropertyHook
ThrowsRule`, needing a hook's own `@throws` doc comment and `final`
modifier threaded through the parser at all - `ast::PropertyHooks::
get_doc`/`set_doc`/`get_final`/`set_final`), `MethodThrowTypeCovarianceRule`
(opt-in, `Registry::check_method_throw_type_covariance` - a pure
declaration-vs-declaration comparison against the immediate parent
class/every transitive interface/abstract trait declaration, real
PHPStan's own default `exceptions.implicitThrows: true` hardcoded rather
than separately configurable), and an opt-in "missing checked exception"
family with no real PHPStan equivalent at the function/method level
(`throws.missing`, `Registry::check_missing_checked_exceptions`, default
off given the real corpus's documented inconsistent `@throws` tagging -
consumes only a declaration's own *explicit* throw points, never a
called function's).
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
no-op conditions (including `arrayFilter.same`/`arrayFilter.alwaysEmpty`,
the element-type-based sub-checks - unconditional, matching real
PHPStan's own unconditional level-5 default; previously shipped opt-in
after two corpus false positives, both since root-caused and fixed -
see Phase 68), `array_multisort()`'s stub fix,
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
`compact()`/`get_defined_vars()` (PHASE_BACKLOG Phase 159) - a real
`Type::ArrayShape` built directly from `Locals`, the same whole-scope
variable-type map every other purpose here already reads, not a new
tracking mechanism: `compact()` resolves each string-literal argument
(recursively, for a nested array-literal argument - real PHP's own
`compact(['a', ['b']])` form since 7.3) via `locals.get(name)`, silently
omitting a name with no matching local (matches real PHP's own "skip
anything undefined" behavior); a spread argument, or any argument that
isn't a literal name (or a literal-nested array of them), degrades the
*whole* call back to the plain stub-declared `array` rather than
guessing a partial shape. `get_defined_vars()` needed no new tracking at
all - every currently-tracked `Locals` entry, filtered through the same
`is_real_variable_key` predicate that already excludes `narrowing_key`'s
own synthetic property-chain/array-index keys elsewhere. `array_flip()`
also gained a precise, field-by-field flip specifically for a real
`Type::ArrayShape` input (the shape `compact()` now produces, among
others) - each literal-int/string-valued field becomes a real named
field in the result, keyed by that literal and valued at the field's own
original key (`types::canonical_int_key` recovering an int-keyed field's
real numeric key from its shape-internal string name); a field whose
value isn't a literal (or the shape's own unsealed `"..."` tail) can't
name a real result field at all, so it folds into the result's own
`"..."` tail instead of discarding precision for every other field too.
A plain, non-shape `Type::Array` input keeps the prior, already-correct
`array_key_type`/`array_element_type`-based flip unchanged.
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
`bool`; zero arguments is `string|false`). `new DateTime()`/`new
DateTimeImmutable()`'s own constructor-string validation (Phase 78's
last remaining piece, previously declined as needing "real PHP/ICU
runtime simulation") is done too: `src/php_date_parser.rs` hand-rolls
the format families PHP's own `ext/date/lib/parse_date.re` grammar
accepts most often (ISO 8601 absolute dates, `@timestamp`, the unified
relative-delta family with a full unit table including `fortnight`/
`weekday`, `next`/`last <weekday|unit>`, `first`/`last day of <month>`,
and a handful of bare keywords), each shape cross-checked directly
against a real local PHP 8.5 binary since `parse_date.re` itself isn't
vendored anywhere in this repo. `strtotime()`'s own existing narrowing
(this same grammar's relative-delta fragment) now calls into this one
shared module instead of keeping its own separate, silently-divergent
copy. Deliberately conservative: an unrecognized shape (natural-language
month names, ordinal weekdays, and the rest of the grammar's long tail)
stays silent rather than risk a false positive - only a recognized
shape whose own fields are provably out of range (`"2024-13-01"`'s
month, `"2024-01-15T25:00:00"`'s hour) is ever flagged.
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

**Real contravariant-parameter/covariant-return subtyping between two
different `Closure(...)`/`callable(...)` shapes** (PHASE_BACKLOG Phase
160). `types::subtype::is_subtype`'s own `Callable(Some)` vs.
`Callable(Some)` arm used to be an unconditional `true` between any two
shapes at all, real contravariance not attempted; replaced with
`callable_sig_is_subtype`, reusing the identical variance-composition
pattern `registry/assignability.rs` already has for generic template
arguments (parameters contravariant, return type covariant is that
pattern's own textbook motivating example, not a new concept). Walks
every parameter position either signature has anything to say about
(`effective_callable_param_at`'s own variadic-tail absorption on both
sides), so a real arity mismatch - `a` missing a position `b`'s own
callers could supply, or `a` requiring an extra position `b`'s callers
never would - is a genuine, confident rejection rather than a "don't
know, stay lenient" retreat; an extra optional/variadic parameter on
`a` beyond what `b` declares is still fine. The two bare-marker arms
(`Callable(Some)` vs. `Callable(None)`, either direction) are unchanged
- there's no shape to compare against there at all.

**Dynamic static-call target resolution (`$obj::method()`)** (PHASE_BACKLOG
Phase 158). `ExprKind::StaticCall`'s first field is now a real
`StaticTarget` enum (`Name(String, Span)`/`Expr(Box<Expr>)`, the
identical shape `InstanceofTarget` already established for the same
"literal name or dynamic expression" receiver problem) instead of a
plain `String` that discarded a non-literal receiver into a permanent
`"?"` sentinel at parse time. `resolve_static_target_class`/
`infer_static_target_class` (`checker/resolve.rs`) resolve a `Name`
exactly as before; for an `Expr` receiver, the expression is checked in
full (so an undefined variable inside it is still caught, matching
every other checked sub-expression) and resolved via its own real
inferred type - a known `Type::Object(fqn)` gets checked against `fqn`
exactly like a literal class name already is (existence, visibility,
arg-type checking, return-type inference all now work through
`$obj::method()`, not just `Foo::method()`); an unresolvable type
(`Mixed`, an unhinted parameter, ...) stays the same silent no-op as
before. `StaticTarget::as_name()` is the drop-in `Option<&str>`
replacement every one of the ~25 real fan-out sites across ~20 files
that only ever needed the old sentinel string uses (`unwrap_or("?")`
reproduces that exact prior behavior byte-for-byte); `ast_walk.rs`'s
`expr_children`/`any_expr_child` now also walk into a dynamic
receiver's own expression (mirroring `Instanceof`'s identical `Expr`
target), so every generic tree-walking consumer (`deps.rs`'s name
collection, `use_sites.rs`'s reference tracking) picks up names inside
it for free rather than needing its own special case.
`lsp/navigation.rs`'s own hover/go-to-definition gained real coverage
for this shape too - a dynamic receiver's own member name now resolves
via the identical `Resolved::Instance` Stage-B type-probe `$x->method()`
already uses (agnostic to `->` vs. `::`, since a real PHP static method
is still just a method looked up on the receiver's own resolved class
either way).

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
(`types::shape_key_type`, mirroring real PHPStan's own `ConstantArrayType::
getIterableKeyType()`, each int-canonical key contributing its own real
`Type::IntLiteral` rather than a widened `Type::Int`) is wired into both
`registry::is_assignable`'s `Array`/`ArrayShape` arms and `types::is_subtype`'s
own - a real, previously-unsound gap this closes: `array{name: string}`
(all-string-keyed) no longer silently satisfies `array<int, string>`.

**Opt-in array-key cast warnings** (PHASE_BACKLOG Phase 103's Z1,
`array.keyCast`/`offsetAccess.keyCast`, mirroring real PHPStan's
`LiteralArrayKeyCastRule`/`ArrayDimCastRule`). `array_key_cast_
description` (`checker/expr/array_ops.rs`) mirrors `ConstantScalarType::
toArrayKey()`: a float truncates to int, `true`/`false` become `1`/`0`,
`null` becomes `''`, and a canonical-integer string
(`canonical_int_key`, already built for `is_canonical_int_key`) becomes
that int - checked at both the same call sites `check_array_item_key_
type`/`check_dim_fetch_key_type` (Phase 50) already instrument. Gated
behind `Registry::check_array_key_cast`, off by default: real PHPStan
itself only ever registers these two rules in its own private
issue-bot config (`Rules/Playground/`), never an installable package/
level/strict-rules extension, so there's no real-world signal on how
noisy this is on an ordinary codebase. A bare float literal expression
infers to `Type::Float` everywhere else in this checker (no
`FloatLiteral` produced from parsing a literal directly), so the float
case is checked directly off the AST rather than the inferred type,
the one shape needing that.

**Opt-in `constructor.missingParentCall`** (PHASE_BACKLOG Phase 101's
E3, mirroring real `phpstan-strict-rules`' `RequireParentConstructCall
Rule`). `Checker::check_parent_constructor_call` (`checker/oop.rs`)
flags a concrete class's own `__construct()` that never calls `parent::
__construct()` anywhere in its body (recursing into nested `if`/`try`/
`switch`/loop blocks the same way other whole-body scans here do), when
`lookup_ancestor_method_with_owner` finds a real, non-abstract,
non-private ancestor constructor to call. Gated behind `Registry::
check_parent_constructor_call`, off by default: real PHPStan itself
ships this only via the separate, opt-in `phpstan/phpstan-strict-rules`
package, and a legitimate constructor genuinely choosing not to call an
otherwise-real parent one (composition-over-inheritance, a parent
constructor that's a no-op by design) is real enough to keep it opt-in
here too. Feeds Phase 111's I4 codemod once built.

**Closure `use (...)` capture checks** (Phase 17). A by-value `use
($typo)` capture of a variable never assigned anywhere in the outer
scope now runs through the same undefined-variable machinery a direct
read already gets (`variable.undefined`), at both real call sites
(`ExprKind::Closure` and `check_closure_literal_with_param_overrides`).
The full `InvalidLexicalVariablesInClosureUseRule` family lands in the
same two loops: `use ($this)` (`closure.useThis`), `use ($_SERVER)`/any
superglobal (`closure.useSuperGlobal`), and a capture name colliding
with one of the closure's own parameters (`closure.useDuplicate`), all
non-ignorable per real PHPStan's own `#[RegisteredRule(level: 0)]`.

**PHPStan-API vendor-directory check, plus two adjacent bugs** (Phase
40). `is_phpstan_code` (relocated `is_phpstan_namespace` into
`registry.rs`, plus a new `Registry::phpstan_class_files` map populated
at the two existing file-load call sites) ports real `ApiRuleHelper::
isPhpStanCode`'s second half: a `PHPStan\*`-namespaced *caller* now also
needs its target's declaring file to actually live under a `vendor/`
directory before a same-namespace API-usage diagnostic fires - wired
into the three consuming `report_*` helpers, not into `resolve_if_
phpstan` itself (whose target-only namespace pre-filter also feeds a
tenth call site, `check_instanceof_deprecated`, that real PHPStan runs
unconditionally off a hardcoded attribute table with no `isPhpStanCode`
involvement at all - gating it the same way would have been a new false
negative). Two more real, pre-existing bugs found in the same file and
fixed in the same pass: a bare `namespace PHPStan;` (no trailing
backslash) failed `is_phpstan_namespace`'s own length guard, unlike real
PHPStan's own special-cased exact match; and `ExprKind::StaticCall`'s
own pre-filter gated on the *literally-written* class name's namespace
before ever reaching the real declaring-class check one line later, so
`MyNonPhpStanSubclass::inheritedPhpStanMethod()` (a non-`PHPStan\*`-
namespaced subclass of a real PHPStan API class) never even reached the
existing member check - fixed by resolving the real declaring class
first and gating on *that*, matching real `ApiStaticCallRule`'s own
single-check shape.

**Too-wide return/property types, by-ref parameter checks, `@param-out`**
(Phase 45). New `@param-out Type $name` PHPDoc tag support
(`ParamSig::out_ty`) unlocks both halves of this item. 45A: `Checker::
current_function_return_points`/`current_function_saw_explicit_return`
(mirroring the existing `TooWideFunctionThrowTypeRule` port's own
choke points) feed `check_too_wide_return_type`
(`TooWideFunctionReturnTypehintRule`/`...MethodReturnTypehintRule`,
`return.unusedType`, level 4, opt-in for protected/public non-final
methods). Its property-assignment sibling
(`checker/property_type_too_wide.rs`'s `check_too_wide_property_types`,
`property.unusedType`/`staticProperty.unusedType`) was actually built
in a later session (Phase 166/174 below) - this paragraph's own prior
text claimed it was already done here, which a `property.unusedType`/
`current_class_property_assign_types` grep turned up zero hits for
anywhere in the codebase, including tests; corrected rather than left
stale once found. A real, load-bearing correction found via the test suite before this even
reached corpus verification: real PHPStan's own `TooWideTypeCheck::
narrowType` only ever acts when the declared type is literally a
multi-member `UnionType` - a single, non-Union declared type (a bare
`list<int>`, an `Intersection`, ...) must bail unchanged, not be treated
as "one member to check," or it produces a redundant second diagnostic
duplicating what `return.type` already reports. 45B: `Checker::current_
function_by_ref_params` feeds `check_param_out_assignment`/`check_by_
ref_value` (`parameterByRef.type`, level 3) on a direct `$byRefParam =
expr;`/compound-assign, checked against `@param-out`'s widened type when
present or the native declared type otherwise. 45C
(`ParameterOutExecutionEndTypeRule`, `parameterByRef.executionEnd`) and
45D (`TooWideParameterOutTypeCheck`/`TooWideMethodParameterOutTypeRule`,
`parameterByRef.tooWide`, opt-in for protected/public non-final methods
via `Registry::check_too_wide_by_ref_type_in_protected_and_public_methods`,
threaded through the same 8-file plumbing the return-type flag uses)
are now done too, built on a genuine per-exit-point accumulator
(`Checker::current_function_by_ref_exit_snapshots`, snapshotting every
by-ref parameter's own `locals`-tracked type at each real
`return`/falls-off-the-end point) rather than a flat "ever assigned
anywhere" union - real `TooWideParameterOutTypeCheck` merges the scope
at each real exit point into one final scope, which a flat accumulator
systematically over-widens against (missing real too-wide findings
whenever a variable is reassigned to a different type before its real
exit). 45C only ever fires for a parameter with a genuine `@param-out`
tag, matching `getOutType() === null`'s own bail in the real rule - the
tag-independent 45B keeps its native-type fallback unchanged. Both also
close a real, live gap found in the same pass: a closure/arrow-fn body
capturing an outer by-ref parameter by reference/by value and
reassigning it inside was being checked against the *outer* function's
own by-ref contract - fixed by resetting/restoring the by-ref state
around both `ExprKind::Closure`'s and `ExprKind::ArrowFn`'s own body
check, matching real PHPStan's `isInAnonymousFunction()` exemption.
Destructuring-target (`[$a, $byRef] = foo()`, any nesting depth,
reusing `bind_assignment_target`'s own per-position/per-key element-type
resolution) and `foreach ($x as &$byRef)`-rebind coverage (the
per-iteration bind inside `converge_loop_body`'s own closure,
`checker/stmt/loops.rs`) for the 45B/45C/45D checks are done too. Two
real, severe bugs the Piwigo corpus-verification pass itself found and fixed
in the shared type-inference machinery 45A's own comparison depends on,
neither specific to this item's own new code: (1) `checker::resolve::
resolve_pseudo` never recursed into a generic class reference's own
type arguments (`Type::Object(name, args)`) or a PHPDoc conditional
type's nested positions, so `self`/`static`/`parent` written *inside* a
generic argument (`SortEntry<self, string>`) never got resolved - a
real, pre-existing bug (confirmed present in this project before this
session), not something 45A introduced, just the first consumer to
depend on the comparison being correct enough to expose it; (2)
`check_too_wide_return_type`'s own actual-vs-declared comparison used a
plain boolean `is_assignable`, where real PHPStan's own algorithm is
trinary (`isSuperTypeOf(...)->no()` - only a *definite* no flags a
member; `MixedType::isSubTypeOf` returns `Maybe` against anything else,
which real PHPStan treats as "keep, don't flag") - so any return whose
actual type widened to `Mixed` (an untyped Doctrine hydration result,
`ReflectionMethod::invoke()`, an anonymous class's own untracked
property, ...) got every declared member flagged as "never returned,"
a systemic false positive across the entire unscoped corpus fixed by
treating an actual `Mixed` member as automatically satisfying every
declared member, matching the real trinary "Maybe" answer.

**45A's property-assignment sibling, and the two-pass checking
architecture (Phases 166/174).** `check_too_wide_property_types`
(`checker/property_type_too_wide.rs`) applies the identical trinary
algorithm to `Checker::property_assign_facts` (`name -> Vec<Type>` for
every private-property write's own assigned type, accumulated at
`check_property_assignment`'s own choke point, scoped to
`write_visibility == Private` - the only visibility this project can
claim to have seen every real write for). A promoted constructor
property is never assigned via a real `Assign` node, so it accumulates
no facts and is silently skipped - a disclosed leniency, not a claimed
catch. Called once per class, right after its own method loop (mirrors
`check_unused_private_members`'s identical "needs every body scanned
together" choke point), and once for a trait genuinely unused by any
real host (its own only possible consumer).

The real remaining problem - a trait's own property-donated write,
re-checked once per host via `check_trait_methods` with `current_class`
set to that host, accumulates correctly under the host's own fqn, but
nothing consumed that accumulator for the host unless the host's own
`ClassDecl` happened to be processed after the trait, in the identical
file - needed a genuinely new two-pass (collect-then-consume) checking
architecture, not a bigger version of anything single-file already
had: `Project::collect_property_assign_facts` runs a first pass over
every loaded file purely to gather each `Checker`'s own `property_
assign_facts` (`take_property_assign_facts`), merging every file's
per-fqn/per-property contribution into one whole-project map;
`Project::check_file_with_property_assign_facts` is the real,
diagnostic-producing second pass, consuming that merge via a new
`Checker::set_cross_file_property_assign_facts` setter.
`Registry`'s own transitive-purity fixed point (Phase 41) is this
project's one working whole-program mechanism, but its own module doc
comment discloses why it can't be reused directly here: it deliberately
only resolves call targets knowable *without* a live `Checker`/`Scope`,
precisely because purity is a declaration-level fact computable from
raw signatures, while a property-assignment fact can only be
discovered by fully flow-sensitively checking a function body -
exactly the machinery `Checker::check_file` otherwise discards the
moment one file's check finishes. `check_file` itself is unchanged
(same-file-only fallback via `cross_file_property_assign_facts: None`),
so incremental/LSP use pays no cost from this; only `Project::check_all`
and `--check-project`'s own batch orchestration
(`main/check_project.rs`) run the two-pass path, via a new collect
pass ahead of the existing parallel per-file check loop.

Resolves the identical trait-property blind spot `check_unused_private_
members` still has today (that check only scans a `ClassDecl`'s own
literally-declared members, never a trait-flattened one, at all - not
yet wired through this architecture; see PHASE_BACKLOG's Phase 175 for
the one remaining named instance still needing this architecture,
blocked on a different discriminant-shape gap of its own).

**`method.notFound` "did you mean?" tip** (PHASE_BACKLOG Phase 96's
D8). `checker::suggest::suggest_similar_name` - plain case-insensitive
Levenshtein distance, capped at 3, over `Registry::all_method_names`
(the already-precomputed `method_owner_by_class` key set for a class,
own + inherited, no new ancestor walk) - attaches a `DiagnosticBuilder::
tip()` (`"Did you mean foo()?"`) at both `method.notFound` call sites
(`checker/expr.rs`'s `MethodCall` handling and its statement-context
duplicate) when a close-enough candidate exists; silently omitted
otherwise rather than suggesting an unrelated name. `method.notFound`
was the one undefined-symbol family with no tip at all before this -
class/function/property already had at least a static "learn more"
URL.

**Opt-in "boolean trap" detection** (PHASE_BACKLOG Phase 96's D2,
`argument.booleanTrap`). `Checker::check_boolean_trap_arguments`
(`checker/calls.rs`) flags a bare `true`/`false` literal passed
*positionally* (never named, never through a spread) into a bare-
`Type::Bool`-typed parameter - a call shape no type-checking rule can
ever see, since it's always type-correct. Gated behind a new
`Registry::check_boolean_trap`/`Config::check_boolean_trap`/
`--check-boolean-trap` flag, wired through `Project::
set_check_boolean_trap` and `check-project`'s own config-merge
plumbing exactly like every other opt-in check - off by default,
since an unambiguous single-bool-parameter call (`$x->setEnabled
(true)`) is common and would otherwise be pure noise project-wide. A
named argument (`setVisible(visible: true)`) or a variable/expression
argument is never flagged - only the bare literal shape is structurally
invisible to ordinary type checking.

**`@see` PHPDoc tag target validation** (PHASE_BACKLOG Phase 96's D5,
`phpDoc.seeTag`). Real PHPStan's own `phpstan/phpdoc-parser` has no
`SeeTagValueNode` at all - not an architectural choice, the capability
doesn't exist upstream. `checker::oop::check_doc_see_tag_targets`
(wired into the existing `check_unknown_phpstan_tags_on` call sites, so
it runs on every class/method/property/const/function doc comment
already) resolves a `@see` value shaped like a bare class name or a
`Class::member()`/`Class::$prop`/`Class::CONST` reference against the
registry. A bare (no `::`) target only counts as a class reference when
it starts uppercase/backslash - PHP's own class-naming convention -
since a lowercase bare word is far more likely the first word of free-
text prose, which the same missing upstream grammar can't tell apart
from a real reference either; a URL or anything not identifier-shaped
is left alone the same way. The `@covers`/`#[CoversClass]` sibling this
phase originally bundled in is still open - see PHASE_BACKLOG's "D5
remainder".

**Duplicate-operand detection** (PHASE_BACKLOG Phase 96's D7,
`expr.duplicateOperands`) - zero PHPStan precedent, since its whole
`Rules/` catalog reasons purely from inferred *type*, never operand-
expression identity. `Checker::check_duplicate_operands` (`checker/
operators.rs`) flags a comparison (`===`/`!==`/`==`/`!=`/`<`/`>`/`<=`/
`>=`/`<=>`) or `Sub` whose two operands are the exact same side-effect-
free expression (`$x === $x`, `$a - $a`, `$obj->x <=> $obj->x`);
`check_self_assignment` covers the `$x = $x` sibling separately (a
compound self-assign like `$x += $x` is a real, meaningful doubling
operation, deliberately not covered). `is_duplicate_side_effect_free_
operand` is deliberately narrow - a bare variable, or a `PropertyAccess`/
`Index` chain rooted in one, recursing structurally - never a call/
`new`/etc. (could have side effects or return a fresh value each time)
and never a literal (`1 === 1` isn't a copy-paste mistake the way
`$x === $x` is). Silent on whether the result is always true/false/zero
(a float `NAN` makes `$x === $x` false) - just that comparing/
subtracting an expression from itself is almost always a mistake
regardless of what it evaluates to.

**Opt-in constructor-property-promotion suggestion** (PHASE_BACKLOG
Phase 97's AA3, `constructor.promotionCandidate`) - real PHPStan's
promoted-property machinery only ever *validates* an existing
promotion, never *proposes* one. `Checker::check_promoted_property_
candidate` (`checker/oop.rs`) flags a constructor that's a trivial
forwarder: every parameter, in order, has exactly one matching
`$this->name = $name;` statement and nothing else in the body, none of
the parameters are already promoted (a partially-promoted constructor
is left alone - this is about converting the *whole* thing), and each
target property is a plain (non-static, non-hooked) declaration on the
same class with the identical native type. Gated behind `Registry::
check_promoted_property_suggestions` (opt-in - a purely stylistic
suggestion, not a bug) and the configured PHP version being >= 8.0
(`PhpFeature::PromotedProperties`) - without a configured version, this
doesn't guess forward any more than any other version-gated check here
does.

**Opt-in "only booleans in a condition" family** (PHASE_BACKLOG Phase
101's E1, mirroring real `phpstan-strict-rules`' `BooleansInConditions`
rule family). `Checker::check_non_boolean_condition`/`check_non_
boolean_and_or_operand` (`checker/operators.rs`) flag an `if`/`elseif`/
`while`/`do-while`/full-ternary condition, a `&&`/`||` operand, or a
negated (`!`) expression whose inferred type isn't confidently `bool`
(`Bool`/`True`/`False`, or a `Union`/`Intersection` reducing to one) -
genuinely different from this project's existing *constant*-condition
checks (`inferred_constant_condition`, which only ever flags an
always-true/false value, never "not bool-typed at all"). The short/
"elvis" ternary (`$cond ?: $else`) is exempted, matching real
`BooleanInTernaryOperatorRule` exactly - it's a coalescing idiom, not a
real boolean condition. `Type::Mixed` is always exempted too: this
project has no "explicit vs. implicit mixed" provenance tracking on an
inferred expression type the way real PHPStan's own `isExplicitMixed()`
distinction needs, so every `Mixed` reads as "don't know, may be bool,"
matching this project's "don't guess" precedent rather than a literal
port of that distinction. Gated behind `Registry::check_boolean_
conditions`, off by default - real PHPStan ships this only via the
separate, opt-in `phpstan/phpstan-strict-rules` package.

**Opt-in strict `checkDynamicProperties` in `isset`/`empty`/`??`/`??=`**
(PHASE_BACKLOG Phase 102's W1). `check_expr_quiet_root`'s own
`PropertyAccess` arm (`checker/expr/probe_checks.rs`) previously always
matched real PHPStan's lenient default - `isset($x->undefined)`'s whole
point is probing for exactly that, so it stayed silently unchecked with
no flag to ever turn it on. Now, gated behind `Registry::
check_dynamic_properties`, that same arm additionally infers the
base's real type and reuses `member_confirmed_missing` (the identical
existence check the ordinary, non-quiet `PropertyAccess` path already
runs) to flag a property that genuinely doesn't exist there, still
`property.notFound`. Deliberately skips the ordinary path's
`guarded_properties`/Pest-hook/visibility exemptions - a narrower,
opt-in-only re-check, not a full duplicate of the real rule. Off by
default, mirroring real PHPStan's own opt-in `checkDynamicProperties`
parameter.

**Opt-in swapped-adjacent-argument-name detection** (PHASE_BACKLOG
Phase 96's D6, `arguments.possiblySwapped`) - a real, disclosed
heuristic: two adjacent, same-typed parameters are structurally
invisible to real PHPStan's whole type-system-based architecture as
"different roles" (`resize(int $width, int $height)` type-checks a
swapped `resize($height, $width)` call just fine). `Checker::
check_swapped_argument_names` (`checker/calls.rs`) fires only on the
exact "crossed names" shape - both arguments are bare variables, each
one's name matches the *other* position's declared parameter name
case-insensitively, the two parameters are adjacent and identically
typed (never `Mixed`) - never a fuzzy/partial match, to keep the
false-positive rate low enough to be worth shipping. Gated behind
`Registry::check_swapped_arguments`, off by default.

**Opt-in `never`-return-type suggestion** (PHASE_BACKLOG Phase 97's
AA5, `return.neverCandidate`) - real PHPStan already computes this
exact fact (`NeverRuleHelper::shouldReturnNever`), but its own
consuming rules are registered only in phpstan.org's private issue-bot
config, never shipped to a real installable package - a product-
distribution gap, not a technical one. `check_function`
(`checker/function_decl.rs`) reuses the exact same `body_falls_off_end`/
`current_function_saw_explicit_return` facts its own `return.missing`/
`return.never` fall-through check already computes: when the body
provably always terminates (no fallthrough) and never executed a real
`return` statement anywhere (a bare `return;` counts too), and the
declared return type is undeclared (reads as `Mixed`) or `void`, it
suggests `never` instead. Scoped away from a function already declaring
some other real return type that never returns - that's a separate,
unbuilt dead-code question. Gated behind `Registry::
check_never_return_suggestions`, off by default (a purely stylistic
suggestion, not a bug).

**Opt-in missing-`default:`-arm detection** (PHASE_BACKLOG Phase 147's
147f, `switch.missingDefault`). `check_switch` (`checker/stmt/
switch.rs`) flags a `switch` with no `default:` case at all. Opt-in:
a switch over an exhaustively-listed set of values (every case of a
bounded string enumeration, say) can be a deliberate, correct choice
this checker has no way to confirm is truly exhaustive, unlike a real
PHP `enum`'s own `match`-exhaustiveness the checker already reasons
about elsewhere. Gated behind `Registry::check_switch_missing_default`.

**Opt-in missing-exception-message detection** (PHASE_BACKLOG Phase
147's 147d, `throw.missingExceptionMessage`). `check_throw` (`checker/
stmt/try_catch.rs`) flags a `throw new SomeClass();` with zero
constructor arguments - a real, common "forgot the message" bug.
Deliberately narrow: only a bare `new` expression at the throw site
qualifies, so re-throwing a caught exception variable (`throw $e;`) is
never touched. Opt-in, since a custom exception subclass can
legitimately hardcode its own message internally, making a bare `new`
call correct rather than a mistake. Gated behind `Registry::
check_throw_without_message`.

**Opt-in base-Exception/Error-class detection** (PHASE_BACKLOG Phase
147's 147e, `throw.baseClass`). `check_throw`'s identical `New`-shaped
check flags `throw new Exception(...)`/`throw new Error(...)` naming
the bare base class rather than a specific subclass, using the exact
type `check_expr` already inferred for the thrown value (`Type::
Object`) rather than re-resolving the class name from the `new`
expression itself. A rethrow of an already-caught variable (`throw
$e;`) is never a `New` expression, so it's untouched regardless of what
concrete class it resolves to. Opt-in, since deliberately throwing the
generic base class is a legitimate style choice in a project with no
custom exception hierarchy. Gated behind `Registry::
check_throw_base_class`.

**Opt-in discarded-`new`-exception detection** (PHASE_BACKLOG Phase
147's 147c, `new.exceptionNotThrown`). A real, common "forgot the
`throw` keyword" typo: `new SomeException(...);` sitting on its own
line as a whole statement, discarded rather than thrown -
`check_new_exception_discarded` (`checker/stmt/
new_exception_discarded.rs`) fires only when the constructed type is a
confirmed `Throwable` subtype, using `check_no_discard_statement`'s own
call site in `check_stmt`'s `Expression` arm. Assigning the value to a
variable first (`$e = new Exception(...); throw $e;`) is never touched
- only the exact bare-statement shape qualifies. Opt-in, since building
(without throwing) a `Throwable` for some other purpose - handing it to
a logger, say - is legitimate code this checker can't distinguish from
the typo. Gated behind `Registry::check_new_exception_discarded`.

**Opt-in assertion-less-test ("risky test") detection** (PHASE_BACKLOG
Phase 99, `test.risky`). PHPUnit's own `--fail-on-risky` flags a test
method that performed zero assertions - a real, common mistake (a
copy-pasted test body whose assertion line got deleted) PHPUnit itself
only catches at runtime; neither real PHPStan ruleset has any static
equivalent. `check_risky_test_methods`/`check_pest_risky_test`
(`checker/risky_tests.rs`) compose already-built pieces rather than new
architecture: `is_subclass_of` finds a real `PHPUnit\Framework\
TestCase` subclass, whose own `test`-prefixed methods get scanned by a
recursive whole-body walk (`body_has_assertion`/`stmt_has_assertion`/
`expr_has_assertion`, mirroring `control_flow::generators::
stmt_contains_yield`'s own exhaustive per-`StmtKind` shape) for any
`assert*`/`expectException*`/`markTestSkipped`/`markTestIncomplete`/
`addToAssertionCount` call; a Pest `it()`/`test()` closure (recognized
via the same `pest_config::is_this_binding_call` predicate the existing
`$this`-rebinding special case already uses) gets the identical scan,
with a call to the global `expect()` function standing in for PHPUnit's
`assert*` family. Opt-in, since a test that only ever calls a custom,
differently-named assertion-helper method (one whose own name doesn't
start with `assert`/`expectException`) is a real, disclosed false
positive this purely syntactic scan can't rule out. Gated behind
`Registry::check_risky_tests`.

**Whole-program diagnostic-emission pipeline, plus opt-in dead-enum-case
detection built on top of it as its first real consumer** (PHASE_BACKLOG
Phase 92, `enumCase.unused`). Two real architecture gaps were found
scoping Phase 92 (PHASE_BACKLOG's own disclosure, since corrected): this
project had no whole-project, non-per-file diagnostic-emission path at
all, and `Project::decl_locations` (needed to resolve a whole-program
fact's own reportable location) was gated behind `retain_source`, off in
batch `check-project` mode. Both are now fixed, unblocking every other
Phase 86-94 whole-program item and Phase 113-120 composer-hygiene item,
not just this one:
- `decl_locations` no longer shares `retain_source`'s gate at all
  (`Project::insert_parsed_file`/`unindex_file`/`update_file`) - it
  never needed the retained source text in the first place
  (`DeclLocations::index` takes only `&ast`/`&path`), only Tier-1
  `use_sites` genuinely does (`split_identifier_spans`), and stays
  LSP-only.
- `Project::check_whole_program` is the new entry point: gated on
  `Registry::check_dead_enum_cases` (a full no-op when every
  whole-program check is off, the default), it resolves each finding
  through `decl_locations`, applies the same per-line `@phpstan-ignore`
  suppression every ordinary diagnostic gets, and returns the same
  `HashMap<PathBuf, Vec<Diagnostic>>` shape `check_all`/`check_file`
  already do - `check-project`'s own primary loop merges it in once,
  before level-filtering/`ignore_errors`/baseline, so both apply to a
  whole-program finding exactly like a per-file one.
- The real cost this needed to avoid: every whole-program check needs
  `ensure_member_index_built`'s Tier-2 use-site index, which - built the
  naive way - reruns a full second `Checker` pass over every file right
  after `check-project`'s own primary pass already did, doubling total
  check time whenever any whole-program check is enabled. Avoided by
  folding Tier-2 collection into the primary pass instead: `check_file_
  with_property_assign_facts_collecting_member_refs` (a new sibling of
  the existing `check_file_and_member_refs`) runs inside the exact same
  already-parallel, already-panic-isolated, already-large-stack-pooled
  `pool.install(|| par_iter())` closure the primary pass always ran
  anyway; `Project::seed_member_refs`/`mark_member_index_built` adopt
  its results afterward, making `check_whole_program`'s own later
  `ensure_member_index_built` call (needed for correctness when nothing
  pre-seeds it - e.g. a direct library caller, or the LSP's own
  unrelated `references`/`rename` use of the same method) a complete
  no-op in the batch-mode case that actually matters for performance.
- `checker::whole_program::dead_enum_cases::find_dead_cases` is the
  actual check: every real `Enum` class's every case, unless referenced
  via `EnumName::CaseName` (`record_member_ref`, already fired by the
  existing `StaticAccess` arms in both `check_expr` and `infer_expr` -
  which, via `collect_default_and_const_value_member_refs`, already
  covers a property/const default value too), a `match`/`switch` arm
  value (already reached by the ordinary hand-written `check_switch`/
  `Match`-arm traversal, not the generic `stmt_children` walker a naive
  read of the original finding's own "Switch gotcha" would suggest), or
  a literal `::from()`/`::tryFrom()` argument (the one signal genuinely
  missing - now recorded at both of `narrow_enum_from_call`'s call
  sites). A `::cases()` call or a dynamic (`{dynamic}`) member access on
  a real enum escapes the whole enum, conservative all-or-nothing
  (`Checker::escaped_enums`, canonical-cased via the same `owner`/
  `lookup_class` idiom `record_member_ref` itself already relies on, to
  survive a differently-cased call site). Disclosed, not fixed (real but
  narrow): a case reachable only through framework magic/reflection; one
  referenced only inside a `Closure`/`ArrowFn` literal's own default
  parameter value (inherited from `collect_default_and_const_value_
  member_refs`'s own pre-existing gap); one referenced only via PHP 8.1
  first-class-callable syntax on `cases()` specifically (`Suit::
  cases(...)`, a separate, pre-existing `ExprKind::FirstClassCallable`
  code path this project doesn't chase for such an unrealistic pattern).
  Gated behind `Registry::check_dead_enum_cases`; `check-project`-only
  for now (LSP support explicitly out of scope - a whole-program scan
  doesn't fit the LSP's own per-edit cost model the way a per-file check
  does, and none of the phases this unblocks need it either).

  **Real corpus measurement** (Piwigo, `~/piwigo17-frozen`, 2807 first-
  party files, `--no-composer`, warm filesystem cache, three comparable
  runs of the real release binary): the pre-this-work commit (`200a040`)
  ran in 54.64s/2.652 GiB peak RSS; after `decl_locations` decoupling
  with `--check-dead-enum-cases` left off, 58.16s/2.657 GiB - a ~5 MiB
  RSS delta (confirming "cheap, no technical dependency on retain_source"
  at real scale, not just in the LSP's own already-known-fine case) and
  a wall-time difference within ordinary run-to-run noise for a run this
  length; with `--check-dead-enum-cases` on, 57.59s/2.773 GiB - a real
  ~116 MiB RSS cost for Tier-2 tracking across every file, but *no*
  runtime doubling (in this trial, actually faster than the flag-off
  run) - confirming the fold-into-the-primary-pass fix avoided the
  redundant-second-pass cost it was built to avoid. A byte-for-byte sort-
  and-diff of the flag-off run against the pre-this-work run's own output
  showed zero differences - `decl_locations`'s decoupling is confirmed
  behavior-neutral, not just fast.

**Whole-program dead-function/dead-class detection** (PHASE_BACKLOG
Phase 86's B2, `function.unused`/`class.unused`), built directly on the
whole-program pipeline above as its second consumer. The backlog's own
sketch proposed reusing `Project.dependents` - already live, in memory,
for incremental re-check invalidation - as the reverse-reference index
this needs. Checking that against the actual code (not assumed) found
it's the wrong tool: `dependents`'s keys are the raw, unresolved string
exactly as each reference was written (`deps::referenced_names`'s own
deliberate simplification, "no backslash-stripping, no namespace/`use`-
import resolution"), so a class referenced only via a relatively- or
fully-qualified spelling (not its own bare, `use`-imported name) inserts
a *different* map key than a bare-name query for the same class would
look up - a real false-positive risk on ordinary, unremarkable
namespaced PHP, not an exotic edge case. Fixed by relying on
`Project::use_sites`'s Tier-1 index instead (`RefTarget::Class`/
`RefTarget::Function`, already real, canonical, `NameContext`-resolved
FQNs, built for `textDocument/references`/`rename`) - its own one real
gap for a batch `check-project` run was needing retained source text
purely to carve a precise per-identifier span out of a combined multi-
identifier expression (`split_identifier_spans`), never for the
resolution itself. New `use_sites::index_class_fn_refs_without_source`
runs the identical walk with no source text at all, falling back to a
real (if less narrowly-scoped) AST-node span at the 3 call sites that
used to need one - invisible to an existence-only caller, which never
reads a use-site's span - so Tier 1 now runs unconditionally
(`Project::insert_parsed_file`), not gated behind `retain_source`
either, mirroring `decl_locations`'s own earlier fix. Two more real bugs
surfaced and were fixed alongside this:
- `Project::check_whole_program` never filtered a finding to first-
  party (analyzed) files - `decl_locations` indexes every loaded file,
  vendor/context included, so a dead vendor-only enum case or class
  used to leak into `check-project`'s own `results` map as a brand new,
  never-otherwise-checked entry. Fixed once, generically, in a new
  shared `whole_program_diagnostic` helper every whole-program check
  (both this one and `check_dead_enum_cases`) now goes through - caught
  by its own regression test, since the existing `dead_enum_cases`
  fixture tests turned out to use non-analyzed `load_file`, not
  `load_analyzed_file`, and started failing the moment the filter was
  added.
- `deps::referenced_names` (feeding `Project.dependents`, unrelated to
  this check's own final design but still a real, independently-worth-
  fixing incremental-invalidation gap) never walked any `Attribute`
  group at all - a class referenced only via `#[MyAttribute]` on some
  other declaration was invisible to it. Fixed generically (`collect_
  attrs`, called from every `attrs`/`get_attrs`/`set_attrs` site).
  `use_sites`'s own Tier-1 walker has the identical gap, left unfixed
  deliberately - fixing it risks corrupting `textDocument/rename`'s
  real rewrite spans for comparatively little benefit - and disclosed
  instead as a real, narrow false-positive risk for `class.unused`.

`checker::whole_program::dead_code::find_dead_functions`/`find_dead_
classes` are the actual check: every `Registry.functions`/`.classes`
entry with zero `use_sites` references anywhere, except `__autoload`
(PHP's own deprecated magic autoload function, called by the engine
itself with no source-level reference ever). Gated behind `Registry::
check_dead_code`; `check-project`-only, needs no Tier-2 use-site index
at all (Tier 1 alone is enough), so it costs nothing beyond its own
`Registry`/`use_sites` iteration. Disclosed, not fixed: a function/class
that's this project's own public API, consumed only by code outside the
analyzed set (a library, not an application - real PHPStan has no such
rule at all, for exactly this open-world reason); a function referenced
only via a string-callable (`array_map('myFunc', ...)`); a class
referenced only as a PHP attribute target (the `use_sites` gap above);
anything reachable only through framework magic/reflection.

**Real corpus measurement** (Piwigo, `~/piwigo17-frozen`, 2792 first-
party files plus 11459 loaded for context, warm filesystem cache, same
release binary, `--config phpstan-rs.toml`). The first version of this
fix ran `index_class_fn_refs_without_source` unconditionally for every
loaded file, mirroring `decl_locations`'s own unconditional character -
measuring it caught a real, serious regression this reasoning missed:
26.86s/1.76 GiB with the flag off became 2:17.31/1.84 GiB, more than
5x slower, because it walked all ~14k loaded files (context included)
on every run regardless of whether anything consumed the result. Fixed
by gating the walk on `check_dead_code` itself (`Project::
insert_parsed_file`'s own doc comment) rather than running it
unconditionally - after the fix, the flag-off run returned to
26.86s/1.76 GiB, a byte-for-byte identical 63 diagnostics to the
pre-this-work run. With `--check-dead-code` on: 27.08s/1.84 GiB - a
real ~85 MiB RSS cost for Tier 1 across the whole corpus, no measurable
wall-time cost (Tier 1 is a plain syntactic walk, not a `Checker` pass,
unlike Tier 2's own real per-file cost for `check_dead_enum_cases`) -
326 new findings (`function.unused`/`class.unused`) on top of the same
63 pre-existing ones, none of the 63 changed.

**Namespace-level circular-dependency detection** (PHASE_BACKLOG Phase
86's B3, `namespace.circularDependency`) - a namespace with a class that
`extends`/`implements`/`use`s a class in another namespace that, via
some other class pair, has an edge back. No PHPStan rule for this
exists at all. The backlog's own original sketch called for Tarjan's
SCC over `Project.dependents`, the same "already-live reverse-reference
graph" B2 (above) tried first and found too imprecise - rather than
repeat that mistake, this reads directly off `Registry.classes`' own
`extends`/`implements`/`uses` fields instead, each already resolved to
its target's real canonical FQN at declaration time (`ctx.resolve`, in
`class_info.rs`'s own `ClassInfo::from_decl`) - no new indexing pass at all,
not even B2's own cheap Tier-1 use-site index. This trades completeness
for precision and cost: a cycle formed purely through method-body calls
or type hints (not a structural `extends`/`implements`/`uses` edge) is
out of scope, disclosed rather than silently dropped, but every edge
this check *does* see is a real, already-canonical fact - zero false
edges, unlike a `dependents`-based version would have risked.
`checker::whole_program::circular_namespace_deps::find_namespace_cycles`
builds the namespace-level graph (an edge only when two classes'
namespaces actually differ - a same-namespace `extends`/`implements`/
`uses` never contributes a self-loop) and runs a standard recursive
Tarjan's SCC over it; each strongly-connected component of 2+ namespaces
is one finding, reported once, attached to the alphabetically-first
class contributing an edge between the cycle's own alphabetically-first
namespace pair - deterministic, and always resolvable through `decl_
locations`, unlike trying to report a namespace-level fact against no
single real declaration site at all. Gated behind `Registry::
check_circular_dependencies`; needs no Tier-1/Tier-2 use-site index, so
it costs nothing beyond its own graph traversal regardless of project
size - confirmed on Piwigo (`~/piwigo17-frozen`, warm filesystem cache):
29.15s/1.76 GiB with the flag on versus 26.86s/1.76 GiB with every
whole-program check off, well within ordinary run-to-run noise, and
zero findings (Piwigo's own namespace structure has no such cycle).

**Whole-program dead-abstract-class detection** (PHASE_BACKLOG Phase 93,
`class.deadAbstract`) - an `abstract class` with zero concrete
descendants anywhere in the closed universe, a permanent structural
fact rather than "currently unused" (distinct from Phase 86's B2 above -
a polymorphic-dispatch-only abstract class referenced only through a
type hint on its own name has non-zero Tier-1 references there even
with zero real subclasses, so B2 alone would never catch this). No
PHPStan rule exists for this at all - the same open-world reason Phase
85's A5 (closed sealed-hierarchy inference, this phase's own original
backlog citation) doesn't have one either. Deliberately narrower than
A5's own full scope: A5 is a pervasive change to `ObjectType::
isInstanceOf`'s own type-narrowing semantics (returning certain `false`
instead of PHPStan's permanent `Maybe`), needing a persistent reverse
(parent-to-children) index on `Registry`; this only needs A5's own
narrower "does *any* concrete descendant exist" existence query, answered
directly via `Registry::all_ancestor_names` inverted (every concrete,
non-abstract `ClassKind::Class` in the registry is a "witness" - its own
ancestor set, already computed by that existing method, names every
abstract superclass it proves alive) - no persistent reverse index
needed at all, since nothing else yet consumes one. `checker::whole_
program::dead_abstract_classes::find_dead_abstract_classes` builds this
witness set once, then flags every `is_abstract`, `ClassKind::Class`
entry missing from it - an abstract class extended only by *other*
abstract classes (no real, instantiable descendant anywhere) is still
flagged, exactly matching "permanent structural fact," not "currently
unused." Gated behind `Registry::check_dead_abstract_classes`; disclosed,
not fixed: a concrete subclass generated only at runtime by framework
magic (a Doctrine proxy, a mocking framework) is invisible to this
project's static, closed-world registry. Needs no Tier-1/Tier-2 use-site
index either, so it costs nothing beyond its own traversal - confirmed
on Piwigo (`~/piwigo17-frozen`, warm filesystem cache): 25.73s/1.76 GiB
with the flag on versus 26.86s/1.76 GiB off, well within run-to-run
noise, and one real finding (`Piwigo\Tests\Support\
OpenApiContractAssertions`, an unused test-support base class).

**Whole-application unused protected/public member detection**
(PHASE_BACKLOG Phase 86's B1, `method.unused`/`property.unused`/
`classConstant.unused`) - closes out Phase 86 entirely (B2/B3 above).
Real PHPStan's own dead-member rule gates on `isPrivate()` specifically,
since verifying no *other file* reaches a broader-visibility member
needs cross-process collector machinery; this project's closed-world
`Registry`/Tier-2 `use_sites` index (`RefTarget::Member`, already
canonical-owner-keyed) already has that fact for free - every ordinary
access arm (`checker/expr.rs`'s `PropertyAccess`/`MethodCall`/
`StaticAccess`/`StaticCall`) already calls `record_member_ref`, so an
ordinary reference needed no new instrumentation at all. Two real gaps
did, both scoping open questions from when B1 was first triaged:

- **The escape-hatch mechanism** (this phase's own main design task): a
  genuinely dynamic member access (`$obj->{$expr}`/`$obj->$name`) used
  to have zero escape-hatch tracking, unlike `check_dead_enum_cases`'s
  own `{dynamic}`/`::cases()` hooks. New `Checker::escape_targets_for_
  receiver_type` + `escaped_member_classes` (mirroring `escaped_enums`'
  own shape, threaded through `CheckWithMemberRefs`/`seed_member_refs`
  exactly like it): `check_expr`'s `DynamicPropertyAccess`/
  `DynamicMethodCall` arms first check whether the dynamic name is a
  *literal string* - PHP-semantically, `$obj->{'foo'}` **is** `$obj->foo`,
  so that case now records a real reference instead of escaping
  anything (a precision improvement for every Tier-2 consumer, not just
  this one). A genuinely computed name escapes every class the
  receiver's own resolved type could reach: itself, its ancestors (an
  inherited member is reachable through a subclass instance too), and -
  for an interface/non-final-abstract receiver - every real subclass/
  implementor anywhere in the registry, found by inverting `Registry::
  all_ancestor_names` (the same technique Phase 93 above uses). A
  `Union`/`Intersection` receiver escapes every member's own targets
  (conservative - under-escaping is the wrong-direction mistake for an
  escape hatch, unlike this project's usual "favor false negatives"
  bias). A genuinely unresolvable (`Mixed`) receiver contributes no
  escape protection at all - a real, disclosed risk with no fix
  possible without real type information. Also found, pre-existing and
  unrelated to this check: `$obj->$name(...)` (a bare variable, not the
  braced form above) used to parse (`parser/postfix.rs`'s own "best-
  effort" modeling) as an ordinary access literally named after the
  *variable's own source name*, never its runtime value - a real,
  common dynamic-dispatch idiom this project couldn't model correctly.
  Disclosed here first, then actually fixed at the parser level in a
  follow-up pass (the same session): it now parses identically to the
  braced form above, so both get the same escape-hatch treatment - see
  `src/parser/postfix.rs`'s own doc comment. That same pass also found
  and fixed a real, latent performance bug this exact gap had been
  masking: `escape_targets_for_receiver_type`'s own interface/abstract
  branch (a linear scan over every registry class) ran unconditionally
  regardless of `collect_member_refs`, with only its *recording* step
  gated - harmless while the bare-variable form was rare, but the
  parser fix would have made it common. Both call sites now gate the
  whole computation, not just the recording, on `collect_member_refs`.
- **Which methods PHP itself calls implicitly, with no source-level
  reference this project could ever see.** Three real categories, found
  in that order by actually measuring a real corpus (Piwigo) rather than
  assuming the first two were enough: PHP's own magic method names
  (`__construct`, `__toString`, ...), hard-excluded by name; a method
  overriding *any* real ancestor's own same-named method (an
  interface's contract method, or a regular ancestor's method reached
  only through polymorphic dispatch on a base-typed variable) - this
  replaced an initial small hand-maintained table of well-known SPL
  interfaces (`Countable`, `Iterator`, ...) after the first real Piwigo
  measurement (3807 findings) caught that table's own real blind spot:
  several genuinely-used `PhpParser\NodeVisitor` implementations
  (`enterNode`/`leaveNode`, called polymorphically by php-parser's own
  vendored traverser) were flagged dead. The generic ancestor-override
  check catches every such interface automatically, with no library-
  specific knowledge at all (down to 2506 findings); a PHPUnit test
  method or lifecycle callback (`setUp`, `test*`) on a real `TestCase`
  subclass - the actual *dominant* source of noise a second measurement
  revealed (thousands of `test*` methods, each only ever invoked by
  PHPUnit's own reflection-based discovery), recognized the exact same
  way `checker::risky_tests::check_risky_test_methods` already does
  (down to 193 findings, a workable signal-to-noise ratio). An
  interface/trait's own method declaration is never itself a candidate
  either (never instantiable, so never itself "dead" in a meaningful
  sense - only a real class/enum's own methods/properties/consts are).
  An enum's built-in `name`/`value` properties and an enum case's own
  `consts` entry (already Phase 92's own domain) are excluded too.

Gated behind `Registry::check_dead_members`; needs the same Tier-2
use-site index `check_dead_enum_cases` does (`main/check_project.rs`'s
own primary loop now triggers Tier-2 collection whenever *either* flag
is on). Real, disclosed false-positive risks left unfixed (ship
opt-in): the whole-program pass only ever runs over analyzed (first-
party) files, never vendor/context ones - a call made entirely from
within vendor code (the exact shape the `NodeVisitor` case above turned
out to be, before the generic override check independently fixed it) is
invisible to this check for that architectural reason; a member
reachable only through framework magic/reflection generally (a
template calling a method by name, a DI container definition); a
member called only through a `[$obj, 'methodName']` array-callable - a
real, known-name reference this project doesn't yet resolve, unlike the
literal-string-dynamic-access case above; a property and a method
sharing the exact same name on one class (legal PHP) share one Tier-2
usage signal, so using one can mask the other's own real deadness - a
real, but only ever false-*negative* limitation, the safe direction.
Not built, deliberately deferred (`tomasvotruba/unused-public`'s own
idea): flagging a public method *never* called from outside its own
declaring class as "could be private" (over-visible, not dead) - a
different, suggestion-shaped finding, not this check's own "is it dead"
question.

**Real corpus measurement** (Piwigo, `~/piwigo17-frozen`, warm
filesystem cache, `--config phpstan-rs.toml`): 27.56s-29.95s/1.83-1.87
GiB across the design's three iterations (each within ordinary
run-to-run noise of the ~26.86s/1.76 GiB baseline with every whole-
program check off - the added RSS is Tier 2's own real per-node
tracking cost, the same character `check_dead_enum_cases` already
measured, not a new cost category). The finding *count* moved far more
than the cost did as the design was corrected against real evidence:
3807 findings (initial hand-maintained-SPL-table design) → 2506 (after
the generic ancestor-override fix) → 193 (after the PHPUnit test-method/
lifecycle-callback fix) - each drop a real, measured correction, not a
guess, and each documented above rather than silently folded away.

**Doctrine ORM entity metadata resolution** (PHASE_BACKLOG Phase 168's
168a, the first landed sub-item of a larger in-progress phase - see
PHASE_BACKLOG.md for 168b onward). `ClassInfo::doctrine_entity`
(`registry/class_info/doctrine_entity.rs`), extracted from a class's own
`#[ORM\Entity]`/`#[ORM\Id]`/`#[ORM\ManyToOne]`/`OneToOne`/`OneToMany`/
`ManyToMany` attributes (resolved through each file's own `use`
imports/aliases, e.g. `use Doctrine\ORM\Mapping as ORM;`, to their real
fully-qualified `Doctrine\ORM\Mapping\*` names - not a bare short-name
match, unlike this project's own native `#[\Deprecated]`/
`#[\AllowDynamicProperties]` handling, which can safely use one since
those are unambiguous global-namespace attributes). Reuses each mapped
property's own already-resolved native type (`PropertySig::ty_native`)
for both scalar field types and a to-one association's own target
entity - real Piwigo entities are 100% PHP-8-attribute-mapped with
natively-typed properties, so no from-scratch Doctrine mapping-metadata
resolver is needed at all, unlike real PHPStan's own `phpstan-doctrine`,
which actually instantiates a live `ClassMetadataFactory` at analysis
time. A `OneToMany`/`ManyToMany` collection-typed property falls back to
its attribute's own explicit `targetEntity: ...` argument, since a
`Collection`/`array`-typed property carries no target class name of its
own. A composite identifier (more than one `#[ORM\Id]` property, real
and valid Doctrine) deliberately records no single `id_field` rather
than picking one arbitrarily. Entity inheritance
(`#[ORM\InheritanceType]`/discriminator columns) is a disclosed gap,
confirmed absent from the real corpus - PHASE_BACKLOG's own 168j.

**Symbolic Doctrine `QueryBuilder` fluent-chain resolution**
(PHASE_BACKLOG Phase 168's 168b). `Checker::resolve_query_builder_chain`
(`checker/doctrine/query_builder.rs`), given the receiver expression of
a `->getQuery()` call, walks the fluent `MethodCall` chain backward
collecting every call that can affect the query's own result-row shape
(`select`/`addSelect`/`from`/`join`/`innerJoin`/`leftJoin`) - purely
syntactically, unlike real PHPStan's own extension, which actually
replays the chain onto a live `QueryBuilder` from a real
`EntityManager`. `where`/`andWhere`/`having`/`groupBy`/`orderBy`/
`setParameter`/etc. are dropped unconditionally rather than merely
tolerated when unresolvable, since none of them can ever add or remove
a result column. Two real chain-root shapes are both handled, checked
directly against `EntityRepository::createQueryBuilder()`'s own real
source: a plain `$em->createQueryBuilder()` (no implicit select/from),
and the corpus-dominant `$this->createQueryBuilder($alias)` repository
shorthand (100+ real call sites across 18 repository files - likely
more common than the explicit-`from()` form), which real Doctrine
defines as pure sugar for `->select($alias)->from($entityName, $alias)`
and is resolved here via `Registry::repository_entity`'s own reverse
lookup against 168a's `repository_class` field. `select()` replaces the
accumulated select list; `addSelect()` appends to it, matching their
real, distinct Doctrine semantics. A `select`/`from`/`join` argument
that isn't a literal or class-constant-fetch expression makes the whole
chain undecidable (`None`), the identical fallback real PHPStan's own
extension takes at the same point.

**`QueryBuilder` chains split across a variable assignment** (commit
`45005e2`). The walk above originally only ever followed a syntactic
fluent `MethodCall` chain - hitting a bare `Variable` node stopped it
cold, with no way back to the assignment that built it, despite this
project's own prior doc comments claiming real corpus usage was "100%
one direct chained expression, never split across a variable assignment
first." Confirmed false, and confirmed *recurring*, not a one-off: 5
real corpus sites split `$qb` into a variable (usually to attach a
conditional `andWhere()`/`orderBy()`) before its own terminal call.
Fixed with the same "replay the original expression" idea
`condition_aliases` already uses for a narrowing condition: a new
`QueryBuilderAliasMap` stores `$qb = <chain>`'s own RHS verbatim
whenever it looks chain-shaped, purged by `Locals::declare` on
reassignment. The walk's own `cursor` became a `Cursor` enum
(`Borrowed`/`Owned`) so it can substitute the stored `Rc<Expr>` in
place of a bare `Variable` read and keep walking - `$qb2 = $qb;` chains
of aliases resolve the same way, one substitution at a time.

**A small DQL select-item parser** (PHASE_BACKLOG Phase 168's 168c).
`checker::doctrine::dql::parse_select_items` (`checker/doctrine/dql.rs`)
- a hand-rolled tokenizer/parser (not a reuse of this project's own PHP
`Lexer` - DQL is Doctrine's own, unrelated grammar) over 168b's already-
split `select()`/`addSelect()` argument strings, each further split on
its own top-level commas (a single comma-joined argument, e.g.
`select('t, COUNT(x) AS c')`, is equally legal DQL). Recognizes a bare
alias, a dotted scalar field, `IDENTITY(alias.assoc)`, the five
aggregate functions (`COUNT`/`SUM`/`AVG`/`MIN`/`MAX`, `DISTINCT`
consumed but not tracked - it changes cardinality, never an aggregate's
own result type), `CASE WHEN ... THEN <arm> ... ELSE <arm> END` (only a
bare field-ref arm modeled; a `WHEN` condition's own text is tokenized
generically as opaque, never-interpreted tokens purely so it doesn't
break tokenization, matching 168b's own WHERE-clause exclusion), `NEW
Some\Dto(...)` construction (its own constructor arguments are skipped,
not parsed - the result is always exactly `Object(Dto, [])` regardless),
and a function-call-shaped item whose name isn't one of the five
built-ins as a custom, project-registered function (PHASE_BACKLOG Phase
168 168h). An unrecognized construct anywhere - including a scalar
subquery nested inside a select item, `SELECT (SELECT COUNT(...) FROM
...) AS cnt`, a disclosed follow-on (168k) - makes the whole query
undecidable (`None`), never a parse error or a silent guess.

**Result-shape builder** (PHASE_BACKLOG Phase 168's 168d).
`checker::doctrine::result_shape::build_result_row_type` - given 168c's
parsed select items plus 168b's `FROM`/`JOIN` alias map and 168a's
entity metadata, computes one query row's own `Type`, mirroring real
Doctrine's default `HYDRATE_OBJECT` hydration exactly: exactly one bare-
entity select item with no scalar siblings and no `AS`-alias produces
the entity object directly, unwrapped; every other shape produces an
`ArrayShape` keyed by each item's own `AS`-alias when it has one,
otherwise by its absolute position in the select list (matching real
Doctrine's own positional/named mixing rule, not a separately-counted
"unaliased items only" index). `IDENTITY(alias.assoc)` resolves through
to the target entity's own `id_field` type. Aggregate-function return
types are transcribed directly from `phpstan-doctrine`'s own
`QueryResultTypeWalker::walkAggregateExpression`, not inferred: `COUNT`
is always `int<0, max>` *regardless* of whether its own operand
resolves (real Doctrine's `COUNT` never actually depends on the
operand's type); `SUM`/`AVG`/`MIN`/`MAX` all four resolve to the
operand's own type instead (the real walker calls the identical
`walkSimpleArithmeticExpression` for all four - there's no dedicated
`SUM`/`AVG` output type). A deliberate design split from 168b/168c's own
"undecidable" contract: those two fail the *whole query* on a real DQL
syntax problem (an unparseable select item, an unresolvable
`QueryBuilder` chain), matching what a real DQL parser or a live
`QueryBuilder` replay would do; this module runs only after both already
succeeded, so anything it still can't resolve (an unknown alias, an
association 168a's metadata doesn't know) is a metadata gap, not a
malformed query, and localizes to `Mixed` for that one field instead -
matching real `phpstan-doctrine`'s own walker, which does the identical
thing rather than discarding an otherwise-precise row shape.

**Wiring `getQuery()`/`getResult()`/`getOneOrNullResult()`/
`getSingleResult()`/`execute()`/`toIterable()`** (PHASE_BACKLOG Phase
168's 168e). `checker::expr::member_resolution::method_call_return_type`
gained a `recv: &Expr` parameter (the receiver AST, not just its
inferred `Type`) specifically so `QueryBuilder::getQuery()` can walk its
own fluent chain (168b) - resolving the row type via 168c/168d and
carrying it as `Doctrine\ORM\Query`'s own generic-argument slot
(`Type::Object("Doctrine\\ORM\\Query", vec![row_type])`). The five
terminal methods real `phpstan-doctrine` itself resolves precisely (via
its own `QueryResultDynamicReturnTypeExtension`) read that slot back
out: `getResult()`/bare `execute()`/bare `toIterable()` → `list<row_type>`
(`getResult()` keeps its pre-existing `list<mixed>` fallback when the
chain itself was undecidable, so this regresses nothing);
`getOneOrNullResult()` → `row_type|null`; `getSingleResult()` →
`row_type` directly (real Doctrine throws
`NoResultException`/`NonUniqueResultException` otherwise - unmodeled,
same as every other unmodeled-exception-path precedent in this
codebase). `execute()`/`toIterable()`'s own hydration-mode argument
(index 1 and 0 respectively) is resolved against the literal
`HYDRATE_OBJECT` constant or an omitted/explicit-`null` default; any
other named mode falls back to `Mixed` until 168f's own per-mode
transforms exist (`getArrayResult()`/etc. have no dynamic-return-type
extension in real `phpstan-doctrine` either, so nothing here can be
checked against a reference implementation until that hydrator-source
transcription work lands).

A real, structural bug was caught building this: `resolve_query_builder_
chain`'s own backward walk unconditionally unwrapped *every*
`MethodCall`, including `createQueryBuilder()` itself, so `resolve_
chain_root` ended up receiving the *variable underneath* it (`$em`) as
its own "root" argument rather than the `createQueryBuilder(...)` call
node it expects - silently failing every chain, always falling back to
`Mixed`, with no compiler error to catch it (only an end-to-end
`assertType()` test surfaced it, one bare `$em->createQueryBuilder()->
select('t')->from(...)->getQuery()->getResult()` call producing plain
`list` instead of `list<Entity>`). Fixed by stopping the backward walk
exactly at the `createQueryBuilder` call, without descending past it.

**`getArrayResult()`/`getSingleColumnResult()`/`getSingleScalarResult()`/
`getScalarResult()`** (PHASE_BACKLOG Phase 168's 168f) - the largest real
payoff of the whole Doctrine phase (`getArrayResult`/
`getSingleColumnResult`/`getSingleScalarResult` alone are 295 of the
corpus's 526 real terminal-method call sites), and beyond real PHPStan:
`phpstan-doctrine`'s own stub declares all four bare (`mixed[]`/
`mixed`), no dynamic-return-type extension covers any of them, so
nothing here is checkable against a reference implementation's own
`dumpType()` output - each transcribed directly from Doctrine ORM's own
hydrator source instead (`ArrayHydrator`/`ScalarHydrator`/
`ScalarColumnHydrator`).
`checker::doctrine::result_shape::{build_array_result_row_type,
single_scalar_column_type, build_scalar_result_row_type}`:
`getArrayResult()` converts every entity member of `getResult()`'s own
row shape into a nested `object{...}` shape instead of a real object -
a to-one association converts to a further-nested `object{}` carrying
just its own target's identifier (real Doctrine's default, unfetched-
association array shape); a to-many association is omitted from the
shape entirely rather than guessed, since its own array-hydration shape
needs a separate, recursive-list rule this project doesn't model yet.
`getSingleColumnResult()`/`getSingleScalarResult()` share one
requirement (`single_scalar_column_type`): the select must have exactly
one item whose own resolved type isn't an entity object - `list<T>` for
the former, `T` directly for the latter (real Doctrine throws
`NoResultException`/`NonUniqueResultException` otherwise, unmodeled,
same precedent as `getSingleResult()`). `getScalarResult()` flattens
every selected entity's own scalar fields directly into the row under
`{alias}_{field}`-keyed entries, alongside every plain scalar/aggregate
item under its own key - zero real corpus usage (confirmed), built for
completeness with correspondingly lower confidence than the other
three: `AbstractHydrator::gatherScalarRowData`'s own exact key-naming
rule was inferred from `ScalarHydrator`'s own doc comment ("almost the
same as a regular SQL result set"), not independently confirmed against
the method body itself - flagged as needing verification against real
Doctrine before trusting this specific shape on a real query.

Wired differently from 168e's five methods: rather than reading back a
pre-computed row type from `getQuery()`'s own stored `HYDRATE_OBJECT`
shape, `checker::expr::member_resolution::doctrine_get_query_receiver`
recognizes `->getQuery()` as `recv`'s own immediate receiver and
re-resolves the fluent chain (168b) and select items (168c) directly at
each of these four call sites - every one of them needs its own,
differently-shaped hydration transform, not a single fixed shape to
reuse, and real corpus usage of every terminal method is confirmed 100%
one direct chained expression (never split across a variable assignment
first), so this loses no real coverage in exchange for not building a
second, parallel row-type-storage mechanism per hydration mode.

**Direct `EntityManager::createQuery($dql)`** (PHASE_BACKLOG Phase 168's
168g). `checker::doctrine::dql::parse_select_statement` - a full
`SELECT <items> FROM Entity alias [[INNER|LEFT|OUTER] JOIN <target>
alias [WITH <condition>]]*` statement parser, sharing 168c's own
select-item grammar entirely rather than a second, parallel one (each
select item between `SELECT` and the top-level `FROM` keyword - found
via a paren-depth-aware token scan, not a naive text search - is parsed
by the exact same per-item parser 168b's own `QueryBuilder`-chain-
derived arguments use). A `JOIN` target reduces to the identical text
format `QueryBuilder::join()`'s own argument already does (`alias.assoc`
or a qualified entity name), so 168d's existing alias-resolution logic
applies completely unchanged. A `WITH <condition>` clause is skipped
(never affects row shape, matching every other WHERE-shaped exclusion
in this phase) by scanning to the next real join/terminating keyword -
safe because a condition's own comparison/literal tokens all tokenize
as an opaque `Other` token, never as an `Ident` this scan could mistake
for one. No WHERE/GROUP BY/HAVING/ORDER BY parsing, for the same reason.
Wired in `checker::expr::member_resolution` to produce the identical
`Type::Object("Doctrine\\ORM\\Query", vec![row_type])` shape
`QueryBuilder::getQuery()` does, so `getResult()`/`getOneOrNullResult()`/
`getSingleResult()`/`execute()`/`toIterable()` (168e) work through it
unchanged. Not corpus-confirmed necessary (the real corpus's only
literal-DQL-string usages are `DELETE`/`UPDATE` DML, never `SELECT`),
but the same select-item grammar doing strictly less work, so building
it keeps the feature honestly general rather than `QueryBuilder`-only.
`getArrayResult()`/`getSingleColumnResult()`/`getSingleScalarResult()`/
`getScalarResult()` (168f) don't benefit the same way - their own
dispatch specifically re-resolves a `->getQuery()`-shaped receiver
expression, which a `createQuery()`-produced `Query` value doesn't have
- a small, disclosed asymmetry matching this item's own zero real
corpus need.

**Custom DQL function return types** (PHASE_BACKLOG Phase 168's 168h,
the last buildable sub-item of the phase at the time - 168i was then a
genuine, permanent boundary (see the correction below); 168j/168k
remain real, disclosed, sequenced-later work).
`doctrine_functions::DoctrineCustomFunctionEntry`
(`[[doctrine_custom_functions]]`, config-file-only, no CLI flag, same
shape as this project's existing `ignore_errors`/`restricted_symbols`) -
mirrors real `phpstan-doctrine`'s own identical `customNumericFunctions`/
`customStringFunctions`/`customDatetimeFunctions` config keys, since a
project's own `Configuration::addCustomStringFunction()`-registered DQL
function (`DATE_FORMAT_YEAR_MONTH`, confirmed real and project-
registered in the corpus this phase targets) has no return type
derivable from the DQL parser alone - true of real PHPStan's own
extension too, not a gap specific to this project. `Numeric` resolves to
`int|float`; `String`/`Datetime` both resolve to plain `string` - a DQL
scalar result is always a raw, unhydrated database value, never a real
`DateTime` object, regardless of which category a function's own
semantic meaning falls into. Consulted from
`checker::doctrine::result_shape`'s existing `SelectItemKind::Custom`
handling (previously always `Mixed`), keyed by lowercased function name
to match DQL's own case-insensitive function-name lookup.

With 168a-168h now built, PHASE_BACKLOG Phase 168's entire *buildable*
scope is complete: every terminal `AbstractQuery` method with real
corpus usage now gets DQL-aware row-shape inference (168e's five methods
checkable against real `phpstan-doctrine`'s own `dumpType()` output;
168f's four beyond it, transcribed from Doctrine's own hydrator source),
fed by either a `QueryBuilder` fluent chain (168b) or a literal DQL
string (168g), with a config surface for what neither this project nor
real PHPStan can infer on its own (168h). 168j (entity inheritance) and
168k (a scalar subquery nested inside a select item) remain real,
scoped, disclosed follow-on work with no known real-corpus instance to
build or verify against yet.

**168i's own "permanent boundary" framing is superseded, not just
narrowed** - see the live SQL query reflection phase below (originally
`delegated-tumbling-dongarra.md`): reevaluated once a genuinely new
mechanism became available (a live database connection or a static DDL
schema catalog, neither of which existed when 168i was first disclosed),
`fetchAssociative()`/`fetchAllAssociative()`/`fetchOne()`/
`fetchFirstColumn()`/`executeQuery()` (both `Connection`'s own literal-
SQL form and `QueryBuilder`'s own chain-based aliases) all now get real,
schema-derived row shapes when `[sql_reflection]` is configured -
`array<string,mixed>|false` remains the honest, correct fallback only
when it isn't.

**Live SQL query reflection** (originally planned in
`delegated-tumbling-dongarra.md`, sub-phases A-E all now built; F -
`EXPLAIN`-based query-plan analysis and general SQL style linting -
stays explicitly out of scope). A new, always-on `[sql_reflection]`
config block (`sql_reflection_config::SqlReflectionConfig`, config-file-
only, same shape as `restricted_symbols`) names either a live database
connection (MySQL/Postgres/SQLite, each via a real driver crate -
`mysql`/`postgres`+`r2d2_postgres`/`rusqlite`+`r2d2_sqlite`) or a static
per-engine DDL dump to parse (`sql_reflection_schema`, using the new
`sqlparser` dependency), or both for a cross-check. Absent entirely,
every raw-SQL call site keeps today's exact fallback - confirmed via a
byte-identical corpus diff with no block configured, at every sub-phase.

A `Doctrine\DBAL\Query\QueryBuilder` fluent chain (`checker::sql_
reflection::query_builder`) folds into one literal SQL string mirroring
real `QueryBuilder::getSQL()`'s own clause order, reusing 168b's own
`Locals::query_builder_aliases` map. `checker::sql_reflection::resolve`
answers from the static schema catalog first (scoped to the dominant
single-table/no-JOIN shape), falling through to a live, lazily-
connected, `Registry`-held `OnceLock` reflector (`sql_reflection_
reflector` - deliberately a real connection pool per engine, not a
`Mutex<Connection>`, so `rayon`'s own parallel check phase never
serializes on it) that `PREPARE`s (never executes) the resolved SQL and
reads the real result-set column metadata. Column-type translation is
hand-kept-consistent (not code-shared - the inputs are shape-
incompatible) between the static and live paths, covering the real
per-engine type variety surveyed directly against Piwigo's own DDL dump
(`ENUM` -> a real literal-string union, cross-referenced from the
static catalog even on the live path since wire-protocol metadata alone
never carries an enum's permitted values; `JSON`/`DECIMAL`/timestamp
family -> `String`; `TINYINT(1)` -> `Int`, never `Bool`; SQLite's own
`NUMERIC` affinity -> a genuine `Union([Int, Float])`, a real semantic
fact about SQLite's own storage-class fallback, not a hedge). Wired into
real dispatch (`checker::expr::member_resolution`) for `Connection`'s
own directly-SQL-taking `fetchAssociative`/`fetchAllAssociative`/
`fetchOne`/`fetchFirstColumn`, `QueryBuilder`'s own no-argument aliases
for the same four, and the `executeQuery()` -> synthesized `Doctrine\
DBAL\Result` generic-slot two-hop propagation (mirroring 168e's own
`Doctrine\ORM\Query` convention) - `executeStatement()` needed no
override, already a correct `int|string` via ordinary signature
resolution. A separate check (`checker::sql_reflection::
parameter_validation`) cross-references every real placeholder the
resolved SQL references against the actual bound values from either
`setParameter`/`setParameters` (the corpus-dominant real mechanism -
542 real call sites) or `Connection`'s own `$params`/`$types` arguments
(built as a real, general capability regardless of which one a given
corpus favors), flagging a real unbound placeholder or an array bound
without the real, documented `ArrayParameterType::*` exemption (load-
bearing, not an edge case - 53+ real call sites use exactly that
pattern for `IN (...)`-list binding).

Two real, previously-invisible gaps surfaced and fixed entirely by this
phase's own live corpus/live-database verification, not by inspection:
(1) DBAL-resolved SQL text carries DBAL's own `?`/`:name` placeholder
syntax unchanged, but every live reflector bypasses DBAL's own runtime
entirely - nothing was rewriting that into each engine's real native
`PREPARE` syntax (confirmed directly: real MySQL rejects `:name`
outright, real Postgres rejects both `?` and `:name`, requiring
numbered `$1`/`$2`/... instead), silently failing to reflect any real
named-placeholder query (the dominant real shape) before the fix; (2)
the parameter-validation check's first cut collapsed "undecidable" (a
plain PHP variable passed as `$params`, not a literal array) into "zero
bindings," flagging every real placeholder in real Piwigo code as
missing - caught by the very same corpus regression check this whole
phase's own discipline required at every step, not shipped silently.
Real corpus payoff, confirmed live against a real MySQL instance: the
scoped corpus's own `NotificationByMailSenderTest.php:148` finding -
168i's own original motivating example - resolves once a live
connection or static schema is configured, with zero new false
positives across the full unscoped corpus (including `vendor/`'s own
internal DBAL usage of its own APIs).

**Remediation pass - every gap the first pass above disclosed is now
closed**, triggered by a direct "check if everything was completed"
re-read of the original plan against shipped code (never trust one's
own completion summary over the plan's own text): the
zero-real-corpus-usage `fetchNumeric`/`fetchAllNumeric`/
`fetchAllKeyValue`/`fetchAllAssociativeIndexed`/`iterate*` ×5/
`rowCount`/`columnCount`/`executeCacheQuery` methods are wired into
real dispatch (`checker::expr::member_resolution`'s own
`dbal_row_method_return_type`, 13 methods total across `Result`'s own
no-argument form and `Connection`'s SQL-argument form, plus the
`executeCacheQuery()` two-hop propagation).

`COUNT(...)` (bare or windowed - `COUNT(*) OVER()`) is refined from
whatever the static/live path reports to `int<0, max>` unconditionally,
matching Phase 168's own DQL-side aggregate-type answer
(`resolve_sql_row_type`'s own `refine_count_columns`, re-parsing the
resolved SQL's own SELECT list to find the aggregate's projection
position) - deliberately not extended to `SUM`/`AVG`/`MIN`/`MAX`,
where the driver's own live-reflected type is already more precise
than 168's DQL-side approximation (real aggregation can widen a type,
e.g. `SUM(tinyint_col)` -> `BIGINT`).

`foldable_sql_string` now also folds a heredoc/double-quoted string
whose every interpolated part is itself foldable, and a `sprintf()`
call whose format string and every value argument fold to a literal
(scoped to `%s`/`%d`/`%u`/`%%` only - no flags/width/precision/
positional `$`, where real PHP's own padding/sign/base-conversion
rules would need reproducing just to fold a SQL string) - both scoped
to this module alone rather than changing `check_expr`'s own general
`Interpolated` narrowing, which deliberately stays plain `Type::String`
for its own implicit-`__toString()` diagnostics.

A second, connection-independent source of parameter-type truth beyond
`ArrayParameterType::*`: `ParameterType::INTEGER`/`Types::INTEGER`/
`SMALLINT`/`BIGINT` (real DBAL, already present in the call site's own
PHP source, no live/static schema needed) bound to a literal, non-
numeric string is flagged under `sqlReflection.invalidParameterType`
too - deliberately narrow to `INTEGER` and literal strings only, since
`STRING`/`BOOLEAN`-declared types stay unchecked (almost any scalar
coerces to those safely).

Resolution-order steps 1 (`resolve_pure_ast` - a select list of only
literals and/or a bare `COUNT`, needing no schema/connection at all,
live-confirmed identical across all 3 engines for integer/string/
`NULL` literals, driver-dependent for decimal/boolean literals where
engines genuinely diverge) and 2 (`Registry::sql_reflection_cache`, an
in-memory single-run cache by SQL text alone - this project supports
exactly one schema source/connection per run, so that identity is
invariant and needs no separate cache-key component) are now built.

**Virtual relations - live-verified to need no dedicated resolution
logic on the live path at all**: views, CTEs, derived tables, and
`UNION` (including its own per-column numeric/string coercion, e.g.
`int UNION varchar` reporting as a single `string` column, never a
PHP-level `Type::Union`) all resolve correctly through the exact same
`reflect()`/`PREPARE` call already used for a plain `SELECT`, confirmed
directly against real MySQL, Postgres, and SQLite - the live engine's
own query planner already does this work. The static-schema-only path
(no live connection at all) genuinely needed new code, since it has no
engine to delegate to: `resolve_from_static_schema` now resolves a
derived table or a non-recursive CTE through the same column-list
primitive a real table's own schema entry uses. `UNION` coercion rules,
`CREATE VIEW` parsing, and `WITH RECURSIVE` stay a disclosed gap in
this static-only path specifically - each needs real per-engine
behavior this project only trusts once verified live, which a schema-
file-only configuration has no database to verify against.

**Window functions** turned out to already resolve correctly once a
real, unrelated bug was fixed: `normalize_placeholders_for_driver`
rebuilt the whole SQL string by tokenizing and rejoining every token
with a single space, which is lossy - real, live-confirmed MySQL
rejects a space between a function name and its own opening paren
specifically when the argument is `*` (`COUNT(*)` is valid, `COUNT (*)`
is a genuine syntax error), so any query with `COUNT(*)` or a window
function silently failed to `PREPARE` the moment it also needed a
placeholder rewritten (unconditional on every MySQL/Postgres
`reflect()` call, regardless of whether the SQL even had a
placeholder). Fixed by rewriting only the real placeholder spans via
`tokenize_with_location()`, copying every other byte of the original
SQL through unchanged.

**Session-variable parity**: a new optional `session_init_sql` config
field runs real SQL once on every new physical reflection connection,
mirroring an app's own real session-init mechanism (e.g. `MYSQLI_
INIT_COMMAND`'s `SET SESSION sql_mode=...`) - live-verified this
actually matters (a weaker session default can silently accept a query
at `PREPARE` time that the app's own real, more strictly configured
connection would reject).

**Schema drift check**: when both a `schema_file` and a live-connection
config are configured together, each table the static catalog knows
about is compared against its own live-reflected column list once at
startup, warning on a real mismatch - reuses the existing `reflect()`
interface (`SELECT * FROM <table>`) rather than writing new per-engine
`information_schema`/`PRAGMA` queries by hand. Verified end to end
against the real, full DDL dump and the real live `piwigo_test`
database - zero false positives across all 38 real tables.

**Doctrine ORM entity metadata cross-reference** (PHASE_BACKLOG Phase
168's own entity-metadata extraction never needed a real DB table/
column name at all, since DQL resolves through property names - new
capability, not a reuse): `DoctrineEntity` now also carries `table_name`
(`#[ORM\Table(name: ...)]`) and `column_names` (`#[ORM\Column(name:
...)]`, falling back to the property's own name when absent, matching
real Doctrine's own default naming strategy). A raw SQL query against a
table a `DoctrineEntity` maps prefers that entity's own already-resolved
native PHP property type over a freshly-derived DB-column-type
translation, for any column that's also a mapped scalar field, only
when the entity's type is a genuine narrowing (`is_subtype(entity_ty,
column_ty)` - e.g. a column nullable in the DB whose mapped property is
declared non-nullable because the app enforces it); a disagreement that
isn't a narrowing keeps the column-derived answer.

**Entity inheritance mapping (single-table/joined-table)**
(PHASE_BACKLOG Phase 168's 168j - the first of the two previously-
disclosed-but-not-yet-built follow-ons; 168k remains open).
`ClassInfo::doctrine_entity`'s own `discriminator_map` field
(`registry/class_info/doctrine_entity.rs`) is now populated from a root
entity's own real `#[ORM\DiscriminatorMap(['cat' => CatEntity::class,
...])]` attribute (its value -> concrete-subclass map, accepting either
a named `value:` argument or a positional one, matching real Doctrine's
own attribute constructor signature). `checker::doctrine::
result_shape::entity_row_type` consumes it: a bare-entity select item
whose alias resolves to an entity carrying a real, non-empty
discriminator map produces a union across every mapped subclass instead
of the base type alone - any one row of a real, unfiltered polymorphic
`FROM BaseEntity` query is genuinely one concrete subclass instance,
never the abstract base type itself, and a plain `Object(entity, [])`
would misrepresent that. `getArrayResult()`'s own entity-to-array
conversion (168f) was extended to recurse into a `Union` member-by-
member for the identical reason - it previously only matched a bare
`Type::Object`, which would have silently skipped a polymorphic row
entirely (left as real objects instead of converted to array shapes).
Zero real corpus usage to verify against (confirmed, not assumed) -
unit-tested only, following this project's own "corpus incidence
affects verification confidence, never whether a real, well-defined
feature gets built" standard, since real Doctrine's own inheritance
mapping is exactly that: a real, common, and precisely well-defined
feature, not a guess.

**Scalar subqueries nested inside a SELECT item** (PHASE_BACKLOG Phase
168's 168k - the last of the two previously-disclosed-but-not-yet-built
follow-ons; with this, every item Phase 168 ever disclosed is built).
`SelectItemKind::Subquery` (`checker/doctrine/dql.rs`) recognizes
`(SELECT ... FROM ...)` at a select item's own position and recurses
directly into 168b/168c's own grammar via
`parse_select_statement_tokens` - the core of 168g's own
`parse_select_statement` factored out to operate on an already-
tokenized buffer at an arbitrary starting position, shared rather than
duplicated, so the top-level `createQuery($dql)` entry point and a
nested subquery can never drift into two subtly different grammars.
`checker::doctrine::result_shape::select_item_type`'s new `Subquery` arm
resolves the inner query against its own, entirely independent alias
map (never the outer query's), then reuses `single_scalar_column_type`
(168f) completely unchanged for the "resolves to exactly one scalar
column" requirement a subquery-used-as-a-value needs - the identical
requirement `getSingleColumnResult()`/`getSingleScalarResult()` already
have. Only an *uncorrelated* inner query (a complete, self-contained
`FROM` of its own) is recognized at all; a subquery referencing an
outer alias isn't modeled and falls through to the ordinary
"undecidable" outcome, the same as any other construct this grammar
doesn't recognize. Zero real corpus usage (confirmed) - unit-tested
only, the same "corpus incidence affects verification confidence, never
whether a real, well-defined feature gets built" standard 168j was
built under.

**Opt-in call-site function/method-name-case checking, plus a real
case-insensitive-method-call bugfix** (PHASE_BACKLOG Phase 102's W3,
now fully built). Extends the existing `function.nameCase` mechanism
(`Registry::check_function_name_case`, previously only checking a `use
function` import's own casing) two ways: a plain function call's
`resolved_name` in `ExprKind::Call`'s handling (`checker/expr.rs`) is
already namespace-qualified the exact same way `check_class_name_
case`'s own `resolved` variable is (the raw, as-written spelling, just
namespace-prefixed), so the identical "does the canonical declared
spelling differ" comparison via `lookup_function_with_name` applies
unchanged; `Checker::check_call_site_method_name_case`
(`checker/resolve.rs`) covers a method/static-method call the same way,
via a new `Registry::lookup_method_canonical_name` (mirrors `lookup_
class`'s own case-insensitive-fallback shape), wired into both
`MethodCall` arms and both `StaticCall` arms.

Building the method-call half surfaced a real, pre-existing false-
positive bug along the way, fixed as part of the same change:
`Registry::lookup_method`/`lookup_method_with_owner` were exact-
spelling-only, unlike `lookup_class`/`lookup_function` (which have
always had a case-insensitive fallback) - so `$obj->DoStuff()` calling
a real, declared `doStuff()` was wrongly reported as `method.notFound`,
even though real PHP resolves method calls case-insensitively just
like class/function names. Both now fall back to a case-insensitive
scan of the already-precomputed `method_owner_by_class` map, fixing
every consumer transitively (undefined-method detection, visibility
checks, first-class-callable resolution, static calls) in one place.
Deliberately scoped to `lookup_method`/`lookup_method_with_owner`
only, not `lookup_ancestor_method_with_owner`/`find_method_signature_
prototype` (used for override-compatibility checking) - those must stay
exact-match, or Phase 101's E4 (`method.nameCase` for an override whose
own casing differs from its ancestor's) would lose the exact signal it
depends on to fire at all.

**PCRE pattern-validity checking** (Phase 46). New optional `pcre2-sys`
dependency (a new, first-ever C dependency, gated behind a `pcre-
validate` Cargo feature defaulted on - `--no-default-features` keeps the
crate building 100% pure Rust at the cost of only this one diagnostic),
using the low-level `pcre2_compile_8` FFI entry point directly rather
than the higher-level `pcre2` crate's safe `bytes::RegexBuilder` (which
has no way to set PHP's `J`/`PCRE2_DUPNAMES` modifier - a real, legal
pattern like `/(?<a>x)|(?<a>y)/J` would otherwise always report a false
"duplicate capture group" compile error). `checker/preg_pattern_
validity.rs` wires `regexp.pattern` (matching real PHPStan's own
identifier, level 0/always-on) into all 8 real `preg_*` functions
taking a pattern argument, including the array-of-patterns form for
`preg_replace`/`preg_replace_callback`/`preg_filter` and the keyed-array
form for `preg_replace_callback_array`. Also validates the no-closing-
delimiter case (own wording, since PHP's own exact C-source text for it
isn't vendored here to match verbatim) and PHP's own version-gated
delimiter/modifier wording. A real, corpus-confirmed bug found and
fixed during verification, in the shared `regex_groups::split_
delimiters_and_modifiers` this check and the pre-existing capture-group
scanner both depend on: real PHP's own `ext/pcre` skips leading
whitespace before reading the delimiter character (and tolerates
trailing whitespace after the modifiers) - confirmed live, `preg_split("
/abc/x", ...)` (a literal leading space) compiles and runs fine. This
isn't contrived: PHP's own flexible heredoc/nowdoc indentation stripping
(7.3+) only removes whitespace common with the *closing marker's* own
indentation, so a multi-line `/x`-flagged regex written with nested,
readable indentation deeper than its own closing marker legitimately
keeps literal leading spaces before its real delimiter - Symfony's own
`OptionsResolver::splitOutsideParenthesis()` is exactly this shape, and
was a real false positive before the fix.

**`@internal`'s remaining PHPDoc/attribute surface, and `restricted_
symbols`** (Phase 67). The `@internal` engine's `RestrictedClassNameUsage
Extension`-equivalent coverage - previously member access, instantiation/
instanceof/catch, and native param/return hints only - now also covers
every PHPDoc-tag class-name position (`@var`, `@assert`, `@method`,
`@mixin`, `@property`, `@phpstan-require-extends`/`-implements`/
`-sealed`, `@template` bound/default) and attribute usage, reusing the
exact threading network Phase 48's own `class.nameCase` work had just
built for those same positions - plus the real member-is-internal
exemptions (a static call/property/const access whose own *member* is
itself internal). New, separate declarative `restricted_symbols` config
(no CLI flag - a structured list doesn't fit one): bans a specific
function/method/static-method/property/static-property/class-constant/
class project-wide with a custom message, opt-in, mirroring the real
`spaze/phpstan-disallowed-calls` package's own shape rather than real
PHPStan core (which has no built-in implementation of this family at
all - every real instance is a hand-written third-party extension).
Case-sensitivity matches real PHP exactly: property/static-property/
class-constant keys are case-sensitive, function/method/static-method/
class keys are case-insensitive. The `Class` kind's own detection
surface (instantiation, `instanceof`, `catch`, `extends`/`implements`,
param/return type hints) reuses the identical `@internal`-engine
threading above.

**Class-specific `ReflectionMethod<T>`/`ReflectionProperty<T>` modeling**
(Phase 77). A synthetic, stub-patched `@template-covariant TClass of
object` (not present in real phpstorm-stubs, purely an internal
enrichment on top of it - the same technique already used for
`ReflectionClass<T>`'s own constructor binding) lets `ReflectionClass<T>
::getMethod()`/`getMethods()`/`getProperty()`/`getProperties()` return
`ReflectionMethod<T>`/`ReflectionProperty<T>` (propagating the
*receiver's* own template) instead of the stub's bare, untemplated
collections, and lets `ReflectionMethod<TClass>::getDeclaringClass()`/
`ReflectionProperty<TClass>::getDeclaringClass()` hand back a properly
`ReflectionClass<TClass>`. The new synthetic `TClass` is explicitly
exempted from `missingType.generics` (`ReflectionMethod`/
`ReflectionProperty` added to the same exemption list as `Traversable`/
`Iterator`/etc.) - required, not optional: without it, every ordinary
bare `ReflectionMethod $m` type hint anywhere (a return type, a param, a
`@var`) would start producing a brand-new false positive at level 6+,
since real PHPStan does not consider either of these two classes
generic at all.

**`DOMDocument::createElement()`/`SimpleXMLElement` validation** (Phase
78). `checker/xml_name.rs`'s `is_valid_xml_name` implements the W3C XML
1.0 `Name` production (colon-permissive - confirmed live against PHP
8.5, `createElement('a:b')` succeeds, so this is the base `Name`
grammar, not the namespace-restricted `NCName` one) validating
`(new DOMDocument())->createElement($name)`'s single tag-name string
argument before it would throw `DOMException` - structurally identical
in shape to the already-solved `DateInterval` duration-spec grammar, not
"full XML-content validation" the way ROADMAP originally, incorrectly,
lumped it with SimpleXML's own item. `checker/xml_wellformedness.rs`
adds a new, ordinary (non-optional, no C toolchain) `roxmltree`
dependency - cheaper than the C dependency Phase 46 already accepts in
this same pass - validating `new SimpleXMLElement($literal)`/
`simplexml_load_string($literal)`'s own XML well-formedness; the latter
gets the `argument.type` diagnostic only (it returns `false` on failure,
never throws, so it never touches the `@throws`-tag/dynamic-throw-type
family). Both wire into `dynamic_new_throw_type`/`dynamic_method_throw_
type` for the `@throws`-tag consumer (`DOMException` for `createElement`,
a plain `\Exception` for `SimpleXMLElement`'s constructor, confirmed
live against PHP 8.5 - there's no dedicated SimpleXML exception class).

**`reset`/`end`/`next`/`prev` exempted from by-ref argument widening;
`array_unique` non-emptiness preserved** (commit `06b1769`). `Checker::
calls`'s generic by-ref-argument-widening loop was unconditionally
widening these four functions' by-ref argument to its declared param
type, discarding a precise argument type their own return-type
computation still needed - they're real native by-ref stubs, but only
PHP's internal array pointer moves, never the array's own value, so
widening was always wrong (confirmed against `vendor/phpstorm-stubs` and
`dumpType()`). Fixing that exposed a second, real, pre-existing bug it
had been masking: `array_unique()` incorrectly stripped `NonEmptyArray`
too (real PHPStan preserves non-emptiness through `array_unique()`,
unlike `array_diff`/`array_intersect`). Fixes `SearchService::
getRegularSearchResults`'s `reset($imageIdsForFilter)`/`array_map(intval(...),
$first)` finding - the originally-suspected `ArrayShape`-all-optional-
fields gap (see this same finding's own former Known-limitations entry)
was investigated via a scratch probe and confirmed *not* the actual
cause; that mechanism (an `upgrade_array_to_non_empty` arm for an
all-optional-but-nonempty-field-list `ArrayShape`) was never built, and
remains a real, separately-unconfirmed gap if the same symptom
resurfaces elsewhere for a genuinely different reason.

**`array_splice()`'s missing by-ref effect** (commit `9320a82`). Added to
`predefine_byref_or_unknown_args`'s by-ref effects table, mirroring the
existing `array_push`/`array_shift` arms: a provably non-empty literal
`$replacement` argument alone proves the result non-empty (real PHP
always clamps the removal count to at most `count($array_before)`
regardless of `$offset`/`$length`, matching real PHPStan's own
`ArrayType::spliceArray()`), independent of the array's own prior state.
Fixes `SearchService::qsearchGetTextTokenSearchSql:1037`'s `return.type`
finding - `array_splice()` previously fell through to the generic
declared-param-type widening, discarding `$values`'s own precision down
to a bare `array`. This finding was originally investigated as a
genuine cross-variable correlation (`$clauses`/`$fts`'s own mutually-
exclusive loop growth) needing general formula-memory machinery - that
story was real but never actually load-bearing for this diagnostic: the
declared return type uses plain `list<T>` on both tuple slots, never
`non-empty-list<T>`, so `$clauses`'s non-emptiness was never a real
proof obligation, and the actual failure was this much narrower,
already-precedented by-ref-effect gap.

**Constants declared inside a trait need PHP 8.2+** (PHASE_BACKLOG Phase
136's `ConstantsInTraitsRule` item, `classConstant.inTrait`,
non-ignorable). `check_class_constant_modifiers` (`checker/oop.rs`,
already the home of `classConstant.finalNotSupported`/`.finalPrivate`/
`.nativeTypeNotSupported`) gained a fourth, independent check: when the
declaring class-like is a `Trait`, every one of its own `const`
declarations is flagged unless the configured PHP version is >= 8.2 -
the already-defined `PhpFeature::ConstantsInTraits` gate finally gets a
call site. Fires once per constant, at its own declaration site,
regardless of how many classes use the trait (matching real PHPStan's
own per-`ClassConst`-node rule shape).

**Three PHP-8.x-version-gated cast diagnostics** (PHASE_BACKLOG Phase
135, `UnsetCastRule`/`VoidCastRule`/`DeprecatedCastRule`). All three
reuse `check_cast_validity`'s own call site in `check_expr`'s `Cast`
arm, and all three already had their `PhpFeature` gate data defined
with no consumer (`UnsetCastRemoved`, `VoidCast`,
`DeprecatedNonStandardCasts`) - only the check logic itself
(`checker/coercion.rs`) was missing:
- `cast.unset` (non-ignorable): `(unset)` was removed as of PHP 8.0.
- `cast.voidNotSupported` (non-ignorable, PHP < 8.5) and `cast.void`
  (non-ignorable, whenever the cast isn't its own bare top-level
  statement) - independent checks, both can fire on the same
  occurrence. Reuses `check_expr`'s own already-existing
  `is_bare_statement` flag (the same one `check_void_cast`'s "first-
  level statement" question needs) rather than adding a new one.
- `cast.deprecated` (PHP >= 8.5): the four non-canonical spellings
  `(integer)`/`(boolean)`/`(double)`/`(binary)` - required widening
  `ExprKind::Cast(String, Box<Expr>)` to a 3-tuple
  (`..., bool`), since the normalized cast target alone can't
  distinguish `(integer)` from `(int)` once parsed. `(real)` (the
  *other* non-canonical spelling of `(float)`) is deliberately left
  unflagged, faithfully matching real PHPStan's own
  `DeprecatedCastRule`, which only checks `KIND_DOUBLE`, never
  `KIND_REAL`.

**`InvalidAssignVarRule`, all three cases** (PHASE_BACKLOG Phase 135,
`nullsafe.assign`/`nullsafe.byRef`/`assign.invalidExpr`, all non-
ignorable). Three `checker/assignment.rs` checks:
- `contains_nullsafe` walks an assignment target's own access chain
  (`PropertyAccess`/`MethodCall`'s own nullsafe flag, recursing through
  `Index`/a `StaticCall` with a real expression receiver/a destructuring
  `ArrayLiteral`'s items) - `$a?->b = 1;` is a real, unconditional PHP
  fatal regardless of any type information, not a narrowing question.
  Doesn't see through `DynamicPropertyAccess`/`DynamicMethodCall`
  (`$a?->{$expr}`), which don't carry their own nullsafe flag at all
  (see `ExprKind::DynamicPropertyAccess`'s own doc comment) - a real,
  disclosed, narrow gap. Checked against both `Assign`'s/`AssignRef`'s
  own LHS (`nullsafe.assign`) and, for `AssignRef` specifically, its own
  RHS too (`nullsafe.byRef` - `$a = &$b?->c;`).
- `nullsafe.byRef` needed a genuinely new `ExprKind::AssignRef` variant
  first, distinct from `Assign` - this project previously had no way to
  tell an assign-by-reference (`$a = &$b;`) apart from a plain
  assignment at all (the `&` was just consumed and discarded at parse
  time), so there was no RHS position to check nullsafe against.
  Propagated through every real consumer a full semantic review turned
  up (narrowing, purity disqualification, uninitialized-property/
  private-member/global-variable write tracking, ...), each decided
  independently on whether `AssignRef` should behave like `Assign`
  (almost always yes) or differently (declining a few combinations real
  PHP itself doesn't allow, like a byref-assigned literal or `new`
  expression).
- `is_non_assignable_target` confirmed a real, live gap first: unlike a
  grammar that would reject it at parse time, this project's own
  `parse_assignment_tail` accepts *any* expression as an assignment's
  left-hand side (needed for the `if (false === $x = fn())`-style
  guard-clause idiom documented on that same function), so `foo() =
  1;`/`1 = 2;` genuinely reach the checker rather than already being a
  parse error. Mirrors real PHPStan's own `containsNonAssignableExpression`
  allow-list (`Variable`/`PropertyAccess`/`Index`/`StaticAccess`/a
  recursively-valid destructuring `ArrayLiteral`), with one deliberate
  addition: `DynamicPropertyAccess` also counts as assignable, since
  real PHPStan's own AST never distinguishes a computed property name
  from a plain one at the node-type level in the first place (both are
  just `PropertyFetch`) - `MethodCall`/`DynamicMethodCall` intentionally
  get no such treatment, since neither is ever a valid assignment target
  in real PHP either.

**`BacktickRule`** (PHASE_BACKLOG Phase 135, `backtick.deprecated`, not
non-ignorable). A new `ExprKind::ShellExec` wraps the identically-shaped
`Call(Name("shell_exec"), [cmd])` a plain `shell_exec($cmd)` call would
itself produce (the parser already built this exact node for a `` `cmd`
`` occurrence) purely so this check can tell "this was literally
backtick syntax" apart from an author writing `shell_exec()`
themselves - every other consumer (type inference, argument checking,
the generic `ast_walk` traversal helpers) delegates straight through to
the wrapped `Call`, unchanged.

**`PipeOperatorRule`** (PHASE_BACKLOG Phase 135, `pipe.byRef`). PHP
8.5's `|>` always passes by value, so a right-hand callable whose first
parameter is declared by-reference can never actually receive anything.
Real PHPStan resolves this against the right side's own *type*
(`getCallableParametersAcceptors`), but this project's own inferred type
for a closure/arrow-fn *literal* is the coarse `Type::Object("Closure",
[])`, not a real parameter-shaped `Type::Callable` - building that
machinery first would be a large, disproportionate undertaking for this
one narrow check. `checker/pipe_operator.rs` instead resolves directly
against the right-hand side's own literal syntax - a closure/arrow-fn
literal, a first-class callable (`foo(...)`/`$obj->method(...)`/
`Foo::method(...)`), a bare string function name, or a two-element
`[$obj, 'method']`/`['Class', 'method']` array callable - covering every
real pipe-target idiom; anything else (a plain variable holding an
already-built `Closure`) is silently skipped, matching real PHPStan's
own silent skip whenever its type-level resolution can't produce a
confident `isCallable()` type either.

## Performance & tooling

CI-friendly output formats (`json`, `checkstyle`). Baseline suppression
(`--generate-baseline`/`--baseline`), on by default reporting any
baseline entry a run no longer matches against a current diagnostic
(mirrors real PHPStan's own `reportUnmatchedIgnoredErrors`), and inline
`@phpstan-ignore-line`/
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

`check-project`'s own parse and check phases both run on a shared
rayon thread pool (`main/check_project.rs`) - `Project::check_file`
only needs `&self`, so Rust's ownership model gets cross-core sharing
of one live `Registry` for free, unlike real PHPStan's own separate-
process model. Files are pre-sorted largest-first before the check
phase's own `par_iter()` to shrink the straggler tail beyond what
rayon's built-in work-stealing alone closes. Never previously recorded
here despite being fully built - this note corrects that gap.

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

**LSP `textDocument/codeAction` fix wiring, whole-project references/
rename, `signatureHelp`, `workspace/symbol`, `documentHighlight`,
`semanticTokens`, and `inlayHint`** (PHASE_BACKLOG Phase 104's F2/F3,
108's AM1, 107's AH2, 109's AO2, and 106's Y1/Y2 - all found already
fully built during this cleanup pass, via the `VSCODE_EXTENSION_PLAN.md`
work, never previously recorded in this Done section). `textDocument/
codeAction` (F2) reads `Diagnostic.fix: Option<DiagnosticFix>` off
`self.last_diagnostics` and offers one quick-fix per diagnostic that
carries one - the wiring itself has zero production call sites
populating a real `fix` yet (that's Phase 111's own, still-open job).
Whole-project `textDocument/references`/`rename` (F3) is backed by a
real two-tier use-site index (`project::use_sites`): Tier 1 (class/
function references) is a purely syntactic name-resolution walk kept
live on every `insert_parsed_file`/`update_file`; Tier 2 (member
references) needs the checker's own receiver-type resolution and is
built lazily, merged in by `Project::ensure_member_index_built`/
`reindex_tier2_for_file`. Local-variable references/rename stay
explicitly out of this v1's scope. `textDocument/signatureHelp` (AM1)
resolves the active parameter from a real span-containment/gap query
(the cursor is typically *between* argument spans, including the
not-yet-typed-argument and trailing-comma cases), served from the
last-successfully-parsed AST. `workspace/symbol` (AH2) fuzzy-searches
every declared class/enum/interface/trait, top-level function, and
class/enum member off `Project::decl_locations()`, resolving each
match's own `SymbolKind`. `textDocument/documentHighlight` (AO2)
generalizes `navigation::resolve_at`'s single-best-match pattern into a
same-file `Vec` accumulator via `navigation::collect_all`.
`textDocument/semanticTokens` (Y1) push-highlights every
`@deprecated`/`@internal` usage site unconditionally, independent of
whether either diagnostic flag is enabled - the checker already looks
up the callee's sig at every such site regardless, only the
diagnostic-emission call is gated. `textDocument/inlayHint` (Y2) covers
both halves: parameter-name hints before call arguments, and inferred-
type hints on unannotated local-variable assignments, both collected in
one `Checker::check_program` pass via the `collect_inlay_hints` gate
and cached per-file by version counter.

**LSP `textDocument/typeDefinition`** (PHASE_BACKLOG Phase 108's AM2).
Jumps from an inferred `Type` to that type's own class declaration(s),
reusing the identical `probe_receiver_type`/`type_to_classes` pair
hover's `Instance`/`VariableSelf`/`VariableRead` arms already use to
answer "what type is at this span" - the only new work is resolving
each class name the `Type` names to its own `decl_locations` entry
instead of formatting it as text. On a member-access receiver
(clicking the member name), it jumps to the *receiver's own* inferred
class, not wherever the member itself is declared - a real, different
answer from ordinary `textDocument/definition` whenever the member is
inherited (`$x->bar()` where `$x: Foo extends Base` and only `Base`
declares `bar` still jumps to `Foo`). A `Union`/`Intersection` receiver
resolves to every real class it names, mirroring `definition`'s own
multi-location handling. `Class`/`Function`/`Member` decline (`None`):
none names a value with an inferred type to jump *from* in this v1.
AM2's own stated blocker on Phase 104's F1 turned out to already be
moot - `probe_hit_at` was already wired for `ExprKind::Variable` by
Phase 17 above, so the bare-local-variable half needed no new
checker-side plumbing at all.

**LSP `textDocument/documentSymbol`** (Phase 107 AH1). A per-file
hierarchical outline, built directly from `Project::ast`'s own
top-level declarations (`src/lsp/document_symbol.rs`) rather than the
project-wide, FQN-flattened `decl_locations` index above, which has no
per-file nesting to offer an outline. Classes/interfaces/enums nest
their own consts/properties/methods/cases as children; a declaration
inside a top-level `if`/`Block` is still walked, mirroring
`DeclLocations::index`'s own recursion.

**LSP server hardening and the VSCode extension client, all of it**
(`VSCODE_EXTENSION_PLAN.md` in full - the document itself is retired,
folded in here, since every one of its ~30 action items landed with
nothing left open; see this paragraph's own closing note for the one
disclosed exception). The plan's own diagnosis - "the LSP server today
behaves like a per-file hover/definition helper, not like `check-project`
running live," plus "the VSCode extension client doesn't exist anywhere
in this repo yet" - is no longer true of either half:

- *Trustworthy diagnostics* (the plan's Phase 1): `lsp/workspace_config.rs`
  resolves a workspace's own `phpstan-rs.toml` (or an override) and calls
  the same `composer::discover_vendor_context` sequence `check-project`
  itself uses, before the workspace ever loads - a team's CI-enforced
  settings (deprecated/internal-usage checks, a baseline, `php_version`)
  now apply identically in the editor. The workspace scan filters
  `vendor/`/`.git/`/`node_modules/` the same way `check-project`'s own
  analyzed-vs-context-only split does, loading `vendor/` only through the
  precise autoload-map-driven vendor context rather than a blind recursive
  walk. Diagnostics for the *whole* workspace publish once loading
  finishes, not only for files that happen to get opened/edited in a
  session. `Diagnostic::level` now maps to a real `DiagnosticSeverity`
  tier instead of every finding rendering as a hardcoded red squiggle.
- *Concurrency* (Phase 2): the server no longer parks its only thread in
  a blocking stdin read - `Connection::stdio()` plus a `recv_timeout`-
  based loop (`RUN_LOOP_TICK`, `src/lsp.rs`) frees it to debounce and
  answer `hover`/`definition` against last-good state while a recheck
  runs. `spawn_debounce_worker`/`DebounceHandle`/`handle_debounced_change`
  coalesce rapid `didChange` bursts behind a short timer, replace-and-
  restart on a newer edit for the same URI, and drop an already-completed
  but now-stale recheck's diagnostics via a per-URI generation counter
  (`current_generation`) rather than publishing them. `Project::
  remove_file`/`remove_files` unwind a deleted/moved file's registry
  contributions the same way `update_file` already unwinds stale
  declarations before re-indexing. `publishDiagnostics` is stamped with
  the real document version. The initial workspace scan no longer blocks
  `initialize`'s own response.
- *Protocol completeness* (Phase 3): `workspace/didChangeWatchedFiles`
  (external `git checkout`/`composer install`/other-tool changes),
  multi-root workspace loading plus `workspace/didChangeWorkspaceFolders`,
  live `workspace/didChangeConfiguration` reload, outbound-request
  plumbing (the concrete prerequisite the previous three needed), UTF-8
  `position_encoding` negotiation at `initialize` (`starts_with_drive_letter`'s
  own sibling fix, Windows `file:///C:/...` URI round-tripping, landed
  alongside it), `window/logMessage` surfacing for a workspace file that
  fails to load (previously silent `eprintln!`), and `$/cancelRequest`
  plus exit-code-compliant `exit` handling for inbound requests.
- *The VSCode extension client itself* (Phase 4, previously "100% net-new
  work... confirmed absent everywhere," now real, tested, and directly
  used): lives under `editors/vscode/`, a standard `vscode-languageclient`
  extension (`extension.ts`) with `phpstan-rs.enable`/`.configPath`/
  `.serverPath`/`.trace.server` settings, a `phpstan-rs.restartServer`
  command, and a config-change prompt. Binary distribution follows rust-
  analyzer's own pattern - a release CI job cross-compiles per target
  triple and the extension's `serverBinary.ts`/`serverDownload.ts`
  auto-download and cache the matching asset on first activation, with
  `serverPath` as a local-build override. A real test harness
  (`editors/vscode/src/test/`, `@vscode/test-electron` for an integration
  lane inside a genuine Extension Development Host, plain `node:test` for
  the unit lane) plus its own CI workflow
  (`.github/workflows/extension-ci.yml`) back it, and a packaged
  `phpstan-rs-vscode-0.0.1.vsix` exists and has been used directly in a
  real VSCode session.

Beyond the plan's own original ~19 numbered items, the same cleanup pass
found several further LSP capabilities already fully built with no
previous record here: `textDocument/completion` (member, static-access,
and class-name completion) is the one genuinely new mention added by
this paragraph; `codeAction`/references+rename/`signatureHelp`/
`workspace/symbol`/`documentHighlight`/`semanticTokens`/`inlayHint`/
`typeDefinition`/`documentSymbol` were each already recorded in their own
paragraphs elsewhere in this Done section (see just above) rather than
repeated here.

Two small, disclosed items stay open, neither a correctness gap:
local-variable references/rename stay explicitly out of scope (the
whole-project reference index is class/function/member-level only, not
per-variable), and the extension has no bundler (esbuild/webpack) yet -
its packaged `.vsix` ships `node_modules/` as-is for its one runtime
dependency (`editors/vscode/README.md`).

**Fuzz testing infrastructure** (Phase 19). `cargo-fuzz` for the lexer/
parser: `lex_parse` (primary target, mirroring production's exact
`String::from_utf8_lossy`+lex+parse pipeline, including its own 64MiB
worker-thread stack - libFuzzer's default ~8MiB main-thread stack would
trivially and repeatedly rediscover an already-fixed stack-overflow bug,
drowning out any new finding) and `token_stream_parse` (secondary,
bypasses the lexer via `arbitrary`-derived `Token`/`Keyword`/
`StringPart`, gated behind a new opt-in `fuzzing` Cargo feature so the
main crate never pulls in `arbitrary` for ordinary builds), both seeded
from this project's own already-vendored real-world PHP. Two more
targets, independently developed and merged in alongside these: `structured_php`
builds a small, hand-written `Arbitrary`-driven PHP-fragment generator
(`if`/`while`/`match`/`class`/closures/named+spread arguments/heredoc/
nullsafe access/first-class-callable syntax, each with randomized real
pieces) instead of raw bytes, going through the heavier `project::
parse_file` pipeline (`check-project`'s own path, a superset of
`lex_parse`'s lighter `check`-command one) rather than a raw `Lexer`+
`Parser` pair - reaches real recursive nesting depth through genuine
structure and always actually enters PHP mode, rather than relying on
byte mutation to stumble onto a `<?php` prefix by chance; `parse_type_string`
fuzzes `types::parse_type_string` (the PHPDoc type-string parser)
directly - a separate recursive-descent parser over raw untrusted
strings, same risk shape as the lexer/parser but previously unfuzzed,
with no `MAX_EXPR_DEPTH`-style guard of its own. A libFuzzer dictionary
(`fuzz/php.dict`, built from this project's own token tables) is also
available for `-dict=`. `token_stream_parse` found a real, still-open
crash within the first few seconds of a smoke run merging this in: a
synthetic `Token::InterpString` containing a `ComplexExpr` with garbage
`Position` values causes a "deadly signal" (not a clean panic) - not yet
root-caused. Dev-only, an ongoing local workflow (`cargo +nightly fuzz
run`), not a CI gate.

**Extension API, Axis 1 + Axis 2** (Phase 18). Axis 1 (declarative,
config-driven symbol restriction) is `restricted_symbols`, above. Axis
2: a new public `ReflectionRule` trait (`src/reflection_rule.rs`) over
already-latent-public reflection data (`Project::registry()`/
`Registry.classes`/`.functions`, `Diagnostic`, `ast::{Stmt, Expr, ...}`
were already `pub` with fully-`pub` fields, an accidental extension
surface this formalizes rather than invents) - default no-op trait
method bodies, `Project::run_reflection_rules` iterating every known
class/function. Explicitly documented as *not* parity with real
PHPStan's own `Rule<TNodeType>`: no inferred per-expression type, no
flow-sensitive narrowing, no `Locals` visibility. Axis 3 Stage 1 (an
internal `Vec<Box<dyn InternalRule>>` trait-object dispatch mechanism,
proving the pattern for a future public `Rule<TNode>` equivalent) is
also done - see Pending for Stage 2, gated on this Stage's own corpus
diff (confirmed exactly clean).

---

# Pending / partial

Each entry: what's missing, and the root cause blocking it where one's
known. Numbered by the original phase for cross-reference; the number
is not a priority order. Cross-phase dependencies, when a phase's own
entry has one, are named inline in that entry's own text - a separate
dependency graph (`PHASE_DEPENDENCIES.md`) was maintained through most
of this project's development, retired once every edge it tracked
became satisfied, then reinstated after this session's reevaluation of
the boundaries/won't-port lists produced new phases (158-175) with live
dependency edges among them.

**18 - Extension/plugin API, Axis 3 Stage 2 (PARTIAL).** Split into 3
axes: Axis 1 (declarative `restricted_symbols`) and Axis 2 (a public
`ReflectionRule` trait over already-public reflection data) are fully
done (see Done). Axis 3 (a flow-sensitive, AST-node-dispatched
`Rule<TNode>` equivalent) is staged: Stage 1 - migrating `Method
ThrowTypeCovarianceRule` off its hardcoded call site onto a real
internal `Vec<Box<dyn InternalRule>>` trait-object dispatch mechanism -
is done and its own mandatory full corpus diff came back exactly clean
(a pure internal refactor, zero behavior change), which is what actually
unblocks Stage 2 rather than assuming it. Stage 2 - exposing a public
`trait Rule`/`Scope`-equivalent, now that Stage 1 has proven the
dispatch pattern - is not yet built: still needs a public-API-stable
`Scope` facade over `Checker`'s currently `pub(crate)`/module-private
flow-sensitive state (`Locals`, `ProvenanceMap`, `GuardedMemberSet`) and
a real per-node-kind injection point into `check_program`'s single-pass
traversal, both real, disclosed engineering work rather than a size/
demand call. Declined outright, not staged: any `dlopen`/`cdylib`
dynamic-loading mechanism (no stable Rust ABI without a large separate
FFI project) and a NEON registration syntax (no parser exists project-
wide for NEON, already declined elsewhere for the same reason).

**48 - Symbol-existence pipeline, remaining stretch goals (PARTIAL).**
The existence-check MVP, typed-class-constant validation, multi-
namespace-per-file positional tracking, a `use`-imported build-
tool-mangled-prefix check (`class.prefixed`), and the full `class.
nameCase`/`interface.nameCase`/`trait.nameCase`/`enum.nameCase` family
(every native-syntax and PHPDoc-typed position, including property
hooks' own `set()` parameter, `@phpstan-assert`-family tag targets,
`@phpstan-require-extends`/`-implements`/`-sealed` RHS, and closure/
arrow-fn literal param/return hints) are all done (see Done), along with
the real `Parser::take_name_segment` keyword-as-identifier case-
corruption bug (`class Enum { ... }` silently renaming itself to `enum`)
that family's own corpus round first surfaced. `ExistingClassesInPropertyHookTypehintsRule`'s
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

**54 - Generic template richness (PARTIAL, variance-aware assignability,
arity, and cross-path conflict checking all done).** See Done for what
shipped - by this point essentially all of `GenericObjectTypeCheck`
(pervasive arity, bound-checking, call-site-variance
conflict/redundancy), `TemplateTypeCheck`'s own declaration-validity
family, `MethodTemplateTypeRule`'s shadow-check, `VarianceCheck`'s
own-declaration half, `UsedTraitsRule`'s bound/arity/variance half via
`@use Trait<Args>`, and `CrossCheckInterfacesHelper`
(`generics.interfaceConflict`, cross-path diamond-generic conflicts)
are built - only each rule's own real-PHPStan-specific residual stays
disclosed-open (bound/default existence-checking against this
project's inherently partial registry; `UsedTraitsRule`'s own separate
tag-validity diagnostics and its call-site-variance-never-allowed
rule). Declined, each for a verified reason: real PHPStan's `Static`
call-site/internal variance kind (pure internal control-flow plumbing,
no user-observable gap); the ~19-concrete-subclass internal type
representation real PHPStan uses internally (no independently
observable behavior this project's single-variant-plus-sentinel
representation doesn't already produce); `generics.notGeneric`
(indistinguishable, in this project's own `@template`-tag model, from a
legitimate stub-granted-generics case on a real zero-`@template`
built-in); a `@method` tag's own inline generic template syntax (zero
corpus value, and this project's `@method`-tag parser has no
template-syntax support to build on at all).

Phase 54/77 follow-up (now fully done, see Done): constructor-argument-
driven class-level `@template` binding, including a bare-string-literal
`new ReflectionClass('Foo')` form (not just `Foo::class`). The
previously-disclosed `Intersection`-vs-`Intersection` covariant-generic-
in-`list<>` gap was re-investigated and confirmed **not to reproduce**
against current HEAD - the specific mechanism blamed (a generic `.any()`
arm decomposing `sub` before `sup`) cannot occur for that shape at all,
since the specific, textually-earlier `Intersection`-vs-`Intersection`
arm has existed since Phase 14, predating the note that named it a gap.

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

**67 - `@internal`/`restricted_symbols`, remaining (PARTIAL).** The
`@internal` restricted-usage engine (calls, property/constant access,
instantiation, `instanceof`, `catch`, `extends`/`implements`/trait-use,
native param/return type-hints, first-class-callables, implicit
`__toString()`, and now every PHPDoc-tag/attribute-usage class-name
position too) and the declarative `restricted_symbols` config list (this
phase's own answer to the separate, user-configured restricted-usage
mechanism real PHPStan's third-party extensions implement) are both done
- see Done. Declined, a genuine missing-prerequisite gap rather than a
scope trim: the real `spaze/phpstan-disallowed-calls` package's own much
larger allow/disallow-in filter DSL (~12 keys, attribute-*context*-
matching beyond plain attribute *usage*) - no call-site-scope-path/
attribute-context matching mechanism exists anywhere in this checker to
build on yet, and its namespace/enum/superglobal/keyword-specific
siblings are each a materially different detection shape needing its
own separate investigation.

**77 - Runtime-capability modeling, remaining (PARTIAL).** Most of the
originally-scoped functions, `SimpleXMLElement`'s own magic-property
type resolution, `ReflectionClass<T>`'s own constructor-argument-driven
binding, and class-specific `ReflectionMethod<T>`/`ReflectionProperty<T>`
modeling (`getMethods()`/`getProperties()`/`getMethod()`/`getProperty()`
now propagate the receiver's own `T` instead of returning the stub's
generic, untemplated collections) are all done (see Done). Binding a
*specific* member's own value type (a literal property/method name
resolved at the type level for `getValue()`/`invoke()`) remains a
further, separate stretch beyond what any real PHPStan extension does
either - not attempted, not a disguised decline.

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

**84 - `RuleLevelHelper`-equivalent level-aware type-acceptance,
remaining (PARTIAL).** `Registry::accepts_at_configured_level` (see
Done) closes the real gap at its own 8 target call sites: `project.level`
now genuinely modulates argument/return/property-type strictness
(stripping `Null` below level 8, `.any()`-vs-`.all()` below level 7),
including the level-8+-strict nullable-type case that was the feature's
single most common real-world trigger. Declined, a change large and
disconnected enough from this item's own 8-call-site scope to need its
own future phase: `checkExplicitMixed`/`checkImplicitMixed`/
`checkBenevolentUnionTypes` (real level 9/10) - `Type::Mixed` has no
explicit-vs-implicit provenance field to key them off, and no
`BenevolentUnionType` analogue exists at all; building either means
threading provenance through every inference site that currently
collapses to `Mixed`.

**Receiver-type-checks phase (DONE).** Nine real-PHPStan identifiers for
"this receiver's own type provably can never support the operation at
all" - `method.nonObject`/`property.nonObject` (share one classifier,
with a `stdClass`-specific carve-out for the method half only, verified
against `ObjectType::canCallMethods()`/`canAccessProperties()`/
`isCloneable()`), `offsetAccess.nonOffsetAccessible` (with `Null`
exempted, matching `NullType::isOffsetAccessible()`'s own real-PHPStan
`yes`), `foreach.nonIterable`, `offsetAccess.nonArray` (array/list
destructuring, `Null` *not* exempted here, unlike offset access),
`clone.nonObject`, `callable.nonCallable` (with a `resource`-specific
carve-out and a `Null`-stripped-from-`Union` correction found by a real
pre-existing test failure, not by re-reading), and the two spread/unpack
shapes `arrayUnpacking.nonIterable`/`argument.unpackNonIterable`. Landed
as 7 separately-committed sub-items (shared classifiers folding into the
first check that needed them), each with its own targeted tests plus a
full `checker::` regression run. Deliberately stricter than real
PHPStan's own filtered-Union default for these checks specifically (a
`Union` is flagged the moment *any* member is confirmed closed, mirroring
this project's own pre-existing `member_confirmed_missing` precedent,
not real PHPStan's `TrinaryLogic::lazyExtremeIdentity`-plus-`reportMaybes`
combinator) - confirmed via a follow-up real-PHPStan corpus diff (see
"Newly identified gaps" below) to produce zero false positives on
piwigo16: every phpstan-rs-only diagnostic in that diff traced either to
real PHPStan describing the same location as bare `mixed` (a message-
granularity difference, not a disagreement) or to real PHPStan's own
scope losing precision after a variable's first array-key write and
silently downgrading to `mixed` for every later read (this project's own
array-shape tracking doesn't degrade the same way, so it keeps flagging
locations real PHPStan's default configuration goes quiet on - still
genuine unchecked-`X|false` reads). Corpus-verified against both
`~/piwigo16` (148 new diagnostics, 0 removed) and `~/piwigo17-frozen`
(10 new diagnostics, 0 removed); `callable.nonCallable`/`clone.nonObject`/
both unpack identifiers had zero non-`mixed` corpus hits in this
particular corpus, per this project's own "corpus incidence isn't a
decline reason" precedent - shipped unconditionally regardless, backed
by their own unit tests.

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

**Follow-up batch (real-PHPStan-vs-phpstan-rs corpus diff, not the
20-agent audit)**: after the receiver-type-checks phase landed, a direct
`vendor/bin/phpstan analyse` (level 10, its own configured level) run
against `~/piwigo16` was diffed identifier-by-identifier against a
same-scope `phpstan-rs check-project --config phpstan-rs.toml` run on
the same checkout, restricted to files both tools actually scan. Every
identifier real PHPStan emitted that phpstan-rs's own source has no
matching `"identifier.string"` literal for at all (not just "zero hits
in this corpus" - confirmed by grepping `src/` directly, since level
10's `checkImplicitMixed` inflates or silences many identifiers'
corpus incidence in ways unrelated to whether the check exists) was
cross-checked against every existing phase/Pending/Won't-port/Known-
limitations mention project-wide before being added below - most of
real PHPStan's ~116 distinct identifiers on this corpus were already
covered or already documented as a gap elsewhere (e.g. `identical.
alwaysFalse`/`alwaysTrue`, `instanceof.alwaysTrue`/`alwaysFalse`,
`new.static`, the whole `unset.*` family, `parameter.defaultValue`/
`.phpDocType`, `property.defaultValue`, `varTag.misplaced` - all
already have a line above). What follows is only the residue: real,
verified-absent-from-`src/`, previously-undocumented gaps.

- **`Rules/Comparison/BooleanAndConstantConditionRule.php`/
  `BooleanOrConstantConditionRule.php` - `and`/`or` word-operator
  identifier fidelity**: not a missing check - `check_boolean_and_or`
  (`src/checker/control_flow/boolean_conditions.rs:68-145`) already
  covers the "left/right/whole-condition already constant" family
  completely. The gap is identifier *naming*: this project's AST has a
  single `BinOp::And`/`BinOp::Or` variant (`src/ast.rs:578-579`) for
  both the symbol spelling (`&&`/`||`) and the word spelling (`and`/
  `or`), so the check always reports under `booleanAnd.*`/`booleanOr.*`
  - real PHPStan's own parser keeps them distinct and reports the
  word-operator spelling as `logicalAnd.*`/`logicalOr.*` instead (a
  different `$identifierType` string passed into the same shared rule
  class). Cosmetic for a human reader, but would break exact-identifier
  matching against a real-PHPStan-generated baseline file for any
  `and`/`or`-spelled condition.
- **`Rules/Comparison/ImpossibleCheckTypeFunctionCallRule.php`**:
  flags calling a type-check function (`is_string()`, `is_int()`,
  `is_array()`, ...) whose outcome is already statically decided by the
  argument's own inferred type - `function.alreadyNarrowedType` (the
  check is always true, e.g. `is_int($x)` where `$x` is already known
  `int`) and `function.impossibleType` (always false, e.g. `is_string($x)`
  where `$x` is already known `int`). phpstan-rs has the PHPDoc-tag
  analogue of this exact pair - `assert.alreadyNarrowedType`/
  `.impossibleType` (`src/checker/narrowing.rs:1488-1499`, for
  `@phpstan-assert`/`-if-true`/`-if-false` tag validity) - but nothing
  wired to the far more commonly-hit plain-function-call case (108095
  scoped diagnostics dominate the piwigo16 level-10 corpus overall, but
  isolating this one rule alone: 60 `function.alreadyNarrowedType` + 26
  `function.impossibleType` hits).

## DeadCode

- **`Rules/DeadCode/NoopRule.php`'s generic fallback**: phpstan-rs's own
  `noop.rs` covers a set of specific named side-effect-free-statement
  cases (`function.resultUnused`, `method.resultUnused`,
  `staticMethod.resultUnused`, `logicalXor.resultUnused`,
  `ternary.resultUnused`, and the `booleanAnd`/`booleanOr` family's own
  `.resultUnused` sub-case) but has no generic catch-all for every
  *other* provably-side-effect-free expression statement (a bare
  comparison `$a == $b;`, a bare arithmetic expression, a bare property/
  variable read used as a whole statement) - real PHPStan's own
  `expr.resultUnused` is exactly this fallback, the last case in
  `NoopRule.php` after every more specific one has been ruled out.

## Classes

- **`Rules/Classes/UnusedConstructorParametersRule.php`**: shares its
  underlying `UnusedFunctionParametersCheck` with phpstan-rs's own
  already-implemented `parameter.unused`/`closure.unusedUse`
  (`src/checker/unused.rs:66,98`), but real PHPStan's own check
  deliberately reports an unused *constructor* parameter under its own
  distinct identifier, `constructor.unusedParameter`, rather than the
  generic `parameter.unused` - phpstan-rs's `unused.rs` never special-
  cases `__construct`, so an unused constructor parameter is still
  correctly flagged today, just always under the generic identifier.

- **`Rules/Classes/ImpossibleInstanceOfRule.php`**: Real PHPStan's ImpossibleInstanceOfRule reports instanceof.alwaysTrue/instanceof.alwaysFalse/instanceof.invalidExprType whenever an `$x instanceof Foo` expression's own inferred type resolves to a literal boolean (or the dynamic class-name expr is not string|object) - phpstan-rs's own unreachable-code family deliberately excludes Instanceof from its generic if/while/ternary...
- **`Rules/Classes/NewStaticRule.php`**: Real PHPStan's NewStaticRule reports the well-known 'Unsafe usage of new static().' (identifier new.static) whenever `new static()` is used in a non-final class without a consistent-constructor guarantee; NewStaticInAbstractClassStaticMethodRule reports a related, more specific case (new.staticInAbstractClassStaticMethod) inside an abstract class's own static method.
- **`Rules/Classes/InstantiationCallableRule.php`**: Real PHP's grammar (and nikic/php-parser's New_ extending CallLike with isFirstClassCallable()) genuinely accepts `new Foo(...)` syntactically; real PHPStan's InstantiationCallableRule exists specifically to flag it as illegal ('Cannot create callable from the new operator.', identifier callable.notSupported) since it's a real semantic error, not a parse error.
- **`Rules/Classes/InstantiationRule.php`**: phpstan-rs's `new` expression check (src/checker/expr.rs, ExprKind::New arm) only implements two of InstantiationRule's several sub-checks: new.interface (instantiating an interface) and new.abstract (instantiating an abstract class).
- **`Rules/Classes/ExistingClassesInClassImplementsRule.php`**: phpstan-rs's own check_class_references_exist (src/checker/oop.rs:263-331) is deliberately, per its own doc comment, existence-only: it confirms an extends/implements/trait-use target resolves to *some* known symbol, but never checks it's the *right kind* of symbol.
- **`Rules/Classes/InvalidPromotedPropertiesRule.php`**: Real PHPStan's InvalidPromotedPropertiesRule enforces several legality constraints on constructor property promotion: promotion is only legal inside `__construct` (property.invalidPromoted otherwise), not inside an abstract constructor, never on a variadic parameter, and is itself gated on PHP 8.0+ (property.promotedNotSupported) with `final` promotion additionally gated on...
- **`Rules/Classes/ReadOnlyClassRule.php`**: The PHP-version-gate data for this exact rule already exists in the codebase (PhpFeature::ReadOnlyClasses / ReadOnlyAnonymousClasses in src/php_version.rs, with a doc comment explicitly naming ReadOnlyClassRule as the real-PHPStan source), but it is never actually consumed by any checker call site - a `readonly class Foo {}` (needs PHP 8.2+) or `new readonly class {}` (needs...

## Functions

- **`FunctionReturnTypeCheck.php`'s bare-`return;` validity check**:
  `return.empty` flags a bare `return;` (no value) inside a function/
  method whose declared return type requires one (i.e. isn't `void`/
  `never`, and isn't an implicitly-nullable type). Distinct from the
  already-covered `return.missing` (a code path falling off the end
  with no `return` at all) and the already-tracked `return.never`
  (Done, `FunctionReturnTypeCheck.php`/`MissingReturnRule.php`'s own
  validate-direction check) - phpstan-rs has neither this specific check nor any mention
  of it (5 corpus hits on piwigo16 at level 10).
- **`Functions/CallCallablesRule.php (and the related dynamic-target half of FunctionCallableRule.php's callable.nonCallable check)`**: Real PHPStan's CallCallablesRule checks a dynamic call `$callable(...)` (callee is an arbitrary expression, not a plain function name): flags a non-callable value (callable.nonCallable), an inaccessible private/protected array-callable method (callable.inaccessibleMethod), and runs full arity/parameter-type checking against the resolved callable's signature.
  **Partially closed**: the receiver-type-checks phase (Done) built
  `callable.nonCallable` for exactly this dynamic-callee shape
  (`type_confirmed_not_callable`, `src/checker/assignment.rs`). Still
  open: `callable.inaccessibleMethod` and running arity/parameter-type
  checking against a resolved `[$obj, 'method']`/`'Foo::bar'`-shaped
  callable's own signature.
- **`Functions/IncompatibleDefaultParameterTypeRule.php (+ IncompatibleClosureDefaultParameterTypeRule.php, IncompatibleArrowFunctionDefaultParameterTypeRule.php)`**: Real PHPStan (level 2) checks that a parameter's default value expression's type is assignable to that parameter's declared type hint (identifier parameter.defaultValue) for functions, closures, and arrow functions alike. phpstan-rs's AST carries a `default: Option<Expr>` on every parameter but no checker code anywhere validates it against the parameter's type hint.
- **`Functions/ReturnNullsafeByRefRule.php (+ ArrowFunctionReturnNullsafeByRefRule.php)`**: Real PHPStan flags 'Nullsafe cannot be returned by reference' (identifier nullsafe.byRef) when a by-ref function/arrow-function returns an expression whose access chain passes through a nullsafe operator (?->). phpstan-rs has a `contains_nullsafe_access` helper already used for the analogous by-ref-argument check, but nothing calls it against a by-ref function/arrow-fn's own...
- **`Functions/InvalidParameterNameRule.php + InvalidLexicalVariablesInClosureUseRule.php + RedefinedParametersRule.php + VariadicParametersDeclarationRule.php`**: A family of small level-0 parameter/closure-use-list sanity checks (parameter named $this or a superglobal; a closure `use($this)`/`use($_GET)`/a use-variable duplicating a param name; a duplicate parameter name; a non-last variadic parameter) has no phpstan-rs equivalent at all - none of the identifiers (parameter.superglobal, parameter.this, closure.useThis,...
- **`Functions/ParameterCastableToStringRule.php + ParameterCastableToNumberRule.php + ImplodeParameterCastableToStringRule.php + SortParameterCastableToStringRule.php`**: Real PHPStan checks that array-argument values passed to specific stdlib functions (implode/join; array_combine/natsort/natcasesort/array_count_values/array_fill_keys; array_intersect/array_diff family; array_unique/sort/rsort/asort/arsort; and a castable-to-number variant) are actually castable to string/number, since these functions implicitly cast at runtime and a...

## Methods

- **`Rules/Methods/IncompatibleDefaultParameterTypeRule.php`**: Real PHPStan (level 2) checks that a method parameter's default value type is assignable to its declared parameter type, reporting identifier 'parameter.defaultValue'. phpstan-rs has no equivalent check anywhere - grepping the whole checker for default-value/parameter-type-compatibility logic finds nothing beyond override-compatibility's unrelated 'ancestor made it optional'...
- **`Rules/Methods/MissingMagicSerializationMethodsRule.php`**: Real PHPStan flags a non-abstract class implementing Serializable that lacks __serialize()/__unserialize() (identifier 'class.serializable', PHP-version-gated via serializableRequiresMagicMethods()). phpstan-rs's only Serializable-related logic is the unrelated 'enum cannot implement Serializable' check in oop.rs; the magic-method-presence check itself doesn't exist, and...
- **`Rules/Methods/AbstractMethodInNonAbstractClassRule.php`**: Real PHPStan's second half of this rule flags a non-abstract, non-interface method with no body at all (identifier 'method.nonAbstract') - real PHP's parser accepts this syntax even though it's semantically invalid, so PHPStan itself must catch it.
- **`Rules/Methods/MethodCallableRule.php`**: Real PHPStan gates first-class-callable syntax (`$obj->method(...)`) behind a PHP-8.1 version check, reporting 'callable.notSupported' when the configured target is older. phpstan-rs built an extensive PhpFeature/php_version.rs framework covering roughly a dozen similarly narrow PHP-version-gated syntax checks (noncapturing catch, throw-expression, promoted properties,...
- **`Rules/Methods/StaticMethodCallCheck.php`'s own static/instance-context
  mismatch half**: distinct from the file's already-tracked
  casing-mismatch check (ROADMAP's identifier-casing family, `Static
  MethodCallCheck.php:336`) - real PHPStan's `method.staticCall`
  identifier flags calling a genuinely non-static method through
  `Foo::method()` static-call syntax (or the reverse). No phpstan-rs
  equivalent exists for this specific mismatch (2 corpus hits on
  piwigo16 at level 10).
- **`Rules/Methods/OverridingMethodRule.php`'s tentative-return-type
  sub-check**: `method.tentativeReturnType` flags overriding a native
  PHP method that core has annotated with a "tentative" return type
  (PHP's own deprecation-notice mechanism for adding return types to
  built-in classes without breaking userland overrides) without
  matching it. Niche - only relevant when extending certain SPL/core
  classes - not built and not previously documented (6 corpus hits, all
  from vendored stdlib-adjacent code, not Piwigo's own).

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

- **`Rules/PhpDoc/InvalidPhpDocTagValueRule.php`**: `phpDoc.parseError`
  flags a PHPDoc tag whose value is syntactically malformed enough that
  `phpstan/phpdoc-parser` can't parse it into a type at all (e.g. a
  stray unclosed generic bracket in a `@param` type). Not implemented -
  no evidence of `phpDoc.parseError` or malformed-tag-value detection
  anywhere in `src/`; a high corpus count (155 hits), spread across
  Piwigo's own code (`functions_url.inc.php`, `functions_mysql.inc.php`,
  `template.class.php`, ...) rather than concentrated in one vendored
  file, worth a closer look given the count.
- **`Rules/PhpDoc/IncompatiblePhpDocTypeRule.php, IncompatiblePropertyPhpDocTypeRule.php, IncompatiblePropertyHookPhpDocTypeRule.php, IncompatibleClassConstantPhpDocTypeRule.php`**: Real PHPStan reports a real diagnostic (parameter.phpDocType/return.phpDocType/property.phpDocType/classConstant.phpDocType, plus *.unresolvableType siblings) when a @param/@return/@var/property/class-constant PHPDoc type is incompatible with (not a sub/supertype of) its own native type hint; phpstan-rs has no such diagnostic anywhere - it silently reconciles/coerces the doc...
- **`Rules/PhpDoc/IncompatibleSelfOutTypeRule.php`**: @phpstan-self-out (asserting a method returns $this narrowed to a different subtype) is entirely unimplemented in phpstan-rs - the tag is only listed in the typo-detection whitelist so a real use of it isn't wrongly flagged as unknown, but it's never actually parsed into a type or consumed/checked (selfOut.type/.static/.unresolvableType, plus generic-type validation, have no...
- **`Rules/PhpDoc/FunctionAssertRule.php, MethodAssertRule.php (AssertRuleHelper)`**: Phase 48.7 closed the plain class-existence slice of this (`class.notFound` on a `@phpstan-assert`/`-if-true`/`-if-false` tag's own asserted type - `checker/narrowing.rs`'s `check_assert_tag_types`). Still open: the deeper assertion-*validity* half real PHPStan validates - assert.impossibleType (an assertion that can never happen); assert.alreadyNarrowedType; assert.unresolvableType; assert.trait; assert.unknownExpr; generic-type/missing-typehint checks on the asserted type.
- **`Rules/PhpDoc/FunctionConditionalReturnTypeRule.php, MethodConditionalReturnTypeRule.php (ConditionalReturnTypeRuleHelper)`**: Real PHPStan validates a conditional PHPDoc return type's own declaration: conditionalType.subjectNotFound (the condition's subject isn't one of the function's own @template types) and conditionalType.alwaysTrue/alwaysFalse (the condition is statically always one way, given the subject/target types).
- **`Rules/PhpDoc/InvalidPhpDocVarTagTypeRule.php, WrongVariableNameInVarTagRule.php, VarTagChangedExpressionTypeRule.php`**: phpstan-rs's inline-@var checking (Phase 36 Done, varTag.type) is deliberately and correctly scoped to only the single case of a plain assignment statement whose RHS type doesn't match the tag. Real PHPStan's much larger @var-tag-validity surface beyond that - WrongVariableNameInVarTagRule's own name-matching diagnostics for foreach/static/global/return/misplaced-declaration...

## Generics

- **`Rules/Generics/TemplateTypeCheck.php (shared by ClassTemplateTypeRule.php, InterfaceTemplateTypeRule.php, FunctionTemplateTypeRule.php, MethodTemplateTypeRule.php, TraitTemplateTypeRule.php)`**: Phase 54 closed the declaration-validity family (`check_template_declaration_validity`, `checker/generics/bounds.rs`) - `generics.existingClass`/`.existingTypeAlias`/`.notSupportedBound`/`.templateDefaultOutOfBounds`/`.requiredTypeAfterOptional`, class- and method-level. Deliberately, permanently narrower in one disclosed way: a bound's/default's own referenced class is never existence-checked (`class.notFound`/`generics.traitBound`) - this project's registry is inherently partial (an unregistered class, e.g. a PHAR-bundled dependency, is indistinguishable from a genuine typo), matching the identical "don't know, don't guess" precedent `check_generic_bounds`'s own bound-*violation* check already established.
- **`Rules/Generics/MethodTagTemplateTypeCheck.php (used by MethodTagTemplateTypeRule.php, MethodTagTemplateTypeTraitRule.php)`**: Same TemplateTypeCheck declaration-validity family as above, applied to a class's own @method-tag template parameters, plus its own extra check: a @method tag's template name shadowing the enclosing class's own @template name (methodTag.shadowTemplate). Not built - a @method tag's own template parameters (distinct from a real method's `@template`) aren't modeled at all.
- **`Rules/Generics/UsedTraitsRule.php`**: Phase 54 closed the bound/arity/call-site-variance half - `@use Trait<Args>` now parses (from the `use` *statement*'s own doc comment, matching real PHPStan) into `ClassInfo::uses_args`, feeding the same `check_generic_bounds`/`check_generic_reference_arity`/`check_call_site_variance` machinery `extends`/`implements` already use, via `check_ancestor_generic_bounds`. Still open: `UsedTraitsRule`'s own separate diagnostics (incompatible/unresolvable type on the tag itself, a class with an @use tag that doesn't actually use any trait, the tag naming a different trait than the one actually used) and its one real behavioral difference from `@extends`/`@implements` - a call-site variance annotation inside `@use<...>` is *never* valid there at all (real PHPStan's own dedicated "is not allowed" message), which this project's shared `check_call_site_variance` doesn't yet special-case.
- **`Rules/Generics/CrossCheckInterfacesHelper.php (wired into ClassAncestorsRule.php, InterfaceAncestorsRule.php, EnumAncestorsRule.php)`**: Closed (`check_cross_path_interface_conflicts`, `checker/generics/interfaces.rs`) - walks every path (own @implements/@extends plus every ancestor's own, recursively) by which a class/interface/enum reaches the same generic interface, flagging it (generics.interfaceConflict) when two different paths pin that interface's same template parameter to two different concrete types. Corpus-verified against Piwigo's own unscoped vendor tree, which surfaced and fixed two real false positives along the way: local `@phpstan-type` aliases weren't expanded in `extends`/`implements`/`@use` tag arguments at all (a real, broader gap now fixed for every consumer of `ClassInfo::extends_args`/`implements_args`/`uses_args`, not just this check), and a class's parent chain was walked even with no real `@extends<Args>` tag linking them (real PHPStan's own `checkParents` gate, now ported).

## Arrays

- **`Rules/Arrays/IterableInForeachRule.php`**: Real PHPStan flags `foreach` over a subject whose inferred type isn't iterable at all (identifier `foreach.nonIterable`); phpstan-rs's Foreach handling in the checker never validates the subject's type before iterating, it just falls back to `Mixed` element type via `array_element_type(...).unwrap_or(Type::Mixed)`.
  **(Closed - see the receiver-type-checks phase in Done:
  `type_confirmed_not_iterable`, `check_foreach`, `src/checker/stmt/
  loops.rs`.)**
- **`Rules/Arrays/DeadForeachRule.php`**: Real PHPStan flags a `foreach` whose subject is provably an always-empty array (`foreach.emptyArray`, level 4) as dead code; phpstan-rs has no `isIterableAtLeastOnce()`-style check anywhere and never reports this.
- **`Rules/Arrays/ArrayDestructuringRule.php`**: Real PHPStan's list()/[] destructuring assignment checks that the RHS is actually array-like (`offsetAccess.nonArray`) and that each destructured key actually exists on it (via NonexistentOffsetInArrayDimFetchCheck). phpstan-rs's `bind_assignment_target`/`check_assignment_target` for an `ArrayLiteral` LHS only ever infers/binds - a non-array RHS silently falls to the `_ =>...
  **`offsetAccess.nonArray` half closed** (receiver-type-checks phase,
  Done: `type_confirmed_not_destructurable`, `src/checker/
  assignment.rs`). The each-key-exists-on-the-RHS half (via
  `NonexistentOffsetInArrayDimFetchCheck`) is still open.
- **`Rules/Arrays/ArrayUnpackingRule.php`**: Real PHPStan flags `[...$arr]` when `$arr`'s key type could be string and the target PHP version predates 8.1's string-key unpacking support (`arrayUnpacking.stringOffset`). phpstan-rs's ArrayLiteral checking in `check_expr` never inspects `item.spread` at all - a spread item's value is only type-inferred, never validated against its key type or the configured PHP version.
- **`Rules/Arrays/UnpackIterableInArrayRule.php`**: Real PHPStan flags `[...$x]` when `$x` isn't iterable (`arrayUnpacking.nonIterable`). phpstan-rs's array-literal checking never validates a spread item's value type against iterability - see ArrayUnpackingRule finding above, same root cause (spread items aren't specially checked at all).
  **(Closed - see the receiver-type-checks phase in Done:
  `type_confirmed_not_iterable`, `src/checker/expr.rs`'s `ArrayLiteral`
  arm.)**
- **`Rules/Arrays/OffsetAccessWithoutDimForReadingRule.php`**: Real PHPStan flags `$x[]` used in a read position (a PHP compile-time fatal error PHPStan pre-empts statically) via the non-ignorable `offsetAccess.noDim`. phpstan-rs's parser parses `expr[]` identically in any postfix position (no read/write distinction), and `check_expr`'s Index arm silently returns `Type::Mixed` for a `None` dim with no diagnostic.
- **`Rules/Arrays/NonexistentOffsetInArrayDimFetchRule.php`**: Real PHPStan's NonexistentOffsetInArrayDimFetchRule/Check has two halves: (1) flag reading a nonexistent key (`offsetAccess.notFound`) across constant arrays, general arrays (opt-in) and shapes, and (2) flag indexing into a type that isn't offset-accessible at all (`offsetAccess.nonOffsetAccessible`, e.g. an int or a non-ArrayAccess object).
  **Half (2) closed** (receiver-type-checks phase, Done:
  `type_confirmed_not_offset_accessible`, `check_offset_accessibility`,
  `src/checker/assignment.rs`; ships as phpstan-rs's own
  `shapeAccess.notFound` for the array-shape-key case, not literally
  named `offsetAccess.notFound` - a pre-existing naming difference, not
  new). Half (1)'s general-array/constant-array-key existence checking
  beyond shapes is still open.
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
  **(Closed since this entry was written - see the receiver-type-checks
  phase in Done: `type_can_never_be_object`/`clone.nonObject`,
  `src/checker/expr.rs`'s `Clone` arm.)**
- **`Rules/Variables/NullCoalesceRule.php`'s own dead-`??`-operand
  check**: phpstan-rs's existing `nullCoalesce.variable`/`.property`/
  `.offset` (`src/checker/...`) only fire when the left side is
  *definitely undefined*. Real PHPStan's own `nullCoalesce.expr`
  additionally flags the opposite, equally-common case - the left side
  is defined and its inferred type already excludes `null` entirely, so
  `??`'s right side can never run at all (`$definitelyNotNull ?? $y`).
  Not implemented; needs its own type-based check (probably
  `type_definitely_excludes_null`, already used elsewhere in this
  codebase) rather than the existing definedness-tracking machinery (5
  corpus hits).

## Operators+Cast

- **`Rules/Operators/InvalidIncDecOperationRule.php`**: a whole
  unimplemented rule family - flags `$x++`/`$x--`/`++$x`/`--$x`
  (`postInc.type`/`postDec.type`/`preInc.type`/`preDec.type`) on a type
  PHP can't meaningfully increment/decrement (an array, a bool, a plain
  object without numeric-string semantics). phpstan-rs's checker infers
  a type for pre/post inc/dec expressions but performs no operand-type
  legality validation at all - no matching identifier or check exists
  anywhere in `src/`, and this rule class isn't mentioned in ROADMAP.md
  or the backlog docs. Non-trivial corpus incidence for legacy PHP:
  postInc.type 41, postDec.type 14, preInc.type 3, preDec.type 2.

## Traits+InternalTag

- **`Rules/Traits/ConflictingTraitConstantsRule.php`**: Real PHPStan checks that a class constant overriding one supplied by a used trait keeps the same visibility, final-ness, native type, and (for immediately-used traits) the same value - reporting classConstant.visibility/.nonFinal/.final/.nativeType/.missingNativeType/.value.

## Keywords+Debug

- **`Rules/Keywords/ContinueBreakInLoopRule.php`**: Real rule walks a break/continue's enclosing-statement chain (via ParentStmtTypesVisitor) and reports continue.outOfLoop/break.outOfLoop (nonIgnorable) when the statement's numeric level doesn't resolve inside enough loop/switch nesting, or when a closure boundary is crossed first.
- **`Rules/Keywords/DeclareStrictTypesRule.php`**: Real rule validates that `declare(strict_types=...)`'s value is literally 0 or 1 (declareStrictTypes.value, nonIgnorable) and that the declare statement is the file's very first statement (declareStrictTypes.notFirst, nonIgnorable).
- **`Rules/Keywords/GotoUndefinedLabelRule.php`**: Real rule reports goto.labelUndefined (nonIgnorable) when a `goto` targets a label that doesn't exist in the function. phpstan-rs tracks goto targets (collect_goto_targets in src/checker/control_flow.rs) purely to mark a labeled statement as reachable for dead-code purposes - it never validates that every goto's target label actually exists anywhere in the function.
- **`Rules/Keywords/UnusedLabelRule.php`**: Real rule reports label.unused when a `label:` statement is never targeted by any goto in the function. phpstan-rs has no equivalent - collect_goto_targets only builds the set of names goto statements reference (used for reachability), it never inverts that to flag a Label statement with zero incoming gotos.
- **`Rules/Keywords/RequireFileExistsRule.php`**: Real rule resolves require/include/require_once/include_once path expressions (including the __DIR__ . 'x' magic-constant idiom and file_exists()-style guards) against the current working directory, include_path, and the analyzed file's own directory, reporting require.fileNotFound/requireOnce.fileNotFound/include.fileNotFound/includeOnce.fileNotFound when the resolved path is...
- **`Rules/Debug/{DumpTypeRule,DumpNativeTypeRule,DumpPhpDocTypeRule,DebugScopeRule}.php`**: These four rules are PHPStan's own ad-hoc developer-debugging hooks: `\PHPStan\dumpType()`/`dumpNativeType()`/`dumpPhpDocType()` print the inferred type/native-type/phpdoc-type of an expression at a point of interest (phpstan.dumpType/.dumpNativeType/.dumpPhpDocType), and `\PHPStan\debugScope()` dumps the whole scope's tracked variable types (phpstan.debugScope).

## Generators+Pure

- **`Rules/Generators/YieldFromTypeRule.php`**: Real YieldFromTypeRule reports `generator.nonIterable` when the expression after `yield from` is definitely (or, with reportMaybes, possibly) not iterable — a standalone check that only needs `exprType.isIterable()`, independent of the Generator<K,V,TSend,TReturn> generic-shape work.

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
  during the Phase 14.5 corpus-cleanup series; superseded - the real fix
  it called for shipped as commit `701be86`, see Done). Aliasing an
  anonymous class's inferred type to whichever single `extends`/
  `implements` target it names was implemented and corpus-verified, then
  reverted: unsound for the common "spy" test-double pattern (a class
  implementing a real interface but also declaring its own extra
  instrumentation members a test reads directly). A real fix needs
  registering the anonymous class's own full declaration under some
  stable synthetic name, not a type-inference tweak.

---

# Known limitations

Deliberate "this checker doesn't model X" boundaries not tied to any
one still-open phase above (a phase's own gap is described in its own
Pending entry instead).

## Known but not yet scheduled

Every item formerly listed here (the `use const`/`function.nameCase`
gap, the four string pseudo-types, five narrowing precision gaps, the
method-scoped `@template` backfill, correlated boolean-alias narrowing,
the phpstorm-stubs interleaved-parameter-list shape, and the
conditionally-declared-trait signature bug) has been built - see Done,
the areas above. What remains, each with a sharper, corpus-checked
reason to stay deferred rather than the generic placeholder this
section used to carry:

- `strtotime()`'s real accuracy and a raw SQL/DQL query's own result
  shape are both structurally infeasible without embedding a PHP
  runtime or a SQL parser respectively - confirmed real PHPStan needs
  exactly that.
- A long tail of smaller type-system precision gaps, triaged individually
  rather than as one bucket: no per-location type-inference caching
  architecture (a real PHPStan performance mechanism, not a correctness
  one - revisit only if profiling, not a missing diagnostic, calls for
  it); `IntersectionType`'s own `object&object` combination from
  sequential `instanceof` checks is already built and corpus-verified -
  only cosmetic nested-`Intersection` flattening remains, with zero
  confirmed false positive/negative; `static`/`$this` late binding has
  no attached calling class (a real architectural gap - no concrete
  corpus false positive/negative traced yet; watch for a fluent
  base-class builder method chained through a subclass instance);
  `class-string<T>`'s own `instanceof`/sealed-hierarchy
  narrowing is unmodeled (no `is_a()`-with-narrowing or sealed-hierarchy
  pattern found in the corpus this round); no general `TypeCombinator`
  intersect/remove or generic recursive type-tree traverse/map
  combinator (a refactor-timing call, not a missing-diagnostic gap -
  revisit once several more bespoke traverse/map functions accumulate
  and start visibly duplicating structural-recursion boilerplate).
- **The `foreach ($arr as &$v) { $v['key'] = ...; }` case where `$arr`'s
  own element type is already a real `ArrayShape` is done** - see Done's
  own entry (`CategoryCatsRenderer::render`'s `$infosOfImage`/`$info
  ['src_image']`). **The `ConfigurationSubController::processSizes`
  instance (a *uniform*, unshaped `Array(K, V)`, not an `ArrayShape`) is
  also done** - see Done's own `container_field_facts` entry.
- **A closure literal's own `Type::Closure`/`Callable` signature uses its
  bare declared return-type hint, not its more precise inferred body
  return type - unlike an ordinary named function/method, which already
  gets the more careful native/PHPDoc reconciliation above.** Found
  investigating `DbMaintenanceRepositoryTest`'s own `array_map(static fn
  (OptionallyQualifiedName $name): string => $name->getUnqualifiedName()
  ->getValue(), ...)` passed to `OptionallyQualifiedName::unquoted
  (non-empty-string $unqualifiedName, ...)` - flagged `argument.type`
  because the closure's own declared `: string` return hint is plain
  `string`, even though `Identifier::getValue()` (what the body actually
  returns) is itself `non-empty-string`. Confirmed via `\PHPStan\
  dumpType()` against a faithful minimal reproduction: real PHPStan
  infers the closure literal's own type as `Closure(Name): non-empty-
  string` - using the body's own more precise inferred return type, not
  the bare `: string` hint - so `array_map()`'s own result element type
  correctly comes out `non-empty-string` too. A real, well-scoped gap:
  wherever this project currently builds a `Type::Closure`/`Callable`
  signature for a closure/arrow-fn *literal* (as opposed to an ordinary
  named function/method, which already goes through `resolve_type`'s own
  native/PHPDoc reconciliation), it needs to also fold in the body's own
  inferred return type when that's a real subtype of the declared hint -
  the same "prefer the more precise, still-compatible answer" principle
  already applied elsewhere in this codebase, just missing at this one
  specific site (closure-literal signature construction). Not attempted
  here - the exact call site(s) building a closure literal's own
  `Type::Closure` weren't inventoried this round.
- `UploadService::addUploadedFile`'s own `$dbnow` - **root-caused, not
  simply "unresolved"**: a bisection of the real file (a temporary,
  immediately-reverted edit confirmed byte-identical via `md5sum` before
  and after - the frozen corpus checkout itself was never left touched)
  found the outer `if (isset($image_id)) {...} else {...}` genuinely
  *closes* right after the "new photo" branch computes `$file_path` (the
  `do`-`while` unique-filename loop) - not ~250 lines later as it first
  appeared reading the file top-to-bottom. Everything from there through
  the `dateAvailable: $dbnow` use is *shared* code reached by both the
  "existing photo" and "new photo" paths, ending in a *second*,
  independent `if (isset($image_id))` re-check (deciding update-vs-insert)
  whose own `else` branch is where `$dbnow` is actually read. This is
  exactly the already-declined `isset($image_id)`-checked-twice case
  named in "Permanent design boundaries" below (`condition_alias_shape`'s
  own missing "the same, unmodified condition re-tested after intervening
  shared code still correlates" reasoning) - confirmed via direct
  `\PHPStan\dumpType()` against the real file (temporarily, then reverted)
  that real PHPStan's own answer here is a clean `non-falsy-string`, no
  `null` at all. Not a separate, newly-found gap - the same one, just a
  harder-to-spot instance (same variable re-checked, not a proxy flag).
- **Corrected diagnosis (real-PHPStan-verified, supersedes this bullet's
  own original text below the line).** This was originally attributed to
  a missing feature - dynamic-key narrowing not reaching `isset()`/
  `array_key_exists()`-guarded reads - and each finding it produces was
  judged "sound, not a false positive." Direct A/B testing against the
  real `phpstan/phpstan` 2.2.8 phar (the exact version this project's
  own Piwigo target pins) proves that framing wrong: real PHPStan
  reports **zero** findings for the minimal reproduction below, at any
  guard style, confirmed via `\PHPStan\dumpType()` showing its own
  post-loop type for the accumulator is the plain, unqualified
  `array<string, 1>` - no `array{}` alternative survives the loop at
  all, guard or no guard:
  ```php
  $counter = [];
  foreach ($keys as $k) {
      $counter[$k] = 1;
  }
  return new Foo($counter[$someKey]);  // real PHPStan: int. phpstan-rs: int|null.
  ```
  The real root cause is one level upstream of narrowing entirely: this
  project's own `LoopZeroIterations::Possible` merge (`checker/
  control_flow.rs`) does a *symmetric* union of the loop's pre-entry and
  post-body states (`merge_scopes`'s ordinary rule) for `foreach`,
  so an array built via a dynamic-key write inside the loop keeps a
  literal `array{}` (empty) branch in its type even once the loop body
  has run. `index_result_type`'s own real, separately-justified
  "an empty shape + dynamic key -> `Null`" rule (`checker/expr.rs` -
  itself a correct, deliberate fix for a *different* bug, mid-loop
  auto-vivification during `++$counts[$c]`) then fires on that spurious
  surviving branch, unioning a bogus `null` into an otherwise-correct
  read - independent of whether a guard is present, which is why fixing
  narrowing alone was never going to close this. This is the exact gap
  already scoped as this file's own Phase 123 "O1" (foreach needs real
  PHPStan's *asymmetric*, certainty-downgrading join, not a symmetric
  union with the pre-loop entry) - not a separate feature to build.
  O1's own past regression ("the self-mutating-iteratee idiom") is
  already carved out by the existing `subject_exempt` mechanism
  (`LoopZeroIterations::Possible`'s own second field, added after that
  regression), so an asymmetric join for every *other* local no longer
  touches the case that broke it - worth re-verifying directly before
  building, not assuming either way.

  Real corpus impact is far larger than four sites: a direct sweep of a
  single scoped Piwigo corpus run found this exact signature in **33 of
  70** `argument.type`/`return.type` findings (47%) - it's a deliberate,
  repeated idiom throughout this codebase ("parallel accumulator maps
  populated by one loop, read by a second loop over one of the maps' own
  keys," each instance's own doc comment cross-references the pattern by
  name), hitting `ActivityRepository`, `HistoryRepository`,
  `ImageRepository`, `CategoryService`, `TagRepository`, `TagService`,
  `DerivativeImage`, `SearchService`, `CatPermPageRenderer`, and
  `RatingPageRenderer` repeatedly, not just the four originally cited
  below.

  ---

  *Original text, kept for its own real corpus citations (each is an
  instance of the corrected root cause above, not a separate case):*
  `isset($arr[$dynamicKey])`/`array_key_exists($dynamicKey, $arr)` never
  narrows the specific element's own value type (only `array_shape.rs`'s
  own composite `narrowing_key` mechanism does, and only for a *literal*
  string key - `narrowing.rs`'s own dispatch requires `Literal::String`
  outright). Confirmed live at `RatingPageRenderer::render` (`isset(
  $users[$rate_row->userId->value])` guarding `$users[$rate_row->userId
  ->value]` two lines later), `Users\UserService::getBrowserLanguage`
  (`array_key_exists($accept_language_full, $languages_available)`),
  `Image\DerivativeImage::getAll` (a self-referential fallback read, no
  guard at all), and `Admin\CatPermPageRenderer::render` (`$groups[
  $group_id]`, no guard). `tools/p58/trace.php`'s own `rootVariable(
  $sourceCache[$templatePath], ...)`/`viewProperties($view)` (`$view`
  itself read from `$viewCache[$templatePath]`) are a further, no-guard
  variant: both caches are populated key-by-key in one loop, then read
  back by dynamic key in a later one - once O1 lands, this variant may
  still need real cross-loop, cross-key correlation to close fully (a
  genuinely harder problem than the single-loop accumulator shape above)
  if the two loops aren't provably iterating the same key set - reassess
  after O1, don't assume it's fully subsumed.
- Not investigated this round: `StructuralTest::findBareCallSites:903`'s
  own `return.type` finding may partly be `token_get_all()`'s own
  generically-`mixed` stub shape rather than purely the already-
  documented `!== []`-narrowing gap above - not confirmed either way.

## Permanent design boundaries (not planned, by choice)

- A generic bound checked against a class this project has no real data
  for (a PHAR-bundled dependency with no loose source vendored, or an
  unparsed local type alias) silently skips the check.
- Integer overflow/precision, floating-point rounding, and other
  runtime-value-dependent behavior are out of scope by construction -
  this checker only ever reasons about static types, never concrete
  values.
- No cross-variable/parallel-structure correlation: two (or more)
  separately-tracked values that a program's own real logic always
  keeps in lockstep. The *general* mechanism (an arbitrary boolean-
  formula memory over recorded facts, symbolic enough to prove any
  correlated shape) is declined as genuinely unbounded in scope - no
  natural stopping point to "solve cross-variable correlation," unlike
  a single named idiom. That's a scope argument, not "real PHPStan
  doesn't do this either" (matching real PHPStan is never on its own a
  reason to decline a check this project could otherwise justify - see
  `feedback_stricter_than_phpstan_is_fine`). Each named instance was
  individually assessed on that basis, not grouped in by association:
  - Two (or more) parallel dynamically-keyed arrays populated together
    in one loop and read together in a later one
    (`$counterByKey[$key]`/`$dateByKey[$key]`/... - confirmed live at
    `ActivityRepository::findDailyActionCountsSince`,
    `HistoryRepository::findGroupedCountsSince`,
    `ImageRepository::findExtensionBreakdown`, all single-method-scoped
    like every other named instance in this cluster) splits into two
    genuinely different cases on a third reevaluation pass: the
    *same-function* case (both the populating loop and the later read
    are in one method, matching every corpus citation above) is
    buildable - filed as PHASE_BACKLOG Phase 173. The *cross-function*
    generalization (the producing loop and the consuming read site in
    genuinely different functions/methods) splits into two layers on a
    fourth reevaluation pass, only one of which is a real wall:
    (1) persisting the fact across a function boundary at all needs
    PHASE_BACKLOG Phase 174's own two-pass (collect-then-consume)
    checking architecture (extracted into its own phase precisely
    because this is its second independent consumer, alongside Phase
    166) (`registry/purity.rs`'s own transitive-purity fixed point can't be
    reused directly - its own doc comment discloses it deliberately
    only resolves call targets knowable *without* a live `Checker`/
    `Scope`, precisely because a correlation fact, unlike purity, can
    only be discovered by fully flow-sensitively checking a function
    body in the first place) - real, disclosed, large work, not
    impossible, and building it once unlocks both gaps. (2) is the
    actual wall: a fact collected once has to still hold by the time a
    later, different function's check consumes it, and in the general
    case arbitrary intervening code (another method on the same object,
    a subclass override, an alias) could invalidate one side without
    the other - soundly answering "does this correlation still hold
    here" is an invariant-maintenance proof across arbitrary call
    sequences to shared state, not a bigger data-flow analysis - a
    fundamentally different, harder class of problem (the same
    territory as separation logic/ownership analysis) with no known
    bounded design in this codebase to extend, unlike every other item
    in this cluster.
  - A discriminant variable whose value implies another variable was
    already set in the same branch (`AuthService::authKeyLogin`'s own
    `$valid_key === ConnectedWith::ApiKey` implying `$secret_key` is no
    longer `null`; `AlbumsPageRenderer::render`'s own `$order_by_date`
    implying `$ref_dates` was set in the same `if`) is a bounded
    generalization of the already-built `condition_aliases` mechanism
    (co-assignment-under-the-same-branch provenance for an arbitrary
    value, not just one boolean flag) - filed as PHASE_BACKLOG Phase
    169, real and buildable.
  - The *same condition expression* evaluated twice, where the second
    evaluation's branch should inherit the first's already-known effect
    - `NotificationByMailSender::sendMailNotifications`'s own
    `$nbmSendDetailedContent ? $news : null` reusing the identical
    condition `$news`'s own assignment was gated on one line further
    down (a ternary gets no "which if-branch got me here" definedness
    narrowing, only ordinary type narrowing), and
    `UploadService::addUploadedFile`'s own `isset($image_id)`
    re-checked a second time ~150 lines later as a fresh `if`/`else` -
    both are the identical "recognize a syntactically-identical
    condition recurring" shape, generalizing `condition_aliases`'s
    existing keying (currently by flag-variable name) to also key by a
    normalized condition-expression identity - filed as PHASE_BACKLOG
    Phase 170, real and buildable, reuses existing architecture.
  - `RateService::rate`'s own closure - `$userAnonymous &&
    $anonymousId !== $savedAnonymousId` implying `$savedAnonymousId`
    was set to a real, non-null value by the *outer* scope's own
    `if ($userAnonymous) { $savedAnonymousId = ... ?? ...; }` before the
    closure even runs - compounds the discriminant-variable mechanism
    above (Phase 169) with closure-capture timing (the correlation must
    survive into a closure body reading the captured variable); filed
    as PHASE_BACKLOG Phase 175, blocked on Phase 169.
  - A *compound* `&&`/`||` condition assigned to a flag variable, then
    reused later (confirmed at `CategoryService::createVirtualCategory`'s
    own `$parentIdIsEmpty = $parentId === null || $parentId === 0;` and
    three more call sites) is buildable, reevaluated a second time after
    an initially too-shallow "confirmed hard against vendor/phpstan-src"
    pass: `apply_narrowing` already has a *direct*, generalized OR/AND-
    chain mechanism for exactly this shape (`or_chain_narrowing_target`/
    `and_chain_narrowing_target`, `narrowing/predicates.rs:354-414`,
    wired in at `narrowing.rs:228`/`253`) - it already walks an
    arbitrary-depth `||`/`&&` chain checking every leaf narrows the same
    key. It just doesn't recognize an equality-comparison leaf yet (only
    a single-arg `is_*()` predicate call or `instanceof`) - the real gap
    is two missing leaf arms, not missing machinery. `condition_aliases`'
    own replay (`narrowing.rs:767`) just calls `apply_narrowing` on the
    stored expression, so the flag-variable-reuse form is fixed for free
    the moment the direct-condition form works - filed as PHASE_BACKLOG
    Phase 171.
  - A nullsafe call's own narrowed-non-null result implying its
    *receiver* was non-null too (`$x = $a?->b(); if ($x !== null) { /*
    $a is non-null */ }`, confirmed at `ActivityRepository::insertMany`)
    is also buildable, but needs one genuinely new narrowing rule, not
    just a new leaf shape: unlike every existing `apply_narrowing` arm
    (which narrows the *tested expression's own* key), this fact is
    about a *different* key (the receiver, not the whole nullsafe
    chain) - a single new match arm (parallel in shape to the existing
    `Instanceof` arm) that narrows `narrowing_key(receiver)` specifically
    when a nullsafe chain is proven non-null. Once it exists as a
    direct-condition rule, `condition_aliases`' replay picks it up for
    free the same way. Filed as PHASE_BACKLOG Phase 172.
- `token_get_all()` returns bare `array` - confirmed *not* a gap versus
  real PHPStan specifically (no dynamic-return-type extension for it
  exists in `vendor/phpstan-src` either, unlike the several functions
  above that real PHPStan does special-case), so a per-token shape
  (`array{0: int, 1: string, 2: int}|string`) is correctly out of scope
  here too.

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
