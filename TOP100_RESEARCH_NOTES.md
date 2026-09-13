# Per-package research notes (source-grounded, one at a time)

## 1. phpstan/phpstan-phpunit — DONE
Real source read (cloned, `src/Rules/PHPUnit/*.php`). Genuine new
inspirations for phpstan-rs (PHPUnit assertion narrowing already exists
via `checker/narrowing/assertions.rs`, this is additive):
- `ShouldCallParentMethodsRule` (`phpunit.callParent`): a TestCase
  subclass overriding `setUp()`/`tearDown()` when a real parent
  implementation exists (beyond bare `TestCase`) must call
  `parent::setUp()`/`parent::tearDown()` — same "forgot the parent
  lifecycle call" pattern as BEYOND_PHPSTAN's E3
  (`RequireParentConstructCallRule`), applied to PHPUnit's own two
  lifecycle methods instead of `__construct`. Reuses the identical
  ancestor-walk technique E3 already proposes porting.
- `DataProviderDeclarationRule`/`DataProviderHelper`: validates
  `@dataProvider Name`/`@dataProvider Class::method` doc tags AND PHP 8
  `#[DataProvider('method')]`/`#[DataProviderExternal(Class::class,
  'method')]` attributes — target method exists, is public, and (real
  PHPUnit 10+ requirement) is static. A real, concrete, PHPUnit-specific
  tag/attribute validation gap phpstan-rs has nothing like today.
- `ClassCoversExistsRule`/`ClassMethodCoversExistsRule`: `@covers`/
  `@coversDefaultClass` tag target-class/method existence — same shape
  as BEYOND_PHPSTAN's D5 (`@see` tag validation), different tag family.
- `AssertSameBooleanExpectedRule`/`AssertSameNullExpectedRule`/
  `AssertSameWithCountRule`/`AssertEqualsIsDiscouragedRule`: "wrong
  assertion method chosen" family — `assertSame(true/false, ...)` should
  be `assertTrue`/`assertFalse`, `assertSame(null, ...)` should be
  `assertNull`, `assertSame(count($x), ...)` should be `assertCount`,
  `assertEquals`/`assertNotEquals` on two same-typed operands should be
  `assertSame`/`assertNotSame`. A real, narrow, mechanically-checkable
  family — phpstan-rs already resolves `assert*` call targets via
  `apply_phpunit_assertion_narrowing`'s existing method-name table.
- `AttributeRequiresPhpVersionRule`/`ClassAttributeRequiresPhpVersionRule`:
  PHPUnit's own attributes (`#[RequiresPhp]`, etc.) version-gated against
  the configured PHP version — narrow, PHPUnit-specific, lower value.
- Annotation-formatting nitpicks (`NoMissingSpaceIn*AnnotationRule`) —
  skip, PHPUnit-doc-comment-syntax trivia with no real correctness value.

