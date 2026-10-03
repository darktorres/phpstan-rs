//! The `check-project` pipeline as a library: a config file's sections
//! (plus command-line overrides) resolved against an explicit root
//! directory, loaded into a
//! `Project`, checked in parallel into an `AnalysisResult`, which
//! `finalize` post-processes into the per-file diagnostics a consumer
//! reports. The CLI and the fixture harness both drive it, so a fixture
//! exercises exactly what `check-project` runs.

use rustc_hash::{FxHashMap, FxHashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use syntax::span::Span;

use rayon::prelude::*;

use config::ConfigFile;
use serde::Deserialize;

use crate::architecture::{self, ArchitectureSettings};
use crate::assertion_tables::OverlaySettings;
use crate::banned::{self, BannedSettings};
use crate::checker::settings::EngineSettings;
use crate::ext::disallowed_calls::DisallowedCallsSettings;
use crate::ext::mockstan::MockstanSettings;
use crate::ext::sql::SqlSettings;
use crate::parent_calls::{self, ClassShapeSettings};
use crate::registry::settings::ReflectionSettings;
use crate::rules_settings::RulesSettings;
use crate::{ext, ignore_errors};

use super::whole_program::renamed_named_arguments::merge_named_argument_calls;
use super::{ParsedFile, Project, ProjectSettings};
use diagnostics::{self, Diagnostic, DiagnosticBuilder};

/// The level reported when neither the command line nor the config sets
/// one: every level this project's diagnostics are tagged with.
pub const DEFAULT_REPORT_LEVEL: u8 = 6;

/// The run's own top-level keys: what's analysed, at which level, and how
/// the report is post-processed.
#[derive(Debug, Default, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct SessionSettings {
    pub paths: Option<Vec<PathBuf>>,
    pub level: Option<u8>,
    pub baseline: Option<PathBuf>,
    /// Match baseline entries by identifier when their message no longer
    /// matches (`baseline::MatchMode::Identifier`).
    pub baseline_match_identifier: bool,
    /// Write `--generate-baseline` as one file per identifier plus an
    /// index (`baseline::generate_per_identifier`).
    pub baseline_per_identifier: bool,
    /// PHPStan's `ignoreErrors`, re-typed by hand (see
    /// `ignore_errors::IgnoreErrorEntry`).
    pub ignore_errors: Vec<ignore_errors::IgnoreErrorEntry>,
    /// Report inline `@phpstan-ignore*` comments that suppress nothing
    /// (`ignore.unmatchedLine`/`ignore.unmatchedIdentifier`).
    pub report_unmatched_ignored_errors: bool,
}

impl config::Section for SessionSettings {
    const PATH: &'static [&'static str] = &[];
}

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
    /// The process's worker pool (`project::worker_pool`), which every
    /// parallel parse and check runs on.
    pool: Arc<rayon::ThreadPool>,
    root: PathBuf,
    session: SessionSettings,
    /// What the project is configured with.
    settings: ProjectSettings,
    dirs: Vec<PathBuf>,
    scan_dirs: Vec<PathBuf>,
    scan_files: Vec<PathBuf>,
    /// The vendor files and directories of the composer roots, still being
    /// found while the analysed files are listed, and the packages
    /// promoted to analysed paths, which are left out of them.
    vendors: Vec<composer::PendingVendor>,
    promoted: Vec<PathBuf>,
    stub_dirs: Vec<PathBuf>,
    level: u8,
    emission_order: bool,
    banned_constructs: Vec<banned::BannedConstruct>,
    banned_types: Vec<String>,
    check_closed_classes: bool,
    parent_call_rules: Vec<parent_calls::ParentCallRule>,
    architecture_rules: Vec<architecture::ArchitectureRule>,
    check_mock_hygiene: bool,
    mock_banned_classes: Vec<String>,
    /// Non-fatal problems found while resolving (e.g. SQL schema drift,
    /// an old config key).
    pub warnings: Vec<String>,
}

