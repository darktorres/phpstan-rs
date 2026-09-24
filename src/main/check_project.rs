//! `check-project`: command-line flags overlaid onto the config file,
//! run through `project::session::Session`, then baselined, fixed and
//! printed.

use std::path::{Path, PathBuf};

use output::OutputFormat;
use phpstan_rs::project::session::{Overrides, Session};
use phpstan_rs::{baseline, config, output};

/// Load every .php file under the given paths and run the full checker
/// over all of them, reporting every diagnostic.
#[derive(clap::Args)]
pub(crate) struct CheckProjectArgs {
    // Not `required = true`: a TOML config file's own `paths` can
    // supply these instead (see `run_check_project`'s resolution
    // logic) - the "must know what to check" requirement is
    // enforced there, once the config (if any) has been consulted,
    // not statically by clap.
    pub(crate) dirs: Vec<PathBuf>,
    /// Report up to and including this level (see
    /// checker::Diagnostic's own doc comment) - unset means
    /// "report everything".
    #[arg(long)]
    pub(crate) level: Option<u8>,
    #[arg(long)]
    pub(crate) strict_mixed_arrays: bool,
    /// Enables `property.uninitialized` (a typed property with no
    /// default nothing in the class body ever assigns). Off by
    /// default, mirroring real PHPStan's own opt-in
    /// `checkUninitializedProperties` parameter - see
    /// `registry::Registry::check_uninitialized_properties`'s own
    /// doc comment for why.
    #[arg(long)]
    pub(crate) check_uninitialized_properties: bool,
    /// Enables the `@internal` restricted-usage engine (`internal.*`
    /// diagnostics) - restricts a symbol tagged `@internal` (or
    /// declared inside an `@internal`-tagged class) to callers
    /// sharing its own top-level namespace. Off by default,
    /// mirroring real PHPStan's own opt-in `featureToggles.
    /// internalTag` parameter - see `registry::Registry::
    /// check_internal_usage`'s own doc comment for why.
    #[arg(long)]
    pub(crate) check_internal_usage: bool,
    /// Enables `use function`'s own `function.nameCase` check (a
    /// case-mismatched `use function` import, e.g. `use function
    /// STRLEN;`). Off by default, mirroring real PHPStan's own
    /// opt-in `checkFunctionNameCase` parameter - see `registry::
    /// Registry::check_function_name_case`'s own doc comment for
    /// why.
    #[arg(long)]
    pub(crate) check_function_name_case: bool,
    /// Enables `@deprecated` usage warnings (`function.deprecated`/
    /// `method.deprecated`/`staticMethod.deprecated`/
    /// `property.deprecated`/`staticProperty.deprecated`/
    /// `classConstant.deprecated`/`new.deprecated` diagnostics) -
    /// flags a call/access/instantiation whose own doc comment (or,
    /// for instantiation, the class's own doc comment) carries
    /// `@deprecated`. Off by default - real PHPStan's own equivalent
    /// isn't part of core `phpstan/phpstan` at all, but a separate,
    /// opt-in Composer package (`phpstan/phpstan-deprecation-rules`),
    /// see `registry::Registry::check_deprecated_usage`'s own doc
    /// comment for why.
    #[arg(long)]
    pub(crate) check_deprecated_usage: bool,
    /// Enables the reinstated empty-concrete-method-body completeness
    /// check (`return.missing`/`return.never` now also firing on a
    /// literal `{}` body, not just a real fall-through). Off by
    /// default - see `registry::Registry::check_empty_method_bodies`'s
    /// own doc comment for the confirmed corpus false-positive class
    /// (IDE-stub/native-extension declaration files) this avoids.
    #[arg(long)]
    pub(crate) check_empty_method_bodies: bool,
    /// Enables the opt-in "missing checked exception" family
    /// (`throws.missingType` on a real, explicit `throw` whose own
    /// type isn't covered by any `@throws` tag). Off by default - the
    /// riskiest of this project's opt-in checks given the real
    /// corpus's documented inconsistent `@throws` tagging, see
    /// `registry::Registry::check_missing_checked_exceptions`'s own
    /// doc comment for why.
    #[arg(long)]
    pub(crate) check_missing_checked_exceptions: bool,
    /// Enables the strict (vs. the default, castable) printf
    /// placeholder-type matching mode for `sprintf`/`printf`/
    /// `vsprintf`/`vprintf`/`fprintf` argument-type checks. Off by
    /// default - matches real PHPStan's own stock config exactly, see
    /// `registry::Registry::check_strict_printf_placeholder_types`'s
    /// own doc comment for why.
    #[arg(long)]
    pub(crate) check_strict_printf_placeholder_types: bool,
    /// Enables `TooWideMethodThrowTypeRule`'s wider, re-declaration-
    /// only mode for a protected/public method that isn't final (and
    /// isn't on a final class). Off by default - matches real
    /// PHPStan's own stock config exactly, see `registry::Registry::
    /// check_too_wide_throw_type_in_protected_and_public_methods`'s
    /// own doc comment for why.
    #[arg(long)]
    pub(crate) check_too_wide_throw_type_in_protected_and_public_methods: bool,
    /// Enables `TooWideMethodReturnTypehintRule`'s wider, re-
    /// declaration-only mode for a protected/public method that
    /// isn't final (and isn't on a final class) - the return-type
    /// analogue of the flag above. Off by default, same reason - see
    /// `registry::Registry::check_too_wide_return_type_in_protected_and_public_methods`'s
    /// own doc comment.
    #[arg(long)]
    pub(crate) check_too_wide_return_type_in_protected_and_public_methods: bool,
    /// The by-ref-parameter analogue of the flag above
    /// (`TooWideMethodParameterOutTypeRule`) - see `registry::Registry::
    /// check_too_wide_by_ref_type_in_protected_and_public_methods`'s
    /// own doc comment.
    #[arg(long)]
    pub(crate) check_too_wide_by_ref_type_in_protected_and_public_methods: bool,
    /// Enables `MethodThrowTypeCovarianceRule` (a method's own
    /// `@throws` type must be covariant with its immediate parent
    /// class/every transitive interface/abstract trait declaration's
    /// own `@throws`). Off by default - see `registry::Registry::
    /// check_method_throw_type_covariance`'s own doc comment for why.
    #[arg(long)]
    pub(crate) check_method_throw_type_covariance: bool,
    /// Mirrors real PHPStan's own standalone `checkBenevolentUnionTypes`
    /// parameter, opting into its stricter (non-default) behavior -
    /// see `registry::Registry::check_benevolent_union_types`'s own
    /// doc comment for why real PHPStan's own default is already
    /// unconditionally lenient, matching this checker's status quo.
    #[arg(long)]
    pub(crate) check_benevolent_union_types: bool,
    /// Mirrors real PHPStan's own `checkMissingCallableSignature`
    /// parameter (`missingType.callable`) - see
    /// `registry::Registry::check_missing_callable_signature`.
    #[arg(long)]
    pub(crate) check_missing_callable_signature: bool,
    /// Enables opt-in whole-project duplicate top-level-function-
    /// declaration detection (`function.duplicate`). Off by default:
    /// real PHPStan's own `DuplicateFunctionDeclarationRule` is
    /// `#[ValidatesStubFiles]` and never runs against ordinary
    /// scanned files at all, so this checker has no general way to
    /// rule out two same-named functions in separate files that are
    /// never actually loaded together in the same PHP process,
    /// hence off by default - see `registry::Registry::check_duplicate_function_
    /// declarations`'s own doc comment.
    #[arg(long)]
    pub(crate) check_duplicate_function_declarations: bool,
    /// Enables opt-in whole-project duplicate class/interface/trait/
    /// enum-declaration detection (`class.duplicate`/etc.) - going
    /// beyond real PHPStan, which has no whole-project equivalent of
    /// this check at all (see `checker::duplicate_classes`'s own
    /// module doc comment). Off by default: its one known corpus
    /// false-positive shape (a vendored built-in stub override
    /// colliding with the real extension's own class) needs
    /// `--stub-path`/`stub_paths` correctly populated first - see
    /// `registry::Registry::check_duplicate_class_declarations`'s own
    /// doc comment.
    #[arg(long)]
    pub(crate) check_duplicate_class_declarations: bool,
    /// Enables "boolean trap" detection - a bare `true`/`false`
    /// literal passed positionally into a bare-`bool`-typed
    /// parameter. Off by default: unambiguous single-bool-parameter
    /// calls are common and would otherwise be pure noise - see
    /// `registry::Registry::check_boolean_trap`'s own doc comment.
    #[arg(long)]
    pub(crate) check_boolean_trap: bool,
    /// Enables array-key cast warnings (a literal float/bool/null/
    /// numeric-string array key silently cast at runtime). Off by
    /// default - real PHPStan itself only ever registers this in its
    /// own private issue-bot config, never an installable package -
    /// see `registry::Registry::check_array_key_cast`'s own doc
    /// comment.
    #[arg(long)]
    pub(crate) check_array_key_cast: bool,
    /// Enables `RequireParentConstructCallRule` (a concrete class's
    /// own `__construct()` never calling `parent::__construct()`
    /// when a real, callable parent constructor exists). Off by
    /// default - real PHPStan ships this only via the separate,
    /// opt-in `phpstan/phpstan-strict-rules` package - see
    /// `registry::Registry::check_parent_constructor_call`'s own
    /// doc comment.
    #[arg(long)]
    pub(crate) check_parent_constructor_call: bool,
    /// Enables constructor-property-promotion suggestions for a
    /// trivially-forwarding constructor. Off by default - a purely
    /// stylistic suggestion, not a bug - see `registry::Registry::
    /// check_promoted_property_suggestions`'s own doc comment.
    #[arg(long)]
    pub(crate) check_promoted_property_suggestions: bool,
    /// Enables the "only booleans are allowed in a condition" family
    /// (`if`/`elseif`/`while`/`do-while`/ternary conditions, `&&`/
    /// `||` operands, negated booleans). Off by default - real
    /// PHPStan ships this only via the separate, opt-in
    /// `phpstan/phpstan-strict-rules` package - see `registry::
    /// Registry::check_boolean_conditions`'s own doc comment.
    #[arg(long)]
    pub(crate) check_boolean_conditions: bool,
    /// Enables strict `checkDynamicProperties` behavior inside
    /// `isset()`/`empty()`/`??`/`??=` (still flags a property that
    /// doesn't exist there, instead of real PHPStan's lenient
    /// default of silently allowing the probe). Off by default,
    /// mirroring real PHPStan's own opt-in `checkDynamicProperties`
    /// parameter - see `registry::Registry::
    /// check_dynamic_properties`'s own doc comment.
    #[arg(long)]
    pub(crate) check_dynamic_properties: bool,
    /// Enables swapped-adjacent-argument-name detection (`resize
    /// ($height, $width)` where the declared params are `(int
    /// $width, int $height)`). Off by default - a real, disclosed
    /// heuristic (two same-typed parameters are structurally
    /// invisible to any type-system-based check as "different
    /// roles"), not a soundness gap - see `registry::Registry::
    /// check_swapped_arguments`'s own doc comment.
    #[arg(long)]
    pub(crate) check_swapped_arguments: bool,
    /// Enables `never`-return-type suggestions for a function/method
    /// that provably never returns normally but is declared `void`
    /// (or left undeclared). Off by default - real PHPStan already
    /// computes this exact fact, but never ships an installable
    /// rule consuming it - see `registry::Registry::
    /// check_never_return_suggestions`'s own doc comment.
    #[arg(long)]
    pub(crate) check_never_return_suggestions: bool,
    /// Enables missing-`default:`-arm detection for `switch`
    /// statements. Off by default - see `registry::Registry::
    /// check_switch_missing_default`'s own doc comment.
    #[arg(long)]
    pub(crate) check_switch_missing_default: bool,
    /// Enables missing-exception-message detection (`throw new
    /// Exception()` with zero constructor arguments). Off by
    /// default - see `registry::Registry::check_throw_without_
    /// message`'s own doc comment.
    #[arg(long)]
    pub(crate) check_throw_without_message: bool,
    /// Enables base-Exception/Error-class detection (`throw new
    /// Exception(...)`/`throw new Error(...)` instead of a specific
    /// subclass). Off by default - see `registry::Registry::
    /// check_throw_base_class`'s own doc comment.
    #[arg(long)]
    pub(crate) check_throw_base_class: bool,
    /// Requires a `catch` to pass the caught exception as the previous
    /// exception of one it throws, and a catch of a base exception type
    /// to rethrow.
    #[arg(long)]
    pub(crate) check_exception_hygiene: bool,
    /// Flag implicit dependence on the current time instead of an injected clock.
    #[arg(long)]
    pub(crate) check_current_time_dependence: bool,
    /// Flag PSR-3 logger calls that leave out an in-scope Throwable as the exception context key.
    #[arg(long)]
    check_psr3_exception_context: bool,
    /// Check PSR-3 logger calls: placeholders against context keys, static messages, levels and context shape.
    #[arg(long)]
    check_psr3_logging: bool,
    /// Flag magic numbers: numeric literals outside constant declarations.
    #[arg(long)]
    pub(crate) check_magic_numbers: bool,
    /// Suggest native parameter and return types for PHPDoc-only types.
    #[arg(long)]
    pub(crate) check_native_type_hints: bool,
    /// Flag offset assignments that implicitly create an array variable.
    #[arg(long)]
    pub(crate) check_implicit_array_creation: bool,
    /// Flag casts to a type the expression already has.
    #[arg(long)]
    pub(crate) check_useless_casts: bool,
    /// Flag array_filter() calls relying on loose truthiness.
    #[arg(long)]
    pub(crate) check_strict_array_filter: bool,
    /// Flag operators combining a string with a non-string, or an array with a non-array.
    #[arg(long)]
    pub(crate) check_mixed_type_operands: bool,
    /// Flag ordering comparisons between non-comparable or mismatched types.
    #[arg(long)]
    pub(crate) check_comparable_types: bool,
    /// Flag non-numeric operands of arithmetic, unary +/- and ++/-- operators.
    #[arg(long)]
    pub(crate) check_arithmetic_operands: bool,
    /// Flag === and !== between objects whose class defines a matching equals() method.
    #[arg(long)]
    pub(crate) check_identity_vs_equals: bool,
    /// Flag loose in_array()/array_search()/array_keys() calls whose needle and haystack types compare differently under == and ===.
    #[arg(long)]
    pub(crate) check_loose_haystack_comparison: bool,
    /// Flag in_array()/array_search()/array_keys()/base64_decode() calls without strict mode, and iterator_to_array() without preserve_keys.
    #[arg(long)]
    pub(crate) check_strict_function_calls: bool,
    /// Flag switch cases whose type can never match the switch subject's.
    #[arg(long)]
    pub(crate) check_switch_case_types: bool,
    /// Flag foreach/for loop variables that overwrite an already-defined variable.
    #[arg(long)]
    pub(crate) check_loop_variable_overwrite: bool,
    /// Flag static methods called through an instance with ->.
    #[arg(long)]
    pub(crate) check_dynamic_static_calls: bool,
    /// Flag calls to __construct() other than a parent constructor call from a constructor.
    #[arg(long)]
    pub(crate) check_illegal_constructor_calls: bool,
    /// Reports a default arm in a match over an enum, and switch or if-chains over enum cases where a match would be exhaustive.
    #[arg(long)]
    pub(crate) check_enum_exhaustiveness: bool,
    /// Reports isset()/empty() on objects and possibly-null values in string interpolation, concatenation, arithmetic and compound assignments.
    #[arg(long)]
    pub(crate) check_null_coercion: bool,
    /// Enables discarded-`new`-exception detection (`new
    /// SomeException(...);` sitting on its own line, never thrown).
    /// Off by default - see `registry::Registry::check_new_exception_
    /// discarded`'s own doc comment.
    #[arg(long)]
    pub(crate) check_new_exception_discarded: bool,
    /// Enables assertion-less-test ("risky test") detection (a
    /// PHPUnit test method/Pest test closure with zero assertion-
    /// shaped calls anywhere in its body). Off by default - a test
    /// that only ever calls a custom, differently-named assertion-
    /// helper method is a real, disclosed false positive - see
    /// `registry::Registry::check_risky_tests`'s own doc comment.
    #[arg(long)]
    pub(crate) check_risky_tests: bool,
    /// Enables dead-enum-case detection (a declared case never
    /// referenced via `EnumName::CaseName`, a `match`/`switch` arm
    /// value, or a literal `::from()`/`::tryFrom()` argument,
    /// project-wide). Off by default - see
    /// `registry::Registry::check_dead_enum_cases`'s own doc comment
    /// for the real, disclosed false-positive risks.
    #[arg(long)]
    pub(crate) check_dead_enum_cases: bool,
    /// Enables dead-function/dead-class detection (a top-level
    /// function or class/interface/trait/enum never referenced
    /// anywhere in the project). Off by default - see
    /// `registry::Registry::check_dead_code`'s own doc comment for
    /// the real, disclosed false-positive risks.
    #[arg(long)]
    pub(crate) check_dead_code: bool,
    /// Enables namespace-level circular-dependency detection (a
    /// namespace with a class that `extends`/`implements`/`use`s a
    /// class in another namespace that, via some other class pair,
    /// has an edge back). Off by default - see `registry::Registry::
    /// check_circular_dependencies`'s own doc comment.
    #[arg(long)]
    pub(crate) check_circular_dependencies: bool,
    /// Enables dead-abstract-class detection (an `abstract class`
    /// with zero concrete descendants anywhere in the project). Off
    /// by default - see `registry::Registry::
    /// check_dead_abstract_classes`'s own doc comment.
    #[arg(long)]
    pub(crate) check_dead_abstract_classes: bool,
    /// Enables dead-protected/public-member detection (a method/
    /// property/const never referenced anywhere in the project). Off
    /// by default - see `registry::Registry::check_dead_members`'s
    /// own doc comment for the real, disclosed false-positive risks.
    #[arg(long)]
    pub(crate) check_dead_members: bool,
    /// Reports inline `@phpstan-ignore*` comments that suppress
    /// nothing. Off by default - an ignore naming a rule this project
    /// doesn't implement would always be unmatched.
    #[arg(long)]
    pub(crate) report_unmatched_ignored_errors: bool,
    /// Checks the analysed code against the root composer.json:
    /// undeclared, misplaced and unused dependencies, PSR-4 locations,
    /// required extensions, `provide`, `scripts` and
    /// `config.platform.php`. Off by default.
    #[arg(long)]
    pub(crate) check_composer_hygiene: bool,
    /// Reports classes that aren't final, protected members of final
    /// classes, and overridable concrete methods of abstract classes,
    /// where nothing in the project extends or overrides them.
    #[arg(long)]
    pub(crate) check_closed_classes: bool,
    /// Reports PHPUnit mock-builder misuse: ambiguous `expects($this->any())`,
    /// unverified `with()` on stubs, conflicting return configurations,
    /// mocks configured without expectations, and mock-typed unions.
    #[arg(long)]
    pub(crate) check_mock_hygiene: bool,
    /// Enables named-argument-rename-collision detection (a named-
    /// argument call site against a method an override elsewhere
    /// renamed a parameter on). Off by default - see `registry::
    /// Registry::check_renamed_named_arguments`'s own doc comment
    /// for the real added cost.
    #[arg(long)]
    pub(crate) check_renamed_named_arguments: bool,
    /// Target PHP version, as "major.minor" (e.g. "8.1") - gates the
    /// portability rules that check "X is only supported on PHP Y.Z
    /// and later" (see `php_version::PhpFeature`). Unset means no
    /// such rule has anything to check against yet.
    #[arg(long)]
    pub(crate) php_version: Option<String>,
    #[arg(long)]
    pub(crate) generate_baseline: Option<PathBuf>,
    #[arg(long)]
    pub(crate) baseline: Option<PathBuf>,
    /// Matches a baseline entry by its identifier when its message
    /// text no longer matches, so rewording doesn't resurface it.
    #[arg(long)]
    pub(crate) baseline_match_identifier: bool,
    /// Writes `--generate-baseline` as one file per identifier in a
    /// directory named after the baseline file, which becomes an
    /// index including them.
    #[arg(long)]
    pub(crate) baseline_per_identifier: bool,
    /// Applies every suggested fix (the same ones the language server
    /// offers as quick fixes) to the files on disk.
    #[arg(long)]
    pub(crate) fix: bool,
    /// Writes every tracked variable's type after each statement of
    /// every analysed file to this path (a regression-gate snapshot).
    #[arg(long, hide = true)]
    pub(crate) dump_types: Option<PathBuf>,
    /// Prints each file's diagnostics in the order the checks emitted them
    /// (a determinism check), not the usual total order.
    #[arg(long, hide = true)]
    pub(crate) emission_order: bool,
    /// Extra directories loaded for type-resolution context only -
    /// never analyzed or reported on. Repeatable.
    #[arg(long = "scan-dir")]
    pub(crate) scan_dirs: Vec<PathBuf>,
    /// A directory (walked) or single file whose class-like
    /// declarations are designated overrides, never a genuine
    /// accidental duplicate (e.g. a vendored `Imagick.stub.php`) -
    /// see `check_duplicate_class_declarations` above. Repeatable.
    /// Also loaded for type-resolution context, exactly like
    /// `--scan-dir` (unioned into it internally).
    #[arg(long = "stub-path")]
    pub(crate) stub_paths: Vec<PathBuf>,
    /// Disables the automatic Composer-autoloader-based `vendor/`
    /// context load (see `composer::discover_vendor_context`).
    /// `--scan-dir` still works.
    #[arg(long)]
    pub(crate) no_composer: bool,
    /// TOML config file (see src/config.rs) supplying defaults for
    /// paths/level/baseline/strict_mixed_arrays/
    /// check_uninitialized_properties/scan_paths/no_composer/
    /// php_version - any flag above, when given, overrides the same
    /// key in the config.
    #[arg(long)]
    pub(crate) config: Option<PathBuf>,
    /// Output format: `text` (default, human-readable), `json`, or
    /// `checkstyle` (XML) - the latter two for CI pipelines that want
    /// to consume diagnostics as structured data rather than parsing
    /// plain text. CLI-only, matching real PHPStan's own
    /// `--error-format`: unlike `level`/`baseline`/etc., this isn't a
    /// `config.rs` TOML key, since it's how one particular invocation
    /// wants its output shaped, not a standing project setting.
    #[arg(long)]
    pub(crate) format: Option<OutputFormat>,
}

