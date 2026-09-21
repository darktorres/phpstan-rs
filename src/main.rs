use std::path::PathBuf;

use clap::{Parser as ClapParser, Subcommand};

use output::OutputFormat;
use phpstan_rs::{lsp, output};

#[path = "main/check.rs"]
mod check;
#[path = "main/check_project.rs"]
mod check_project;
#[path = "main/demo.rs"]
mod demo;
#[path = "main/incremental_bench.rs"]
mod incremental_bench;

#[cfg(feature = "dhat-heap")]
#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

#[derive(ClapParser)]
#[command(subcommand_required = true, arg_required_else_help = true, version)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Lex+parse every .php file under the given paths, reporting a
    /// pass/fail summary.
    Check { paths: Vec<PathBuf> },
    /// Load every .php file under the given paths and run the full
    /// checker over all of them, reporting every diagnostic.
    CheckProject {
        // Not `required = true`: a TOML config file's own `paths` can
        // supply these instead (see `run_check_project`'s resolution
        // logic) - the "must know what to check" requirement is
        // enforced there, once the config (if any) has been consulted,
        // not statically by clap.
        dirs: Vec<PathBuf>,
        /// Report up to and including this level (see
        /// checker::Diagnostic's own doc comment) - unset means
        /// "report everything".
        #[arg(long)]
        level: Option<u8>,
        #[arg(long)]
        strict_mixed_arrays: bool,
        /// Enables `property.uninitialized` (a typed property with no
        /// default nothing in the class body ever assigns). Off by
        /// default, mirroring real PHPStan's own opt-in
        /// `checkUninitializedProperties` parameter - see
        /// `registry::Registry::check_uninitialized_properties`'s own
        /// doc comment for why.
        #[arg(long)]
        check_uninitialized_properties: bool,
        /// Enables the `@internal` restricted-usage engine (`internal.*`
        /// diagnostics) - restricts a symbol tagged `@internal` (or
        /// declared inside an `@internal`-tagged class) to callers
        /// sharing its own top-level namespace. Off by default,
        /// mirroring real PHPStan's own opt-in `featureToggles.
        /// internalTag` parameter - see `registry::Registry::
        /// check_internal_usage`'s own doc comment for why.
        #[arg(long)]
        check_internal_usage: bool,
        /// Enables `use function`'s own `function.nameCase` check (a
        /// case-mismatched `use function` import, e.g. `use function
        /// STRLEN;`). Off by default, mirroring real PHPStan's own
        /// opt-in `checkFunctionNameCase` parameter - see `registry::
        /// Registry::check_function_name_case`'s own doc comment for
        /// why.
        #[arg(long)]
        check_function_name_case: bool,
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
        check_deprecated_usage: bool,
        /// Enables the reinstated empty-concrete-method-body completeness
        /// check (`return.missing`/`return.never` now also firing on a
        /// literal `{}` body, not just a real fall-through). Off by
        /// default - see `registry::Registry::check_empty_method_bodies`'s
        /// own doc comment for the confirmed corpus false-positive class
        /// (IDE-stub/native-extension declaration files) this avoids.
        #[arg(long)]
        check_empty_method_bodies: bool,
        /// Enables the opt-in "missing checked exception" family
        /// (`throws.missingType` on a real, explicit `throw` whose own
        /// type isn't covered by any `@throws` tag). Off by default - the
        /// riskiest of this project's opt-in checks given the real
        /// corpus's documented inconsistent `@throws` tagging, see
        /// `registry::Registry::check_missing_checked_exceptions`'s own
        /// doc comment for why.
        #[arg(long)]
        check_missing_checked_exceptions: bool,
        /// Enables the strict (vs. the default, castable) printf
        /// placeholder-type matching mode for `sprintf`/`printf`/
        /// `vsprintf`/`vprintf`/`fprintf` argument-type checks. Off by
        /// default - matches real PHPStan's own stock config exactly, see
        /// `registry::Registry::check_strict_printf_placeholder_types`'s
        /// own doc comment for why.
        #[arg(long)]
        check_strict_printf_placeholder_types: bool,
        /// Enables `TooWideMethodThrowTypeRule`'s wider, re-declaration-
        /// only mode for a protected/public method that isn't final (and
        /// isn't on a final class). Off by default - matches real
        /// PHPStan's own stock config exactly, see `registry::Registry::
        /// check_too_wide_throw_type_in_protected_and_public_methods`'s
        /// own doc comment for why.
        #[arg(long)]
        check_too_wide_throw_type_in_protected_and_public_methods: bool,
        /// Enables `TooWideMethodReturnTypehintRule`'s wider, re-
        /// declaration-only mode for a protected/public method that
        /// isn't final (and isn't on a final class) - the return-type
        /// analogue of the flag above. Off by default, same reason - see
        /// `registry::Registry::check_too_wide_return_type_in_protected_and_public_methods`'s
        /// own doc comment.
        #[arg(long)]
        check_too_wide_return_type_in_protected_and_public_methods: bool,
        /// Enables `MethodThrowTypeCovarianceRule` (a method's own
        /// `@throws` type must be covariant with its immediate parent
        /// class/every transitive interface/abstract trait declaration's
        /// own `@throws`). Off by default - see `registry::Registry::
        /// check_method_throw_type_covariance`'s own doc comment for why.
        #[arg(long)]
        check_method_throw_type_covariance: bool,
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
        check_duplicate_function_declarations: bool,
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
        check_duplicate_class_declarations: bool,
        /// Enables "boolean trap" detection - a bare `true`/`false`
        /// literal passed positionally into a bare-`bool`-typed
        /// parameter. Off by default: unambiguous single-bool-parameter
        /// calls are common and would otherwise be pure noise - see
        /// `registry::Registry::check_boolean_trap`'s own doc comment.
        #[arg(long)]
        check_boolean_trap: bool,
        /// Enables array-key cast warnings (a literal float/bool/null/
        /// numeric-string array key silently cast at runtime). Off by
        /// default - real PHPStan itself only ever registers this in its
        /// own private issue-bot config, never an installable package -
        /// see `registry::Registry::check_array_key_cast`'s own doc
        /// comment.
        #[arg(long)]
        check_array_key_cast: bool,
        /// Enables `RequireParentConstructCallRule` (a concrete class's
        /// own `__construct()` never calling `parent::__construct()`
        /// when a real, callable parent constructor exists). Off by
        /// default - real PHPStan ships this only via the separate,
        /// opt-in `phpstan/phpstan-strict-rules` package - see
        /// `registry::Registry::check_parent_constructor_call`'s own
        /// doc comment.
        #[arg(long)]
        check_parent_constructor_call: bool,
        /// Enables constructor-property-promotion suggestions for a
        /// trivially-forwarding constructor. Off by default - a purely
        /// stylistic suggestion, not a bug - see `registry::Registry::
        /// check_promoted_property_suggestions`'s own doc comment.
        #[arg(long)]
        check_promoted_property_suggestions: bool,
        /// Enables the "only booleans are allowed in a condition" family
        /// (`if`/`elseif`/`while`/`do-while`/ternary conditions, `&&`/
        /// `||` operands, negated booleans). Off by default - real
        /// PHPStan ships this only via the separate, opt-in
        /// `phpstan/phpstan-strict-rules` package - see `registry::
        /// Registry::check_boolean_conditions`'s own doc comment.
        #[arg(long)]
        check_boolean_conditions: bool,
        /// Enables strict `checkDynamicProperties` behavior inside
        /// `isset()`/`empty()`/`??`/`??=` (still flags a property that
        /// doesn't exist there, instead of real PHPStan's lenient
        /// default of silently allowing the probe). Off by default,
        /// mirroring real PHPStan's own opt-in `checkDynamicProperties`
        /// parameter - see `registry::Registry::
        /// check_dynamic_properties`'s own doc comment.
        #[arg(long)]
        check_dynamic_properties: bool,
        /// Enables swapped-adjacent-argument-name detection (`resize
        /// ($height, $width)` where the declared params are `(int
        /// $width, int $height)`). Off by default - a real, disclosed
        /// heuristic (two same-typed parameters are structurally
        /// invisible to any type-system-based check as "different
        /// roles"), not a soundness gap - see `registry::Registry::
        /// check_swapped_arguments`'s own doc comment.
        #[arg(long)]
        check_swapped_arguments: bool,
        /// Enables `never`-return-type suggestions for a function/method
        /// that provably never returns normally but is declared `void`
        /// (or left undeclared). Off by default - real PHPStan already
        /// computes this exact fact, but never ships an installable
        /// rule consuming it - see `registry::Registry::
        /// check_never_return_suggestions`'s own doc comment.
        #[arg(long)]
        check_never_return_suggestions: bool,
        /// Enables missing-`default:`-arm detection for `switch`
        /// statements. Off by default - see `registry::Registry::
        /// check_switch_missing_default`'s own doc comment.
        #[arg(long)]
        check_switch_missing_default: bool,
        /// Enables missing-exception-message detection (`throw new
        /// Exception()` with zero constructor arguments). Off by
        /// default - see `registry::Registry::check_throw_without_
        /// message`'s own doc comment.
        #[arg(long)]
        check_throw_without_message: bool,
        /// Enables base-Exception/Error-class detection (`throw new
        /// Exception(...)`/`throw new Error(...)` instead of a specific
        /// subclass). Off by default - see `registry::Registry::
        /// check_throw_base_class`'s own doc comment.
        #[arg(long)]
        check_throw_base_class: bool,
        /// Enables discarded-`new`-exception detection (`new
        /// SomeException(...);` sitting on its own line, never thrown).
        /// Off by default - see `registry::Registry::check_new_exception_
        /// discarded`'s own doc comment.
        #[arg(long)]
        check_new_exception_discarded: bool,
        /// Enables assertion-less-test ("risky test") detection (a
        /// PHPUnit test method/Pest test closure with zero assertion-
        /// shaped calls anywhere in its body). Off by default - a test
        /// that only ever calls a custom, differently-named assertion-
        /// helper method is a real, disclosed false positive - see
        /// `registry::Registry::check_risky_tests`'s own doc comment.
        #[arg(long)]
        check_risky_tests: bool,
        /// Enables dead-enum-case detection (a declared case never
        /// referenced via `EnumName::CaseName`, a `match`/`switch` arm
        /// value, or a literal `::from()`/`::tryFrom()` argument,
        /// project-wide). Off by default - see
        /// `registry::Registry::check_dead_enum_cases`'s own doc comment
        /// for the real, disclosed false-positive risks.
        #[arg(long)]
        check_dead_enum_cases: bool,
        /// Enables dead-function/dead-class detection (a top-level
        /// function or class/interface/trait/enum never referenced
        /// anywhere in the project). Off by default - see
        /// `registry::Registry::check_dead_code`'s own doc comment for
        /// the real, disclosed false-positive risks.
        #[arg(long)]
        check_dead_code: bool,
        /// Enables namespace-level circular-dependency detection (a
        /// namespace with a class that `extends`/`implements`/`use`s a
        /// class in another namespace that, via some other class pair,
        /// has an edge back). Off by default - see `registry::Registry::
        /// check_circular_dependencies`'s own doc comment.
        #[arg(long)]
        check_circular_dependencies: bool,
        /// Enables dead-abstract-class detection (an `abstract class`
        /// with zero concrete descendants anywhere in the project). Off
        /// by default - see `registry::Registry::
        /// check_dead_abstract_classes`'s own doc comment.
        #[arg(long)]
        check_dead_abstract_classes: bool,
        /// Enables dead-protected/public-member detection (a method/
        /// property/const never referenced anywhere in the project). Off
        /// by default - see `registry::Registry::check_dead_members`'s
        /// own doc comment for the real, disclosed false-positive risks.
        #[arg(long)]
        check_dead_members: bool,
        /// Target PHP version, as "major.minor" (e.g. "8.1") - gates the
        /// portability rules that check "X is only supported on PHP Y.Z
        /// and later" (see `php_version::PhpFeature`). Unset means no
        /// such rule has anything to check against yet.
        #[arg(long)]
        php_version: Option<String>,
        #[arg(long)]
        generate_baseline: Option<PathBuf>,
        #[arg(long)]
        baseline: Option<PathBuf>,
        /// Extra directories loaded for type-resolution context only -
        /// never analyzed or reported on. Repeatable.
        #[arg(long = "scan-dir")]
        scan_dirs: Vec<PathBuf>,
        /// A directory (walked) or single file whose class-like
        /// declarations are designated overrides, never a genuine
        /// accidental duplicate (e.g. a vendored `Imagick.stub.php`) -
        /// see `check_duplicate_class_declarations` above. Repeatable.
        /// Also loaded for type-resolution context, exactly like
        /// `--scan-dir` (unioned into it internally).
        #[arg(long = "stub-path")]
        stub_paths: Vec<PathBuf>,
        /// Disables the automatic Composer-autoloader-based `vendor/`
        /// context load (see `composer::discover_vendor_context`).
        /// `--scan-dir` still works.
        #[arg(long)]
        no_composer: bool,
        /// TOML config file (see src/config.rs) supplying defaults for
        /// paths/level/baseline/strict_mixed_arrays/
        /// check_uninitialized_properties/scan_paths/no_composer/
        /// php_version - any flag above, when given, overrides the same
        /// key in the config.
        #[arg(long)]
        config: Option<PathBuf>,
        /// Output format: `text` (default, human-readable), `json`, or
        /// `checkstyle` (XML) - the latter two for CI pipelines that want
        /// to consume diagnostics as structured data rather than parsing
        /// plain text. CLI-only, matching real PHPStan's own
        /// `--error-format`: unlike `level`/`baseline`/etc., this isn't a
        /// `config.rs` TOML key, since it's how one particular invocation
        /// wants its output shaped, not a standing project setting.
        #[arg(long)]
        format: Option<OutputFormat>,
    },
    /// Loads every .php file under `dir` into a Project (timed), then
    /// re-parses/re-checks just one file via the incremental path (also
    /// timed), reporting both.
    IncrementalBench {
        dir: PathBuf,
        file_to_touch: Option<PathBuf>,
    },
    /// Runs the LSP server over stdio.
    Lsp,
    /// Parses the given file (or a built-in sample with none given) and
    /// dumps tokens/AST/resolved signatures/diagnostics - a debug tool,
    /// not the normal way to use this checker.
    Demo { file: Option<PathBuf> },
}

