//! The `check-project` pipeline as a library: a `Config` (plus command-line
//! overrides) resolved against an explicit root directory, loaded into a
//! `Project`, checked in parallel, and post-processed into the per-file
//! diagnostics every consumer reports. The CLI and the fixture harness
//! both drive it, so a fixture exercises exactly what `check-project`
//! runs.

use rustc_hash::{FxHashMap, FxHashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use syntax::name::Interner;

use rayon::prelude::*;

use crate::config::{self, Config};
use crate::{
    architecture, assertion_tables, banned, composer, doctrine_functions, ignore_errors,
    parent_calls, restricted_symbols,
};
use syntax::token::Span;

use super::{ParsedFile, Project};
use diagnostics::{self, Diagnostic};

/// The level reported when neither the command line nor the config sets
/// one: every level this project's diagnostics are tagged with.
pub const DEFAULT_REPORT_LEVEL: u8 = 6;

/// Command-line values that replace the config's. Paths given here are
/// used as given (relative to the root), not resolved against the
/// config file's directory.
#[derive(Debug, Default, Clone)]
pub struct Overrides {
    pub paths: Vec<PathBuf>,
    pub scan_paths: Vec<PathBuf>,
    pub stub_paths: Vec<PathBuf>,
    pub level: Option<u8>,
    /// Reports diagnostics up to this level without configuring it (the
    /// level also drives type acceptance): the fixture harness records
    /// every level at once this way.
    pub report_level: Option<u8>,
    pub php_version: Option<String>,
    pub no_composer: bool,
    /// Leaves each file's diagnostics in the order the checks emitted them
    /// instead of `diagnostic::diagnostic_order`: the determinism gate runs
    /// twice with this and requires identical output.
    pub emission_order: bool,
}

/// A fully resolved `check-project` run: every path, setting and derived
/// table, ready to load.
pub struct Session {
    root: PathBuf,
    config: Config,
    dirs: Vec<PathBuf>,
    scan_dirs: Vec<PathBuf>,
    scan_files: Vec<PathBuf>,
    stub_dirs: Vec<PathBuf>,
    level: u8,
    configured_level: Option<u8>,
    emission_order: bool,
    php_version: Option<types::PhpVersion>,
    psr3_context_key_pattern: Option<regex::Regex>,
    restricted_symbol_entries: Vec<restricted_symbols::RestrictedSymbolEntry>,
    banned_constructs: Vec<banned::BannedConstruct>,
    parent_call_rules: Vec<parent_calls::ParentCallRule>,
    architecture_rules: Vec<architecture::ArchitectureRule>,
    assertion_entries: Vec<assertion_tables::AssertionEntry>,
    class_argument_returns: Vec<assertion_tables::ClassArgumentReturn>,
    dynamic_constant_names: Vec<(String, Option<String>)>,
    doctrine_custom_function_entries: Vec<doctrine_functions::DoctrineCustomFunctionEntry>,
    sql_reflection_config: Option<sql::config::SqlReflectionConfig>,
    sql_reflection_schema: Option<sql::schema::SchemaCatalog>,
    composer_manifests: Vec<composer::manifest::ComposerManifest>,
    phpstan_api_check_enabled: bool,
    /// Every composer root's package directories (`PackageContext::
    /// package_dirs`), for which package owns an analysed file.
    package_dirs: Vec<(PathBuf, String)>,
    /// Non-fatal problems found while resolving (e.g. SQL schema drift).
    pub warnings: Vec<String>,
}

/// A configured `Project` with every file loaded, before any check ran.
pub struct Loaded {
    pub project: Project,
    pool: rayon::ThreadPool,
    analyze_files_by_size: Vec<PathBuf>,
    skipped_analyze: usize,
    skipped_context: usize,
    context_loaded: usize,
    /// What the lexer skipped over in the loaded files, one line each.
    pub lex_warnings: Vec<String>,
}

/// What one run produced, after the full post-processing chain.
pub struct Analysis {
    /// Per-file diagnostics in `diagnostic::diagnostic_order`.
    pub results: FxHashMap<PathBuf, Vec<Diagnostic>>,
    /// Analysed files that parsed and checked without an internal error.
    pub checked: usize,
    pub skipped_analyze: usize,
    pub skipped_context: usize,
    pub context_loaded: usize,
    /// Files whose check panicked, with the panic message, sorted.
    pub check_panics: Vec<(PathBuf, String)>,
    /// What the lexer skipped over in the loaded files, one line each.
    pub lex_warnings: Vec<String>,
}

/// The directory a composer project containing `dirs` is rooted at: the
/// nearest ancestor with a `composer.json`, per directory, falling back
/// to `root`.
fn composer_roots(dirs: &[PathBuf], root: &Path) -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = Vec::new();
    for dir in dirs {
        let start = std::fs::canonicalize(root.join(dir)).unwrap_or_else(|_| root.join(dir));
        let start = if start.is_file() {
            start.parent().map(Path::to_path_buf).unwrap_or(start)
        } else {
            start
        };
        let found = start
            .ancestors()
            .find(|a| a.join("composer.json").is_file())
            .map(Path::to_path_buf)
            .unwrap_or_else(|| root.to_path_buf());
        if !roots.contains(&found) {
            roots.push(found);
        }
    }
    if roots.is_empty() {
        roots.push(root.to_path_buf());
    }
    roots
}