/// A configured `Project` with every file loaded, before any check ran.
pub struct Loaded {
    pub project: Project,
    analyze_files_by_size: Vec<PathBuf>,
    skipped_analyze: usize,
    skipped_context: usize,
    context_loaded: usize,
    /// The analysed files that do not parse, which are reported.
    parse_failures: Vec<(PathBuf, ParseFailure)>,
    /// What the lexer skipped over in the loaded files, and the context
    /// files that do not parse, one line each.
    pub lex_warnings: Vec<String>,
}

/// What one run's analysis produced, before post-processing: every
/// check's diagnostics, and the project they came from, which
/// `finalize` needs for inline ignores. Finalizing doesn't consume it,
/// so one analysis can be finalized several ways.
pub struct AnalysisResult {
    project: Project,
    /// Per-file diagnostics as the checks emitted them.
    results: FxHashMap<PathBuf, Vec<Diagnostic>>,
    /// Analysed files that parsed and checked without an internal error.
    pub checked: usize,
    pub skipped_analyze: usize,
    pub skipped_context: usize,
    pub context_loaded: usize,
    /// Files whose check panicked, with the panic message, sorted.
    pub check_panics: Vec<(PathBuf, String)>,
    /// What the lexer skipped over in the loaded files, one line each.
    pub lex_warnings: Vec<String>,
    /// The `--dump-types` output when requested: every tracked variable's
    /// type after each statement of each analysed file, one
    /// `path:line:col [host] vars` line per statement, files in path
    /// order.
    pub type_dump: Option<String>,
}

/// The post-processing a finalized report gets, in this order: the level
/// filter, inline ignores, `ignore_errors`, the baseline, then the order.
#[derive(Clone)]
pub struct Finalize {
    pub level: u8,
    pub report_unmatched_ignores: bool,
    pub ignore_errors: Vec<ignore_errors::IgnoreErrorEntry>,
    pub baseline: Option<crate::baseline::Baseline>,
    pub order: DiagnosticOrder,
}

/// How each file's diagnostics are ordered in a report.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticOrder {
    /// `diagnostics::sort_diagnostics`' total order.
    Sorted,
    /// The order the checks emitted them in.
    Emission,
}

/// A finalized report.
pub struct Report {
    pub results: FxHashMap<PathBuf, Vec<Diagnostic>>,
    /// The baseline's entries that matched no diagnostic.
    pub stale_baseline_entries: Vec<String>,
}

impl Report {
    /// What the report says beyond its diagnostics, one line each.
    pub fn notices(&self) -> Vec<String> {
        self.stale_baseline_entries
            .iter()
            .map(|stale| format!("baseline entry no longer matches any diagnostic: {stale}"))
            .collect()
    }
}

impl AnalysisResult {
    /// What the analysis says beyond its diagnostics, one line each.
    pub fn notices(&self) -> Vec<String> {
        self.lex_warnings.clone()
    }

    /// The report `options` make of this analysis.
    pub fn finalize(&self, options: &Finalize) -> Report {
        let mut results = self.results.clone();
        for diagnostics in results.values_mut() {
            diagnostics.retain(|d| d.level <= options.level);
        }
        self.project
            .apply_inline_ignores(&mut results, options.report_unmatched_ignores);
        ignore_errors::filter(&mut results, &options.ignore_errors);
        let stale_baseline_entries = match &options.baseline {
            Some(baseline) => crate::baseline::filter(&mut results, baseline),
            None => Vec::new(),
        };
        if options.order == DiagnosticOrder::Sorted {
            for diagnostics in results.values_mut() {
                diagnostics::sort_diagnostics(diagnostics);
            }
        }
        Report {
            results,
            stale_baseline_entries,
        }
    }
}

/// The directory a composer project containing `dirs` is rooted at: the
/// nearest ancestor with a `composer.json`, per directory, falling back
/// to `root`.
/// The PHP versions a project's composer.json declares it runs on (its
/// `config.platform.php`, else `require.php`): the known releases the
/// constraint admits, a release counting when any of its patch versions
/// does.
pub(crate) fn declared_php_versions(
    package: &composer::PackageContext,
) -> Option<types::PhpVersions> {
    let constraint = package.php_constraint()?;
    types::known_php_versions_where(|v| {
        ["0", "99"].iter().any(|patch| {
            composer::constraint::satisfies(constraint, &format!("{v}.{patch}")) == Some(true)
        })
    })
}

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