## 2. phpstan/phpstan-deprecation-rules — DONE
Real source read. Most of this package's core (the `Restricted
Deprecated{Property,Method,Function,ClassName,ClassConstant}
UsageExtension` family) is already ported — matches phpstan-rs's
existing `check_deprecated_usage` flag covering function/method/
staticMethod/property/staticProperty/classConstant/new.deprecated.
`FetchingDeprecatedConstRule` (a deprecated class-constant *fetch*,
distinct from other member deprecation) is also already covered
(`classConstant.deprecated`). One genuinely new, small, concrete find:
- `CallWithDeprecatedIniOptionRule` (`function.deprecated`):
  `ini_get`/`ini_set`/`ini_alter`/`ini_restore`/`get_cfg_var` called
  with a literal string naming a PHP ini directive deprecated as of a
  specific PHP version (hardcoded table: `mbstring.func_overload` since
  7.2, `assert.active` since 8.3, `session.sid_length` since 8.4, etc.)
  — checked against the project's own configured PHP version. A tiny,
  self-contained hardcoded-table + one-call-site addition to phpstan-rs's
  existing `PhpFeature`/`php_version.rs` framework.

## 3. phpstan/phpstan-symfony — DONE
Real source read (`src/Rules/Symfony/*.php`: `UndefinedOptionRule`/
`UndefinedArgumentRule`/`InvalidOptionDefaultValueRule`/
`InvalidArgumentDefaultValueRule`/`ContainerInterfaceUnknownServiceRule`/
`ContainerInterfacePrivateServiceRule`). Every one of these needs a real
Symfony-specific reflection layer phpstan-rs has no equivalent of at
all: resolving a Console `Command`'s own declared arguments/options
(from `configure()`'s `addArgument`/`addOption` calls) to check
`$input->getOption('foo')` names a real one, or resolving the DI
container's real compiled service map to check
`$container->get('foo.bar')` names a real, public service. The bulk of
this package (not in `Rules/`, in `src/Type/Symfony/`) is ~20
dynamic-return-type extensions (`ContainerInterface::get()`,
`ParameterBagInterface::get()`, etc.) — pure framework-magic type
resolution, same shape as Doctrine's below. **Verdict: not an
individual-rule inspiration — this and #4 below both point at one
much larger, genuinely new capability class: framework-specific
service-container/metadata-aware type resolution. phpstan-rs has zero
extension-point mechanism for this today (Phase 18's own axes don't
cover it either — those are about *rule* extension, not *type-
resolution* extension for a specific framework's runtime magic).**

## 4. phpstan/phpstan-doctrine — DONE
Real source read. `EntityColumnRule`/`EntityRelationRule` (checking a
mapped entity property's native PHP type against its `@ORM\Column`/
`@OneToMany` mapping declaration) depend on `ObjectMetadataResolver`,
which at analysis time actually instantiates Doctrine's own real
`ClassMetadataFactory`/mapping-driver machinery to read annotation/
attribute metadata into genuine Doctrine `ClassMetadata` objects — not
static AST inspection, real framework-runtime-equivalent metadata
resolution. Replicating this in phpstan-rs would mean writing a
from-scratch Doctrine ORM mapping-annotation/attribute parser, a
large, framework-specific undertaking on its own. Confirms and deepens
#3's verdict: this is the same "framework-specific metadata/type-
resolution extension" capability class, not a portable generic rule.
`RepositoryMethodCallRule` (validating repository magic-method field
names, e.g. `findByFoo()`) is *already* built in phpstan-rs (Item 4's
Doctrine `findOneBy`-family support, confirmed in ROADMAP.md's Done
section). `QueryBuilderDqlRule`/`DqlRule` (parsing and validating real
DQL query strings) needs an actual DQL grammar parser — its own
separate, large, narrow undertaking.

## 5. phpstan/phpstan-strict-rules — DONE (re-verified from real cloned
source, not the earlier rejected WebFetch summary — confirms it but
finds much more depth). BEYOND_PHPSTAN.md's own §E only covers 4 of
this package's ~45 rule files (boolean-in-conditions, `array_filter`
strictness, `parent::__construct()`, inherited-method name-casing).
Real, concrete, NOT-yet-catalogued findings, grouped:
- **Disallowed-construct family** (`DisallowedShortTernaryRule`/
  `DisallowedLooseComparisonRule`/`DisallowedImplicitArrayCreationRule`/
  `DisallowedEmptyRule`/`DisallowedBacktickRule`): five single-purpose
  opt-in style bans (`?:`, `==`/`!=`, `$arr[] = x` implicit-key-append,
  `empty()`, backticks) — each a trivial AST-shape match against a
  construct phpstan-rs's parser already represents distinctly.
- **Dynamic-member-access ban family** (`VariableVariablesRule`,
  `Variable{Static}{Property,Method}{Call,Fetch}Rule`,
  `VariableStaticMethodCallableRule`): flags `$$foo` and any
  `$obj->$memberNameExpr`/`$class::$memberNameExpr` where the member
  name itself is a runtime expression, not a literal — an opt-in
  strictness ban on constructs phpstan-rs's checker already has to
  treat as untyped (`DynamicPropertyAccess`/dynamic method call already
  collapse to `Mixed` today) — this rule doesn't need to *resolve*
  anything, just flag the syntactic shape.
- **`MatchingTypeInSwitchCaseConditionRule`**: a `switch` case value
  whose type differs from the subject's (switch uses loose `==`
  matching internally — a real footgun `switch(0) { case 'foo': }`
  historically matched). No core-PHPStan or phpstan-rs equivalent.
- **`StrictFunctionCallsRule`**: requires the `$strict`/`$strict_types`
  boolean parameter be explicitly passed to `in_array`/`array_search`/
  `array_keys`/`base64_decode`, since their default (loose) behavior is
  a common source of real bugs (`in_array('0e123', $arr)` type-juggling
  footguns).
- **`DynamicCallOnStaticMethodsRule`/`...CallableRule`**: flags calling
  a statically-declared method via `->` syntax instead of `::` (PHP
  allows both; `->` on a static method is a real readability/intent
  footgun).
- **Arithmetic-operand strictness family** (`OperandsInArithmetic{Addition,
  Subtraction,Multiplication,Division,Modulo,Exponentiation}Rule` +
  `OperandInArithmetic{UnaryPlus,UnaryMinus,Pre/PostIncrement,
  Pre/PostDecrement}Rule`, ~12 files sharing one `OperatorRuleHelper`):
  requires genuinely numeric operands, not just "PHP would coerce this
  numeric-string" — disallows PHP's automatic numeric-string coercion
  in arithmetic entirely when strict. A real, if large (12-operator),
  single-mechanism family.
- **`IllegalConstructorStaticCallRule`/`IllegalConstructorMethodCallRule`**:
  flags calling `__construct()` directly as an ordinary method/static
  call (`$obj->__construct()` re-runs the constructor on an
  already-built object; `Class::__construct()` statically) — a real,
  rare-but-genuine PHP footgun with no phpstan-rs equivalent.
- **`ClosureUsesThisRule`**: flags a non-static closure explicitly
  capturing `$this` via `use ($this)` when it's already implicitly
  bound — pure redundancy flag, easy.
- **`OverwriteVariablesWithForeachRule`/`...ForLoopInitRule`**: flags a
  `foreach`/`for` loop's own loop-variable name shadowing/overwriting a
  variable already holding a value from *outside* the loop — a real
  data-loss footgun, distinct from anything in BEYOND_PHPSTAN's D-series.
- `UselessCastRule` — worth a quick existing-coverage check before
  building (phpstan-rs may already flag some useless-cast shapes
  elsewhere; verify at implementation time rather than assuming either
  way).

## 6/7. larastan/larastan (+ nunomaduro/larastan, identical repo, same
GitHub source — Packagist rename, not a separate package) — DONE
Real source read (`src/Rules/*.php`, 28 rule files, plus separate
`Collectors`/`Reflection`/`ReturnTypes`/`Properties`/`Methods`
directories). Every single rule here is deeply Laravel-specific and
needs Laravel's own runtime object model that phpstan-rs has no
equivalent of at all: Eloquent model attribute/relation resolution
(magic `__get`/`__call`, `RelationExistenceRule`/`ModelAppends
Rule`/`ModelRelationDefaultsRule`), the service container/service
provider lifecycle (`DeferrableServiceProviderMissingProvidesRule`),
queue job serialization semantics (`UniqueJobDeclares*`/`BatchedJob*`/
`JobDispatchedInTransactionUsesAfterCommitRule`), Blade view compilation
(`UnusedViewsRule`/`NoMissingTranslationsRule`), and config repository
resolution (`ConfigCollectionRule`). **Zero portable generic-rule
inspiration** — every item is Laravel-framework-modeling work,
confirming (at the largest scale seen yet) the same "framework support
needs its own dedicated, large, separate capability" verdict from
packages #3/#4. Not directly relevant to phpstan-rs's own Piwigo
verification target either (not a Laravel app). Skip deep-diving the
remaining Laravel-specific packages in this list (#47, #52, #79, #81,
#83, #94) at the same depth — same verdict applies by construction;
still confirm each briefly for anything genuinely generic hiding inside.

## 8. mglaman/phpstan-drupal — DONE
Real source read (~85 files under `src/`). Overwhelmingly Drupal
entity-system/dependency-injection/plugin-manager framework-specific —
same "needs a full framework runtime model" verdict as #3/#4/#6.
`TodoCommentWithIssueUrlRule` and `DiscouragedFunctionsRule` are
Drupal-specific instances of patterns already generically covered
elsewhere in this same top-100 list (staabm/phpstan-todo-by #37,
spaze/phpstan-disallowed-calls #11) — no additional generic
inspiration beyond those. **No new portable finding.**

## 9. phpstan/phpstan-mockery — DONE
Real source read (8 files, all under `src/Type`/`src/PhpDoc`). Purely
Mockery-library-specific mock/stub return-type resolution (`Mockery::
mock(Interface::class)` returning an intersection of the mocked type
and `MockInterface`, `Mockery::namedMock()`, `->shouldReceive()`
fluent-builder typing) — the identical shape to phpstan-phpunit's own
`MockObject` handling from #1, just for a different mocking library.
Mockery isn't in Piwigo's own dependency tree. **No new generic-rule
finding** — pure library-specific type-resolution glue, same
"needs a per-library extension mechanism" theme as the framework
packages, at a much smaller scale.

## 10. phpstan/phpstan-webmozart-assert — DONE
Real source read: one 896-line file, ~64 distinct static-method-name
narrowing rules for `webmozart/assert`'s `Assert::string()`/
`Assert::nullOrString()`/`Assert::allIsInstanceOf()`/`Assert::integer()`/
etc. — the identical *pattern* phpstan-rs's own `apply_phpunit_
assertion_narrowing` already implements for PHPUnit's `assert*` method
family, just for a different, very widely-used (often transitively
pulled in across the Composer ecosystem) general-purpose assertion
library, not a framework. **Genuinely concrete, real port
opportunity**: extend phpstan-rs's existing assertion-narrowing table
mechanism with `Webmozart\Assert\Assert`'s own ~64 static methods
(string/nullOr-prefixed/all-prefixed variants of each basic type/shape
check) — same mechanism, new table entries, not new architecture.
Real corpus relevance plausible given how commonly this package is a
transitive Composer dependency (unlike Mockery/Laravel/Drupal above) —
worth checking Piwigo's own vendor tree for it before triaging size.

## 11. spaze/phpstan-disallowed-calls — DONE (re-verified from real
source; already cited in ROADMAP.md Phase 67 and this project's own
`restricted_symbols` design). Real source confirms `DisallowedKeyword`/
`DisallowedVariable`/`DisallowedNamespace`/`DisallowedAttribute`
(alongside the already-ported Call/Method/Property/Class kinds) each
share the same large `AllowedConfig` allow/disallow-in context-matching
DSL (`getAllowIn`/`getAllowExceptIn`/`getAllowInCalls`/`getAllowIn
InstanceOf`/`getAllowInClassWithAttributes`/etc., ~12 keys) ROADMAP's
own Phase 67 entry already correctly declines for lacking a
call-site-scope-path/attribute-context matching prerequisite. **No new
finding beyond what's already documented** — confirms the existing
citation is accurate, not stale.

## 12. jangregor/phpstan-prophecy — DONE
Real source read: 2 files. Teaches PHPStan that a Prophecy
`ObjectProphecy`'s `__call` magic proxies to the mocked interface's
real methods (so `$prophecy->realMethod()->willReturn(...)` isn't
flagged undefined) — same shape as #1/#9's mock-object handling, for
yet another mocking library. **No new generic finding.**

## 13. symplify/phpstan-rules — DONE
Real source read (~100 rule files under `src/Rules/`). Sampled broadly:
the overwhelming majority are opinionated house-style rules specific to
Symplify's own Symfony/Doctrine/Rector projects (controller/route/
service-wiring conventions, Rector-plugin-authoring meta-rules, entity/
repository-layering conventions) — not portable. `NoDynamicNameRule`
duplicates the same "ban dynamic member-name expressions" pattern
already found in phpstan-strict-rules' Variable* family (#5), no new
information. One genuinely new, small, generic find:
- `ForbiddenMultipleClassLikeInOneFileRule`: flags a file declaring more
  than one top-level class/interface/trait/enum (a real PSR-1-adjacent
  convention many projects want) — a trivial file-level top-level-
  declaration count, easy to add.

## 14. tomasvotruba/type-coverage — DONE
Real source read. A genuinely new idea, distinct from anything already
catalogued: a **project-wide type-coverage ratchet**. Real PHPStan
Collectors tally, project-wide, what fraction of return
types/param types/property types/class constant types are actually
declared (plus `declare(strict_types=1)` file coverage), then a Rule
fails the run if the percentage is below a configured required
threshold (`typeCoverage.returnTypeCoverage` etc.) — a CI-friendly
"coverage must not regress" gate, not a per-site diagnostic.
phpstan-rs already computes every individual missing-type-hint
diagnostic this would aggregate (`missingType.*`) — this is purely a
project-wide count-and-threshold-compare on top of data phpstan-rs
already has per-site, the same "whole-program aggregate from per-site
data already computed" shape BEYOND_PHPSTAN's Part 1 (B/H/X sections)
already establishes repeatedly for other facts.

## 15. szepeviktor/phpstan-wordpress — DONE
Real source read (16 files). Overwhelmingly WordPress-core dynamic-
function type resolution (`apply_filters()`, `wp_parse_args()`,
`esc_sql()`, `stripslashes`/`normalize_whitespace` helpers) plus
`is_wp_error()` assertion narrowing (same already-covered pattern as
#10). One conceptually interesting but tightly WP-specific pattern:
`HookCallbackRule`/`HookDocsRule` cross-check a registered
`add_filter`/`add_action` callback's own parameter signature against
the hook's doc-documented signature (parsed from WordPress core's own
`@param`-annotated `do_action`/`apply_filters` call sites) — a real
"does this event listener's signature match what actually fires it"
idea, but implemented via WP-specific doc-comment mining with no
portable generalization visible without WordPress itself. **No
build-ready generic finding** — WP-specific framework glue.

## 16. phpstan/phpstan-nette - DONE
Real source read (~25 files). Mostly Nette-framework-specific dynamic
type extensions (Strings/Forms/ComponentModel/Presenter/Html), same
theme as #3/#4/#8/#15. RegularExpressionPatternRule wires PCRE pattern
validation into Nette's own Strings::match()/matchAll()/replace()/
split() wrapper calls - the check itself is already built in phpstan-rs
(Item 12's pcre_validate.rs); only the Nette-specific call sites would
be new, and Nette isn't in Piwigo's tree. One genuinely interesting,
more-generalizable pattern found in RethrowExceptionRule
(nette.rethrowException): flags a try block containing a call that can
throw a specific exception subtype when the only catch clause present
is a general catch (\Exception $e)/catch (\Throwable $e) with no more-
specific catch AND no bare `throw $e;` re-throw - the specific
exception's own identity/context gets silently swallowed into a
generic catch-all. As implemented here it needs a hardcoded per-Nette-
method exception table (since Nette's own methods apparently lack real
@throws tags) - but the logic generalizes cleanly to any call once real
per-call-site throw-type information exists project-wide, exactly what
Phase 88's U2 (registry-level transitive @throws inference) would
provide - at that point this becomes a strictly more powerful,
framework-independent version of Nette's own hardcoded rule (works for
any throwing call, not just Nette's own known ones). Worth folding into
Phase 88 as a concrete consumer of U2's own computed throw types, not a
separate framework-specific item.

## 17. ergebnis/phpstan-rules - DONE
Real source read (~30 rule files). A large family of trivial,
single-purpose, opt-in "ban this language construct entirely" rules:
NoCompactRule, NoErrorSuppressionRule (the @ operator), NoEvalRule,
NoIssetRule, NoSwitchRule, NoNamedArgumentRule, NoAssignByReferenceRule/
NoParameterPassedByReferenceRule/NoReturnByReferenceRule (by-ref
anywhere), NoNullableReturnTypeDeclarationRule/NoParameterWithNullable
TypeDeclarationRule/NoParameterWithNullDefaultValueRule/
NoConstructorParameterWithDefaultValueRule (an opinionated "avoid null"
style bundle), NoExtendsRule (ban inheritance, favor composition),
NoParameterWithContainerTypeDeclarationRule (anti-service-locator: ban
typing a param as a DI container interface), NoPhpstanIgnoreRule (ban
@phpstan-ignore comments outright, forcing real fixes instead of
suppression). Distinct from spaze's DisallowedKeyword (#11, declined
for its large allow/disallow-in DSL): every one of these bans a bare
*language construct/keyword*, not a named symbol, and needs zero
context-filtering DSL - a simple, much smaller "ban this AST-node-kind
unconditionally, opt-in per rule" mechanism would cover the whole
family cheaply, distinct from and simpler than what was declined.
Also: FinalRule/FinalInAbstractClassRule/PrivateInFinalClassRule (an
opinionated "closed by default" bundle: classes final unless abstract,
members private unless the class is final) and InvokeParentHookMethodRule
(the same "must call parent's hook method" pattern as #1's
ShouldCallParentMethodsRule/BEYOND_PHPSTAN's E3, generalized to any
class implementing a configured "hook interface" list, not just
PHPUnit TestCase - worth folding into the same family as E3/#1 as one
configurable mechanism instead of three separate framework-specific
versions). DeclareStrictTypesRule duplicates real PHPStan's own core
DeclareStrictTypesRule already catalogued in Phase 137.

## 18. thecodingmachine/phpstan-safe-rule - DONE
Real source read. Core mechanism (UseSafeFunctionsRule/
UseSafeClassesRule) enforces a project-wide convention: call the
thecodingmachine/safe library's exception-throwing wrapper functions
instead of raw PHP stdlib functions that silently return false/null on
failure - a niche, single-ecosystem-convention rule, low generalizable
value without that specific library. PregMatchParameterOutTypeExtension
turned out to be a thin namespace-swapped copy of REAL PHPStan CORE's
own RegexArrayShapeMatcher feature (computing $matches's own array
shape from a static regex pattern) - checked and confirmed this is
ALREADY built in phpstan-rs (src/checker/expr/preg_shapes.rs exists).
No new finding from this package.

## 19. symplify/phpstan-extensions - DONE
Real source read (10 small files). Mostly framework-container glue
(Laravel make(), generic PSR container get()) already covered by the
established "framework support" category, plus a narrow opinionated
leniency tweak (forcing getcwd()/dirname()/realpath() to plain string
instead of string|false). No new generic finding.

## 20. thecodingmachine/phpstan-strict-rules - DONE
Real source read (7 files). A small, genuinely valuable, fully generic
exception-hygiene family, none of it framework-specific:
- ThrowMustBundlePreviousExceptionRule: inside a catch block, a newly
  thrown wrapping exception must pass the caught variable as its own
  $previous constructor argument somewhere - otherwise the original
  stack trace/cause is silently lost. A real, common, high-value bug
  class. Easy: walk a catch body's own throw-new-exception sites,
  check the caught variable appears in the constructor call's own args.
- MustRethrowRule: catching the base \Exception/\RuntimeException/
  \Throwable requires an unconditional rethrow somewhere in the catch
  body (unless deliberately building an exception handler) - a
  stricter, unconditional sibling of Nette's own RethrowExceptionRule
  (#16, which only fires when a MORE SPECIFIC exception was actually
  thrown and swallowed). Combine both as one "exception-swallowing
  hygiene" family when triaging into PHASE_BACKLOG.md.
- EmptyExceptionRule: throw new Exception() with no message argument.
- DoNotThrowExceptionBaseClassRule: throwing the bare \Exception/\Error
  base class instead of a specific subclass.
- SwitchMustContainDefaultRule: a switch statement with no default arm.
- NoSuperglobalsRule: ban direct $_GET/$_POST/etc access (force a
  request-wrapper abstraction) - a bare-keyword-ban, same small
  mechanism family as #17's ban-a-construct rules.
All six are simple, self-contained AST-shape checks with zero
framework dependency - among the highest-value/effort-ratio findings
so far.

## 21. rector/type-perfect - DONE
Real source read (~27 files). Strong independent real-world validation
of BEYOND_PHPSTAN's already-documented Phase 91 (X1): its
PublicClassMethodParamTypesCollector/NarrowPublicClassMethodParamTypeRule
literally IS X1's own proposed mechanism, already shipped by a real,
actively-maintained tool using real PHPStan's own Collector
architecture that core PHPStan itself has no equivalent of - worth
citing as concrete "this exact idea already ships in a popular real
tool" evidence when Phase 91 is picked up. NarrowReturnObjectTypeRule
is the closely-related return-type sibling, closer in spirit to
phpstan-rs's own already-built too-wide-return-type check (Item
11/45A) than to X1. Two more genuinely new, small, generic finds:
- NoMixedMethodCallerRule/NoMixedPropertyFetcherRule: opt-in ban on
  calling a method/accessing a property on a `mixed`-typed value at
  all (forces explicit typing before allowing any member access) - a
  real, simple "no mixed" strictness rule, distinct from anything
  cataloged so far.
- NoIssetOnObjectRule/NoEmptyOnObjectRule/NoArrayAccessOnObjectRule:
  ban isset()/empty()/array-index syntax targeting an object (each
  behaves oddly/silently on a plain object without ArrayAccess/magic
  methods) - real, simple, easy checks.

## 22. phpstan/phpstan-beberlei-assert - DONE
Real source read (9 files). Same shape as #10 (webmozart/assert):
static/fluent assertion-narrowing for the beberlei/assert library
(Assert::that($x)->string(), ->isInstanceOf(), etc.) - confirms the
"assertion-narrowing table" pattern generalizes to a third popular
assert library. No new distinct idea beyond reinforcing #10's finding
(the port mechanism should support pluggable assertion-library tables,
not just one hardcoded library).

## 23. ekino/phpstan-banned-code - DONE
Real source read (3 files). A config-driven "ban this function call by
name" mechanism (Expr_FuncCall keyed config) - same scope as
restricted_symbols' already-built Function kind. BannedUseTestRule is
a narrow test-file-specific `use`-import ban. No new finding.

## 24/100. phpat/phpat (+ carlosas/phpat, identical repo, Packagist
rename) - DONE
Real source read + a real usage example. This is a full, popular,
well-established ARCHITECTURE-TESTING tool (1276 favers - among the
highest faver counts in the entire top 100, meaning real developers
value it, not just download it transitively) - a genuinely new,
significant category not covered anywhere in BEYOND_PHPSTAN.md or
ROADMAP.md. A fluent DSL:
  PHPat::rule()
    ->classes(Selector::inNamespace('App\Domain'))
    ->shouldNotDependOn()
    ->classes(Selector::inNamespace('App\Infrastructure'))
Selectors compose: inNamespace, classname, extends, implements,
isAbstract/isFinal/isInterface/isEnum/isReadonly/isException/isTrait/
isAttribute/appliesAttribute/isStandardClass, with `->excluding(...)`
exemptions. Relations: shouldDependOn/shouldNotDependOn/shouldImplement/
shouldExtend/beFinal/beReadonly/etc. This is genuinely well-suited to
phpstan-rs's own closed-world, single-process, whole-project Registry -
arguably even more naturally than to real PHPStan itself (same
"whole-program fact PHPStan's architecture can't cheaply compute"
theme BEYOND_PHPSTAN's Part 1 already establishes repeatedly): a
project-wide pass resolving every class's own `use`-import/type-hint/
extends/implements references against a small set of configured
namespace/selector rules, flagging a forbidden dependency direction.
Real corpus relevance is plausible for any layered application
(Piwigo's own `src/Piwigo/*` namespace structure could plausibly want
"Domain must not depend on Infrastructure"-style rules, worth checking
at triage time). This deserves its own new backlog phase, not folded
into an existing one - "declarative architecture/dependency-direction
testing," genuinely distinct from restricted_symbols (per-symbol
banning) and from the internal/@internal engine (package-boundary
enforcement, not namespace-layer direction).

## 25. tomasvotruba/cognitive-complexity - DONE
Real source read. Directly validates and refines BEYOND_PHPSTAN's
already-documented D1 (function/method complexity metrics): this real,
popular tool implements "Cognitive Complexity" specifically (Campbell/
SonarSource's metric, which weighs nested control flow more heavily
than a flat sequence of branches, unlike plain cyclomatic complexity) -
worth citing as the specific recommended algorithm when D1 is built,
not just "some complexity metric." One additional, genuinely novel
finding: ClassDependencyTreeRule sums a class's own constructor-
injected dependencies' complexity *transitively* (current class's
complexity + the complexity of everything its __construct() pulls in,
recursively) and flags when the total exceeds a threshold - a "god
object with a deep, complex dependency graph" smell distinct from a
single function/method's own local complexity. Needs recursive
constructor-param-type resolution through the closed-world registry,
which phpstan-rs already has.

## 26. shipmonk/dead-code-detector - DONE
Real source read (~48 files). A comprehensive, production-grade,
whole-project dead-code detector - strong, direct validation of
BEYOND_PHPSTAN's already-documented Phase 86 (B1: whole-application
unused protected/public member detection; B2: dead function/class
detection). Two real refinements worth folding into Phase 86 when it's
picked up:
1. It ships a real AUTO-FIX capability (RemoveDeadCodeFormatter/
   RemoveDeadCodeTransformer/RemoveClassMemberVisitor actually delete
   the dead declaration's source text) - not just detection. Worth
   pairing Phase 86 with a Part-6-style codemod once built, the same
   "whole-declaration deletion" shape Phase 111's I3 already proposes
   for the narrower private-member case.
2. Its ~20 "UsageProvider" classes (Twig/Symfony/Laravel/Nette/
   Doctrine/Eloquent/Blade/Behat/PHPUnit/PhpBench/Composer/generic-
   Reflection) reveal the real scoping risk a sound whole-project
   dead-code detector must handle: a member can be "used" only via
   framework magic (a Twig template calling a method by name, a
   Symfony service-container definition, PHPUnit data-provider/attribute
   discovery, plain runtime Reflection) invisible to ordinary static
   reference-counting. Phase 86's own B1/B2 should explicitly inherit
   the same conservative "escaped means don't flag" discipline
   phpstan-rs's existing private-member `MemberUsage.escaped` fallback
   already uses, generalized rather than assumed away, when widening to
   protected/public and top-level functions/classes.

## 27. bnf/phpstan-psr-container - DONE
Real source read: 1 file, PSR-11 container get() dynamic return type.
Same DI-container-magic category already noted (#3). No new finding.

## 28. saschaegerer/phpstan-typo3 - DONE
Real source read (~28 files). Entirely TYPO3-framework-specific
(Context/Site/Request "aspect" system, DI service definitions,
GeneralUtility helpers). Same "framework support" category as
#3/#4/#6/#8/#15/#16. No new generic finding.

## 29. tomasvotruba/unused-public - DONE
Real source read (~40 files). A second, independent, real production
implementation of Phase 86's B1 (same author as #14/#21/#25) - strong
triple-validation (with #26) that whole-project unused-public-member
detection is a real, valued, actively-built category. Two refinements
for B1 when picked up: (1) explicitly covers properties and class
constants too, not just methods (UnusedPublicPropertyRule/
UnusedPublicClassConstRule) - broaden B1's own scope statement
accordingly; (2) a related but distinct idea worth noting alongside it:
LocalOnlyPublicClassMethodRule flags a public method that's *never
called from outside its own declaring class* (over-visible, not dead -
call sites exist, just none external) as a "could be private" suggestion,
complementary to but different from "provably unused anywhere."  Same
template-usage-escape-hatch theme as #26 (Twig/Blade regex-based usage
detection) reinforces the same escape-hatch discipline note already
recorded there.

## 30. phpstan/phpstan-php-parser - DONE
Real source inspected: no src/ content at all, just stub files
(Name.stub/Function_.stub/Const_.stub/ClassLike.stub) narrowing
nikic/php-parser's own AST node property types for PHPStan's own
self-analysis precision. Not a rule/check package at all - a
self-analysis stub-precision tool specific to PHPStan's own codebase.
No applicable finding.

## 31. timeweb/phpstan-enum - DONE
Real source read. Targets the legacy myclabs/php-enum userland
"enum simulation" library (pre-PHP-8.1), not native PHP enums -
duplicate-backing-value detection for that library's own class-constant
convention. Narrow, dated technique; Piwigo's own corpus uses native
PHP enums. No new finding (native-enum duplicate-value detection is a
different, already-more-relevant surface, already real PHPStan core
functionality).

## 32. shipmonk/phpstan-rules - DONE
Real source read (~36 rule files, headers/purpose confirmed per file
name + spot checks of the most novel ones). By the same author as #26
(shipmonk/dead-code-detector) - high quality, generic, zero framework
dependency across the whole package. One of the richest finds in the
whole top-100. Grouped by theme:

**Exception hygiene (reinforces #16/#20):**
- RequirePreviousExceptionPassRule: third independent implementation of
  the exception-chaining rule (#16 Nette, #20 thecodingmachine) - strong
  signal this is a real, widely-valued, easy check.
- ForbidUnusedExceptionRule: `new Exception(...)` constructed but never
  actually thrown anywhere - a real, common "forgot the throw keyword"
  typo bug. Easy: flag a `new`-exception expression used only as a bare
  statement/discarded value.
- ForbidCheckedExceptionInYieldingMethodRule/
  ForbidCheckedExceptionInCallableRule: exception-declaration
  consistency checks that complement phpstan-rs's own existing
  missing-checked-exception work.

**"No mixed" family (reinforces #21):**
- ForbidMethodCallOnMixedRule/ForbidFetchOnMixedRule: third independent
  confirmation (after #21's NoMixedMethodCallerRule/
  NoMixedPropertyFetcherRule) of the "ban member access on mixed" idea.

**Enum-exhaustiveness family (genuinely novel, high value):**
- ForbidMatchDefaultArmForEnumsRule: a `match` over an enum subject that
  has a `default` arm defeats PHPStan's own exhaustiveness benefit -
  when a new enum case is added later, the default arm silently
  swallows it instead of surfacing a missing-arm error. Real, valuable,
  novel.
- EnforceEnumMatchRule: require `match` (not `switch`/if-chains) when
  branching over an enum value, so exhaustiveness checking actually
  applies. Pairs directly with the rule above.
- EnforceReadonlyPublicPropertyRule: a *hard requirement* (not a
  suggestion) that public properties be readonly - the enforcement-mode
  sibling of Phase 97's AA1 readonly-candidate *suggestion*, third
  independent confirmation the idea has real value.

**Null-coercion-safety family (novel):**
- ForbidNullInInterpolatedStringRule/ForbidNullInBinaryOperationsRule/
  ForbidNullInAssignOperationsRule: ban a possibly-null value silently
  participating in string interpolation (renders as `""`), a binary
  operator, or a compound assignment (`+=` etc.) - PHP's silent
  null-to-empty-string/zero coercion is a real, common bug source. A
  clean, novel, buildable family.

**Native-typehint-preference family (novel, easy):**
- EnforceNativeReturnTypehintRule/EnforceClosureParamNativeTypehintRule:
  require a *native* type hint (not just a PHPDoc-only one) wherever
  PHP's own grammar could already express it - closes real precision
  gaps PHPDoc-only typing leaves open to drift.
- ForbidPhpDocNullabilityMismatchWithNativeTypehintRule: flags when a
  PHPDoc type's own nullability disagrees with its native type hint's
  nullability - a sharper, easier-to-implement special case of the
  already-catalogued Phase 130 IncompatiblePhpDocTypeRule family, worth
  building first as a quick win before the full rule.

**Real, sharp-edge gotcha checks (novel, easy):**
- EnforceIteratorToArrayPreserveKeysRule: `iterator_to_array($x)` should
  pass `$preserve_keys` explicitly - silently losing/colliding keys
  when iterating a Generator yielding non-sequential/string keys is a
  real, common, easy-to-miss bug.
- ForbidUnsafeArrayKeyRule, AllowComparingOnlyComparableTypesRule
  (restricts `<`/`>`/`<=`/`>=` to genuinely comparable type pairs,
  avoiding PHP's own confusing cross-type comparison semantics),
  ForbidIdenticalClassComparisonRule, ForbidIncrementDecrementOnNonIntegerRule/
  ForbidArithmeticOperationOnNonNumberRule (narrower siblings of #5's
  already-catalogued arithmetic-operand-strictness family) - each a
  small, self-contained, real correctness check worth a closer read at
  triage time.

**Lower priority / narrower, noted but not detailed further:**
ForbidReturnInConstructorRule, ForbidReturnValueInYieldingMethodRule,
ForbidProtectedEnumMethodRule, ForbidCustomFunctionsRule (config-driven
ban, same shape as restricted_symbols), ForbidCastRule (opinionated,
bans casts outright), ForbidUnsetClassFieldRule,
ForbidVariableTypeOverwritingRule (reassigning a variable to an
unrelated, non-narrowing type - a real "variable reuse" smell, worth a
closer look later), UselessPrivatePropertyDefaultValueRule,
EnforceListReturnRule, BackedEnumGenericsRule, ClassSuffixNamingRule,
ForbidEnumInFunctionArgumentsRule, ForbidUnusedClosureParametersRule,
ForbidUnusedMatchResultRule.

## 33. symplify/astral - DONE (abandoned, unreachable)
Confirmed via Packagist API: officially marked abandoned (suggested
replacement: nikic/php-parser), and its GitHub repository has been
deleted (404). Nothing to investigate. No finding possible or needed -
its own download count is legacy/historical, not a live tool.

## 34. bitexpert/phpstan-magento - DONE
Real source read (~29 files). Entirely Magento-framework-specific
(ObjectManager/DataObject magic methods, Magento code-generation
autoloaders, resource models). Same "framework support" category as
the other framework packages. No new generic finding.

## 35. staabm/phpstan-dba - DONE
Real source read (top-level structure + Rules/). A sophisticated real
SQL-query analyzer: validates raw SQL strings passed to PDO/mysqli/
Doctrine/Dibi against a real database schema (needs actual schema
reflection or a schema definition file), including
QueryPlanAnalyzerRule which connects to a live/reflected database to
analyze real EXPLAIN query plans. Strongly reinforces (not a new
action item) ROADMAP's own already-declined Phase 140a verdict
("a raw SQL/DQL query's own result shape [is] structurally infeasible
without embedding... a SQL parser") - this real, working tool
confirms that's exactly the scale of investment needed (a genuine SQL
grammar parser plus schema-reflection connectivity), not something to
reconsider building. No new actionable finding.

## 36. slam/phpstan-extensions - DONE
Real source read (~15 files). A genuinely novel, valuable family: "no
implicit current-time dependence" ("NotNow" namespace) - NoTimeRule
(bans time()), NoRelativeStrtotimeRule (bans strtotime('now'/'+1 day'/
etc - anything relative to the current instant rather than a fixed,
explicit reference)), NoRelativeDateTimeInterfaceRule (bans `new
DateTime()`/`new DateTimeImmutable()` with no explicit fixed
date/reference), NoDateWithoutSecondArgumentRule (bans `date($format)`
without its optional second $timestamp argument, which implicitly uses
the current time). All four are real, simple AST-shape/literal-argument
checks with zero framework dependency - genuinely useful for testable,
deterministic code (a function silently depending on wall-clock "now"
is a classic source of flaky tests and hard-to-reproduce bugs). Other
files (UnusedVariableRule, MissingClosureParameterTypehintRule,
StringToClassRule, GotoRule) look like narrower/likely-already-covered
variants of things phpstan-rs or real PHPStan core probably already
handles - not detailed further given time budget.

## 37. staabm/phpstan-todo-by - DONE
Real source read (~21 files). A genuinely novel, well-designed
"actionable TODO comment" family - turns `// TODO` comments into live,
checkable facts instead of text that rots forever:
- TodoByDateRule: `// TODO: fix by 2024-01-01` flags once that date has
  passed - pure, self-contained (compare a parsed date literal against
  wall-clock "now"), no network needed, easy.
- TodoByVersionRule/TodoByPackageVersionRule: `// TODO: remove when php
  >= 8.3` / `// TODO: ... when symfony/console >= 6.0` flags once the
  project's own configured PHP version or a specific installed Composer
  package version satisfies the condition - also pure/self-contained
  (phpstan-rs already resolves configured PHP version and could resolve
  an installed package's version from composer.lock).
- TodoByTicketRule: `// TODO: JIRA-123` flags once that real ticket is
  closed, via a *live* HTTP call to Jira/GitHub Issues/YouTrack at
  analysis time - the one genuinely different, network-dependent
  variant; a real design choice this project doesn't currently make
  anywhere (no check does live I/O), worth flagging honestly as a
  bigger step than the date/version variants if pursued.
Directly complements Phase 96's already-catalogued D5 (@see tag
validation) in spirit (checking a comment's own claim against reality)
but is a distinct, separately-valuable idea.

## 38. pepakriz/phpstan-exception-rules - DONE
Real source read (~27 files). A comprehensive @throws-tag ecosystem;
much of it overlaps with phpstan-rs's own already-built throw-type
machinery (UselessThrowsPhpDocRule - a `@throws X` tag naming
something the body never actually throws - is already covered by
phpstan-rs's own ported TooWideFunctionThrowTypeRule/
TooWideMethodThrowTypeRule; ThrowsPhpDocInheritanceRule/covariance
already covered by MethodThrowTypeCovarianceRule). One genuinely new,
valuable pair not yet catalogued anywhere:
- UnreachableCatchRule/DeadCatchUnionRule: a `catch (SpecificException
  $e)` clause (or one member of a `catch (A|B $e)` union) that matches
  nothing the try block's own statements can actually throw, given the
  aggregate of everything inside it (using the same real, already-
  computed per-call throw-type information Phase 88's U2 - or the
  existing @throws-tag machinery even before U2 - already provides).
  A real, valuable, previously-uncatalogued dead-code family
  specifically for exception handling.

## 39. shipmonk/phpstan-baseline-per-identifier - DONE
Real source read (8 files). A focused, genuinely novel baseline-
management idea, directly relevant to already-documented Phase 122
(baseline robustness, N1-N3): split one monolithic baseline file into
separate files per diagnostic identifier (e.g. baseline/argument.type.
neon, baseline/return.type.neon) - makes git diffs on baseline changes
far smaller/more reviewable and lets a team pay down baseline debt one
error category at a time instead of one unmanageable blob. A 4th
baseline-robustness sub-item worth adding to Phase 122 alongside its
existing N1 (bounded occurrence count)/N2 (identifier-anchored match)/
N3 (stale-entry detection) - a pure file-organization/generation
feature on top of phpstan-rs's own existing baseline.rs, no new
matching logic needed.

## 40. prestashop/phpstan-prestashop - DONE
Real source read (8 files). UseTypedReturnForNewMethodsRule/
UseTypeHintForNewMethodsRule/UseStrictTypesForNewClassesRule turned
out simpler than the name suggests: "new" isn't git-blame/date-based,
it's a plain hand-maintained exclusion-list config (grandfather
existing violations once, then the rule fires on everything else) -
functionally a narrower, manually-curated, per-rule form of what
phpstan-rs's own baseline mechanism already provides more generally
and automatically (Phase 122). Low incremental value once baseline
robustness improvements land. One small, easy, genuinely separate
finding: ClassConstantsMustHaveVisibilityRule (PHP 7.1+ allows
explicit visibility on class constants; require it) - trivial, opinionated,
easy to add if wanted.

## 41. spaze/phpstan-stripe - DONE
Real source read: purely Stripe SDK-specific dynamic property modeling
(Stripe's PHP SDK response objects use magic __get extensively). Same
"third-party SDK magic-property modeling" theme as the framework
packages, just for one specific SDK. No new generic finding.

## 42. swissspidy/phpstan-no-private - DONE
Real source read (6 files). Implements the same restriction semantics
as phpstan-rs's already-built @internal engine (Phase 67), just keyed
off the legacy `@access private` PHPDoc tag spelling (a pre-visibility-
modifier-era convention still used heavily by WordPress core and
similarly old codebases) instead of `@internal`. Would be a trivial
additional-tag-spelling addition to the existing engine if WordPress-
style legacy code were a real target, but low general value otherwise -
narrow, single-ecosystem convention.

## 43. yamadashy/phpstan-friendly-formatter - DONE
Real source read (6 files) + README. A custom output formatter
rendering each diagnostic with a syntax-highlighted source code frame
around the offending line (the same UX convention modern compilers use
- rustc, ESLint's codeframe formatter - versus PHPStan's own terse
default "file:line: message" one-liner). phpstan-rs already ships
text/json/checkstyle formats; a "friendly"/codeframe terminal formatter
would be a real, self-contained, genuinely nice UX addition - no new
diagnostic logic, purely a new output::Format variant reading the
already-resolved file's own source text plus the diagnostic's already-
known span.

## 44. php-standard-library/phpstan-extension - DONE
Real source read (4 files): type/assertion narrowing for the
azjezz/psl (PHP Standard Library) utility library. Same
assertion-narrowing-table pattern already noted (#10/#22). No new
distinct idea.

## 45. staabm/phpstan-psr3 - DONE
Real source read (1 file). A real, valuable, easy, generalizable
finding: validates PSR-3 logger calls (any Psr\Log\LoggerInterface
method - log/debug/info/notice/warning/error/critical/alert/
emergency) by cross-checking the message string's `{placeholder}`
tokens against the literal keys of the passed context array (flagging
a placeholder with no matching key, and vice versa), plus checking
context values used for interpolation are stringable. This is the
exact same shape as phpstan-rs's own already-built printf/sprintf
placeholder-type checking (Item... existing format_string.rs
machinery) - just PSR-3's `{key}`-brace syntax instead of printf's
`%s`-style, and checking against a literal array's own keys instead of
positional arguments. Genuinely good real-corpus-value potential given
how nearly-universal PSR-3 logging is in any DI-framework-based PHP
app.
cd /tmp/claude-1000/-home-torres-projects-phpstan-rs/2c47181b-0480-4b7e-8676-db42b8807004/scratchpad/repos
git clone --depth 1 "https://github.com/canvural/larastan-strict-rules.git" larastan-strict-rules 2>&1 | tail -2
find larastan-strict-rules/src -name "*.php" | xargs -I{} basename {}

## 46. staabm/phpstan-deployer6 - DONE
Real source inspected: no rule logic at all, purely PHP stub files for
the Deployer deployment-tool library's own DSL (functions.stub,
Task.stub) - a niche type-precision stub package, not a check. No
finding.

## 47. canvural/larastan-strict-rules - DONE
Real source read (8 files). Entirely Laravel-specific opinionated
style rules (facades, dynamic where(), query scopes, controllers).
Same Laravel-framework theme as #6/#7 - no portable finding.

## 48. cakedc/cakephp-phpstan - DONE
Real source read (~20+ files). Entirely CakePHP-framework-specific
(TableLocator/Entity/Repository/Component/Controller dynamic type
resolution). Same "framework support" theme. No new generic finding.

## 49. korbeil/phpstan-generic-rules - DONE
Real source read (4 files). MbStringRule bans 12 byte-oriented string
functions (strlen/substr/strtolower/strpos/stripos/strstr/etc.) in
favor of their mb_*-prefixed multibyte-safe counterparts - a real,
common UTF-8-correctness bug class. Given restricted_symbols already
exists generically, this is best framed as a ready-made *curated
preset* (a bundled example config banning these 12 names) rather than
new architecture. GenericFunctionRule/DebugRule are generic
config-driven function bans, already covered by restricted_symbols.
NeverEmptyRule not deep-dived (low apparent novelty vs. existing
always-true/false condition checks).

## 50. phpstan/phpstan-dibi - DONE
Real source read: 2 files, Dibi (Nette-ecosystem DB library) fluent
query-builder magic-method modeling. Same framework/library-magic
theme. No new finding.

## 51. rector/phpstan-rules - DONE (abandoned)
Confirmed via Packagist API: abandoned, merged into symplify/
phpstan-rules (already fully investigated as #13). No separate
investigation needed.

## 52. calebdw/larastan-livewire - DONE
Real source read (7 files). Laravel Livewire-component-specific magic
property/method modeling. Same Laravel-framework theme (#6/#7/#47/#83
below). No portable finding.

## 53. marc-mabe/php-enum-phpstan - DONE (re-verified from source;
already cited in ROADMAP.md for the MabeEnum\Enum keyword-corruption
bug). Type modeling for marc-mabe/php-enum's own userland enum
simulation library. No new finding beyond what's already documented.

## 54. tomasvotruba/bladestan - DONE
Real source read (~20+ files). Analyzes Laravel Blade template files
(compiles them to PHP, then type-checks variable usage against what
the calling controller passed in) - a genuinely interesting
cross-file (PHP-to-template) type-safety idea in concept, but tightly
coupled to Blade's own template compiler. No portable finding without
Blade itself; Piwigo doesn't use Blade.

## 55. ticketswap/phpstan-error-formatter - DONE
Real source read (2 files). A small formatter tweak: detects the
GITHUB_ACTIONS env var to switch the file-location link format
between a clickable-terminal style and a plain one. Minor, low-priority
idea - not the full GitHub Actions ::error annotation protocol, just a
link-format switch.

## 56. codeigniter/phpstan-codeigniter - DONE
Real source read (~20 files). CodeIgniter-framework-specific dynamic
type extensions. Same framework-support theme, no new finding.

## 57. friendsoftypo3/phpstan-typo3 - DONE
Real source read (6 files). TYPO3-framework-specific, same theme as
#28. No new finding.

## 58. sidz/phpstan-rules - DONE
Real source read (13 files). A genuinely novel, comprehensive "no
magic numbers" family: bans a bare numeric literal (int/float, with a
configurable exempt-numbers list e.g. [0, 1, -1] and an optional
numeric-string variant) from appearing across ~12 distinct AST
positions (assignment, ternary, switch case, return, match, logical/
comparison/bitwise/arithmetic operators, default parameter value,
property assignment, function argument) - a real, well-known code
quality convention requiring named constants instead, with zero
existing phpstan-rs equivalent. Shares one clean base-class detection
predicate (`isNumeric`, handling unary +/-, string-casts, optional
numeric-string literals) that all position-specific checks reuse -
easy to port as one shared helper plus N small call-site checks.

## 59. swisnl/phpstan-faker - DONE
Real source read: 2 files, Faker library magic-property/method
modeling ($faker->name, $faker->email via __get/__call). Same
library-magic pattern already noted. No new finding.

## 60. grifart/phpstan-oneline - DONE
Real source read: 1 file. A compact, configurable-template, exactly-
one-line-per-error formatter (grep/editor-quickfix friendly). Minor;
phpstan-rs's existing default text format may already be close to this
shape - worth a quick side-by-side comparison at triage time rather
than assuming a gap.

## 61. dave-liddament/phpstan-php-language-extensions - DONE
Real source read (~28 files). A genuinely rich, novel "extended
access-control" family, complementing (not duplicating) the already-
built @internal/restricted_symbols work:
- Friend{Static,New,Method}CallRule: a real `#[Friend(AllowedCaller::
  class)]` PHP attribute placed on a method/class, explicitly
  whitelisting which specific OTHER classes may call it - the classic
  C++ `friend` concept, ported via attributes. Notably a *simpler*,
  possibly-easier design angle on the same underlying need Phase 67's
  own declined spaze allow/disallow-in DSL was trying to solve
  generically: instead of one global ban+context-filter DSL, the
  whitelist is declared per-symbol, at the target itself, via a plain
  attribute argument - no path/context-matching engine needed. Worth
  considering as an alternative, cheaper-to-build mechanism when
  Phase 67's declined DSL is revisited.
- NamespaceVisibility{Static,New,Method}CallRule: declares a whole
  namespace/class as visible only from specific other namespaces via
  attribute - a coarser-grained sibling of Friend.
- TestTag{Static,New,Method}CallRule: marks a method `@testOnly`
  (or attribute-equivalent) and flags *production* code calling it -
  the inverse framing of Phase 115's autoload-dev-symbol-reachability
  idea, but keyed on an explicit author-written marker rather than
  composer.json's own PSR-4 split.
- RestrictTraitToRule: restricts a trait to only be usable (via `use`)
  by a whitelisted set of classes - a real, novel, narrow access-
  control primitive with no phpstan-rs equivalent.
- Package{Static,New,Method}CallRule: real Composer-package-boundary
  visibility enforcement - directly overlaps with Phase 94's P2.
MustUseResultRule/OverrideRule look like already-covered patterns
(NoDiscard/result-unused, #[Override] validation) - not detailed
further.

## 62. ekino/phpstan-sonata - DONE
Real source read: 3 files, Sonata Admin (Symfony ecosystem)-specific.
No new finding.

## 63. nepada/phpstan-nette-tester - DONE
Real source read: 2 files, Nette Tester's own Assert::* narrowing.
Same assertion-narrowing pattern already established. No new finding.

## 64. hiperdk/phpstan-rules - DONE
Real source read: 1 file. TypedConstantRule requires every class
constant carry an explicit PHP 8.3 native type declaration
(`const int FOO = 5;`, not bare `const FOO = 5;`) - a small, real,
easy opinionated strictness rule, a natural sibling of phpstan-rs's
own existing missingType.* family (param/property/return) if class
constants aren't already covered by an equivalent native-type check.

## 65. struggle-for-php/sfp-phpstan-psr-log - DONE
Real source read (~18 files). A second, more thorough, independent
PSR-3 logging package - deepens #45's already-noted finding with
genuinely new sub-checks beyond simple placeholder matching:
- MessageStaticStringRule: requires the log message itself be a
  static/literal string, not a dynamically-concatenated one -
  dynamic values belong in the context array's own placeholders for
  structured-logging/log-aggregation friendliness (concatenating
  `"User $id logged in"` defeats log grouping/searching). A real,
  valuable, novel best-practice check.
- LogMethodLevelRule: validates a log-level argument is a recognized
  PSR-3 level constant.
- ContextTypeRule/ContextKeyRule/ContextRequireExceptionKeyRule:
  validates the *structure* of the context array itself (e.g.
  requiring a context['exception'] key actually be a Throwable) - a
  real, valuable structured-logging discipline check, distinct from
  pure placeholder-matching.
Fold these into the same PSR-3-logging-hygiene family as #45 when
triaging - a strictly richer version of the same idea.

## 66. roave/no-floaters - DONE
Real source read (6 files). Bans the `float`/`double` type from
appearing anywhere in a codebase (property types, function/method
param/return signatures, variable assignments) - an opinionated but
real, well-known convention (avoid floating-point for money/precision-
critical domains, force integer-cents or decimal-string/BCMath
representations instead). A distinct, novel category from the
already-established "ban this construct"/"ban this call" families:
"ban this native *type*" - a simple, self-contained, easy check family
(walk every type-hint position, flag Type::Float).

## 67. pestphp/pest-plugin-phpstan - DONE
Real source read (~45 files: src/Rules, src/Type/Pest, src/Analysis/
Expectation). Entirely Pest-test-framework-specific: every rule
(ImpossibleExpectationRule, RedundantExpectationRule,
DuplicateTestDescriptionRule, EmptyTestClosureRule,
DisallowedCallInDescribeRule, InvalidGroupNameRule,
RepeatWithInvalidValueRule, StaticTestClosureRule) and every type
extension (closure $this-binding for it()/test()/describe()/beforeEach()
bodies, expect()-chain type narrowing, TestCase trait composition) is
built entirely around detecting Pest's own DSL functions
(it/test/describe/expect/uses/dataset) via PestFunctionDetector - none
of it is reachable without Pest's specific global functions existing as
a concept at all. Same root cause as the other framework-integration
packages: no portable generic-rule value without building Pest-specific
DSL-detection infrastructure first, which phpstan-rs has zero of today
(different from PHPUnit's assertion-narrowing table, which works off
ordinary PHPUnit method calls, not a global-function DSL). Interesting
*shapes* (statically-impossible-assertion detection, duplicate-test-name
detection, empty-test-body detection) but not extractable as generic
rules since they're all defined in terms of Pest's own call structure -
noted for completeness, not a new finding category.

## 68. slam/phpstan-laminas-framework - DONE
Real source read (~20 files). Entirely Laminas/Zend-framework-specific:
ServiceManager/PluginManager container get() dynamic-return-type
resolution based on the framework's own service-config wiring
(AbstractServiceLocatorGetDynamicReturnTypeExtension,
ServiceManagerGetDynamicReturnTypeExtension, PSR-container get()
extension), plus a StdlibAbstractOptionsPropertiesClassReflectionExtension
(magic property access via Laminas's AbstractOptions base class). Same
root cause as every other DI-container/service-locator framework
package already catalogued (Symfony, Doctrine, Laravel, etc.) - no
portable generic-rule value without framework-specific config-resolution
infrastructure. No new finding category.

## 69. mavimo/phpstan-junit - DONE (genuinely new, easy finding)
Real source read (1 file, ~70 lines). A JUnit-XML `ErrorFormatter` -
each diagnostic becomes a `<testcase>` (failure) in a `<testsuite>`,
consumable natively by Jenkins/GitLab-CI/GitHub Actions test-report UI
widgets. phpstan-rs's own `src/output.rs:42-47` already has an
`OutputFormat` enum (`Text`/`Json`/`Checkstyle`) - adding `Junit` as a
4th variant is a small, self-contained, real CI-integration value-add
(test-report-shaped output, distinct from log-shaped Checkstyle/JSON),
not a checker rule. Genuinely novel, not covered anywhere in
PHASE_BACKLOG.md/ROADMAP.md.

## 70. brainbits/phpstan-rules - DONE (small, genuinely new finding)
Real source read (2 files). PHPUnit `@covers`/`#[CoversClass]`/
`#[CoversFunction]`/`#[CoversNothing]` tag hygiene, config-driven by a
unit-test-namespace-pattern string:
- CoversClassExistsRule: validates the class named in a `@covers`/
  `#[CoversClass]` tag actually exists (via ReflectionProvider) - a
  real, generic, cheap symbol-existence check, natural extension of
  phpstan-rs's own existing Phase 48 symbol-existence family applied to
  one more PHPDoc-tag/attribute position. Small, self-contained, real
  value for any PHPUnit-using project (catches a renamed/typo'd covered
  class going silently stale).
- CoversClassPresentRule: opinionated team-convention enforcement
  (every unit-test class must have *some* `@covers`/attribute) - narrow,
  team-specific policy, lower priority than the exists-check.

## 71. johnbillion/wp-compat - DONE
Real source read (main SinceVersionRule.php, ~400 lines, plus support
files). WordPress-core version-gating: given a `Requires at least:`
plugin-header version, flags any WP core function/method/filter/action
call requiring a newer WP version than declared, driven by a vendored
`symbols.json`/wp-hooks data file. Conceptually the same *pattern* as
phpstan-rs's own existing `PhpFeature`/`php_version.rs` gating
framework, just applied to WordPress's own API surface instead of core
PHP - but entirely dependent on a WordPress-specific version-tagged
symbol database this project has no reason to vendor. Same root cause
as other framework-specific packages (needs a framework's own metadata,
not a portable generic mechanism). One somewhat novel wrinkle noted for
completeness only, not actionable here: it tracks version-gating not
just at the whole-symbol level but per individual *parameter addition*
over a hook's lifetime (a hook's 4th argument added in a later WP
version than the hook itself) - a finer-grained version of the
`PhpFeature` idea than phpstan-rs currently models even for pure PHP,
but not worth pursuing without a concrete pure-PHP trigger (no core PHP
function has ever had a parameter's own independent "since" version
distinct from the function's own "since" that this project has needed
to model, unlike WordPress's hook-parameter-versioning convention).

## 72. nextras/orm-phpstan - DONE
Real source read (4 files). Nextras ORM-specific: entity magic-property
reflection extensions (annotation-driven property/relationship/datetime
type resolution for Nextras\Orm\Entity\IEntity subclasses) and a
setValue()/setReadOnlyValue() call-site type-check against that same
ORM-specific reflection. Same root cause as every other ORM/framework
package already catalogued - needs the ORM's own entity-metadata
resolution mechanism, no portable generic-rule value. No new finding.

## 73. swisnl/phpstan-bitbucket - DONE
Real source read (1 file). Not a pure-text output format like #69's
JUnit formatter - this one makes live network calls to Bitbucket's own
REST API (BitbucketApiClient::createReport/addAnnotation) to post
"code insights" report annotations directly to a Bitbucket Pipelines
run. Different category from a text-serialization `OutputFormat`
variant: needs live API credentials/network access during error
formatting, which is a CI-vendor-integration concern outside a local
static-analysis engine's natural scope. Noted as reinforcing the
CI-output theme (alongside #69's JUnit finding) but not itself
actionable the same cheap way - would need a deliberate decision to
add network-calling side effects to a diagnostic formatter, unlike the
pure-text JUnit case.

## 74. gosuperscript/phpstan-rules - DONE (two genuinely new findings)
Real source read (3 files, small package). Both rules are novel and
NOT framework-specific:

- StrictComparisonOfObjectsRule (high value, easy, fully generic):
  flags `$a === $b` (Identical/NotIdentical) whenever `$a`'s static
  type is a class defining a native `equals($other)` method whose
  parameter type accepts `$b`'s type - the class author clearly
  intended value-equality semantics via `->equals()`, so `===`
  (reference identity) is almost always a real bug (two logically-equal
  but distinct instances won't be `===`-equal, a classic value-object/
  DTO comparison mistake). Completely generic, no framework dependency,
  cheap to implement (walk `BinOp::Identical`/`NotIdentical`, check LHS
  type for a native 1-arg `equals` method accepting RHS's type). Not
  covered anywhere in PHASE_BACKLOG.md/ROADMAP.md - genuinely new.

- RestrictImplicitDependencyUsage (high value, novel, more involved):
  implements `RestrictedClassNameUsageExtension` to flag any class
  reference whose owning Composer package is NOT explicitly declared in
  the *current module's own* `composer.json` `require`/`require-dev` -
  even though Composer's flat autoloader makes it *resolvable*
  (installed transitively via some other direct dependency). Catches a
  real, common monorepo/multi-package bug class: "implicit dependency"
  usage that silently breaks the moment the actual direct dependency
  that happened to also require the transitive one is removed/upgraded
  away. Conceptually PHP/Composer's analogue of Node's well-known
  `no-extraneous-dependencies` ESLint rule. Needs: parsing the project's
  own `composer.json` (already-declared requires) + `vendor/composer/
  installed.json` (every installed package's own PSR-4 namespace
  mapping) to build a class-name -> owning-package index, then checking
  every resolved class reference's owning package against the declared-
  requires set. A distinct, new, high-value category - not the same as
  the existing "declarative architecture testing" phpat idea (directory/
  namespace layering rules) - this is Composer-manifest-driven
  dependency-declaration validation specifically. Worth its own new
  phase.

## 75. symplify/template-phpstan-compiler - DONE (abandoned, repo deleted)
Packagist confirms `abandoned: true`, repository URL
(deprecated-packages/template-phpstan-compiler) 404s - the repo has
been deleted entirely, not just archived. No source available to
review. No finding possible.

## 76. kcs/psr-phpstan-rules (real repo: alekitto/psr-phpstan-rules) - DONE
Real source read (1 real file, rest is tests). PSR-11 container
`->get('service-id')` dynamic-return-type extension via a config-driven
service-id-to-class map. Same DI-container/PSR-11 family already
catalogued repeatedly (kcs on Packagist is an alias to alekitto's real
repo). No new finding.

## 77. glpi-project/phpstan-glpi - DONE
Real source read (8 files). Entirely single-application-specific (GLPI
ticketing system's own house rules), version-gated to GLPI's own
release stream: bans `exit`/`die` (GLPI's own post-request-routine
architecture constraint) and `http_response_code()` (citing a specific
PHP bug affecting GLPI's own usage), plus GLPI-specific global-variable
type resolution (`$CFG_GLPI` etc.) and `CommonDBTM::input`-hook param-out
typing (GLPI's own base-model ORM pattern). The two "forbid this call"
rules are the same shape as the already-covered generic ban-mechanism
(Phase 67's `restricted_symbols`) - no new mechanism, just app-specific
config values. The globals/ORM-hook resolvers are GLPI-app-specific,
zero portable value. No new finding.

## 78. rector/mockstan - DONE (genuinely new family, high value)
Real source read (17 files: all Rules + support classes). A rich,
entirely generic (PHPUnit-native-mock-API-only, no framework-specific
type-extension infra needed) family of PHPUnit mock-hygiene checks -
each a pure AST/method-call-shape pattern, buildable with phpstan-rs's
existing test-class detection + method-name matching, no new mechanism:
- AvoidAnyExpectsRule: `->expects($this->any())` is ambiguous - use an
  explicit count or a stub instead.
- NoWithOnStubRule: `->method('x')->with(...)` without a preceding
  `->expects(...)` call is a stub, and PHPUnit's own `with()` on a stub
  is silently a no-op (a real, easy-to-miss test bug - the assertion
  looks like it's checking arguments but never does).
- NoDoubleConsecutiveTestMockRule: combining
  `willReturnOnConsecutiveCalls()` and `willReturnCallback()` on the
  same mock is redundant/confusing - use callback alone.
- NoMockObjectAndRealObjectPropertyRule: a property typed as a union/
  intersection mixing `MockObject` with a real class is an ambiguous
  smell - pick one.
- ExplicitExpectsMockMethodRule: a mocked method configured via
  `->method(...)` needs an explicit `->expects(...)` count, not left
  implicit.
- ForbiddenClassToMockRule: config-driven ban on mocking specific
  classes (same shape as `restricted_symbols`, applied to mock-target
  class names specifically).
- NoDocumentMockingRule/NoEntityMockingRule: Doctrine-specific (mocking
  an ODM document/entity is discouraged vs. constructing a real
  instance) - the two Doctrine-dependent rules in an otherwise generic
  package.
- NoMockOnlyTestRule: a test consisting only of mocked properties with
  no real/non-mocked property is suspicious - it isn't testing anything
  real.
- RequireAtLeastOneRule: `$this->atLeast(0)` is meaningless (matches any
  call count including zero) - require >= 1.
This meaningfully deepens the existing PHPUnit-assertion-narrowing-table
theme with a genuinely distinct, generic sub-family: native PHPUnit
mock-builder API misuse detection (not narrowing assertion types, but
catching mock-configuration mistakes). Worth its own set of checks/
sub-phase, most of them individually cheap (single AST-shape pattern
match each).

## 79. mrpunyapal/peststan - DONE (near-duplicate of #67)
File listing confirms this is essentially the same package as #67
pestphp/pest-plugin-phpstan (identical class names/namespaces structure:
PestFunctionDetector, ExpectationSemanticAnalyzer, the same Rules/ and
Type/Pest/ file set) - almost certainly a fork/predecessor or the
package #67 itself forked from. Same entirely-Pest-DSL-specific
conclusion as #67 applies; no new finding.

## 80. cambis/silverstan - DONE (one generic finding, rest framework-specific)
Real source read (~15 files: Rule/, Reflection/, PhpDoc/,
ConfigurationResolver/). Overwhelmingly SilverStripe-CMS-specific:
DataObject ORM config-property resolution (SilverStripe's own
`private static $db`/`$has_one` config-inheritance-with-middleware
system), `Extensible`/`Injectable` trait-driven mixin/DI detection,
config-property override/deprecation rules - all needs SilverStripe's
own metadata model, same root cause as other CMS/framework packages.

One rule is genuinely generic and worth noting -
**RequireParentCallInOverridenMethodRule**: config-driven list of
`{class, method, isFirst?}` entries; any override of that method in a
subclass of that class is required to call `parent::method()` (optionally
required to be the *first* statement). A real, useful, easy, fully
generic mechanism (walk `ClassMethod` overrides, check class-is-a
against the configured list, scan body for the matching `parent::`
static call) - the general "lifecycle-hook base-class contract" pattern,
applicable project-wide to any codebase with a documented "must call
parent::onX()" convention (many base-class/framework patterns beyond
SilverStripe specifically - abstract test-case setUp/tearDown-style
hooks, kernel boot sequences, etc.). Not covered anywhere in
PHASE_BACKLOG.md/ROADMAP.md - genuinely new, small, worth adding.

## 81. calebdw/fakerstan - DONE
Real source read (7 files). Faker library-specific: resolves the
concrete return type of Faker's magic `$faker->word()`/`->email()`/etc.
provider-method calls (Faker exposes these via `__call` magic dispatch
across provider classes, invisible to static analysis without this
extension). Single-library-specific dynamic-method-return-type
resolution, same shape as the many other single-library type-extension
packages already catalogued (needs Faker's own provider-method
signature catalog, not a portable generic mechanism). No new finding.

## 82. mark-gerarts/phpstan-automapper-plus - DONE
Real source read (no PHP source at all - just a `phpstan.neon`-style
`ignoreErrors` config suppressing one specific "too many args" false
positive for AutoMapper+'s own `MapperInterface::map()` BC-compat
history). Not a checker mechanism at all, purely a config workaround for
one library's own interface-versioning quirk. No finding.

## 83. calebdw/larastan (abandoned, renamed to calebdw/phpstan-laravel) - DONE
Packagist confirms abandoned/renamed. Real source read of the renamed
package's Rules/ directory (28 rule files) - entirely Laravel-specific:
Eloquent Model magic-method/scope/accessor/relation checks, Queue job
batching/uniqueness/serialization contract checks, config()/env()/trans()
helper-call validation, Octane compatibility, route/view-name existence.
All require Laravel's own runtime conventions and metadata (model
attribute registries, service container bindings, queue connection
config) - same root cause as every other Laravel package already
catalogued repeatedly. A few rules (NoUnnecessaryCollectionCallRule,
NoUselessValueFunctionCallsRule, NoUnnecessaryEnumerableToArrayCallsRule)
are instances of a generically-interesting "redundant/no-op call in a
fluent method chain" pattern, but each is defined in terms of Laravel's
own Collection/Enumerable API semantics specifically - not extractable
as a portable check without that framework's own API knowledge. No new
finding category.

## 84. pb30/phpstan-composer-analysis - DONE (strong 2nd confirmation of #74's idea, deepens it significantly)
Real source read (3 files). A thin PHPStan-collector wrapper around the
separate standalone tool `shipmonk/composer-dependency-analyser`
(vendored as a real dependency, not reimplemented) that runs once per
analysis and reports 4 distinct composer.json-hygiene checks, each
scoped per prod-path/dev-path classification:
1. **Shadow dependency**: a class is used but its owning package is NOT
   declared in `composer.json` (only resolvable via Composer's flat
   autoloader because some *other* direct dependency happens to also
   require it) - this is the exact same idea as #74's
   RestrictImplicitDependencyUsage, now independently confirmed by a
   second, different, more widely-used real tool. Strengthens that
   finding from "one package's idea" to "two independent real
   implementations agree this is valuable."
2. **Dev dependency used in production code**: a class from a
   `require-dev`-only package is referenced from a file classified as
   "prod" (not a dev/test path) - would break a `composer install
   --no-dev` production deployment. Genuinely new sub-check beyond #74's
   scope (dev/prod misclassification, not just "undeclared").
3. **Prod dependency used only in dev paths**: the inverse - a
   `require`-declared package is only ever referenced from dev/test
   paths, meaning it should be `require-dev` instead. Also new.
4. **Unused dependency**: a `composer.json`-declared package
   (require or require-dev) has zero real usage anywhere in the scanned
   paths at all - can be removed. Distinct from shadow/misclassification,
   its own real value (dependency-bloat detection).
All 4 need the same core infrastructure: (a) parsing `composer.json`'s
own `require`/`require-dev` plus `vendor/composer/installed.json`'s
per-package PSR-4 namespace map (build a class-FQN -> owning-package
index, same as #74 needs), and (b) a prod-path-vs-dev-path
classification of every scanned file (config-driven, e.g. `tests/`
patterns = dev). Confirms this whole category (Composer-manifest-driven
dependency-declaration hygiene) is a real, multiply-validated,
high-value new phase candidate - fold #74 and #84's findings together
into ONE new phase covering all 4 sub-checks when triaging, not two
separate half-finished phases.

## 85. localheinz/phpstan-rules - DONE (rich, highest-value package found so far - fork of ergebnis/phpstan-rules by the same author)
Real source read (~35 files). Namespace/copyright headers confirm this
is Andreas Möller's own fork/republish of the well-known
`ergebnis/phpstan-rules` package (already referenced earlier in this
session as the source of the "ban eval/isset" family) - this is the
canonical, most complete, best-designed version of that whole family.
Entirely generic, zero framework dependency, all cheap AST-shape checks:

**"Ban this construct" family (deepens the already-noted ergebnis
family significantly):** NoEvalRule, NoIssetRule, NoCompactRule (bans
`compact()` - discourages implicit variable-name-to-array-key magic),
NoErrorSuppressionRule (bans the `@` operator), NoSwitchRule (bans
`switch` entirely, opinionated), NoAssignByReferenceRule (bans `=&`),
NoNamedArgumentRule (bans PHP 8 named arguments, opinionated),
NoPhpstanIgnoreRule (bans `@phpstan-ignore`/`@phpstan-ignore-line`
comments entirely - forces fixing real issues instead of suppressing,
an interesting "eat your own dog food" policy check).

**Class-shape policy family (novel, generic, cheap):** FinalRule
(require every class be declared `final`), NoExtendsRule (ban class
inheritance project-wide, forcing composition-over-inheritance),
DeclareStrictTypesRule (require `declare(strict_types=1)` in every
file), FinalInAbstractClassRule (concrete methods in an abstract class
must be final - only overridable via the abstract methods), PrivateInFinalClassRule
(properties/methods in a `final` class should be `private`, not
`protected`, since there's no subclass to protect from).

**Signature-shape policy family (repeated 3x - Functions/Methods/
Closures, each its own file, same shape, real opportunity for one
shared generic implementation instead of 3 near-duplicates):**
NoNullableReturnTypeDeclarationRule, NoParameterWithNullableTypeDeclarationRule,
NoParameterWithNullDefaultValueRule, NoParameterPassedByReferenceRule,
NoReturnByReferenceRule (methods/functions only, not closures) - all
opinionated "avoid nullable/by-ref" signature-shape bans.
NoConstructorParameterWithDefaultValueRule (constructor promotion
hygiene - force explicit assignment, no implicit defaults).
NoParameterWithContainerTypeDeclarationRule (bans a parameter typed as
a DI-container/service-locator interface itself - a real, valuable
anti-service-locator-pattern check, generic and not tied to any
specific DI framework, just to well-known container interface names
like PSR-11's `ContainerInterface`).

**InvokeParentHookMethodRule (HIGH VALUE - the canonical, best-designed
version of #80's SilverStripe-specific finding, confirms and upgrades
it):** the fully-generic version of "require calling `parent::method()`
in an override" - config-driven `{className, methodName, invocation:
first|last|any|never, hasContent: yes|no|maybe}` entries, shipped with
sensible defaults for PHPUnit's own `setUp`/`tearDown`/
`setUpBeforeClass`/`tearDownAfterClass`/`assertPreConditions`/
`assertPostConditions` and Codeception's own hook methods. This is a
strictly better design than #80's narrower version (supports
first/last/any/never positioning, not just first-or-anywhere; walks up
to the real declaring parent class via reflection rather than requiring
an exact match). **Replace #80's note as the design reference for this
finding** - build against this shape, not SilverStripe's narrower one.
Real, concrete value even with zero config (the PHPUnit/Codeception
defaults alone are broadly useful for any PHPUnit-based codebase).

**Lower priority, narrower / niche:** TestCaseWithSuffixRule (naming
convention), NoParameterWithNullableTypeDeclarationRule variants.

## 86. bonami/phpstan-collections - DONE
Real source read (7 files). Entirely specific to the `bonami/collections`
generic-collections library's own classes (ArrayList/Map/etc.):
late-static-binding return-type propagation for its own fluent builder
methods, `groupBy()`/`filter()`/`map()`-without-nulls element-type
narrowing, all keyed off that one library's own template parameters and
method names. Same shape as every other single-library type-extension
package (needs that library's own generic class model, not portable).
One general *pattern* worth noting for completeness only (not
actionable without a similar first-party or common generic collection
type in phpstan-rs's own scope): narrowing a collection's element type
after `->filter($predicate)` based on the predicate callable's own
type-guard behavior (e.g. `fn($x): bool => is_string($x)`) - conceptually
adjacent to phpstan-rs's own existing assertion-narrowing-table
mechanism, just applied to a higher-order-function argument instead of
a direct boolean expression. No concrete new finding.

## 87. symplify/twig-phpstan-compiler - DONE (abandoned, repo deleted)
Same situation as #75: `abandoned: true`, repository URL 404s (deleted
entirely). No source available, no finding possible.

## 88. jiripudil/phpstan-sealed-classes - DONE (reinforces already-covered ground)
Real source read (4 files). A custom `#[Sealed(SubtypeA::class, ...)]`
PHP attribute (not real PHPStan's own native `@phpstan-sealed` PHPDoc
tag) enforcing exhaustive, closed-set subtyping: (1) the attribute can
only be applied to a class/interface, (2) every class named in the
attribute's own permitted-subtype list must actually be a direct
subtype (immediate extends/implements) of the sealed type - the inverse
direction from real PHPStan's own `@phpstan-sealed` tag (which
constrains "may X extend/implement Y", this instead validates "are all
the *claimed* permitted descendants actually real direct descendants").
This is conceptually the same territory already covered by real
PHPStan's native `@phpstan-sealed` tag support (already referenced in
this project's own Phase 67 design as one of the `@internal`-adjacent
PHPDoc-tag positions needing wiring) - no new mechanism, just a
different (attribute-based, not tag-based) source syntax for declaring
the same constraint, plus one validation direction (declared-permitted-
subtype-is-real) worth double-checking is included when phpstan-rs's
own `@phpstan-sealed` support is built/audited. Not a new finding
category.

## 89. mahocommerce/maho-phpstan-plugin - DONE
Real source read (8 files). Maho (a Magento 1 fork/OpenMage successor)
e-commerce-platform-specific: VarienObject magic-property/magic-method
reflection (Magento's own base-object pattern, `getData('key')`/
`__call` magic getters/setters), Mage::getModel()/getSingleton()
factory-string-to-class resolution. Same DI/magic-property framework
root cause as every other e-commerce-platform package already
catalogued (Magento/CakePHP/Drupal/WordPress/TYPO3/SilverStripe/GLPI).
No new finding.

## 90. symplify/latte-phpstan-compiler - DONE (abandoned, chain of dead redirects)
Packagist shows `abandoned -> reveal/latte-phpstan-compiler`, but that
replacement package doesn't exist on Packagist either, and the GitHub
org named in the original repository URL (`reveal-php/latte-phpstan-
compiler`) 404s too - same dead-end pattern as #75/#87 (the whole
Symplify template/twig/latte-compiler trio appears genuinely
abandoned/deleted with no live successor). No source available, no
finding possible.

## 91. eloquent/phpstan-phony - DONE
Real source read (9 files). Type extensions for the `eloquent/phony`
third-party mocking library's own fluent builder API (MockBuilder,
InstanceHandle, mock-return-type propagation) - same shape as the
already-catalogued Mockery/Prophecy support in the pluggable
assertion/mock-narrowing family. No new mechanism, one more library
this project's future pluggable-table design would need a mapping
entry for. No new finding.

## 92. voku/phpstan-rules - DONE (one high-value new finding, one family-reinforcement)
Real source read (21 files). Entirely generic, no framework dependency:

- **InArrayLooseComparisonRule (HIGH VALUE, new, easy)**: flags
  `in_array($needle, $haystack)`/`array_search(...)` called without an
  explicit strict `true` third argument when `$needle`/haystack-element
  types are a non-numeric-string vs. int/float pair - the exact PHP 7-to-
  PHP-8 loose-comparison-semantics hazard (`0 == "foo"` flipped from
  true to false in PHP 8). Deliberately fail-open (skips constant
  arrays, mixed/union types, numeric strings whose loose comparison
  didn't change). A real, concrete, version-relevant correctness check
  fitting naturally alongside phpstan-rs's own existing `PhpFeature`-
  gated PHP-version-behavior-change checks. Not covered anywhere in
  PHASE_BACKLOG.md/ROADMAP.md - genuinely new.

- **ExtendedBinaryOpRule/ExtendedAssignOpRule (deepens #32's shipmonk
  arithmetic-strictness family further - 3rd independent confirmation)**:
  bans a string operand combined with any non-string, non-mixed operand
  across most binary operators (except `===`/`!==`/`??`/`&&`/`||`, and
  except `.`/`==`/`!=` when the string is the empty-string constant) -
  and separately bans an array operand combined with a non-array,
  non-mixed operand. Same "ban this specific operand-type-mismatch
  combination" idea as shipmonk's ForbidArithmeticOperationOnNonNumberRule/
  AllowComparingOnlyComparableTypesRule family - strong repeated signal
  this whole family is a real, valued category worth building.

- **IfCondition* family (IfConditionRule/BooleanAndRule/BooleanOrRule/
  BooleanNotRule/TernaryOperatorRule/SwitchCaseRule/MatchRule, config-
  driven)**: primarily style/lint-shaped (yoda-condition enforcement,
  disallowing specific classes' instances in conditions) largely
  overlapping real PHPStan's own native impossible-condition/always-
  true-false checks - lower priority, mostly redundant with existing
  coverage. One config option worth flagging on its own,
  `checkForAssignments` - detects `if ($x = foo())` (assignment instead
  of comparison inside a condition), a classic real typo bug
  (`=` vs `==`) - if not already caught by an existing phpstan-rs check,
  worth a quick standalone verification.

- WrongCastRule: narrow, app-specific (config list of classes to check
  a string-to-int cast against) - low priority.

## 93. fooman/phpstan-magento2-magic-methods - DONE
Real source read (2 files). Magento 2-specific: resolves
`AbstractExtensibleModel`-derived classes' code-generated magic
`getX()`/`setX()`/`hasX()`/`unsX()` accessors from their own extension-
interface attribute lists. Same magic-property/framework-metadata root
cause as every other e-commerce/CMS-platform package already
catalogued. No new finding.

## 94. canvural/phpstan-blade-rule - DONE
Real source read (~24 files). Laravel Blade-template-specific: compiles
`.blade.php` templates to plain PHP (reusing Laravel's own Blade
compiler), analyzes the compiled PHP, and maps diagnostics back to the
original template's line numbers via a source-map mechanism, plus
resolving `view('name', ['var' => $val])`/`compact(...)`-passed
variables into the template's own scope. Same "compile a template DSL
to checkable PHP, source-map errors back" architecture as the already-
noted (and abandoned/dead) Symplify twig/latte-compiler packages -
confirms that general pattern is real and would be needed for any
future template-language support, but adds no new generic mechanism
beyond what's already conceptually noted from those dead packages, and
is itself entirely Blade/Laravel-View-specific. No new finding.

## 95. bellangelo/phpstan-migration-rules - DONE
Real source read (14 files, duplicated 1:1 across Laravel-migration and
Phinx-migration variants). A "database migration hygiene" rule family -
genuinely distinct category not seen elsewhere in this survey, but
entirely tied to a specific migration framework's own schema-builder DSL
(Laravel's `Schema::create()`/Phinx's `$table->...()` fluent builders),
so still framework-specific in the same sense as the DI-container/ORM
packages (needs that framework's own schema-builder call shape, not
portable). Rules: EnforceCollationRule (require explicit charset/
collation on new tables - avoids silent server-default drift),
ForbidAfterRule (bans `->after('column')` positional column placement -
a real zero-downtime-deployment/DB-engine-portability hazard),
ForbidEnumColumnRule (bans SQL ENUM columns - well-known "hard to
modify later" anti-pattern), ForbidMultipleTableCreationsRule (one
table per migration file - single-responsibility), ForbidRawSqlRule
(bans raw SQL strings, forces the schema-builder API for portability/
reviewability), NoDownMethodRule (opinionated forward-only-migration
policy - discourages writing reversible `down()` methods). Noted for
completeness as a real, coherent, novel category, but low priority for
this project's own target corpus (Piwigo is a plain PHP app, not built
on Laravel-migrations/Phinx) and would need building a schema-builder-
DSL-aware checker layer from scratch with no existing precedent in this
project - not a natural next step without a concrete first consumer.

## 96. sustainabil-it/phpstan-higher-order-collections - DONE
Real source read (11 files). Laravel-Collection-specific: resolves the
type of Laravel's "higher order collection proxy" pattern
(`$collection->each->someMethod()`/`->map->someMethod()`, where the
magic `__get('each')` returns a proxy forwarding the following method
call to every element) - needs Laravel's own Collection/HigherOrder
proxy classes and their own template-parameter model. Same Laravel-
framework-specific root cause as every other Laravel package already
catalogued. No new finding.

## 97. raul338/cakephp-phpstan-extensions - DONE
Real source read (3 files). CakePHP ORM-specific: `Table::findByX()`
magic-finder method resolution and CRUD-component dynamic-method-return
typing - same CakePHP-metadata root cause already catalogued at #48.
No new finding.

## 98. patchlevel/event-sourcing-phpstan-extension - DONE
Real source read (5 files). Entirely specific to the
`patchlevel/event-sourcing` library's own AggregateRoot pattern:
validates event-`#[Apply]`-method conventions (don't call `recordThat()`
from within an `apply()` method, don't mutate state outside an `apply()`
method), plus a "write-only property" check scoped specifically to
AggregateRoot subclasses (a property assigned somewhere but never read
anywhere, since event-sourced state should only ever be read via getters
- otherwise it's dead state). Checked against this project's own
existing member-usage machinery first: `src/checker/unused/
member_usage.rs` already tracks write-only/read-only distinctly
(comments at lines 78-85, 352-355 confirm an "only written"-vs-"only
read" distinction already mirrors real PHPStan's own `property.
onlyWritten`/`property.unread`) - so the *general* write-only-property
idea is already covered by existing infrastructure, just not scoped to
one specific library's base class the way this package narrows it. No
new finding; confirms existing coverage is already at least as complete
as this package's own generic half.

## 99. zorac/phpstan-php-di - DONE
Real source read (1 file). PHP-DI-specific: resolves an
`#[Inject]`-annotated property's type from the container's own service
definitions. Same DI-container root cause as every other DI package
already catalogued (Symfony/Laminas/PSR-11/etc.). No new finding.