impl Session {
    /// Resolves `config` (loaded from `config_dir`, if any) and the
    /// command-line `overrides` against `root`, the directory the run's
    /// relative paths are relative to.
    pub fn resolve(
        config: Config,
        config_dir: Option<PathBuf>,
        root: PathBuf,
        overrides: Overrides,
    ) -> Result<Session, String> {
        let resolve_against_config = |p: PathBuf| match &config_dir {
            Some(dir) => dir.join(p),
            None => p,
        };
        let dirs: Vec<PathBuf> = if !overrides.paths.is_empty() {
            overrides.paths
        } else {
            config
                .paths
                .clone()
                .unwrap_or_default()
                .into_iter()
                .map(resolve_against_config)
                .collect()
        };
        if dirs.is_empty() {
            return Err("no paths given on the command line or in the config file".to_string());
        }
        let configured_level = overrides.level.or(config.level);
        let level = overrides
            .report_level
            .or(configured_level)
            .unwrap_or(DEFAULT_REPORT_LEVEL);
        let psr3_context_key_pattern = config
            .psr3_context_key_pattern
            .as_deref()
            .map(regex::Regex::new)
            .transpose()
            .map_err(|e| format!("invalid psr3_context_key_pattern: {e}"))?;
        let php_version = overrides
            .php_version
            .clone()
            .or(config.php_version.clone())
            .map(|s| types::parse_php_version(&s))
            .transpose()?;
        let mut restricted_symbol_entries = config.restricted_symbols.clone().unwrap_or_default();
        for preset in config.restricted_symbol_presets.clone().unwrap_or_default() {
            restricted_symbol_entries.extend(banned::restricted_symbol_preset(&preset)?);
        }
        let banned_constructs = config
            .banned_constructs
            .clone()
            .unwrap_or_default()
            .iter()
            .map(|name| banned::BannedConstruct::parse(name))
            .collect::<Result<Vec<_>, _>>()?;
        let assertion_entries = config.assertions.clone().unwrap_or_default();
        let class_argument_returns = config.class_argument_returns.clone().unwrap_or_default();
        if let Some(error) = assertion_entries
            .iter()
            .map(|e| e.validate())
            .chain(class_argument_returns.iter().map(|e| e.validate()))
            .find_map(Result::err)
        {
            return Err(error);
        }
        let mut parent_call_rules = config.parent_calls.clone().unwrap_or_default();
        if config.default_parent_calls.unwrap_or(true) {
            parent_call_rules.extend(parent_calls::default_rules());
        }
        let architecture_rules = config.architecture.clone().unwrap_or_default();
        for rule in &architecture_rules {
            rule.validate()?;
        }
        let dynamic_constant_names = config
            .dynamic_constant_names
            .as_ref()
            .map(config::DynamicConstantNames::entries)
            .unwrap_or_default();
        let doctrine_custom_function_entries =
            config.doctrine_custom_functions.clone().unwrap_or_default();
        let sql_reflection_config = config.sql_reflection.clone();
        let sql_reflection_schema = match sql_reflection_config
            .as_ref()
            .and_then(|c| c.schema_file.as_ref().map(|f| (f.clone(), c.driver)))
        {
            Some((path, driver)) => {
                let path = resolve_against_config(path);
                Some(sql::schema::parse_ddl_file(&root.join(path), driver)?)
            }
            None => None,
        };
        let mut warnings = Vec::new();
        if let Some(schema) = &sql_reflection_schema
            && let Some(sql_config) = &sql_reflection_config
        {
            let handle = sql::reflector::SqlReflectorHandle::default();
            if let Some(reflector) = handle.get(Some(sql_config)) {
                warnings.extend(sql::drift::schema_drift_warnings(
                    schema,
                    reflector,
                    sql_config.driver,
                ));
            }
        }

        let mut scan_dirs: Vec<PathBuf> = if !overrides.scan_paths.is_empty() {
            overrides.scan_paths
        } else {
            config
                .scan_paths
                .clone()
                .unwrap_or_default()
                .into_iter()
                .map(resolve_against_config)
                .collect()
        };
        let no_composer = overrides.no_composer || config.no_composer.unwrap_or(false);
        let stub_dirs: Vec<PathBuf> = if !overrides.stub_paths.is_empty() {
            overrides.stub_paths
        } else {
            config
                .stub_paths
                .clone()
                .unwrap_or_default()
                .into_iter()
                .map(resolve_against_config)
                .collect()
        };
        scan_dirs.extend(stub_dirs.iter().cloned());

        let mut scan_files = Vec::new();
        let mut phpstan_api_check_enabled = false;
        let mut composer_manifests = Vec::new();
        let mut package_dirs = Vec::new();
        let mut dirs = dirs;
        if !no_composer {
            let canonical_root = std::fs::canonicalize(&root).unwrap_or_else(|_| root.clone());
            let mut promoted: Vec<PathBuf> = Vec::new();
            for composer_root in composer_roots(&dirs, &canonical_root) {
                // The root itself keeps its own spelling (`.` for the CLI),
                // so the manifest's paths compare against analysed paths
                // spelled the same way.
                let manifest_root = if composer_root == canonical_root {
                    root.clone()
                } else {
                    composer_root.clone()
                };
                let package = composer::PackageContext::load(&composer_root, &manifest_root);
                scan_dirs.extend(package.vendor.dirs.iter().cloned());
                scan_files.extend(package.vendor.files.iter().cloned());
                phpstan_api_check_enabled |= package.requires_phpstan();
                package_dirs.extend(package.package_dirs());
                if let Some(manifest) = package.manifest {
                    promoted.extend(
                        manifest
                            .path_repository_packages()
                            .into_iter()
                            .map(|p| p.vendor_dir.clone()),
                    );
                    if config.check_composer_hygiene.unwrap_or(false) {
                        composer_manifests.push(manifest);
                    }
                }
            }
            if !promoted.is_empty() {
                // Relative scan paths are relative to the root.
                let is_promoted = |p: &Path| {
                    let p = root.join(p);
                    promoted
                        .iter()
                        .any(|dir| composer::manifest::path_starts_with(&p, dir))
                };
                scan_dirs.retain(|d| !is_promoted(d));
                scan_files.retain(|f| !is_promoted(f));
                dirs.extend(promoted);
            }
        }

        Ok(Session {
            root,
            config,
            dirs,
            scan_dirs,
            scan_files,
            stub_dirs,
            level,
            configured_level,
            emission_order: overrides.emission_order,
            php_version,
            psr3_context_key_pattern,
            restricted_symbol_entries,
            banned_constructs,
            parent_call_rules,
            architecture_rules,
            assertion_entries,
            class_argument_returns,
            dynamic_constant_names,
            doctrine_custom_function_entries,
            sql_reflection_config,
            sql_reflection_schema,
            composer_manifests,
            phpstan_api_check_enabled,
            package_dirs,
            warnings,
        })
    }

