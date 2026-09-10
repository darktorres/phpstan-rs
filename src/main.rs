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

#[derive(ClapParser)]
#[command(subcommand_required = true, arg_required_else_help = true)]
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
    let cli = Cli::parse();
    match cli.command {
        Command::Check { paths } => check::run_check(&paths),
        Command::CheckProject {
            dirs,
            level,
            strict_mixed_arrays,
            check_uninitialized_properties,
            check_internal_usage,
            check_deprecated_usage,
            php_version,
            generate_baseline,
            baseline,
            scan_dirs,
            no_composer,
            config,
            format,
        } => check_project::run_check_project_command(
            dirs,
            level,
            strict_mixed_arrays,
            check_uninitialized_properties,
            check_internal_usage,
            check_deprecated_usage,
            php_version,
            generate_baseline,
            baseline,
            scan_dirs,
            no_composer,
            config,
            format,
        ),
        Command::IncrementalBench { dir, file_to_touch } => {
            incremental_bench::run_incremental_bench(&dir, file_to_touch.as_deref())
        }
        Command::Lsp => lsp::run(),
        Command::Demo { file } => demo::run_demo(file.as_deref()),
    }
}

#[cfg(test)]
#[path = "main/tests.rs"]
mod tests;