/// A file read and parsed: how long it is as checking weighs it, its text
/// when the project keeps it (empty otherwise), and its parse.
struct ReadFile {
    len: usize,
    source: String,
    parsed: ParsedFile,
}

/// A file that was not loaded because it does not parse, where, and why.
struct ParseFailure {
    line: u32,
    col: u32,
    message: String,
}

/// What reading a file came to.
enum Read {
    Loaded(Box<ReadFile>),
    /// The text is not PHP.
    Unparsed(ParseFailure),
    /// The file could not be read, or the parser failed internally.
    Unread,
}

impl Session {
    /// Resolves `config`, whose relative paths are relative to
    /// `config_dir` (or to `root` without one), and the command-line
    /// `overrides` against `root`, the directory the run's relative paths
    /// are relative to.
    pub fn resolve(
        config: &ConfigFile,
        config_dir: Option<PathBuf>,
        root: PathBuf,
        overrides: Overrides,
        pool: Arc<rayon::ThreadPool>,
    ) -> Result<Session, String> {
        let session = config.section::<SessionSettings>()?;
        let reflection = config.section::<ReflectionSettings>()?;
        let engine = config.section::<EngineSettings>()?;
        let overlays = config.section::<OverlaySettings>()?;
        let rules = config.section::<RulesSettings>()?;
        let banned_settings = config.section::<BannedSettings>()?;
        let architecture = config.section::<ArchitectureSettings>()?;
        let class_shape = config.section::<ClassShapeSettings>()?;
        let disallowed_calls = config.section::<DisallowedCallsSettings>()?;
        let sql_settings = config.section::<SqlSettings>()?;
        let mockstan = config.section::<MockstanSettings>()?;
        let psr3 = config.section::<ext::psr3::Psr3Settings>()?;

        let resolve_against_config = |p: PathBuf| match &config_dir {
            Some(dir) => dir.join(p),
            None => p,
        };
        let dirs: Vec<PathBuf> = if !overrides.paths.is_empty() {
            overrides.paths
        } else {
            session
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
        let configured_level = overrides.level.or(session.level);
        let level = overrides
            .report_level
            .or(configured_level)
            .unwrap_or(DEFAULT_REPORT_LEVEL);
        psr3.context_key_pattern()?;
        let php_version = overrides
            .php_version
            .clone()
            .or(reflection.php_version.clone())
            .map(|s| types::parse_php_version(&s))
            .transpose()?;
        let restricted_symbols = disallowed_calls.entries()?;
        let banned_constructs = banned_settings
            .constructs
            .iter()
            .map(|name| banned::BannedConstruct::parse(name))
            .collect::<Result<Vec<_>, _>>()?;
        overlays.validate()?;
        let parent_call_rules = class_shape.rules();
        let architecture_rules = architecture.0;
        for rule in &architecture_rules {
            rule.validate()?;
        }
        let sql_reflection = sql_settings.reflection;
        let sql_schema = match sql_reflection
            .as_ref()
            .and_then(|c| c.schema_file.as_ref().map(|f| (f.clone(), c.driver)))
        {
            Some((path, driver)) => {
                let path = resolve_against_config(path);
                Some(sql::schema::parse_ddl_file(&root.join(path), driver)?)
            }
            None => None,
        };
        let mut warnings: Vec<String> = config.warnings().to_vec();
        if let Some(schema) = &sql_schema
            && let Some(sql_config) = &sql_reflection
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
            reflection
                .scan_paths
                .iter()
                .cloned()
                .map(resolve_against_config)
                .collect()
        };
        let no_composer = overrides.no_composer || reflection.no_composer.unwrap_or(false);
        let stub_dirs: Vec<PathBuf> = if !overrides.stub_paths.is_empty() {
            overrides.stub_paths
        } else {
            reflection
                .stub_paths
                .iter()
                .cloned()
                .map(resolve_against_config)
                .collect()
        };
        scan_dirs.extend(stub_dirs.iter().cloned());

        let mut scan_files: Vec<PathBuf> = Vec::new();
        let mut vendors = Vec::new();
        let mut promoted: Vec<PathBuf> = Vec::new();
        let mut phpstan_api_check_enabled = false;
        let mut composer_manifests = Vec::new();
        let absolute_root = std::fs::canonicalize(&root).unwrap_or_else(|_| root.clone());
        let mut composer_php_versions = None;
        let mut root_manifest = None;
        let mut dirs = dirs;
        if !no_composer {
            let canonical_root = absolute_root.clone();
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
                if composer_root == canonical_root {
                    composer_php_versions = declared_php_versions(&package);
                    root_manifest = package.manifest.clone();
                }
                vendors.push(package.vendor.clone());
                phpstan_api_check_enabled |= package.requires_phpstan();
                if let Some(manifest) = package.manifest {
                    promoted.extend(
                        manifest
                            .path_repository_packages()
                            .into_iter()
                            .map(|p| p.vendor_dir.clone()),
                    );
                    composer_manifests.push(manifest);
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
                dirs.extend(promoted.iter().cloned());
            }
        }

        let php_versions = php_version
            .map(types::PhpVersions::exactly)
            .or(composer_php_versions);
        let extensions = ext::ActiveExtensions::resolve(config, &|p| {
            ext::installed_in(root_manifest.as_ref(), p)
        })?;

        let settings = ProjectSettings {
            working_dir: Some(root.clone()),
            absolute_root: Some(absolute_root),
            packages: composer::PackageIndex::new(composer_manifests),
            php_versions,
            configured_level,
            phpstan_api_check_enabled,
            defer_inline_ignores: true,
            retain_source: false,
            built_once: true,
            rules,
            extensions,
            extras: config.section::<ext::extras::ExtrasSettings>()?,
            strict_rules: config.section::<ext::strict_rules::StrictRulesSettings>()?,
            shipmonk: config.section::<ext::shipmonk::ShipmonkSettings>()?,
            voku: config.section::<ext::voku::VokuSettings>()?,
            slam: config.section::<ext::slam::SlamSettings>()?,
            psr3,
            sidz: config.section::<ext::sidz::SidzSettings>()?,
            phpunit: config.section::<ext::phpunit::PhpunitSettings>()?,
            restricted_symbols,
            doctrine: config.section::<ext::doctrine::DoctrineSettings>()?,
            dynamic_constant_names: engine.dynamic_constant_entries(),
            overlays,
            sql_reflection,
            sql_schema,
        };

        Ok(Session {
            pool,
            root,
            session,
            settings,
            dirs,
            scan_dirs,
            scan_files,
            vendors,
            promoted,
            stub_dirs,
            level,
            emission_order: overrides.emission_order,
            banned_constructs,
            banned_types: banned_settings.types,
            check_closed_classes: class_shape.check_closed_classes,
            parent_call_rules,
            architecture_rules,
            check_mock_hygiene: mockstan.check_mock_hygiene.unwrap_or(extensions.mockstan),
            mock_banned_classes: mockstan.mock_banned_classes,
            warnings,
        })
    }