    /// A fixture's session: `stem.php` (or the directory `stem/`) analysed
    /// under `stem.toml` if present (its paths relative to the fixture's
    /// directory), with composer discovery off unless the config turns it
    /// on, reporting every level. Returns the session and that directory.
    pub fn for_fixture(stem: &Path) -> Result<(Session, PathBuf), String> {
        let file_subject = stem.with_extension("php");
        let (subject, base) = if file_subject.is_file() {
            let base = file_subject.parent().unwrap_or(Path::new("")).to_path_buf();
            let name = PathBuf::from(file_subject.file_name().unwrap_or_default());
            (name, base)
        } else if stem.is_dir() {
            (PathBuf::new(), stem.to_path_buf())
        } else {
            return Err(format!(
                "{}: no `.php` file or directory subject",
                stem.display()
            ));
        };
        let config_file = stem.with_extension("toml");
        let mut config = if config_file.is_file() {
            Config::load(&config_file)?
        } else {
            Config::default()
        };
        if config.paths.is_none() {
            config.paths = Some(vec![subject]);
        }
        if config.no_composer.is_none() {
            config.no_composer = Some(true);
        }
        let overrides = Overrides {
            report_level: Some(u8::MAX),
            ..Overrides::default()
        };
        // Paths stay relative to the fixture's own directory, so where the
        // fixture suite lives (under `tests/`) never shows in the analysed
        // paths - path-sensitive checks see only the fixture's own layout.
        let session = Session::resolve(config, None, base.clone(), overrides)?;
        Ok((session, base))
    }