#[hotpath::main]
fn main() {
    // Held for the whole run; its `Drop` writes `dhat-heap.json` on
    // clean exit (every `check-project` error path below that matters
    // for a profiling run falls through to a normal return, not
    // `process::exit`, so this always flushes).
    #[cfg(feature = "dhat-heap")]
    let _profiler = dhat::Profiler::new_heap();

    let cli = Cli::parse();
    match cli.command {
        Command::Check { paths } => check::run_check(&paths),
        Command::CheckProject {
            dirs,
            level,
            strict_mixed_arrays,
            check_uninitialized_properties,
            check_internal_usage,
            check_function_name_case,
            check_deprecated_usage,
            check_empty_method_bodies,
            check_missing_checked_exceptions,
            check_strict_printf_placeholder_types,
            check_too_wide_throw_type_in_protected_and_public_methods,
            check_too_wide_return_type_in_protected_and_public_methods,
            check_method_throw_type_covariance,
            check_duplicate_function_declarations,
            check_duplicate_class_declarations,
            check_boolean_trap,
            check_array_key_cast,
            check_parent_constructor_call,
            check_promoted_property_suggestions,
            check_boolean_conditions,
            check_dynamic_properties,
            check_swapped_arguments,
            check_never_return_suggestions,
            check_switch_missing_default,
            check_throw_without_message,
            check_throw_base_class,
            check_new_exception_discarded,
            check_risky_tests,
            check_dead_enum_cases,
            check_dead_code,
            check_circular_dependencies,
            check_dead_abstract_classes,
            check_dead_members,
            php_version,
            generate_baseline,
            baseline,
            scan_dirs,
            stub_paths,
            no_composer,
            config,
            format,
        } => check_project::run_check_project_command(
            dirs,
            level,
            strict_mixed_arrays,
            check_uninitialized_properties,
            check_internal_usage,
            check_function_name_case,
            check_deprecated_usage,
            check_empty_method_bodies,
            check_missing_checked_exceptions,
            check_strict_printf_placeholder_types,
            check_too_wide_throw_type_in_protected_and_public_methods,
            check_too_wide_return_type_in_protected_and_public_methods,
            check_method_throw_type_covariance,
            check_duplicate_function_declarations,
            check_duplicate_class_declarations,
            check_boolean_trap,
            check_array_key_cast,
            check_parent_constructor_call,
            check_promoted_property_suggestions,
            check_boolean_conditions,
            check_dynamic_properties,
            check_swapped_arguments,
            check_never_return_suggestions,
            check_switch_missing_default,
            check_throw_without_message,
            check_throw_base_class,
            check_new_exception_discarded,
            check_risky_tests,
            check_dead_enum_cases,
            check_dead_code,
            check_circular_dependencies,
            check_dead_abstract_classes,
            check_dead_members,
            php_version,
            generate_baseline,
            baseline,
            scan_dirs,
            stub_paths,
            no_composer,
            config,
            format,
        ),
        Command::IncrementalBench { dir, file_to_touch } => {
            incremental_bench::run_incremental_bench(&dir, file_to_touch.as_deref())
        }
        Command::Lsp => {
            let exit_code = lsp::run();
            // 0 falls through to main's own normal return (so the
            // dhat-heap profiler's, and a --features hotpath build's
            // HotpathGuard's, Drop still flushes) - only the spec-
            // mandated nonzero (protocol-violation: exit with no prior
            // shutdown) path short-circuits via process::exit, the same
            // way run_check_project's own error paths already do.
            if exit_code != 0 {
                std::process::exit(exit_code);
            }
        }
        Command::Demo { file } => demo::run_demo(file.as_deref()),
    }
}

#[cfg(test)]
#[path = "main/tests.rs"]
mod tests;