/// A flag given on the command line turns its config key on; one left
/// off leaves the config's own value.
fn overlay_flags(args: &CheckProjectArgs, config: &mut config::Config) {
    if args.strict_mixed_arrays {
        config.strict_mixed_arrays = Some(true);
    }
    if args.check_uninitialized_properties {
        config.check_uninitialized_properties = Some(true);
    }
    if args.check_internal_usage {
        config.check_internal_usage = Some(true);
    }
    if args.check_function_name_case {
        config.check_function_name_case = Some(true);
    }
    if args.check_deprecated_usage {
        config.check_deprecated_usage = Some(true);
    }
    if args.check_empty_method_bodies {
        config.check_empty_method_bodies = Some(true);
    }
    if args.check_missing_checked_exceptions {
        config.check_missing_checked_exceptions = Some(true);
    }
    if args.check_strict_printf_placeholder_types {
        config.check_strict_printf_placeholder_types = Some(true);
    }
    if args.check_too_wide_throw_type_in_protected_and_public_methods {
        config.check_too_wide_throw_type_in_protected_and_public_methods = Some(true);
    }
    if args.check_too_wide_return_type_in_protected_and_public_methods {
        config.check_too_wide_return_type_in_protected_and_public_methods = Some(true);
    }
    if args.check_too_wide_by_ref_type_in_protected_and_public_methods {
        config.check_too_wide_by_ref_type_in_protected_and_public_methods = Some(true);
    }
    if args.check_method_throw_type_covariance {
        config.check_method_throw_type_covariance = Some(true);
    }
    if args.check_benevolent_union_types {
        config.check_benevolent_union_types = Some(true);
    }
    if args.check_missing_callable_signature {
        config.check_missing_callable_signature = Some(true);
    }
    if args.check_duplicate_function_declarations {
        config.check_duplicate_function_declarations = Some(true);
    }
    if args.check_duplicate_class_declarations {
        config.check_duplicate_class_declarations = Some(true);
    }
    if args.check_boolean_trap {
        config.check_boolean_trap = Some(true);
    }
    if args.check_array_key_cast {
        config.check_array_key_cast = Some(true);
    }
    if args.check_parent_constructor_call {
        config.check_parent_constructor_call = Some(true);
    }
    if args.check_promoted_property_suggestions {
        config.check_promoted_property_suggestions = Some(true);
    }
    if args.check_boolean_conditions {
        config.check_boolean_conditions = Some(true);
    }
    if args.check_dynamic_properties {
        config.check_dynamic_properties = Some(true);
    }
    if args.check_swapped_arguments {
        config.check_swapped_arguments = Some(true);
    }
    if args.check_never_return_suggestions {
        config.check_never_return_suggestions = Some(true);
    }
    if args.check_switch_missing_default {
        config.check_switch_missing_default = Some(true);
    }
    if args.check_throw_without_message {
        config.check_throw_without_message = Some(true);
    }
    if args.check_throw_base_class {
        config.check_throw_base_class = Some(true);
    }
    if args.check_exception_hygiene {
        config.check_exception_hygiene = Some(true);
    }
    if args.check_current_time_dependence {
        config.check_current_time_dependence = Some(true);
    }
    if args.check_magic_numbers {
        config.check_magic_numbers = Some(true);
    }
    if args.check_native_type_hints {
        config.check_native_type_hints = Some(true);
    }
    if args.check_implicit_array_creation {
        config.check_implicit_array_creation = Some(true);
    }
    if args.check_useless_casts {
        config.check_useless_casts = Some(true);
    }
    if args.check_strict_array_filter {
        config.check_strict_array_filter = Some(true);
    }
    if args.check_mixed_type_operands {
        config.check_mixed_type_operands = Some(true);
    }
    if args.check_comparable_types {
        config.check_comparable_types = Some(true);
    }
    if args.check_arithmetic_operands {
        config.check_arithmetic_operands = Some(true);
    }
    if args.check_identity_vs_equals {
        config.check_identity_vs_equals = Some(true);
    }
    if args.check_loose_haystack_comparison {
        config.check_loose_haystack_comparison = Some(true);
    }
    if args.check_strict_function_calls {
        config.check_strict_function_calls = Some(true);
    }
    if args.check_switch_case_types {
        config.check_switch_case_types = Some(true);
    }
    if args.check_loop_variable_overwrite {
        config.check_loop_variable_overwrite = Some(true);
    }
    if args.check_dynamic_static_calls {
        config.check_dynamic_static_calls = Some(true);
    }
    if args.check_illegal_constructor_calls {
        config.check_illegal_constructor_calls = Some(true);
    }
    if args.check_enum_exhaustiveness {
        config.check_enum_exhaustiveness = Some(true);
    }
    if args.check_null_coercion {
        config.check_null_coercion = Some(true);
    }
    if args.check_new_exception_discarded {
        config.check_new_exception_discarded = Some(true);
    }
    if args.check_risky_tests {
        config.check_risky_tests = Some(true);
    }
    if args.check_dead_enum_cases {
        config.check_dead_enum_cases = Some(true);
    }
    if args.check_dead_code {
        config.check_dead_code = Some(true);
    }
    if args.check_circular_dependencies {
        config.check_circular_dependencies = Some(true);
    }
    if args.check_dead_abstract_classes {
        config.check_dead_abstract_classes = Some(true);
    }
    if args.check_dead_members {
        config.check_dead_members = Some(true);
    }
    if args.report_unmatched_ignored_errors {
        config.report_unmatched_ignored_errors = Some(true);
    }
    if args.check_composer_hygiene {
        config.check_composer_hygiene = Some(true);
    }
    if args.check_closed_classes {
        config.check_closed_classes = Some(true);
    }
    if args.check_mock_hygiene {
        config.check_mock_hygiene = Some(true);
    }
    if args.check_renamed_named_arguments {
        config.check_renamed_named_arguments = Some(true);
    }
}