    /// The directories analysed (after composer path-repository
    /// promotion), relative to the root as given.
    pub fn dirs(&self) -> &[PathBuf] {
        &self.dirs
    }

    fn flag(value: Option<bool>) -> bool {
        value.unwrap_or(false)
    }

    fn configure(&self, project: &mut Project) {
        let c = &self.config;
        project.set_working_dir(self.root.clone());
        project.set_strict_mixed_arrays(Self::flag(c.strict_mixed_arrays));
        project.set_check_uninitialized_properties(Self::flag(c.check_uninitialized_properties));
        project.set_check_internal_usage(Self::flag(c.check_internal_usage));
        project.set_check_function_name_case(Self::flag(c.check_function_name_case));
        project.set_check_deprecated_usage(Self::flag(c.check_deprecated_usage));
        project.set_check_empty_method_bodies(Self::flag(c.check_empty_method_bodies));
        project
            .set_check_missing_checked_exceptions(Self::flag(c.check_missing_checked_exceptions));
        project.set_check_strict_printf_placeholder_types(Self::flag(
            c.check_strict_printf_placeholder_types,
        ));
        project.set_check_too_wide_throw_type_in_protected_and_public_methods(Self::flag(
            c.check_too_wide_throw_type_in_protected_and_public_methods,
        ));
        project.set_check_too_wide_return_type_in_protected_and_public_methods(Self::flag(
            c.check_too_wide_return_type_in_protected_and_public_methods,
        ));
        project.set_check_too_wide_by_ref_type_in_protected_and_public_methods(Self::flag(
            c.check_too_wide_by_ref_type_in_protected_and_public_methods,
        ));
        project.set_check_method_throw_type_covariance(Self::flag(
            c.check_method_throw_type_covariance,
        ));
        project.set_check_benevolent_union_types(Self::flag(c.check_benevolent_union_types));
        project
            .set_check_missing_callable_signature(Self::flag(c.check_missing_callable_signature));
        project.set_check_duplicate_function_declarations(Self::flag(
            c.check_duplicate_function_declarations,
        ));
        project.set_check_duplicate_class_declarations(Self::flag(
            c.check_duplicate_class_declarations,
        ));
        project.set_check_boolean_trap(Self::flag(c.check_boolean_trap));
        project.set_check_array_key_cast(Self::flag(c.check_array_key_cast));
        project.set_check_parent_constructor_call(Self::flag(c.check_parent_constructor_call));
        project.set_check_promoted_property_suggestions(Self::flag(
            c.check_promoted_property_suggestions,
        ));
        project.set_check_boolean_conditions(Self::flag(c.check_boolean_conditions));
        project.set_check_dynamic_properties(Self::flag(c.check_dynamic_properties));
        project.set_check_swapped_arguments(Self::flag(c.check_swapped_arguments));
        project.set_check_never_return_suggestions(Self::flag(c.check_never_return_suggestions));
        project.set_check_switch_missing_default(Self::flag(c.check_switch_missing_default));
        project.set_check_throw_without_message(Self::flag(c.check_throw_without_message));
        project.set_check_throw_base_class(Self::flag(c.check_throw_base_class));
        project.set_check_exception_hygiene(Self::flag(c.check_exception_hygiene));
        project.set_check_current_time_dependence(Self::flag(c.check_current_time_dependence));
        project.set_check_psr3_exception_context(Self::flag(c.check_psr3_exception_context));
        project.set_check_psr3_logging(Self::flag(c.check_psr3_logging));
        project.set_psr3_context_key_pattern(self.psr3_context_key_pattern.clone());
        project.set_check_magic_numbers(Self::flag(c.check_magic_numbers));
        project.set_magic_number_options(
            c.magic_number_exemptions
                .as_ref()
                .map(|numbers| numbers.iter().map(|n| n.value()).collect()),
            Self::flag(c.magic_numbers_ignore_numeric_strings),
        );
        project.set_check_native_type_hints(Self::flag(c.check_native_type_hints));
        project.set_check_implicit_array_creation(Self::flag(c.check_implicit_array_creation));
        project.set_check_useless_casts(Self::flag(c.check_useless_casts));
        project.set_check_strict_array_filter(Self::flag(c.check_strict_array_filter));
        project.set_check_mixed_type_operands(Self::flag(c.check_mixed_type_operands));
        project.set_check_comparable_types(Self::flag(c.check_comparable_types));
        project.set_check_arithmetic_operands(Self::flag(c.check_arithmetic_operands));
        project.set_check_identity_vs_equals(Self::flag(c.check_identity_vs_equals));
        project.set_check_loose_haystack_comparison(Self::flag(c.check_loose_haystack_comparison));
        project.set_check_strict_function_calls(Self::flag(c.check_strict_function_calls));
        project.set_check_switch_case_types(Self::flag(c.check_switch_case_types));
        project.set_check_loop_variable_overwrite(Self::flag(c.check_loop_variable_overwrite));
        project.set_check_dynamic_static_calls(Self::flag(c.check_dynamic_static_calls));
        project.set_check_illegal_constructor_calls(Self::flag(c.check_illegal_constructor_calls));
        project.set_check_enum_exhaustiveness(Self::flag(c.check_enum_exhaustiveness));
        project.set_check_null_coercion(Self::flag(c.check_null_coercion));
        project.set_check_new_exception_discarded(Self::flag(c.check_new_exception_discarded));
        project.set_check_risky_tests(Self::flag(c.check_risky_tests));
        project.set_check_dead_enum_cases(Self::flag(c.check_dead_enum_cases));
        project.set_check_dead_code(Self::flag(c.check_dead_code));
        project.set_check_circular_dependencies(Self::flag(c.check_circular_dependencies));
        project.set_check_dead_abstract_classes(Self::flag(c.check_dead_abstract_classes));
        project.set_check_dead_members(Self::flag(c.check_dead_members));
        project.set_defer_inline_ignores(true);
        project.set_assertion_tables(
            self.assertion_entries.clone(),
            self.class_argument_returns.clone(),
        );
        project.set_check_renamed_named_arguments(Self::flag(c.check_renamed_named_arguments));
        project.set_configured_level(self.configured_level);
        project.set_restricted_symbols(&self.restricted_symbol_entries);
        project.set_dynamic_constant_names(&self.dynamic_constant_names);
        project.set_doctrine_custom_functions(&self.doctrine_custom_function_entries);
        project.set_sql_reflection_config(self.sql_reflection_config.clone());
        project.set_sql_reflection_schema(self.sql_reflection_schema.clone());
        project.set_php_version(self.php_version);
        project.set_phpstan_api_check_enabled(self.phpstan_api_check_enabled);
        project.set_package_dirs(self.package_dirs.clone());
    }

