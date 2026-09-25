use rustc_hash::{FxHashMap, FxHashSet};
use std::sync::Arc;

use crate::types::Type;
use syntax::ast::Stmt;

pub mod access_attributes;
mod anonymous_class;
mod assertion_table;
mod class_info;
mod dynamic_constants;
mod function_sig;
mod generics;
mod global_vars;
mod hierarchy;
mod index;
mod name_context;
mod purity;
mod relation;
mod resolve;

pub(crate) use anonymous_class::collect_anonymous_classes;
pub(crate) use class_info::hook_body_references_property;
pub(crate) use class_info::object_class_names;
pub use class_info::{ClassInfo, ConstSig, PropertySig};
pub(crate) use function_sig::apply_implicit_nullable_default;
pub use function_sig::format_signature;
pub(crate) use function_sig::is_allow_dynamic_properties_attribute_name;
pub(crate) use function_sig::is_deprecated_attribute_name;
pub(crate) use function_sig::is_no_discard_attribute_name;
pub(crate) use function_sig::is_override_attribute_name;
pub(crate) use function_sig::param_label;
pub(crate) use function_sig::resolve_type;
pub use function_sig::{AssertInfo, AssertTarget, FunctionSig, ParamSig};
pub(crate) use function_sig::{
    build_template_params, infer_const_type, mark_template_params, substitute_type_aliases,
};
pub(crate) use index::{BuiltDecl, build_declarations, register_global_constant};
pub(crate) use name_context::{collect_class_name_case_refs, resolve_names_in_type};
pub use purity::PurityDependency;
pub(crate) use purity::{collect_impure_points, is_non_overridable};

/// Real PHPStan's own `ApiRuleHelper::isPhpStanName()` - whether
/// `fqn` is under the `PHPStan\` namespace itself (excluding the two
/// bundled-but-not-really-PHPStan namespaces it explicitly carves out).
/// Relocated here (from `checker/phpstan_api.rs`) so the vendor-directory
/// check's own registry-side plumbing (`phpstan_class_files`, populated
/// during indexing) can call it without a backward `registry -> checker`
/// dependency.
pub(crate) fn is_phpstan_namespace(fqn: &str) -> bool {
    // Real `isPhpStanName()` special-cases a bare `namespace PHPStan;`
    // (no trailing backslash, 7 characters) as true too
    // (`ApiRuleHelperTest.php`'s own `PHPStan`/no-declaring-file row
    // expects `true`) - a plain `fqn.len() >= 8` guard would reject it.
    fqn.eq_ignore_ascii_case("PHPStan")
        || (fqn.len() >= 8
            && fqn.as_bytes()[..8].eq_ignore_ascii_case(b"PHPStan\\")
            && !fqn.starts_with("PHPStan\\PhpDocParser\\")
            && !fqn.starts_with("PHPStan\\BetterReflection\\"))
}

