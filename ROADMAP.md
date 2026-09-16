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
Two real, disclosed, pre-existing gaps this newly exercises (both in
adjacent-but-separate subsystems, not this resolution mechanism
itself, confirmed via isolated repros with zero conditional-type
involvement) rather than fixed here: this project's own return-
statement type inference doesn't correlate a value accumulated across
several separate `if`/`else` branches/loops with an outer condition
made much earlier in the same function (a real corpus find,
`NotificationByMailSender::sendMailNotifications()`, where the
returned list's element type depends on the same `$action` a
conditional return type's own subject also names, but the two aren't
tracked as related); and ternary/if-else branches don't track a
`bool`/string literal narrowly enough for a declaration-level
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
present or the native declared type otherwise. Not built, disclosed:
45C (`ParameterOutExecutionEndTypeRule`, unioning each by-ref
parameter's exit-point values), 45D (too-wide by-ref narrowing), and
destructuring/`foreach`-rebind coverage for 45B - see Pending. Two real,
severe bugs the Piwigo corpus-verification pass itself found and fixed
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

**LSP `textDocument/documentSymbol`** (Phase 107 AH1). A per-file
hierarchical outline, built directly from `Project::ast`'s own
top-level declarations (`src/lsp/document_symbol.rs`) rather than the
project-wide, FQN-flattened `decl_locations` index above, which has no
per-file nesting to offer an outline. Classes/interfaces/enums nest
their own consts/properties/methods/cases as children; a declaration
inside a top-level `if`/`Block` is still walked, mirroring
`DeclLocations::index`'s own recursion.

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

**17 - LSP hover/go-to-definition (PARTIAL).** See Done for what
shipped. What's left is narrow and deliberate: a closure's own
`use (&$x)`/`use ($x)` capture only resolves when the outer scope
already has tracked provenance for that name, and only lexically
visible binding syntax updates provenance at all - an ordinary call's
incidental effect on an already-declared variable never does, on
purpose (a read should resolve to where the variable was really
declared, not every call that happened to touch it since).

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

**45 - Declared-vs-actual-narrowest-type inference (PARTIAL).** The
return/property variant (45A) and the by-ref direct-assignment variant
(45B, gated on new `@param-out` tag support) are both done - see Done.
Not built: `ParameterOutExecutionEndTypeRule` (45C - unioning each
by-ref parameter's value at every `return`/execution-end point against
its declared/`@param-out` type) and `TooWideFunctionParameterOutTypeRule`/
`TooWideMethodParameterOutTypeRule` (45D - too-wide narrowing for by-ref
parameters), plus destructuring-target (`[$a, $byRef] = foo()`) and
`foreach ($x as &$byRef)`-rebind coverage for the by-ref check 45B
already builds for a plain `$x = ...`/`$x += ...` assignment. 45A's own
property half (including cross-file trait-supplied property-assignment
tracking) is now done too - see Done, Phases 166/174.

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

**78 - DateTime, remaining (PARTIAL, declined).** `DateInterval`'s own
duration-string grammar, `DateTimeInterface::format()`/`DateInterval::
format()` return-type wiring, `DateTimeInterface::diff()`'s `days: int`
narrowing, `SimpleXMLElement::asXML()`'s arity-dependent overload split,
`DOMDocument::createElement()`'s own W3C XML `Name`-grammar tag-name
validation, and `SimpleXMLElement`/`simplexml_load_string()`'s own XML
well-formedness validation are all done (see Done). Only `DateTime`'s
relative-date-string parsing stays declined - it needs real PHP/ICU
runtime simulation, not a formally-specified grammar any crate
replicates (unlike XML well-formedness, which does have one). `checker/
throws/dynamic.rs`'s own doc comment names this as the one remaining
excluded dynamic-throw-type extension.

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

- **`Rules/Generics/TemplateTypeCheck.php (shared by ClassTemplateTypeRule.php, InterfaceTemplateTypeRule.php, FunctionTemplateTypeRule.php, MethodTemplateTypeRule.php, TraitTemplateTypeRule.php)`**: Phase 54 closed the declaration-validity family (`check_template_declaration_validity`, `checker/generics/bounds.rs`) - `generics.existingClass`/`.existingTypeAlias`/`.notSupportedBound`/`.templateDefaultOutOfBounds`/`.requiredTypeAfterOptional`, class- and method-level. Deliberately, permanently narrower in one disclosed way: a bound's/default's own referenced class is never existence-checked (`class.notFound`/`generics.traitBound`) - this project's registry is inherently partial (an unregistered class, e.g. a PHAR-bundled dependency, is indistinguishable from a genuine typo), matching the identical "don't know, don't guess" precedent `check_generic_bounds`'s own bound-*violation* check already established.
- **`Rules/Generics/MethodTagTemplateTypeCheck.php (used by MethodTagTemplateTypeRule.php, MethodTagTemplateTypeTraitRule.php)`**: Same TemplateTypeCheck declaration-validity family as above, applied to a class's own @method-tag template parameters, plus its own extra check: a @method tag's template name shadowing the enclosing class's own @template name (methodTag.shadowTemplate). Not built - a @method tag's own template parameters (distinct from a real method's `@template`) aren't modeled at all.
- **`Rules/Generics/UsedTraitsRule.php`**: Phase 54 closed the bound/arity/call-site-variance half - `@use Trait<Args>` now parses (from the `use` *statement*'s own doc comment, matching real PHPStan) into `ClassInfo::uses_args`, feeding the same `check_generic_bounds`/`check_generic_reference_arity`/`check_call_site_variance` machinery `extends`/`implements` already use, via `check_ancestor_generic_bounds`. Still open: `UsedTraitsRule`'s own separate diagnostics (incompatible/unresolvable type on the tag itself, a class with an @use tag that doesn't actually use any trait, the tag naming a different trait than the one actually used) and its one real behavioral difference from `@extends`/`@implements` - a call-site variance annotation inside `@use<...>` is *never* valid there at all (real PHPStan's own dedicated "is not allowed" message), which this project's shared `check_call_site_variance` doesn't yet special-case.
- **`Rules/Generics/CrossCheckInterfacesHelper.php (wired into ClassAncestorsRule.php, InterfaceAncestorsRule.php, EnumAncestorsRule.php)`**: Closed (`check_cross_path_interface_conflicts`, `checker/generics/interfaces.rs`) - walks every path (own @implements/@extends plus every ancestor's own, recursively) by which a class/interface/enum reaches the same generic interface, flagging it (generics.interfaceConflict) when two different paths pin that interface's same template parameter to two different concrete types. Corpus-verified against Piwigo's own unscoped vendor tree, which surfaced and fixed two real false positives along the way: local `@phpstan-type` aliases weren't expanded in `extends`/`implements`/`@use` tag arguments at all (a real, broader gap now fixed for every consumer of `ClassInfo::extends_args`/`implements_args`/`uses_args`, not just this check), and a class's parent chain was walked even with no real `@extends<Args>` tag linking them (real PHPStan's own `checkParents` gate, now ported).

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
  **corrected** - `BenevolentUnionType` genuinely is needed, this
  entry's own prior claim was wrong. A Piwigo/real-PHPStan corpus diff
  round confirmed a real, live gap: real PHPStan's own PHPDoc `Type[]`
  array-shorthand resolution (`TypeNodeResolver::resolveArrayTypeNode`)
  keys the result with a `BenevolentUnionType([Integer, String])`, not a
  plain `Union` - and `RuleLevelHelper::accepts()` treats a Benevolent
  union specially, skipping the strict per-member check a plain `Union`
  gets. Verified directly, not assumed: a `@var Item[]` property read
  via `foreach` and passed to a `string`-typed parameter produces *zero*
  errors from real PHPStan (`dumpType()` even displays it distinctly,
  parenthesized - `(int|string)` - vs. a plain-Union `int|string` shown
  unparenthesized, which *does* get flagged against the same parameter).
  This project's own `Type::unknown_array_key()` (`Union([Int, String])`,
  used both for the identical `Type[]`-shorthand parse in `types/
  parse.rs` and for the array-union `+` operator's own fallback key type)
  has no such marker, so every one of these sites gets treated as a
  strict `Union` - a confirmed source of real false positives: `Block
  Manager::apply`'s `foreach ($this->display_blocks as $id => $block)`
  (`@var DisplayBlock[]`) passing `$id` to `hideBlock(string $block_id)`;
  `CategoryAvailableListController::__invoke`'s equivalent `$catId` case;
  `SpecStructureTest`'s `strtoupper($key)` over an `openApiOperations()`-
  shaped array; `openapi-generate-input-dto.php`'s `narrowingExpr()` call
  - all four still open, all four now root-caused (previously listed as
  an "untriaged `int|string` array-key-type-imprecision cluster" in
  PHASE_BACKLOG's own Phase 123 O1 trade-off note). Not built this round:
  a real fix needs a genuine `Type::Union`-adjacent "benevolent" marker
  (a new variant or accessory) threaded through `Type[]`-shorthand
  parsing, the array `+` operator's key-type fallback, and every
  `is_assignable`/`accepts_at_configured_level` call site that should
  respect it - comparable in size to Phase 84's own already-declined
  `checkBenevolentUnionTypes` item, and naturally the same future phase
  that item already reserves, not a new one.
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
  instance below this line remains open, and is now known to be a
  genuinely different, larger gap than the one just fixed - not simply
  an unbuilt instance of the same mechanism**, confirmed by direct
  `\PHPStan\dumpType()` verification rather than assumed: `$pderivatives`
  carries an explicit `@var array<string, array<string, string|int|bool
  |null>>` annotation - a *uniform*, unshaped `Array(K, V)`, not an
  `ArrayShape` with named fields at all. The by-ref-widening fix just
  shipped structurally cannot help here (there is no per-field slot to
  narrow in a uniform array type); real PHPStan's own answer for this
  exact shape - confirmed via `dumpType()` against a faithful minimal
  reproduction of the loop (`$row['must_square'] = (...)`, then reading
  `$row['must_square']` back after the loop) - is a precisely-typed
  `bool`, obtained through a different, more general mechanism entirely:
  PHPStan's own `ExpressionTypeHolder` (see `vendor/phpstan-src`'s own
  architecture notes) tracks a flow type keyed by the *exact expression*
  (`$pderivative['must_square']`), independent of - and able to be
  strictly narrower than - whatever the container variable's own overall
  declared/tracked type says. This project has no equivalent: an
  array-index-fetch expression is only ever resolved through its
  *container's* own tracked type, never given its own independent,
  flow-narrowable identity the way a plain variable already has. A real
  fix needs a new per-offset (not per-variable) entry in `Locals` -
  keyed by `(container name, literal key)` or similar - consulted by
  `ExprKind::Index` reads before falling back to the container's own
  general element type, updated by exactly the same assignment sites
  that already update a plain variable's own `Locals` entry. Materially
  bigger than the by-ref-widening fix already shipped: needs its own
  key-collision/invalidation story (the offset's own cached type must be
  purged whenever the *container* is reassigned wholesale, aliased, or
  passed by reference elsewhere) that a plain-variable `Locals` entry
  never had to solve. Not attempted here.
- **A real, confirmed bug in `resolve_type`/`reconcile_with_native`: a
  PHPDoc type that only refines *one member* of a native union type
  silently discards every other native member instead of keeping it
  alongside the refinement.** Found investigating a scoped Piwigo corpus
  false positive (`PictureModifyInteractionTest`'s own `$page->keys
  ($inputSelector, 'Enter')`, third-party `Pest\Browser\Api\Concerns\
  InteractsWithElements::keys(string $selector, array|string $keys)`,
  whose own docblock narrows to `@param array<int, string> $keys` -
  itself contradicted by that method's own body, `is_array($keys) ?
  $keys : [$keys]`, which only makes sense if a plain string really is
  still accepted). Confirmed via `\PHPStan\dumpType()` against the real
  method: PHPStan's own effective type for `$keys` is `array<int,
  string>|string` - the `array` alternative refined, the untouched
  `string` alternative kept verbatim, not dropped. Root-caused directly
  in `vendor/phpstan-src/src/Type/TypehintHelper.php::decideType`: real
  PHPStan's own per-native-union-member merge keeps whichever original
  members the docblock type doesn't disprove, alongside the refined one.
  `resolve_type`'s own doc comment states the opposite design
  intentionally ("a docblock type is trusted as the complete picture and
  otherwise fully replaces the native hint") and `reconcile_with_native`
  implements exactly that: it only ever *filters down* the docblock
  type's own member list (dropping a docblock member the native type
  can't produce), it never adds back a native union member the docblock
  simply never mentioned at all. For an ordinary single-type native hint
  (`string`, `?Foo`, ...) this distinction is invisible - there's only
  one native "member" to reconcile against either way, and the design
  intent (docblock replaces, doesn't merely refine) is exactly right.
  It only diverges from real PHPStan for a **native union** where the
  docblock names a real subtype of *one specific member* while staying
  silent about the rest - confirmed to matter today because it's exactly
  the phpstorm-stubs-style pattern this project already special-cases
  elsewhere (`imagecreatefromstring`'s stale `@return resource|GdImage
  |false`), just from the opposite direction (docblock *narrower* than
  native, not *wider*/stale). A real fix: when `native_ty` is a `Union`,
  `reconcile_with_native` needs to also union in whichever native members
  `doc_ty` doesn't itself cover (i.e. aren't a subtype of any surviving
  doc member) - mirroring `decideType`'s own per-member keep-or-refine
  logic - rather than just filtering the doc type's own member list.
  Scoped, well-understood, not attempted here.
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
- **`$x !== []` (and presumably `$x != []`) isn't recognized as proving
  `$x` non-empty, unlike `count($x) > 0`, which already is.** Found
  investigating `SearchService::getRegularSearchResults`'s own `if
  ($imageIdsForFilter !== []) { if (count($imageIdsForFilter) > 1) {
  ...; } else { $first = reset($imageIdsForFilter); $items = array_map
  (intval(...), $first); } }` - flagged `argument.type`/`return.type`
  because `reset()`'s own existing "provably non-empty excludes `false`"
  precision (`checker/expr/builtin_calls.rs`'s `array_pointer_result`,
  Phase 74) never fires: nothing marks `$imageIdsForFilter`'s own tracked
  type as non-empty from the `!== []` guard alone, so `reset()` still
  returns `T|false`, and `array_map()` over a `T|false` argument can't
  stay `list<T>` - it degrades toward a generic `array`, which is what
  actually produces the observed `list<int>|array` mismatch two levels
  up. Confirmed via `\PHPStan\dumpType()` against a faithful minimal
  reproduction of the exact shape (the outer `!== []` guard, the nested
  `count() > 1`/`else` split, `reset()`, `array_map(intval(...), ...)`):
  real PHPStan's own answer for both `reset()`'s result and the final
  `array_map()` result is a clean `list<int>`, confirming the `!== []`
  guard alone is what real PHPStan uses to exclude the empty case here -
  the `count() > 1`/`else` split only decides *how many* elements, never
  *whether* there's at least one. A real, well-scoped narrowing addition:
  `apply_narrowing`'s existing equality-narrowing arm (`checker/
  narrowing/assertions.rs::apply_equality_narrowing`, the same one
  `$x === null`/`$x !== null` already goes through) needs a case
  recognizing an empty-array-literal `[]` on either side of `===`/`!==`
  as a known "empty" fact, upgrading the narrowed variable to its own
  non-empty-array accessory the same way `count($x) > 0` already does -
  not a new mechanism, an additional recognized shape in an existing one.
  Not attempted here.
- **The 4 `SiteUpdateSubController` `int|string`-vs-`int`/`list<int>`
  findings are not a separate gap - they're another instance of the
  benevolent-`int|string`-array-key-union item above, reached through
  `array_flip()` instead of directly.** `$db_fulldirs` starts as
  `CategoryService::getFulldirs()`'s own `@return string[]` - a `Type[]`-
  shorthand return, whose *key* type is exactly the "benevolent"
  `int|string` union the item above already names - then gets
  `array_flip()`'d, swapping keys and values, so the benevolent marker
  moves from the *key* position into the *value* position. Confirmed via
  `\PHPStan\dumpType()` against a faithful reproduction (a `string[]`-
  returning function, `array_flip()`, then reading a value against an
  `int`-only parameter): real PHPStan's own dumped type is `array<string,
  (int|string)>` - the parenthesized benevolent form, same as the item
  above - and passing that value where `int` is expected produces **zero**
  errors. `$to_delete[]`/`$category_up[]`/`storageCategoryId: $db_fulldirs
  [$dirname]` all read values straight out of this same flipped array (or
  out of `$insert['id_uppercat']`, itself assigned from a value read out
  of it) - once the benevolent-union marker above is built, it needs to
  additionally survive `array_flip()`'s own key/value swap (and, more
  generally, any other place this project moves a key type into a value
  position or vice versa) for all 4 of these findings to resolve for
  free, with no separate fix of their own.
- **A real, confirmed bug in the "degraded" (non-`ArrayShape`) path of a
  spread array literal (`[...$a, ...$b]`): it uses the wrong test for
  whether a spread source keeps the result list-shaped.** Found
  investigating `SearchService::getQuickSearchResultsNoCache`'s own
  `$params = [...$params, ...$ids]` (`$ids` sourced from `qsearchEval()`'s
  own `@return array<int, int>`) - flagged `argument.type` against
  `findImageIdsForRegularSearch`'s `list`-typed parameter. Confirmed via
  `\PHPStan\dumpType()` against a faithful minimal reproduction (a
  function declared `@return array<int, int>` - deliberately *not*
  `list<int>` - spread into an accumulator, once unconditionally and once
  inside an `if`): real PHPStan reports **zero** errors; swapping the
  source's own declared type to `@return list<int>` doesn't change the
  outcome (still zero errors) - but swapping it to `@return array<string,
  int>` correctly *does* produce the same `argument.type` error phpstan-rs
  gives today. Root-caused directly in `checker::expr::array_ops::
  infer_spread_array_literal_type`'s own "degraded" branch: `if !Self::
  is_list_like(&value_ty) { list_safe = false; }` gates purely on whether
  the spread *source* already carries the `ArrayList` accessory - but
  real PHP's own spread semantics (already correctly described in this
  same function's own doc comment, just not applied to this branch) always
  renumber a source's *integer* keys sequentially into the result,
  regardless of whether those keys were already sequential in the source;
  only *string* keys are ever preserved verbatim (and only a string key
  can break the result's own list-ness). `is_list_like` answers "is this
  array itself provably a list" - the right question for most of its
  other callers, but the wrong one here: the real question a spread needs
  answered is "does this source have any possible *string* key at all,"
  which only fails for a plain int-keyed `Type::Array` (`is_list_like`'s
  own `_ => false` catch-all wrongly answers this the same as a
  string-keyed one). A real fix: replace (or supplement) the `is_list_
  like` check in this one branch with one that inspects the source's own
  key type directly (via `array_key_type`) and only sets `list_safe =
  false` when that key type could genuinely include `String` - an
  `ArrayShape` source already takes the separate, non-degraded branch
  above it and isn't affected either way. Scoped, well-understood, not
  attempted here.
- **The opposite mistake, in the same neighborhood: this project is
  *over*-confident that a by-ref closure capture mutated inside a
  callback passed to `preg_replace_callback()` is already list-shaped
  after the call returns - real PHPStan is not.** Found investigating
  `SearchService::positionalCondition`'s own `arrayValues.list` finding
  ("call to `array_values()` on `array{}|non-empty-list`, which is
  already a list, has no effect") - `$values` is captured `use (&$values)`
  by a closure passed to `preg_replace_callback()`, mutated inside it via
  both `$values = [...$values, ...$value]` and `$values[] = $value;`
  across however many regex matches the pattern finds. Confirmed via
  `\PHPStan\dumpType()` against a faithful minimal reproduction of the
  exact shape: real PHPStan's own type for `$values` right after the
  `preg_replace_callback()` call is a bare `array<mixed>` - *not* already
  a list - meaning the very `array_values($values)` call this project
  flags as redundant is, per real PHPStan, doing real, needed work. This
  project's own diagnosis is therefore itself the false positive, not a
  true one needing verification. Root cause (not yet traced to an exact
  line, unlike the fixes above): closure-literal checking apparently
  treats a by-ref `use` capture's own body-local mutations as a single,
  unconditional pass written straight back into the outer scope, the way
  an ordinary statement would be - not the "may run zero or many times,
  each run seeing the *previous* run's own effect as its entry, converge
  to a fixed point, then merge with *never running at all*" treatment
  `Checker::converge_loop_body`/`LoopZeroIterations` already gives an
  ordinary loop body. A real fix likely means routing a by-ref-capturing
  closure literal passed to `preg_replace_callback`/`array_walk`/
  `usort`/`uasort`/`uksort` (every real by-ref-callback-accepting builtin,
  not just this one) through that same convergence machinery instead of
  a single pass - architecturally similar in shape to work already done
  for ordinary loops, but needs its own integration at the closure-
  literal-as-argument checking call site(s). Not attempted here.
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
- `NotificationByMailSender::sendMailNotifications`'s own `return
  $returnList;` (`return.type` against a conditional return type keyed
  on `$action === 'send'`) - **confirmed real via `\PHPStan\dumpType()`
  against a faithful minimal reproduction** (a boolean alias for
  `$action === 'send'`, an accumulator written down two different
  branches of that alias inside a loop, a conditional return type keyed
  on the same discriminant): real PHPStan produces zero errors. This is
  a genuinely *compound* gap, bigger than the base discriminant-
  correlation bucket alone (see "Permanent design boundaries" below):
  it needs that same boolean-alias correlation to survive across much
  deeper, longer-lived control flow (loops, several nested `if`s, ~230
  lines) than `condition_alias_shape` currently attempts *and* it needs
  the resulting correlated union to be checked against a *conditional*
  return type's own discriminant-keyed arms, rather than the ordinary
  flat union every return type gets today. Confirmed scoped, not
  attempted here.
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
- DBAL's own `Connection::fetchAssociative()` (raw, unmapped row fetch)
  is natively `array<string, mixed>|false`, no per-column typing at all -
  confirmed at `NotificationByMailSenderTest::setUp`'s own
  `$this->conn->fetchAssociative(...)` result assigned into a precisely
  `array{check_key: string, ...}`-shaped property. Per-column typing
  needs the actual live SQL/schema; real PHPStan doesn't solve this
  generically either (PHASE_BACKLOG Phase 168's own 168i) - a genuine,
  permanent boundary, unlike its sibling `QueryBuilder`/
  `AbstractQuery::getResult()` gap immediately below, which is being
  actively built.
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