    /// Every `.php` file under `dir` (or `dir` itself), spelled as `dir`
    /// joined with its path below it - the same spelling the command line
    /// gave - while the walk itself happens under the root.
    fn collect_php_files(&self, dir: &Path, out: &mut Vec<PathBuf>) {
        let walked = self.root.join(dir);
        let mut found = Vec::new();
        super::collect_php_files(&walked, &mut found);
        out.extend(
            found
                .into_iter()
                .map(|path| match path.strip_prefix(&walked) {
                    Ok(rest) if rest.as_os_str().is_empty() => dir.to_path_buf(),
                    Ok(rest) => dir.join(rest),
                    Err(_) => path,
                }),
        );
    }

    fn read_and_parse_file(
        &self,
        path: &Path,
        is_stub: bool,
        interner: &Arc<Interner>,
    ) -> Option<(String, ParsedFile)> {
        let bytes = std::fs::read(self.root.join(path)).ok()?;
        let source = String::from_utf8_lossy(&bytes).into_owned();
        let parsed =
            std::panic::catch_unwind(|| super::parse_file(path, &source, is_stub, interner))
                .ok()?
                .ok()?;
        Some((source, parsed))
    }

    /// Loads, checks and post-processes every analysed file. `dump_types`
    /// writes `--dump-types` output there. Fails when the analysed
    /// paths hold no `.php` file.
    pub fn run(&self, dump_types: Option<&Path>) -> Result<Analysis, String> {
        let loaded = self.load()?;
        Ok(self.analyse(loaded, dump_types))
    }