/// A symbol table built by indexing already-parsed source, standing in for
/// the reflection a real PHP runtime would normally provide (see the
/// discussion this project started from: no live interpreter means no
/// `ReflectionClass`, so class/function signatures come from parsing
/// everything up front instead). `merge_stubs` layers in curated built-in
/// signatures (`stubs.rs`) the same way php-src stubs would sit alongside
/// project code in a real implementation.
#[derive(Debug, Default)]
pub struct Registry {
    pub classes: FxHashMap<Arc<str>, ClassInfo>,
    // Configured `[[assertions]]`/`[[class_argument_returns]]` entries,
    // on top of the built-in ones (`assertion_table`).
    pub(crate) configured_assertions: Vec<crate::assertion_tables::AssertionEntry>,
    pub(crate) configured_class_argument_returns: Vec<crate::assertion_tables::ClassArgumentReturn>,
    pub functions: FxHashMap<Arc<str>, FunctionSig>,
    // Global-constant name -> inferred-value-type table, covering both
    // `const NAME = value;` and `define('NAME', value)` (keyed like
    // `functions`: a `const` gets its namespace-qualified name; a
    // `define()`'s name is inserted verbatim, since real PHP evaluates
    // it as a plain runtime string regardless of the surrounding
    // namespace). Only a literal-shaped value is captured
    // (`infer_const_type`'s scope) - anything more dynamic is left
    // unindexed, the same "don't know, don't guess" leniency that
    // function already documents.
    pub global_constants: FxHashMap<Arc<str>, Type>,
    // `global $x;`-declared variable name -> inferred-type table, built by
    // folding every real assignment site to that name anywhere in the
    // corpus (`global_vars::infer_global_variables`) - the project-wide
    // vantage point real PHPStan's own per-file `GlobalHandler` doesn't
    // have, so a bare `global $x;` doesn't have to resolve to `Mixed`
    // unconditionally the way it otherwise would (see
    // `checker::stmt`'s own `StmtKind::Global` arm). Absent entry (not a
    // `Mixed`-valued one) means "no assignment site found or resolved" -
    // callers should treat a missing key the same as they already treat
    // `Mixed`.
    pub global_variables: FxHashMap<Arc<str>, Type>,
    // A strictness-parameter half: an unknown array literal's type
    // (`Array(Mixed)`) freely satisfies any array/shape type it's
    // checked against. Defaults to `false` (today's permissive
    // behavior).
    //
    // This and every `check_*`/opt-in flag below follow the same wiring
    // chain (config.rs field -> CLI flag -> `Project` setter -> here) and
    // the same rationale: matching real PHPStan's own opt-in default
    // (not part of any standard level 0-10 rule set) because this
    // project's version is either narrower than the real rule or the
    // real corpus shows a specific false-positive class it would
    // otherwise produce. Only what's specific to each flag is called out
    // below.
    pub strict_mixed_arrays: bool,
    // `Checker::check_uninitialized_properties` - this project's version
    // is a substantially simpler heuristic than PHPStan's real
    // `ConstructorsHelper`-backed dataflow (see `checker/uninitialized.rs`).
    pub check_uninitialized_properties: bool,
    // `Checker::check_internal_*` (`internal_usage.rs`).
    pub check_internal_usage: bool,
    // `check_use_function_imports`'s own `function.nameCase` half.
    // `use const`'s own existence check (`constant.notFound`) is
    // unconditional, not gated by this flag - real PHP constants are
    // case-sensitive, so there's no "resolves fine but wrong case" case
    // the way there is for a function/class name.
    pub check_function_name_case: bool,
    // `Checker::check_deprecated_*` (`deprecated_usage.rs`) - real
    // PHPStan's equivalent (`phpstan/phpstan-deprecation-rules`) is a
    // separate, opt-in Composer package, not part of core
    // `phpstan/phpstan`; this project has no "separate package" concept,
    // so an opt-in flag is the closest equivalent.
    pub check_deprecated_usage: bool,
    // `Checker::check_missing_checked_exceptions`
    // (`checker/throws/missing.rs`) - real PHPStan has no such rule at
    // all; the riskiest of these opt-in checks given the real corpus's
    // documented inconsistent `@throws` tagging.
    pub check_missing_checked_exceptions: bool,
    // The "genuinely empty concrete method body" return-completeness
    // check (`Checker::check_function_with_ancestor_hint`'s own
    // `return.missing`/`return.never` fall-through) - a real
    // false-positive class (IDE-stub/native-extension declaration files
    // with intentionally empty `{}` bodies) means this needs opt-in for
    // a project that scopes its own `paths`/excludes `vendor/`.
    pub check_empty_method_bodies: bool,
    // Strict (vs. the default, castable)
    // `PrintfPlaceholder::doesArgumentTypeMatchPlaceholder` matching mode
    // for `sprintf`/`printf`/`vsprintf`/`vprintf`/`fprintf`
    // (`checker/calls/format_string.rs`) - castable is real PHPStan's
    // stock default too.
    pub check_strict_printf_placeholder_types: bool,
    // Mirrors real PHPStan's own
    // `checkTooWideThrowTypesInProtectedAndPublicMethods` - without it,
    // `TooWideMethodThrowTypeRule` only runs for a `private` method, a
    // method on a `final` class, or one itself marked `final` (otherwise
    // any ordinary overridable method with an unused `@throws` tag would
    // be flagged, even though a subclass override might need the wider
    // type). When on, an overridable method still gets a pass on its
    // first declaration (`is_first_declaration` below) - only a
    // *re-declaration* narrowing an ancestor's wider `@throws` is
    // checked.
    pub check_too_wide_throw_type_in_protected_and_public_methods: bool,
    // The return-type analogue of the flag above, mirroring real
    // PHPStan's own `checkTooWideReturnTypesInProtectedAndPublicMethods`
    // for `TooWideMethodReturnTypehintRule`.
    pub check_too_wide_return_type_in_protected_and_public_methods: bool,
    // The by-ref-parameter analogue of the flag above, mirroring real
    // PHPStan's own `checkProtectedAndPublicMethods` for
    // `TooWideMethodParameterOutTypeRule`.
    pub check_too_wide_by_ref_type_in_protected_and_public_methods: bool,
    // `MethodThrowTypeCovarianceRule` (`checker/oop/method_overrides.rs`)
    // - a real PHPStan rule, but shipped opt-in here rather than
    // mirroring its unconditional-by-default registration: this
    // project's `@throws` model has no call-graph-wide implicit-throw
    // inference, so even this rule's pure declaration-vs-declaration
    // comparison is worth verifying against a real corpus before
    // defaulting on.
    pub check_method_throw_type_covariance: bool,
    // `project::duplicate_functions::check_duplicate_function_declarations`.
    // Real PHPStan's own `DuplicateFunctionDeclarationRule` is
    // `#[ValidatesStubFiles]` and never runs against ordinary scanned
    // files - two unconditionally-declared same-named top-level
    // functions in separate test files that are never loaded together in
    // the same PHP process are a real, structural false-positive this
    // checker can't rule out in general.
    pub check_duplicate_function_declarations: bool,
    // `project::duplicate_classes::check_duplicate_class_declarations` -
    // its one known false-positive shape (a vendored built-in stub
    // override, e.g. `Imagick.stub.php`, colliding with the real PHP
    // extension's class) can only be defused by a project's `stub_paths`
    // config being correctly populated. See `project::duplicate_classes`
    // for why this goes beyond real PHPStan, which has no whole-project
    // equivalent to mirror.
    pub check_duplicate_class_declarations: bool,
    // A bare `true`/`false` literal passed positionally into a
    // bare-`bool`-typed parameter is always type-correct, so no rule
    // can see it; opt-in since an unambiguous single-bool-parameter call
    // is common and would otherwise be noise project-wide.
    pub check_boolean_trap: bool,
    // Array-key cast warnings (a literal float/bool/null/canonical-
    // integer-string array key silently cast at runtime) - opt-in since
    // there's no real-world precedent for how noisy this is.
    pub check_array_key_cast: bool,
    // Flags a constructor that never calls an existing parent
    // constructor - opt-in since a legitimate constructor can genuinely
    // choose not to (composition-over-inheritance, a no-op parent).
    pub check_parent_constructor_call: bool,
    // Constructor-property-promotion suggestion - a purely stylistic
    // suggestion, not a bug, so opt-in.
    pub check_promoted_property_suggestions: bool,
    // Flags a condition/`&&`/`||`/`!` operand whose type isn't
    // confidently `bool` - opt-in.
    pub check_boolean_conditions: bool,
    // Strict `checkDynamicProperties` behavior inside `isset()`/
    // `empty()`/`??`/`??=` - the lenient default silently allows probing
    // a property that doesn't exist there; the stricter mode still
    // flags it. Opt-in.
    pub check_dynamic_properties: bool,
    // Swapped-adjacent-argument-name detection - a disclosed heuristic
    // (two same-typed parameters are invisible to any type-based check
    // as "different roles"), not a soundness gap, so opt-in.
    pub check_swapped_arguments: bool,
    // `never`-return-type suggestion for a body that provably never
    // returns normally - opt-in, a stylistic suggestion.
    pub check_never_return_suggestions: bool,
    // A `switch` with no `default:` arm - opt-in, since a switch over an
    // exhaustively-listed set of values (e.g. every case of a bounded
    // string enumeration) can be a deliberate, correct choice this
    // checker has no way to confirm is truly exhaustive.
    pub check_switch_missing_default: bool,
    // `throw new Exception()` (or any other class) with zero
    // constructor arguments - opt-in, since a custom exception subclass
    // can legitimately hardcode its own message internally, making a
    // bare `new` call at the throw site correct rather than a mistake.
    pub check_throw_without_message: bool,
    // `throw new Exception(...)`/`throw new Error(...)` - the bare base
    // class instead of a specific subclass - opt-in, since deliberately
    // throwing the generic base class (a project with no custom
    // exception hierarchy at all) is a legitimate style choice, not
    // necessarily an oversight.
    pub check_throw_base_class: bool,
    // Opt-in exception hygiene in `catch` blocks - see
    // `checker::stmt::try_catch::check_catch_hygiene`.
    pub check_exception_hygiene: bool,
    // Opt-in (slam/phpstan-extensions): code reading the wall clock implicitly - `time()`, date functions without their timestamp, relative date strings, argument-less `new DateTime()` (`currentTime.*`).
    pub check_current_time_dependence: bool,
    // Opt-in: a PSR-3 logger call without an `exception` context key while a Throwable variable is in scope (`psr3.contextMissingException`).
    pub check_psr3_exception_context: bool,
    // Opt-in (staabm/phpstan-psr3, sfp-phpstan-psr-log): PSR-3 logger call hygiene - placeholders, message, level and context (`psr3.*`).
    pub check_psr3_logging: bool,
    // `psr3.contextKeyPattern`'s required context-key shape.
    pub psr3_context_key_pattern: Option<regex::Regex>,
    // Opt-in (sidz/phpstan-rules): a bare numeric literal outside a constant declaration, in any of 13 positions (`magicNumber.*`); `magic_number_exemptions` (default `[0, 1]`) and `magic_numbers_ignore_numeric_strings` tune it.
    pub check_magic_numbers: bool,
    // `check_magic_numbers`' exempt values and numeric-string opt-out.
    pub magic_number_exemptions: Option<Vec<f64>>,
    pub magic_numbers_ignore_numeric_strings: bool,
    // Opt-in: a parameter or return type given only by PHPDoc that the configured PHP version can declare natively (`missingNativeType.parameter`/`missingNativeType.return`).
    pub check_native_type_hints: bool,
    // Opt-in (phpstan-strict-rules): `$x[...] = ...` creating the array `$x` implicitly because the variable does not or might not exist (`variable.implicitArray`).
    pub check_implicit_array_creation: bool,
    // Opt-in (phpstan-strict-rules): a cast to a type the expression already has (`cast.useless`).
    pub check_useless_casts: bool,
    // Opt-in (phpstan-strict-rules): `array_filter()` without a callback over values whose truthiness is ambiguous, or with a null callback (`arrayFilter.strict`).
    pub check_strict_array_filter: bool,
    // Opt-in (voku/phpstan-rules): a string operand combined with a non-string one, or an array with a non-array, by a binary operator or compound assignment (`binaryOp.nonStringOperand`/`binaryOp.nonArrayOperand`/`assignOp.*`).
    pub check_mixed_type_operands: bool,
    // Opt-in (shipmonk/phpstan-rules): `<`/`>`/`<=`/`>=`/`<=>` only between int|float, string, DateTimeInterface, or tuples of those (`comparison.nonComparable`/`comparison.differentTypes`).
    pub check_comparable_types: bool,
    // Opt-in (phpstan-strict-rules): arithmetic, unary `+`/`-` and increment/decrement operands that are not numeric (`plus.leftNonNumeric`, `unaryMinus.nonNumeric`, `preInc.nonNumeric`, ...).
    pub check_arithmetic_operands: bool,
    // Opt-in: `===`/`!==` between objects whose class defines an `equals()` method accepting the other side (`identical.equalsMethod`).
    pub check_identity_vs_equals: bool,
    // Opt-in: `in_array()`/`array_search()`/`array_keys()` compared loosely where the needle and haystack value types can be `==` without being `===` (`function.looseComparison`).
    pub check_loose_haystack_comparison: bool,
    // Opt-in (phpstan-strict-rules): `in_array()`/`array_search()`/`array_keys()`/`base64_decode()` without their strict argument set to `true`, and `iterator_to_array()` without `$preserve_keys` (`function.strict`).
    pub check_strict_function_calls: bool,
    // Opt-in (phpstan-strict-rules): a `switch` case whose type can never match the subject's (`switch.type`).
    pub check_switch_case_types: bool,
    // Opt-in (phpstan-strict-rules): a `foreach` key/value variable or `for` initial assignment overwriting an already-defined variable (`foreach.keyOverwrite`/`foreach.valueOverwrite`/`for.variableOverwrite`).
    pub check_loop_variable_overwrite: bool,
    // Opt-in (phpstan-strict-rules): a static method called through an instance, `$obj->staticMethod()` (`staticMethod.dynamicCall`).
    pub check_dynamic_static_calls: bool,
    // Opt-in (phpstan-strict-rules): `__construct()` called on an existing object, or statically outside a parent-constructor call (`constructor.call`).
    pub check_illegal_constructor_calls: bool,
    // Opt-in: enum branching that defeats match exhaustiveness (a default arm, switch, or if-chains over enum cases).
    pub check_enum_exhaustiveness: bool,
    // Opt-in: isset()/empty() on objects and possibly-null values silently coerced by strings, operators and compound assignments.
    pub check_null_coercion: bool,
    // `new SomeException(...)` constructed but discarded as a bare
    // statement, never thrown - opt-in, since a plain `new Foo();`
    // statement for a class with a genuine constructor side effect is
    // ordinary, legal code; only a `Throwable` subtype is even
    // considered, but this project can't rule out every legitimate
    // reason to build one without throwing it (e.g. handing it to a
    // logger).
    pub check_new_exception_discarded: bool,
    // A PHPUnit test method (a `test`-prefixed method on a
    // `PHPUnit\Framework\TestCase` subclass) or Pest `it()`/`test()`
    // closure with zero assertion-shaped calls anywhere in its own
    // body - opt-in, since this is a purely syntactic, per-body scan: a
    // test that only ever calls a custom assertion-helper method (one
    // not itself named with the `assert`/`expectException` prefix,
    // e.g. `$this->assertDatabaseHas(...)`-style Laravel-testing sugar
    // is still caught since that name *does* start with `assert`, but a
    // differently-named wrapper isn't) is a real, disclosed false-
    // positive this check can't rule out.
    pub check_risky_tests: bool,
    // A declared enum case never referenced via `EnumName::CaseName`,
    // never a `match`/`switch` arm value, and never provably produced
    // by a literal `::from()`/`::tryFrom()` argument, project-wide
    // (ROADMAP Phase 92) - opt-in, since this is a whole-program
    // fact this project's own closed-world registry can prove sound for
    // its own analyzed set, but real PHP has escape hatches this
    // project can't rule out in general: a case reachable only through
    // framework magic/reflection (a template calling `EnumName::
    // {$dynamicCaseName}` by a runtime-computed string, a DI container/
    // serialization layer resolving a case by name), or one referenced
    // only from inside a `Closure`/`ArrowFn` literal's own default
    // parameter value (a real, disclosed, narrower gap this check
    // inherits from `checker::expr::collect_default_and_const_value_
    // member_refs`, not something specific to this check).
    pub check_dead_enum_cases: bool,
    // A top-level function or class/interface/trait/enum never referenced
    // anywhere in the project (ROADMAP Phase 86's B2) - opt-in, for
    // real, disclosed reasons real PHPStan has no equivalent rule at all
    // (its own per-file, open-world reflection can't prove a symbol has
    // zero consumers anywhere; this project's closed-world registry
    // usually can, but not always): a function/class that's this
    // project's own public API, consumed only outside the analyzed set
    // (a library, not an application); a function referenced only via a
    // string-callable (`array_map('myFunc', ...)`); a class referenced
    // only as a PHP attribute target (`#[MyAttribute]`); anything
    // reachable only through framework magic/reflection.
    pub check_dead_code: bool,
    // A namespace-level `extends`/`implements`/`use`-trait dependency
    // cycle (ROADMAP Phase 86's B3) - real, structural, and fully
    // precise (every edge is an already-canonical `ClassInfo` field, no
    // approximation), so this is opt-in only because it's a genuinely
    // new architectural-smell category, not because of any disclosed
    // false-positive risk the way most of this project's other opt-in
    // checks are. Deliberately narrower than a full "any reference"
    // dependency graph - see `project::whole_program::circular_
    // namespace_deps`'s own module doc comment for why.
    pub check_circular_dependencies: bool,
    // An `abstract class` with zero concrete descendants anywhere in the
    // closed universe (ROADMAP Phase 93) - opt-in for the one real
    // disclosed risk: a concrete subclass generated only at runtime by
    // framework magic (a Doctrine proxy, a mocking framework) is
    // invisible to this project's static registry.
    pub check_dead_abstract_classes: bool,
    // A protected/public method/property/const never referenced
    // anywhere in the project (ROADMAP Phase 86's B1) - opt-in for
    // real, disclosed reasons real PHPStan's own dead-member rule
    // doesn't cover this visibility at all (it gates on `isPrivate()`
    // specifically, since verifying no *other file* reaches a broader-
    // visibility member needs cross-process collector machinery this
    // project's own closed-world registry doesn't need): a member
    // reachable only through framework magic/reflection (a template
    // calling a method by name, a DI container definition, PHPUnit
    // attribute discovery); a member reachable only through a
    // genuinely dynamic access (`$obj->{$expr}`/`$obj->$name`) whose
    // receiver's own type this project can't resolve at all (a real,
    // narrower risk than the framework-magic one - see `project::
    // whole_program::dead_members`'s own module doc comment for the
    // escape-hatch mechanism that closes the *resolvable*-receiver case).
    pub check_dead_members: bool,
    // Phase 82's `MethodCallWithPossiblyRenamedNamedArgumentRule`
    // (`argument.parameterRenamedInSubtype`) - a named-argument call
    // site against `Base::m(int $old)` that would throw at runtime the
    // moment the actual receiver is a `Sub extends Base` renaming `$old`
    // to `$new` on override. Opt-in, matching every other `project::
    // whole_program` check's own cost/architecture tradeoff (see
    // `check_dead_code`'s own doc comment): unlike those, this one needs
    // a real, type-inferring `Checker` pass over every loaded file (not
    // a cheap syntactic scan or a pure `&Registry` read) purely to
    // harvest named-argument call sites, real added cost for a rare
    // (renamed-parameter-on-override) bug shape.
    pub check_renamed_named_arguments: bool,
    // The project's own `level` setting, *before* `check-project`'s own
    // `MAX_LEVEL`-driven `.unwrap_or(6)` collapse - `None` when never
    // explicitly configured, which `accepts_at_configured_level` treats
    // as "keep today's always-strict answer," not real PHPStan's lenient
    // level-0 default. Set once from `check-project`'s already-resolved
    // `level`/`config.level` merge, not a flag of its own.
    pub configured_level: Option<u8>,
    // Mirrors real PHPStan's own standalone `checkBenevolentUnionTypes`
    // parameter (`conf/config.neon`, `checkBenevolentUnionTypes: false`,
    // not tied to any `-l`/`level` ladder rung at all, unlike
    // `checkExplicitMixed`/`checkImplicitMixed`) - real PHPStan's own
    // default (`false`) is already unconditionally lenient toward a
    // `BenevolentUnionType` accepted value, exactly this project's own
    // existing, always-on `AccessoryKind::Benevolent` leniency in
    // `is_assignable` (not an "opposite gap" from real PHPStan's default
    // posture, contrary to an earlier draft of this ROADMAP's own
    // claim). This flag opts *into* real PHPStan's stricter, non-default
    // behavior: when `true`, `accepts_at_configured_level_rec` strips a
    // Benevolent-marked `sub`'s own leniency and requires *every* member
    // of its wrapped `Union` to satisfy `sup`, mirroring `RuleLevelHelper::
    // transformAcceptedType`'s own `checkBenevolentUnionTypes` branch
    // exactly - unconditional, independent of `configured_level`, matching
    // how real PHPStan's own flag isn't level-gated either.
    pub check_benevolent_union_types: bool,
    // Mirrors real PHPStan's own `checkMissingCallableSignature`
    // parameter (`false` by default in `conf/config.neon`, not tied to
    // any level): `missingType.callable` for a `callable`/`Closure`
    // written with no `(params): return` signature, at every position
    // the `missingType.iterableValue` family already checks.
    pub check_missing_callable_signature: bool,
    // Whether this project consumes real PHPStan as a dependency at all
    // (set once from `composer::detect_phpstan_dependency` before any
    // file is checked). Gates the main `MethodCall` arm's own
    // `phpstanApi.method`/`phpParser.nodeConnectingAttribute` checks
    // (`checker/expr.rs`, `checker/phpstan_api.rs`); every other
    // PHPStan-API-usage check (instantiation, static calls, class-const
    // fetches, `instanceof`, `extends`/`implements`/trait-use) runs
    // unconditionally.
    pub phpstan_api_check_enabled: bool,
    // The one narrow crack in this struct's "path-agnostic" contract
    // (see `project/decl_locations.rs` for why `ClassInfo`/`Registry`
    // are otherwise path-agnostic) - needed to port real
    // `ApiRuleHelper::isPhpStanCode`'s vendor-directory check
    // (`is_phpstan_code`, `checker/phpstan_api.rs`). Only populated for a
    // name already passing `is_phpstan_namespace`, so it stays empty for
    // any corpus with no `PHPStan\*`-namespaced code.
    pub phpstan_class_files: FxHashMap<String, std::path::PathBuf>,
    // A project's own configured `restricted_symbols` list, compiled
    // once via `RestrictedSymbols::compile` (`Project::
    // set_restricted_symbols`).
    pub restricted_symbols: crate::restricted_symbols::RestrictedSymbols,
    // `dynamicConstantNames` - see `registry::dynamic_constants`. Keyed by
    // a global constant's name, or `lowercased class::NAME` for a class
    // constant; `None` reads the declared value's general type.
    pub dynamic_constant_names: FxHashMap<String, Option<Type>>,
    // A project's own `[[doctrine_custom_functions]]` config (ROADMAP
    // Phase 168 168h) - DQL function name (lowercased, matching DQL's own
    // case-insensitive function-name lookup) -> declared category.
    pub doctrine_custom_functions:
        FxHashMap<String, crate::doctrine_functions::DoctrineFunctionKind>,
    // A project's own configured `[sql_reflection]` block (`Project::
    // set_sql_reflection_config`) - `None` when unconfigured, which every
    // raw-SQL call site must treat as "keep today's fallback."
    pub sql_reflection_config: Option<crate::sql_reflection_config::SqlReflectionConfig>,
    // A project's own configured `schema_file` (sub-phase A2), parsed
    // once at startup (`Project::set_sql_reflection_schema`) - `None`
    // when no `schema_file` is configured.
    pub sql_reflection_schema: Option<crate::sql_reflection_schema::SchemaCatalog>,
    // The live SQL reflector (sub-phase C), lazily constructed on first
    // real use from `sql_reflection_config` - see
    // `sql_reflection_reflector::SqlReflectorHandle`'s own doc comment.
    pub sql_reflector: crate::sql_reflection_reflector::SqlReflectorHandle,
    // Resolution-order step 2 (`checker::sql_reflection::resolve`):
    // caches a resolved-or-undecidable answer by the literal SQL text
    // alone, not also a schema/config-identity hash - this project
    // supports exactly one schema source and one connection for the
    // whole analysis run (a deliberate design constraint, not an
    // oversight), so that identity is invariant for the run's own
    // lifetime and needs no separate cache-key component. Shared,
    // in-memory, single-run only (never persisted to disk) - avoids a
    // repeated live `PREPARE` round-trip for the same query text
    // appearing at multiple call sites, `rayon::par_iter`-safe via the
    // `Mutex` (mirrors the real, confirmed concurrency hazard
    // `sql_reflector` above already accounts for).
    pub sql_reflection_cache: std::sync::Mutex<FxHashMap<String, Option<Type>>>,
    // Real DB table name -> the one mapped `DoctrineEntity`'s own class
    // name (`checker::sql_reflection::resolve`'s own entity cross-
    // reference, gap #9 of the live SQL reflection phase) - lazily
    // built once from `classes` on first real use, mirroring `sql_
    // reflector`'s own `OnceLock` shape, since it's a whole-program
    // computation (every class must already be indexed) rather than
    // something computable per-class as `classes` itself fills in.
    // Two entities mapped to the same real table name is a genuine,
    // if unusual, Doctrine possibility (e.g. a `readOnly` reporting
    // entity mapped onto another entity's own table) - the first
    // one found wins rather than picking arbitrarily on every lookup,
    // since this project has no real corpus case to verify a "which
    // one wins" rule against.
    doctrine_entity_table_index: std::sync::OnceLock<FxHashMap<Arc<str>, Arc<str>>>,
    // The project's configured target PHP version, if any - `None`
    // means "not configured," which every version-gated rule should
    // treat as "nothing to check yet." See `crate::php_version::PhpFeature`
    // for the feature-availability lookup table this unlocks.
    pub php_version: Option<crate::php_version::PhpVersion>,
    // Class names with at least one *unconditional* declaration indexed
    // so far - lets `insert_built_decl` tell "this name's entry is
    // itself unconditional, so a second one should merge onto it"
    // (Piwigo's own `phpstan-stubs/Imagick.stub.php` shape) apart from
    // "this name's entry is only a conditional stub, so the first real
    // unconditional declaration should replace it outright" - both look
    // identical from `self.classes.contains_key(&name)` alone.
    unconditionally_declared_classes: FxHashSet<Arc<str>>,
    // Names `merge_stub_classes` provided a built-in native-class stub
    // for (jetbrains/phpstorm-stubs) - a *subset* of
    // `unconditionally_declared_classes` above, distinguishing "a
    // type-checking fiction, never really instantiated" from "a real,
    // unconditionally-declared vendor/first-party class." Merging a
    // version-gated polyfill's members onto a built-in stub is safe
    // (the stub is fictional either way); merging onto a *real* vendor
    // class (`PHPUnit\Framework\TestCase`-vs-`class_exists`-guarded-
    // fallback) would wrongly leak a defensive fallback's members onto
    // the real class everything else sees.
    pub(crate) built_in_stub_classes: FxHashSet<Arc<str>>,
    // The function-side sibling of the tracking set above -
    // phpstorm-stubs represents a real PHP internal function overload
    // (different signatures by argument shape, not version) as two
    // separate top-level `function` declarations sharing one name
    // (`strtr`: a 3-string-argument form and a 2-argument,
    // `array $replace_pairs` form). Needed so the second declaration
    // merges into a callable accepting either shape, instead of a blind
    // `self.functions.insert` discarding the first's params.
    unconditionally_declared_functions: FxHashSet<Arc<str>>,
    // The declaration (its name's `(line, col)`) a `functions` slot was
    // built from - `None` once two declarations merged into it. Lets the
    // checker tell a second same-named declaration from the one the slot
    // describes.
    pub(crate) function_decl_sites: FxHashMap<Arc<str>, Option<(u32, u32)>>,
    // Names `merge_stubs` provided a signature for, not yet
    // overwritten by a real, already-indexed conditional declaration -
    // the same ambiguity `unconditionally_declared_functions` resolves
    // on the unconditional side. Needed so the common
    // `if (!function_exists('str_contains')) { function str_contains
    // (?string $haystack, ...) {...} }` polyfill idiom (real shape:
    // symfony/polyfill-php80) doesn't lose its real, more-precise
    // (here, nullable) parameter types to `entry(...).or_insert_with(...)`
    // being a no-op once the built-in stub already occupies the slot.
    stub_provided_functions: FxHashSet<Arc<str>>,
    // Every bare function/method name with a non-empty `assert` list
    // on some indexed `FunctionSig`. `@phpstan-assert` is rare in real
    // corpora (zero occurrences across the full Symfony monorepo plus
    // every vendored PHP stub), so `checker::stmt`'s bare-statement
    // narrowing pass checks this set-membership first and skips the
    // expensive receiver-type inference + hierarchy walk for a name that
    // provably can't match. Names are only ever added, never removed on
    // `remove_declarations` - a harmless, rare false positive (still a
    // "may still do the real check" set, not a "known good" one), never
    // a false negative.
    pub(crate) names_with_assert_tags: FxHashSet<Arc<str>>,
    // Every class's precomputed `extends`+`implements` transitive
    // closure - see `is_subclass_of`/`precompute_ancestor_names`
    // (`registry/hierarchy.rs`). Rebuilt whenever the class graph could
    // have changed.
    pub(crate) ancestor_names: FxHashMap<Arc<str>, FxHashSet<Arc<str>>>,
    // Every registered class's lowercased name -> canonical key - see
    // `lookup_class`. A byproduct of `canonicalize_class_name_case`'s
    // already-built map, persisted here instead of discarded.
    pub(crate) classes_lowercase: FxHashMap<String, Arc<str>>,
    // Every registered class's precomputed member-name ->
    // owning-ancestor-name map, one per member kind - see
    // `lookup_method`/`precompute_member_owners` (`registry/hierarchy.rs`).
    pub(crate) method_owner_by_class: FxHashMap<Arc<str>, FxHashMap<Arc<str>, Arc<str>>>,
    pub(crate) ancestor_method_owner_by_class: FxHashMap<Arc<str>, FxHashMap<Arc<str>, Arc<str>>>,
    pub(crate) property_owner_by_class: FxHashMap<Arc<str>, FxHashMap<Arc<str>, Arc<str>>>,
    pub(crate) ancestor_property_owner_by_class: FxHashMap<Arc<str>, FxHashMap<Arc<str>, Arc<str>>>,
    pub(crate) const_owner_by_class: FxHashMap<Arc<str>, FxHashMap<Arc<str>, Arc<str>>>,
}