    /// A fixture's session: `stem.php` (or the directory `stem/`) analysed
    /// under `config`, the fixture's `stem.toml` if it has one (its paths
    /// relative to the fixture's directory), with composer discovery off
    /// unless the config turns it on, reporting every level, running on
    /// `pool`. Returns the session and that directory.
    pub fn for_fixture(
        stem: &Path,
        config: &ConfigFile,
        pool: Arc<rayon::ThreadPool>,
    ) -> Result<(Session, PathBuf), String> {
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
        let overrides = Overrides {
            paths: if config.section::<SessionSettings>()?.paths.is_none() {
                vec![subject]
            } else {
                Vec::new()
            },
            no_composer: config
                .section::<ReflectionSettings>()?
                .no_composer
                .is_none(),
            report_level: Some(u8::MAX),
            ..Overrides::default()
        };
        // Paths stay relative to the fixture's own directory, so where the
        // fixture suite lives (under `tests/`) never shows in the analysed
        // paths - path-sensitive checks see only the fixture's own layout.
        let session = Session::resolve(config, None, base.clone(), overrides, pool)?;
        Ok((session, base))
    }

    /// The post-processing this session's configuration asks for, without
    /// a baseline.
    pub fn finalize_options(&self) -> Finalize {
        Finalize {
            level: self.level,
            report_unmatched_ignores: self.session.report_unmatched_ignored_errors,
            ignore_errors: self.session.ignore_errors.clone(),
            baseline: None,
            order: if self.emission_order {
                DiagnosticOrder::Emission
            } else {
                DiagnosticOrder::Sorted
            },
        }
    }