    /// Parses every context and analysed file into a configured,
    /// finished `Project`, ready for `analyse`.
    pub fn load(&self) -> Result<Loaded, String> {
        let mut analyze_files = Vec::new();
        for dir in &self.dirs {
            self.collect_php_files(dir, &mut analyze_files);
        }
        analyze_files.sort();
        let mut context_files: Vec<PathBuf> = self.scan_files.clone();
        for dir in &self.scan_dirs {
            self.collect_php_files(dir, &mut context_files);
        }
        context_files.sort();
        context_files.dedup();
        if analyze_files.is_empty() {
            let dirs: Vec<String> = self.dirs.iter().map(|d| d.display().to_string()).collect();
            return Err(format!("no .php files found under {}", dirs.join(", ")));
        }

        let mut project = Project::new();
        let interner = Arc::clone(project.interner());
        self.configure(&mut project);

        // Deep recursion in the checker needs more than rayon's default
        // worker stack.
        const WORKER_STACK_SIZE: usize = 64 * 1024 * 1024;
        let pool = rayon::ThreadPoolBuilder::new()
            .stack_size(WORKER_STACK_SIZE)
            .build()
            .expect("failed to build check-project worker pool");

        let mut stub_files_vec = Vec::new();
        for dir in &self.stub_dirs {
            self.collect_php_files(dir, &mut stub_files_vec);
        }
        let stub_files: FxHashSet<PathBuf> = stub_files_vec.into_iter().collect();

        let combined_files: Vec<&PathBuf> =
            context_files.iter().chain(analyze_files.iter()).collect();
        let mut combined_parsed: Vec<Option<(String, ParsedFile)>> = pool.install(|| {
            combined_files
                .par_iter()
                .map(|p| self.read_and_parse_file(p, stub_files.contains(p.as_path()), &interner))
                .collect()
        });
        let analyze_parsed = combined_parsed.split_off(context_files.len());
        let context_parsed = combined_parsed;

        // Context files first, so an analysed file's declaration wins a
        // name collision.
        let mut lex_warnings = Vec::new();
        let mut note_lex_warnings = |path: &Path, parsed: &ParsedFile| {
            lex_warnings.extend(parsed.lex_warnings().iter().map(|w| {
                format!(
                    "warning: {}:{}:{}: {}",
                    path.display(),
                    w.span.start.line,
                    w.span.start.col,
                    w.message
                )
            }));
        };
        let mut skipped_context = 0usize;
        for (path, parsed) in context_files.iter().zip(context_parsed) {
            match parsed {
                Some((source, parsed)) => {
                    note_lex_warnings(path, &parsed);
                    project.insert_parsed_file(path.clone(), parsed, source, false)
                }
                None => skipped_context += 1,
            }
        }
        let mut analyze_sizes: FxHashMap<PathBuf, usize> =
            FxHashMap::with_capacity_and_hasher(analyze_files.len(), Default::default());
        let mut skipped_analyze = 0usize;
        for (path, parsed) in analyze_files.iter().zip(analyze_parsed) {
            match parsed {
                Some((source, parsed)) => {
                    note_lex_warnings(path, &parsed);
                    analyze_sizes.insert(path.clone(), source.len());
                    project.insert_parsed_file(path.clone(), parsed, source, true)
                }
                None => skipped_analyze += 1,
            }
        }
        project.finish_loading();

        // Largest files first, so the longest checks don't start last.
        let mut analyze_files_by_size = analyze_files.clone();
        analyze_files_by_size
            .sort_by_key(|path| std::cmp::Reverse(analyze_sizes.get(path).copied().unwrap_or(0)));
        Ok(Loaded {
            project,
            pool,
            analyze_files_by_size,
            skipped_analyze,
            skipped_context,
            context_loaded: context_files.len() - skipped_context,
            lex_warnings,
        })
    }