pub(crate) fn run_check_project_command(args: CheckProjectArgs) {
    let config_dir = args
        .config
        .as_deref()
        .and_then(Path::parent)
        .map(Path::to_path_buf);
    let mut config = match &args.config {
        Some(path) => match config::Config::load(path) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("{e}");
                std::process::exit(1);
            }
        },
        None => config::Config::default(),
    };
    overlay_flags(&args, &mut config);
    let resolve_against_config = |p: PathBuf| match &config_dir {
        Some(dir) => dir.join(p),
        None => p,
    };
    let baseline_path = args
        .baseline
        .clone()
        .or(config.baseline.clone().map(resolve_against_config));
    let baseline_mode =
        if args.baseline_match_identifier || config.baseline_match_identifier.unwrap_or(false) {
            baseline::MatchMode::Identifier
        } else {
            baseline::MatchMode::Message
        };
    let baseline_per_identifier =
        args.baseline_per_identifier || config.baseline_per_identifier.unwrap_or(false);
    let overrides = Overrides {
        paths: args.dirs.clone(),
        scan_paths: args.scan_dirs.clone(),
        stub_paths: args.stub_paths.clone(),
        level: args.level,
        report_level: None,
        php_version: args.php_version.clone(),
        no_composer: args.no_composer,
        emission_order: args.emission_order,
    };
    let session = match Session::resolve(config, config_dir, PathBuf::from("."), overrides) {
        Ok(session) => session,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };
    for warning in &session.warnings {
        eprintln!("{warning}");
    }
    // A file whose parse or check panics is reported per file; don't also
    // print each caught panic's default message.
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let analysis = session.run(args.dump_types.as_deref());
    std::panic::set_hook(default_hook);
    let analysis = match analysis {
        Ok(analysis) => analysis,
        Err(e) => {
            eprintln!("{e}");
            return;
        }
    };
    report(
        analysis,
        args.generate_baseline.as_deref(),
        baseline_path.as_deref(),
        baseline_mode,
        baseline_per_identifier,
        args.fix,
        args.format.unwrap_or_default(),
    );
}