    /// The directories analysed (after composer path-repository
    /// promotion), relative to the root as given.
    pub fn dirs(&self) -> &[PathBuf] {
        &self.dirs
    }

    fn configure(&self, project: &mut Project) {
        project.apply_settings(self.settings.clone());
    }

    /// Whether `path` (relative to the root) is in a package promoted to an
    /// analysed path.
    fn is_promoted(&self, path: &Path) -> bool {
        if self.promoted.is_empty() {
            return false;
        }
        let path = self.root.join(path);
        self.promoted
            .iter()
            .any(|dir| composer::manifest::path_starts_with(&path, dir))
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
        names: &mut syntax::name::NameCache,
    ) -> Read {
        let Ok(bytes) = std::fs::read(self.root.join(path)) else {
            return Read::Unread;
        };
        // Whether the file is UTF-8 is settled here, once: the text is the
        // bytes, moved, and the lexer takes it as it is. What checking
        // costs is told by the length of the text as a lossy reading of the
        // bytes has it, which is the bytes' own for a file that is UTF-8.
        let (len, text) = match String::from_utf8(bytes) {
            Ok(text) => (text.len(), Ok(text)),
            Err(error) => {
                let bytes = error.into_bytes();
                let len = String::from_utf8_lossy(&bytes).len();
                (len, Err(bytes))
            }
        };
        let Ok(parsed) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match &text {
            Ok(text) => super::parse_file_with_names(path, text, is_stub, Some(names)),
            Err(bytes) => super::parse_file_with_names(path, bytes, is_stub, Some(names)),
        })) else {
            return Read::Unread;
        };
        let parsed = match parsed {
            Ok(parsed) => parsed,
            Err(error) => {
                let bytes = match &text {
                    Ok(text) => text.as_bytes(),
                    Err(bytes) => bytes,
                };
                let at = syntax::LineIndex::new(bytes).line_col(error.span.start);
                return Read::Unparsed(ParseFailure {
                    line: at.line,
                    col: at.col,
                    message: error.message,
                });
            }
        };
        // The text of the file is kept only when the project keeps it.
        let source = match text {
            Ok(text) if self.settings.retain_source => text,
            Err(bytes) if self.settings.retain_source => {
                String::from_utf8_lossy(&bytes).into_owned()
            }
            _ => String::new(),
        };
        Read::Loaded(Box::new(ReadFile {
            len,
            source,
            parsed,
        }))
    }

    /// Loads and checks every analysed file, with the `--dump-types`
    /// output when `dump_types` is set. Fails when the analysed paths hold
    /// no `.php` file.
    pub fn run(&self, dump_types: bool) -> Result<AnalysisResult, String> {
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
        composer::path_order::sort_paths(&mut analyze_files);
        // The vendor context was being found while the analysed files were
        // listed.
        let mut scan_dirs = self.scan_dirs.clone();
        let mut context_files: Vec<PathBuf> = self.scan_files.clone();
        for vendor in &self.vendors {
            let found = vendor.get();
            scan_dirs.extend(found.dirs.iter().filter(|d| !self.is_promoted(d)).cloned());
            context_files.extend(found.files.iter().filter(|f| !self.is_promoted(f)).cloned());
        }
        for dir in &scan_dirs {
            self.collect_php_files(dir, &mut context_files);
        }
        composer::path_order::sort_paths(&mut context_files);
        context_files.dedup();
        if analyze_files.is_empty() {
            let dirs: Vec<String> = self.dirs.iter().map(|d| d.display().to_string()).collect();
            return Err(format!("no .php files found under {}", dirs.join(", ")));
        }

        let mut project = Project::new();
        self.configure(&mut project);

        let pool = &self.pool;

        let mut stub_files_vec = Vec::new();
        for dir in &self.stub_dirs {
            self.collect_php_files(dir, &mut stub_files_vec);
        }
        let stub_files: FxHashSet<PathBuf> = stub_files_vec.into_iter().collect();

        let combined_files: Vec<&PathBuf> =
            context_files.iter().chain(analyze_files.iter()).collect();
        let mut combined_parsed: Vec<Read> = pool.install(|| {
            combined_files
                .par_iter()
                // The names a job meets are kept while it parses one file
                // after another, and dropped with the load.
                .map_init(syntax::name::NameCache::default, |names, p| {
                    let mut read =
                        self.read_and_parse_file(p, stub_files.contains(p.as_path()), names);
                    if let Read::Loaded(file) = &mut read {
                        project.index_use_sites(&mut file.parsed, &file.source);
                        project.note_owner(&mut file.parsed, p);
                    }
                    read
                })
                .collect()
        });
        let analyze_parsed = combined_parsed.split_off(context_files.len());
        let context_parsed = combined_parsed;
        project.expect_files(
            context_parsed
                .iter()
                .chain(&analyze_parsed)
                .filter_map(|read| match read {
                    Read::Loaded(file) => Some(&file.parsed),
                    _ => None,
                }),
        );

        // Context files first, so an analysed file's declaration wins a
        // name collision.
        let mut lex_warnings = Vec::new();
        let mut note_lex_warnings = |path: &Path, parsed: &ParsedFile| {
            lex_warnings.extend(parsed.lex_warnings().iter().map(|w| {
                format!(
                    "warning: {}:{}:{}: {}",
                    path.display(),
                    parsed.line_index.line_col(w.span.start).line,
                    parsed.line_index.line_col(w.span.start).col,
                    w.message
                )
            }));
        };
        let mut skipped_context = 0usize;
        let mut parse_failure_lines = Vec::new();
        for (path, parsed) in context_files.iter().zip(context_parsed) {
            match parsed {
                Read::Loaded(file) => {
                    let ReadFile { source, parsed, .. } = *file;
                    note_lex_warnings(path, &parsed);
                    project.insert_parsed_file(path.clone(), parsed, source, false)
                }
                Read::Unparsed(failure) => {
                    skipped_context += 1;
                    parse_failure_lines.push(format!(
                        "warning: {}:{}:{}: {}",
                        path.display(),
                        failure.line,
                        failure.col,
                        failure.message
                    ));
                }
                Read::Unread => skipped_context += 1,
            }
        }
        // How long each analysed file is, by its place in `analyze_files`.
        let mut analyze_sizes: Vec<usize> = vec![0; analyze_files.len()];
        let mut skipped_analyze = 0usize;
        let mut parse_failures = Vec::new();
        for (index, (path, parsed)) in analyze_files.iter().zip(analyze_parsed).enumerate() {
            match parsed {
                Read::Loaded(file) => {
                    let ReadFile {
                        len,
                        source,
                        parsed,
                    } = *file;
                    note_lex_warnings(path, &parsed);
                    analyze_sizes[index] = len;
                    project.insert_parsed_file(path.clone(), parsed, source, true)
                }
                Read::Unparsed(failure) => {
                    skipped_analyze += 1;
                    parse_failures.push((path.clone(), failure));
                }
                Read::Unread => skipped_analyze += 1,
            }
        }
        lex_warnings.extend(parse_failure_lines);
        project.finish_loading();

        // Largest files first, so the longest checks don't start last; of
        // equal ones, the first listed first.
        let mut by_size: Vec<usize> = (0..analyze_files.len()).collect();
        by_size.sort_by_key(|&index| std::cmp::Reverse(analyze_sizes[index]));
        let analyze_files_by_size = by_size
            .into_iter()
            .map(|index| analyze_files[index].clone())
            .collect();
        Ok(Loaded {
            project,
            analyze_files_by_size,
            skipped_analyze,
            skipped_context,
            context_loaded: context_files.len() - skipped_context,
            parse_failures,
            lex_warnings,
        })
    }

    /// Checks every analysed file of `loaded` and runs the project-wide
    /// passes.
    pub fn analyse(&self, loaded: Loaded, dump_types: bool) -> AnalysisResult {
        let Loaded {
            mut project,
            analyze_files_by_size,
            skipped_analyze,
            skipped_context,
            context_loaded,
            parse_failures,
            lex_warnings,
        } = loaded;

        let needs_tier_2 =
            self.settings.rules.check_dead_enum_cases || self.settings.rules.check_dead_members;
        let checked_results = project.check_files(
            &analyze_files_by_size,
            Some(&self.pool),
            needs_tier_2,
            dump_types,
        );

        let mut results: FxHashMap<PathBuf, Vec<Diagnostic>> = FxHashMap::default();
        let mut check_panics: Vec<(PathBuf, String)> = Vec::new();
        let mut named_argument_calls = Vec::new();
        let mut type_dumps = Vec::new();
        for (path, outcome) in checked_results {
            match outcome {
                Ok(check) => {
                    if needs_tier_2 {
                        project.seed_member_refs(
                            &path,
                            check.member_refs,
                            check.escaped_enums,
                            check.escaped_member_classes,
                        );
                    }
                    named_argument_calls.push((path.clone(), check.named_argument_calls));
                    if dump_types {
                        type_dumps.push((path.clone(), check.type_dump));
                    }
                    results.insert(path, check.diagnostics);
                }
                Err(msg) => check_panics.push((path, msg)),
            }
        }
        check_panics.sort();
        named_argument_calls.sort_by(|a, b| a.0.cmp(&b.0));
        let type_dump = dump_types.then(|| {
            type_dumps.sort_by(|a, b| a.0.cmp(&b.0));
            let mut text = String::new();
            for (path, lines) in type_dumps {
                for line in lines {
                    text.push_str(&format!("{}:{line}\n", path.display()));
                }
            }
            text
        });
        let named_argument_calls = merge_named_argument_calls(named_argument_calls);
        if needs_tier_2 {
            project.mark_member_index_built();
        }

        // A file that does not parse is reported where it fails: PHP
        // refuses to run it.
        for (path, failure) in parse_failures {
            let at = syntax::span::LineCol {
                line: failure.line,
                col: failure.col,
            };
            results.entry(path).or_default().push(
                DiagnosticBuilder::new(failure.message, Span::default(), 0, "parse.error")
                    .non_ignorable()
                    .build_at(syntax::span::Range { start: at, end: at }),
            );
        }
        let mut extend = |found: FxHashMap<PathBuf, Vec<Diagnostic>>| {
            for (path, diags) in found {
                results.entry(path).or_default().extend(diags);
            }
        };
        extend(project.check_whole_program(&named_argument_calls));
        if self.check_mock_hygiene {
            extend(project.check_mock_hygiene(&self.mock_banned_classes));
        }
        extend(project.check_class_shapes(self.check_closed_classes, &self.parent_call_rules));
        extend(project.check_banned(&self.banned_constructs, &self.banned_types));
        extend(project.check_architecture(&self.architecture_rules));
        if self.settings.rules.check_composer_hygiene {
            for root in 0..self.settings.packages.manifests().len() {
                extend(project.check_composer_hygiene(root));
            }
        }

        AnalysisResult {
            project,
            results,
            checked: analyze_files_by_size.len() - skipped_analyze - check_panics.len(),
            skipped_analyze,
            skipped_context,
            context_loaded,
            check_panics,
            lex_warnings,
            type_dump,
        }
    }
}