    /// Checks every analysed file of `loaded`, runs the project-wide
    /// passes, and post-processes the result.
    pub fn analyse(&self, loaded: Loaded, dump_types: Option<&Path>) -> Analysis {
        let Loaded {
            mut project,
            pool,
            analyze_files_by_size,
            skipped_analyze,
            skipped_context,
            context_loaded,
            lex_warnings,
        } = loaded;

        // `Project` has no interior mutability a panic could leave
        // half-updated beyond `OnceLock`s that stay unset, so continuing
        // past a caught panic is sound.
        let property_assign_facts = Arc::new(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                project.collect_property_assign_facts()
            }))
            .unwrap_or_default(),
        );
        type MemberRefData = (
            Vec<(Span, String, String)>,
            FxHashSet<String>,
            FxHashSet<String>,
        );
        type CheckOutcome = Result<(Vec<Diagnostic>, Option<MemberRefData>), String>;
        let needs_tier_2 = Self::flag(self.config.check_dead_enum_cases)
            || Self::flag(self.config.check_dead_members);
        let checked_results: Vec<(PathBuf, CheckOutcome)> = pool.install(|| {
            analyze_files_by_size
                .par_iter()
                .map(|path| {
                    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        if needs_tier_2 {
                            let (diagnostics, member_refs, escaped_enums, escaped_member_classes) =
                                project
                                    .check_file_with_property_assign_facts_collecting_member_refs(
                                        path,
                                        &property_assign_facts,
                                    );
                            (
                                diagnostics,
                                Some((member_refs, escaped_enums, escaped_member_classes)),
                            )
                        } else {
                            (
                                project.check_file_with_property_assign_facts(
                                    path,
                                    &property_assign_facts,
                                ),
                                None,
                            )
                        }
                    }));
                    let outcome = outcome.map_err(|payload| {
                        payload
                            .downcast_ref::<String>()
                            .cloned()
                            .or_else(|| payload.downcast_ref::<&str>().map(|s| s.to_string()))
                            .unwrap_or_else(|| "unknown panic".to_string())
                    });
                    (path.clone(), outcome)
                })
                .collect()
        });

        if let Some(out) = dump_types {
            let mut dumps: Vec<(&PathBuf, Vec<String>)> = pool.install(|| {
                analyze_files_by_size
                    .par_iter()
                    .map(|path| (path, project.dump_types(path)))
                    .collect()
            });
            dumps.sort_by(|a, b| a.0.cmp(b.0));
            let mut text = String::new();
            for (path, lines) in dumps {
                for line in lines {
                    text.push_str(&format!("{}:{line}\n", path.display()));
                }
            }
            if let Err(e) = std::fs::write(out, text) {
                eprintln!("failed to write type dump {}: {e}", out.display());
            }
        }

        let mut results: FxHashMap<PathBuf, Vec<Diagnostic>> = FxHashMap::default();
        let mut check_panics: Vec<(PathBuf, String)> = Vec::new();
        for (path, outcome) in checked_results {
            match outcome {
                Ok((diagnostics, member_data)) => {
                    if let Some((member_refs, escaped_enums, escaped_member_classes)) = member_data
                    {
                        project.seed_member_refs(
                            &path,
                            member_refs,
                            escaped_enums,
                            escaped_member_classes,
                        );
                    }
                    results.insert(path, diagnostics);
                }
                Err(msg) => check_panics.push((path, msg)),
            }
        }
        check_panics.sort();
        if needs_tier_2 {
            project.mark_member_index_built();
        }

        let mut extend = |found: FxHashMap<PathBuf, Vec<Diagnostic>>| {
            for (path, diags) in found {
                results.entry(path).or_default().extend(diags);
            }
        };
        extend(project.check_whole_program());
        if Self::flag(self.config.check_mock_hygiene) {
            let banned = self.config.mock_banned_classes.clone().unwrap_or_default();
            extend(project.check_mock_hygiene(&banned));
        }
        extend(project.check_class_shapes(
            Self::flag(self.config.check_closed_classes),
            &self.parent_call_rules,
        ));
        extend(project.check_banned(
            &self.banned_constructs,
            &self.config.banned_types.clone().unwrap_or_default(),
        ));
        extend(project.check_architecture(&self.architecture_rules));
        for manifest in &self.composer_manifests {
            extend(project.check_composer_hygiene(manifest));
        }

        for diagnostics in results.values_mut() {
            diagnostics.retain(|d| d.level <= self.level);
        }
        project.apply_inline_ignores(
            &mut results,
            Self::flag(self.config.report_unmatched_ignored_errors),
        );
        ignore_errors::filter(
            &mut results,
            &self.config.ignore_errors.clone().unwrap_or_default(),
        );
        if !self.emission_order {
            for diagnostics in results.values_mut() {
                diagnostics::sort_diagnostics(diagnostics);
            }
        }

        Analysis {
            results,
            checked: analyze_files_by_size.len() - skipped_analyze - check_panics.len(),
            skipped_analyze,
            skipped_context,
            context_loaded,
            check_panics,
            lex_warnings,
        }
    }
}