impl Registry {
    pub fn build(program: &[Stmt]) -> Registry {
        let mut registry = Registry::index_and_flatten(program);
        registry.resolve_transitive_purity();
        registry
    }

    /// Every step `build` runs except the final purity classification.
    /// `php_stubs.rs` calls this directly instead of `build` to extract
    /// raw `FunctionSig`/`ClassInfo` values from vendored stub source - a
    /// stub's function body is always empty, so classifying it here
    /// would wrongly mark every PHP built-in (`header()`, `exit()`, ...)
    /// as provably pure, baked permanently into the `LazyLock`-cached
    /// stub signatures every real project's registry later merges in.
    pub(crate) fn index_and_flatten(program: &[Stmt]) -> Registry {
        let mut registry = Registry::default();
        registry.index(program, false);
        registry.canonicalize_class_name_case();
        registry.flatten_traits();
        registry.resolve_method_tags();
        // `flatten_phpdoc_inheritance` consumes `lookup_ancestor_method_
        // with_owner`/`lookup_method_with_owner` internally, so the
        // member-owner maps must already reflect every name
        // `flatten_traits`/`resolve_method_tags` just added.
        registry.precompute_member_owners();
        // Aliases and deferred value types resolve first, so inherited
        // PHPDoc copies an ancestor's finished types.
        registry.resolve_deferred_type_imports();
        registry.resolve_deferred_value_of();
        registry.flatten_phpdoc_inheritance();
        registry.apply_assertion_table();
        // Adds more method/property names (merges mixin members in) -
        // the member-owner maps need a second rebuild after this.
        registry.resolve_deferred_mixins();
        registry.precompute_ancestor_names();
        registry.resolve_constant_named_types();
        registry.drop_doc_members_excluded_by_native();
        registry.precompute_member_owners();
        registry.infer_global_variables(&[(std::path::Path::new("<flattened>"), program)]);
        registry
    }

    /// The one `DoctrineEntity`-mapped class whose own `#[ORM\Table(name:
    /// ...)]` matches `table` exactly, if any - see `doctrine_entity_
    /// table_index`'s own doc comment for why this is lazily built
    /// rather than maintained incrementally.
    pub fn doctrine_entity_for_table(
        &self,
        table: &str,
    ) -> Option<&crate::registry::class_info::DoctrineEntity> {
        let index = self.doctrine_entity_table_index.get_or_init(|| {
            let mut index = FxHashMap::default();
            for (class_name, info) in &self.classes {
                if let Some(entity) = &info.doctrine_entity
                    && let Some(table_name) = &entity.table_name
                {
                    index
                        .entry(table_name.clone())
                        .or_insert_with(|| class_name.clone());
                }
            }
            index
        });
        let class_name = index.get(table)?;
        self.classes.get(class_name)?.doctrine_entity.as_ref()
    }
}

#[cfg(test)]
mod tests;