fn report(
    analysis: phpstan_rs::project::session::Analysis,
    generate_baseline_path: Option<&Path>,
    baseline_path: Option<&Path>,
    baseline_mode: baseline::MatchMode,
    baseline_per_identifier: bool,
    apply_fixes: bool,
    format: OutputFormat,
) {
    let phpstan_rs::project::session::Analysis {
        mut results,
        checked,
        skipped_analyze,
        skipped_context,
        context_loaded,
        check_panics,
    } = analysis;
    if let Some(generate_baseline_path) = generate_baseline_path {
        let generated = if baseline_per_identifier {
            baseline::generate_per_identifier(generate_baseline_path, &results)
        } else {
            baseline::generate(generate_baseline_path, &results)
        };
        match generated {
            Ok(count) => println!(
                "baseline written to {}: {count} diagnostic(s)",
                generate_baseline_path.display()
            ),
            Err(e) => eprintln!(
                "failed to write baseline to {}: {e}",
                generate_baseline_path.display()
            ),
        }
        return;
    }

    if let Some(baseline_path) = baseline_path {
        match baseline::Baseline::load(baseline_path) {
            Ok(baseline) => {
                let baseline = baseline.with_match_mode(baseline_mode);
                for stale in baseline::filter(&mut results, &baseline) {
                    eprintln!("baseline entry no longer matches any diagnostic: {stale}");
                }
            }
            Err(e) => eprintln!("failed to load baseline {}: {e}", baseline_path.display()),
        }
    }

    if apply_fixes {
        let mut fixed_files = 0usize;
        let mut applied_total = 0usize;
        let mut paths: Vec<&PathBuf> = results.keys().collect();
        paths.sort();
        for path in paths {
            let diagnostics = &results[path];
            if !diagnostics.iter().any(|d| d.fix.is_some()) {
                continue;
            }
            let Ok(source) = std::fs::read_to_string(path) else {
                eprintln!("failed to read {} to apply fixes", path.display());
                continue;
            };
            let (fixed, applied) =
                phpstan_rs::fix::apply_fixes(&source, phpstan_rs::fix::fixes_of(diagnostics));
            if applied == 0 || fixed == source {
                continue;
            }
            match std::fs::write(path, fixed) {
                Ok(()) => {
                    fixed_files += 1;
                    applied_total += applied;
                }
                Err(e) => eprintln!("failed to write fixes to {}: {e}", path.display()),
            }
        }
        eprintln!("applied {applied_total} fix(es) in {fixed_files} file(s)");
    }

    // Text is the only format carrying anything beyond raw diagnostics
    // (the "N more file(s) loaded for context"/"N file(s) skipped"/"N
    // file(s) errored while checking" prose) - `json`/`checkstyle` exist
    // so a CI pipeline can consume clean, uninterrupted structured data
    // on stdout, so they report only the diagnostics themselves (`json`'s
    // own `totals.files_checked` is the one piece of that extra
    // bookkeeping structured output still carries, already reflecting
    // `checked`'s subtraction of both skipped and panicked files - the
    // rest would need their own stable fields to add without guessing at
    // what a consumer wants, so they're left for a real request rather
    // than invented here).
    match format {
        OutputFormat::Text | OutputFormat::Friendly => {
            let mut paths: Vec<&PathBuf> = results.keys().collect();
            paths.sort();

            let mut total = 0usize;
            if format == OutputFormat::Friendly {
                use std::io::IsTerminal;
                let colour = std::io::stdout().is_terminal()
                    && std::env::var_os("NO_COLOR").is_none_or(|v| v.is_empty());
                print!("{}", output::render_friendly(&results, colour));
                total = results.values().map(Vec::len).sum();
            } else {
                for path in paths {
                    for d in &results[path] {
                        println!("{}:{}", path.display(), output::diagnostic_line(d));
                        if let Some(tip) = &d.tip {
                            println!("    💡 {tip}");
                        }
                        total += 1;
                    }
                }
            }
            for (path, msg) in &check_panics {
                println!("{}: internal error while checking: {msg}", path.display());
            }
            let mut summary = format!("\n{checked} file(s) checked, {total} diagnostic(s)");
            let mut extras = Vec::new();
            if context_loaded > 0 {
                extras.push(format!("{context_loaded} more file(s) loaded for context"));
            }
            let skipped_total = skipped_analyze + skipped_context;
            if skipped_total > 0 {
                extras.push(format!("{skipped_total} file(s) skipped: parse error"));
            }
            if !check_panics.is_empty() {
                extras.push(format!(
                    "{} file(s) errored while checking",
                    check_panics.len()
                ));
            }
            if !extras.is_empty() {
                summary.push_str(&format!(" ({})", extras.join(", ")));
            }
            println!("{summary}");
        }
        OutputFormat::Json => {
            println!("{}", output::render_json(&results, checked));
        }
        OutputFormat::Checkstyle => {
            print!("{}", output::render_checkstyle(&results));
        }
        OutputFormat::Junit => {
            print!("{}", output::render_junit(&results));
        }
    }
}
