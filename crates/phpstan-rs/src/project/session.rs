//! The `check-project` pipeline as a library: a config file's sections
//! (plus command-line overrides) resolved against an explicit root
//! directory, loaded into a
//! `Project`, checked in parallel into an `AnalysisResult`, which
//! `finalize` post-processes into the per-file diagnostics a consumer
//! reports. The CLI and the fixture harness both drive it, so a fixture
//! exercises exactly what `check-project` runs.

use rustc_hash::FxHashMap;
use std::borrow::Cow;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use syntax::span::Span;

use rayon::prelude::*;

use composer::dir_listings::DirListings;
use config::ConfigFile;
use serde::Deserialize;

use crate::architecture::{self, ArchitectureSettings};
use crate::assertion_tables::OverlaySettings;
use crate::banned::{self, BannedSettings};
use crate::checker::settings::EngineSettings;
use crate::ext::disallowed_calls::DisallowedCallsSettings;
use crate::ext::mockstan::MockstanSettings;
use crate::ext::sql::SqlSettings;
use crate::file_id::FileId;
use crate::parent_calls::{self, ClassShapeSettings};
use crate::registry::BuiltDecl;
use crate::registry::settings::ReflectionSettings;
use crate::registry::stub_set::Stubs;
use crate::rules_settings::RulesSettings;
use crate::{ext, ignore_errors};

use super::file_cache;
use super::source::FileSource;
use super::walk::{FileIndex, FinalFiles, Listed, Role, Root, Unstreamed, Walk, Walked};
use super::whole_program::renamed_named_arguments::merge_named_argument_calls;
use super::{DEFAULT_SCAN_EXCLUDED_DIR_NAMES, FileText, ParsedFile, Project, ProjectSettings};
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
    /// The baseline file to report against, over the config's.
    pub baseline: Option<PathBuf>,
    pub baseline_match_identifier: bool,
    pub baseline_per_identifier: bool,
}

/// What `Session::resolve` does with a configuration it cannot use. The
/// command line fails on the first problem; the editor keeps the default of
/// each part it cannot read and reports the problem.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorPolicy {
    /// The first problem ends the resolution with an error.
    Fail,
    /// Each problem is recorded in `Session::warnings`, and the part it
    /// concerns takes its default. Without a configured `paths`, the whole
    /// root is analysed, less `DEFAULT_SCAN_EXCLUDED_DIR_NAMES`.
    Degrade,
}

/// Records `error` under `policy`: `Fail` returns it, `Degrade` adds it to
/// `warnings`.
fn report(error: String, policy: ErrorPolicy, warnings: &mut Vec<String>) -> Result<(), String> {
    match policy {
        ErrorPolicy::Fail => Err(error),
        ErrorPolicy::Degrade => {
            warnings.push(error);
            Ok(())
        }
    }
}

/// `result`, or under `Degrade` `fallback` once its error is recorded.
fn settle<T>(
    result: Result<T, String>,
    policy: ErrorPolicy,
    warnings: &mut Vec<String>,
    fallback: T,
) -> Result<T, String> {
    match result {
        Ok(value) => Ok(value),
        Err(error) => report(error, policy, warnings).map(|()| fallback),
    }
}

/// The section `S` of `config`, or its default under `Degrade`.
fn section<S: config::Section>(
    config: &ConfigFile,
    policy: ErrorPolicy,
    warnings: &mut Vec<String>,
) -> Result<S, String> {
    settle(config.section::<S>(), policy, warnings, S::default())
}

/// A fully resolved `check-project` run: every path, setting and derived
/// table, ready to load.
pub struct Session {
    /// The built-in stubs of the run, which the entry point made before
    /// the session so that they build beside the listing and the parse.
    stubs: Stubs,
    /// The pool `stubs` builds on (`Stubs::pool`), which every parallel
    /// parse and check runs on.
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
    /// The directory listings that the vendor discovery and the walk share,
    /// held until the listing is walked.
    dir_listings: Arc<DirListings>,
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
    /// The directory names the walk of the analysed directories leaves out:
    /// `DEFAULT_SCAN_EXCLUDED_DIR_NAMES` when the root is analysed because no
    /// `paths` are configured, nothing otherwise.
    excluded_dir_names: &'static [&'static str],
    /// The baseline the run reports against, resolved, with how its entries
    /// match and whether it is written per identifier.
    baseline: Option<PathBuf>,
    baseline_mode: crate::baseline::MatchMode,
    baseline_per_identifier: bool,
    /// Non-fatal problems found while resolving (e.g. SQL schema drift, an
    /// old config key, a section that degraded under `ErrorPolicy::Degrade`).
    pub warnings: Vec<String>,
    /// The bytes the files are read from, in place of the disk's where it
    /// has an entry: a session over files given in memory (`for_overlay`).
    source: FileSource,
    /// The root as it resolves on disk, with links followed: the key every
    /// file the session lists is under, and the root `finalize` names files
    /// relative to.
    absolute_root: PathBuf,
    /// What the configuration lists, walked on first use and kept.
    listing: OnceLock<Listing>,
    /// The resolved path of each spelling that differs from it, kept so a
    /// spelling whose file no longer resolves (deleted, or behind a link that
    /// was removed) still names the file it named.
    aliases: Mutex<FxHashMap<PathBuf, PathBuf>>,
    /// The files a session over files given in memory loads, in place of a
    /// listing of the disk.
    overlay_listing: Option<Vec<Listed>>,
}

/// A file a session over files given in memory loads: its path, its bytes,
/// and whether it is analysed and parsed as a stub.
pub struct OverlayFile {
    pub path: PathBuf,
    pub bytes: Vec<u8>,
    pub analysed: bool,
    pub stub: bool,
}

/// What a session lists, walked once: each file once under its resolved
/// path, and the roots the walk was given, which classify a path the walk
/// did not list.
pub(crate) struct Listing {
    files: Vec<Listed>,
    /// Where each file is in `files`, by its path.
    index: FileIndex,
    roots: Vec<Root<'static>>,
}

/// What each thread of a pool keeps through one load: the name cache it parses
/// with, which holds the names it has met in the load, and the buffer it reads
/// each file into. The load's end drops both, so a pool that outlives the load
/// keeps neither.
struct Workers(Vec<Mutex<Worker>>);

/// What one thread keeps from one file to the next.
#[derive(Default)]
struct Worker {
    names: syntax::name::NameCache,
    /// The bytes of the file being read, filled in place for each file.
    bytes: Vec<u8>,
}

/// The largest read buffer a worker keeps once its file is parsed. A larger one
/// is freed then, so one large file does not hold its bytes until the load ends.
const KEPT_READ_CAPACITY: usize = 1 << 20;

impl Workers {
    fn new(threads: usize) -> Self {
        Workers((0..threads.max(1)).map(|_| Mutex::default()).collect())
    }

    /// Runs `work` with the state of the thread it runs on.
    fn with<T>(&self, work: impl FnOnce(&mut Worker) -> T) -> T {
        let slot = rayon::current_thread_index().unwrap_or(0) % self.0.len();
        let mut worker = self.0[slot].lock().unwrap_or_else(PoisonError::into_inner);
        work(&mut worker)
    }
}

/// The parse of a walk's files as the walk lists them. A directory's files
/// whose roles are final are parsed in one task on the pool, while the walk
/// lists the rest. The load parses the other files once the walk ends.
struct StreamParse<'l> {
    session: &'l Session,
    project: &'l Project,
    workers: &'l Workers,
    index_use_sites: bool,
    /// Each file parsed so far, with the role it was parsed under.
    parsed: Mutex<Vec<(PathBuf, Role, Read)>>,
}

impl StreamParse<'_> {
    /// The files parsed so far, by path, with the role each was parsed under.
    fn into_parsed(self) -> FxHashMap<PathBuf, (Role, Read)> {
        self.parsed
            .into_inner()
            .unwrap_or_else(PoisonError::into_inner)
            .into_iter()
            .map(|(path, role, read)| (path, (role, read)))
            .collect()
    }
}

impl FinalFiles for StreamParse<'_> {
    fn take<'s>(&'s self, scope: &rayon::Scope<'s>, files: Vec<Listed>) {
        scope.spawn(move |_| {
            let parsed: Vec<(PathBuf, Role, Read)> = files
                .into_iter()
                .map(|file| {
                    let read = self.workers.with(|worker| {
                        self.session.read_indexed(
                            self.project,
                            &file,
                            worker,
                            file.role != Role::Analysed,
                            self.index_use_sites,
                        )
                    });
                    (file.path.into_path_buf(), file.role, read)
                })
                .collect();
            self.parsed
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .extend(parsed);
        });
    }
}

/// The resolved path of `path`, which is the key its file has in a project.
/// A spelling that resolves on disk is resolved by the disk, and is remembered
/// in `aliases` when it differs from its resolved path. A spelling that does
/// not resolve (its file deleted, or a link on the way removed) takes the
/// resolved path it was last given, or else resolves through its nearest
/// existing ancestor.
pub(crate) fn identify_through(
    aliases: &Mutex<FxHashMap<PathBuf, PathBuf>>,
    path: &Path,
) -> PathBuf {
    match std::fs::canonicalize(path) {
        Ok(resolved) => {
            if resolved != path {
                aliases
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .insert(path.to_path_buf(), resolved.clone());
            }
            resolved
        }
        Err(_) => aliases
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(path)
            .cloned()
            .unwrap_or_else(|| composer::manifest::resolve(path)),
    }
}

impl Root<'_> {
    /// Whether the walk of this root lists `path`: it lies under the root's
    /// resolved path, and no directory below the root has an excluded name.
    fn contains(&self, path: &Path) -> bool {
        let Ok(rest) = path.strip_prefix(&self.path) else {
            return false;
        };
        let mut directories = rest.parent().into_iter().flat_map(Path::components);
        !directories.any(|component| match component {
            Component::Normal(name) => self
                .excluded
                .iter()
                .any(|e| std::ffi::OsStr::new(e) == name),
            _ => false,
        })
    }
}

/// A path a client names, as the project keys it, and the role the
/// configuration gives its file.
pub(crate) struct Classified {
    /// The file's resolved path: its key in the project.
    pub(crate) path: PathBuf,
    pub(crate) role: Role,
    /// Whether the file is parsed as a stub.
    pub(crate) is_stub: bool,
}

/// The role of a file no root lists: context when a directory the walk
/// excludes by default holds it below `root`, analysed otherwise.
fn unlisted_role(path: &Path, root: Option<&Path>) -> Role {
    let shown = super::shown_path(root, path);
    if super::path_under_excluded_dir(shown, DEFAULT_SCAN_EXCLUDED_DIR_NAMES) {
        Role::Context
    } else {
        Role::Analysed
    }
}

/// How `path` is classified when no session lists the project: its resolved
/// path, with the role `unlisted_role` gives it, and not a stub.
pub(crate) fn classify_unlisted(path: &Path) -> Classified {
    let path = composer::manifest::resolve(path);
    let role = unlisted_role(&path, None);
    Classified {
        path,
        role,
        is_stub: false,
    }
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

/// What the checks of a project's analysed files produced, split by the
/// pass that produced it.
pub(crate) struct ProjectChecks {
    /// Each analysed file's own checks.
    pub(crate) per_file: FxHashMap<PathBuf, Vec<Diagnostic>>,
    /// The project-wide passes' diagnostics, by the file each is reported in.
    pub(crate) whole_program: FxHashMap<PathBuf, Vec<Diagnostic>>,
    /// Files whose check panicked, with the panic message, sorted.
    pub(crate) check_panics: Vec<(PathBuf, String)>,
    /// The `--dump-types` output when requested.
    pub(crate) type_dump: Option<String>,
}

/// What one run's analysis produced, before post-processing: every
/// check's diagnostics, and the project they came from, which
/// `finalize` needs for inline ignores.
pub struct AnalysisResult {
    project: Project,
    /// Per-file diagnostics as the checks emitted them.
    results: FxHashMap<PathBuf, Vec<Diagnostic>>,
    /// Analysed files that parsed and checked without an internal error.
    pub checked: usize,
    pub skipped_analyze: usize,
    pub skipped_context: usize,
    pub context_loaded: usize,
    /// Files whose check panicked, named as the report names them, with the
    /// panic message, sorted.
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

    /// The report `options` make of this analysis, which its results move
    /// into. The project is leaked: a process that prints a report is about
    /// to end, and freeing the project's tables one by one would only delay
    /// it.
    pub fn finalize(self, options: &Finalize) -> Report {
        let AnalysisResult {
            project, results, ..
        } = self;
        let report = finalize_results(&project, results, options, project.absolute_root());
        std::mem::forget(project);
        report
    }

    /// The report `options` make of this analysis, which is left as it is: its
    /// results are copied, where `finalize` moves them.
    pub fn finalize_copy(&self, options: &Finalize) -> Report {
        finalize_results(
            &self.project,
            self.results.clone(),
            options,
            self.project.absolute_root(),
        )
    }
}

/// The report `options` make of `results`, checked in `project`: the level
/// filter, inline ignores, `ignore_errors`, the baseline, then the order.
/// The project's own keys name the files inline ignores apply to. The
/// report names each file relative to `root`, as the ignore entries and the
/// baseline do; a file outside `root` keeps its path.
pub(crate) fn finalize_results(
    project: &Project,
    mut results: FxHashMap<PathBuf, Vec<Diagnostic>>,
    options: &Finalize,
    root: Option<&Path>,
) -> Report {
    for diagnostics in results.values_mut() {
        diagnostics.retain(|d| d.level <= options.level);
    }
    project.apply_inline_ignores(&mut results, options.report_unmatched_ignores);
    let mut results: FxHashMap<PathBuf, Vec<Diagnostic>> = match root {
        Some(root) => results
            .into_iter()
            .map(|(path, diagnostics)| {
                let relative = path
                    .strip_prefix(root)
                    .map_or_else(|_| path.clone(), Path::to_path_buf);
                (relative, diagnostics)
            })
            .collect(),
        None => results,
    };
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

/// A file read and parsed: its text, as the insert keeps it, and its parse.
struct ReadFile {
    text: FileText,
    parsed: ParsedFile,
    /// Where the parse is stored once it is complete, for a file that was
    /// not read back from the per-file cache.
    cache_key: Option<file_cache::Key>,
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

/// The declarations of a file that was read, as `Project::insert_declarations`
/// takes them: none for a file that did not load.
fn declaration_of<'a>(
    path: &'a Path,
    analyzed: bool,
    read: &'a Read,
) -> Option<(&'a Path, bool, &'a [BuiltDecl])> {
    match read {
        Read::Loaded(file) => Some((path, analyzed, file.parsed.built_decls())),
        Read::Unparsed(_) | Read::Unread => None,
    }
}

/// Gives each loaded file in `parsed` the number at its place in `numbers`,
/// the two being in the same order.
fn bind_numbers(parsed: &mut [Read], numbers: &[FileId]) {
    debug_assert_eq!(parsed.len(), numbers.len(), "every file has a number");
    for (read, &id) in parsed.iter_mut().zip(numbers) {
        if let Read::Loaded(file) = read {
            file.parsed.bind_file(id);
        }
    }
}

impl Session {
    /// Resolves `config`, whose relative paths are relative to
    /// `config_dir` (or to `root` without one), and the command-line
    /// `overrides` against `root`, the directory the run's relative paths
    /// are relative to. What `policy` says of a configuration that cannot be
    /// used is in `ErrorPolicy`.
    pub fn resolve(
        config: &ConfigFile,
        config_dir: Option<PathBuf>,
        root: PathBuf,
        overrides: Overrides,
        stubs: Stubs,
        policy: ErrorPolicy,
    ) -> Result<Session, String> {
        let mut warnings: Vec<String> = config.warnings().to_vec();
        let session = section::<SessionSettings>(config, policy, &mut warnings)?;
        let reflection = section::<ReflectionSettings>(config, policy, &mut warnings)?;
        let engine = section::<EngineSettings>(config, policy, &mut warnings)?;
        let overlays = section::<OverlaySettings>(config, policy, &mut warnings)?;
        let rules = section::<RulesSettings>(config, policy, &mut warnings)?;
        let banned_settings = section::<BannedSettings>(config, policy, &mut warnings)?;
        let architecture = section::<ArchitectureSettings>(config, policy, &mut warnings)?;
        let class_shape = section::<ClassShapeSettings>(config, policy, &mut warnings)?;
        let disallowed_calls = section::<DisallowedCallsSettings>(config, policy, &mut warnings)?;
        let sql_settings = section::<SqlSettings>(config, policy, &mut warnings)?;
        let mockstan = section::<MockstanSettings>(config, policy, &mut warnings)?;
        let psr3 = section::<ext::psr3::Psr3Settings>(config, policy, &mut warnings)?;

        let resolve_against_config = |p: PathBuf| match &config_dir {
            Some(dir) => dir.join(p),
            None => p,
        };
        let mut excluded_dir_names: &'static [&'static str] = &[];
        let mut dirs: Vec<PathBuf> = if !overrides.paths.is_empty() {
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
            match policy {
                ErrorPolicy::Fail => {
                    return Err(
                        "no paths given on the command line or in the config file".to_string()
                    );
                }
                ErrorPolicy::Degrade => {
                    excluded_dir_names = DEFAULT_SCAN_EXCLUDED_DIR_NAMES;
                    dirs.push(root.clone());
                }
            }
        }
        let configured_level = overrides.level.or(session.level);
        let level = overrides
            .report_level
            .or(configured_level)
            .unwrap_or(DEFAULT_REPORT_LEVEL);
        let psr3 = match psr3.context_key_pattern() {
            Ok(_) => psr3,
            Err(error) => {
                report(error, policy, &mut warnings)?;
                ext::psr3::Psr3Settings::default()
            }
        };
        let php_version = settle(
            overrides
                .php_version
                .clone()
                .or(reflection.php_version.clone())
                .map(|s| types::parse_php_version(&s))
                .transpose(),
            policy,
            &mut warnings,
            None,
        )?;
        let mut restricted_symbols = disallowed_calls.restricted_symbols.clone();
        for preset in &disallowed_calls.restricted_symbol_presets {
            match banned::restricted_symbol_preset(preset) {
                Ok(entries) => restricted_symbols.extend(entries),
                Err(error) => report(error, policy, &mut warnings)?,
            }
        }
        let mut banned_constructs = Vec::new();
        for name in &banned_settings.constructs {
            match banned::BannedConstruct::parse(name) {
                Ok(construct) => banned_constructs.push(construct),
                Err(error) => report(error, policy, &mut warnings)?,
            }
        }
        let overlays = match overlays.validate() {
            Ok(()) => overlays,
            Err(error) => {
                report(error, policy, &mut warnings)?;
                OverlaySettings::default()
            }
        };
        let parent_call_rules = class_shape.rules();
        let mut architecture_rules = Vec::new();
        for rule in architecture.0 {
            match rule.validate() {
                Ok(()) => architecture_rules.push(rule),
                Err(error) => report(error, policy, &mut warnings)?,
            }
        }
        let sql_reflection = sql_settings.reflection;
        let sql_schema = match sql_reflection
            .as_ref()
            .and_then(|c| c.schema_file.as_ref().map(|f| (f.clone(), c.driver)))
        {
            Some((path, driver)) => {
                let path = resolve_against_config(path);
                settle(
                    sql::schema::parse_ddl_file(&root.join(path), driver).map(Some),
                    policy,
                    &mut warnings,
                    None,
                )?
            }
            None => None,
        };
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
        let mut scan_files: Vec<PathBuf> = Vec::new();
        let dir_listings = Arc::new(DirListings::default());
        let mut vendors = Vec::new();
        let mut promoted: Vec<PathBuf> = Vec::new();
        let mut phpstan_api_check_enabled = false;
        let mut composer_manifests = Vec::new();
        let absolute_root = composer::manifest::resolve(&root);
        let mut composer_php_versions = None;
        let mut root_manifest = None;
        if !no_composer {
            for composer_root in composer_roots(&dirs, &absolute_root) {
                // The manifest's paths are resolved, as the files they are
                // compared with are.
                let package =
                    composer::PackageContext::load(&composer_root, &composer_root, &dir_listings);
                if composer_root == absolute_root {
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
                // Relative scan paths are relative to the root, and resolved as
                // the promoted directories are.
                let is_promoted = |p: &Path| {
                    let p = composer::manifest::resolve(&absolute_root.join(p));
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
        let extensions = settle(
            ext::ActiveExtensions::resolve(config, &|p| {
                ext::installed_in(root_manifest.as_ref(), p)
            }),
            policy,
            &mut warnings,
            ext::ActiveExtensions::default(),
        )?;
        let baseline = overrides
            .baseline
            .or(session.baseline.clone().map(resolve_against_config));
        let baseline_mode =
            if overrides.baseline_match_identifier || session.baseline_match_identifier {
                crate::baseline::MatchMode::Identifier
            } else {
                crate::baseline::MatchMode::Message
            };
        let baseline_per_identifier =
            overrides.baseline_per_identifier || session.baseline_per_identifier;

        let settings = ProjectSettings {
            absolute_root: Some(absolute_root.clone()),
            packages: composer::PackageIndex::new(composer_manifests),
            php_versions,
            configured_level,
            phpstan_api_check_enabled,
            retain_source: false,
            built_once: true,
            rules,
            extensions,
            extras: section::<ext::extras::ExtrasSettings>(config, policy, &mut warnings)?,
            strict_rules: section::<ext::strict_rules::StrictRulesSettings>(
                config,
                policy,
                &mut warnings,
            )?,
            shipmonk: section::<ext::shipmonk::ShipmonkSettings>(config, policy, &mut warnings)?,
            voku: section::<ext::voku::VokuSettings>(config, policy, &mut warnings)?,
            slam: section::<ext::slam::SlamSettings>(config, policy, &mut warnings)?,
            psr3,
            sidz: section::<ext::sidz::SidzSettings>(config, policy, &mut warnings)?,
            phpunit: section::<ext::phpunit::PhpunitSettings>(config, policy, &mut warnings)?,
            restricted_symbols,
            doctrine: section::<ext::doctrine::DoctrineSettings>(config, policy, &mut warnings)?,
            dynamic_constant_names: engine.dynamic_constant_entries(),
            overlays,
            sql_reflection,
            sql_schema,
        };

        Ok(Session {
            source: FileSource::default(),
            overlay_listing: None,
            pool: Arc::clone(stubs.pool()),
            stubs,
            root,
            absolute_root,
            listing: OnceLock::new(),
            aliases: Mutex::default(),
            session,
            settings,
            dirs,
            scan_dirs,
            scan_files,
            vendors,
            promoted,
            dir_listings,
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
            excluded_dir_names,
            baseline,
            baseline_mode,
            baseline_per_identifier,
            warnings,
        })
    }

    /// A fixture's session: `stem.php` (or the directory `stem/`) analysed
    /// under `config`, the fixture's `stem.toml` if it has one (its paths
    /// relative to the fixture's directory), with composer discovery off
    /// unless the config turns it on, reporting every level, running on
    /// the pool of `stubs`. Returns the session and that directory.
    pub fn for_fixture(
        stem: &Path,
        config: &ConfigFile,
        stubs: Stubs,
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
        let session = Session::resolve(
            config,
            None,
            base.clone(),
            overrides,
            stubs,
            ErrorPolicy::Fail,
        )?;
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

    /// The baseline the run reports against, as the config or the command
    /// line names it.
    pub(crate) fn baseline_path(&self) -> Option<&Path> {
        self.baseline.as_deref()
    }

    pub(crate) fn baseline_mode(&self) -> crate::baseline::MatchMode {
        self.baseline_mode
    }

    pub(crate) fn baseline_per_identifier(&self) -> bool {
        self.baseline_per_identifier
    }

    /// The config's `ignore_errors` entries.
    pub(crate) fn ignore_error_entries(&self) -> &[ignore_errors::IgnoreErrorEntry] {
        &self.session.ignore_errors
    }

    /// The settings a language server's project is configured with: this
    /// session's, except for the two that describe how the server keeps
    /// its files. The server keeps each file's source and its context
    /// trees, and applies inline ignores when it publishes.
    pub(crate) fn editor_settings(&self) -> ProjectSettings {
        ProjectSettings {
            retain_source: true,
            built_once: false,
            ..self.settings.clone()
        }
    }

    /// Every file the configuration lists, once, under its resolved path,
    /// with its role and whether it is a stub. The command line and the
    /// language server both load from this listing.
    pub(crate) fn listing(&self) -> Vec<Listed> {
        self.listing_of().files.clone()
    }

    /// The listing, walked on first use and kept for the session. The directory
    /// listings the walk and the vendor discovery read are released once it is
    /// walked.
    fn listing_of(&self) -> &Listing {
        self.listing_with(&Unstreamed)
    }

    /// The listing, walked on first use with `sink` taking the files whose roles
    /// are final as the walk lists them. A listing walked already is kept, and
    /// `sink` is given none of its files.
    fn listing_with<S: FinalFiles>(&self, sink: &S) -> &Listing {
        self.listing.get_or_init(|| {
            let listing = self.walk_listing(sink);
            self.dir_listings.clear();
            listing
        })
    }

    fn walk_listing<S: FinalFiles>(&self, sink: &S) -> Listing {
        if let Some(listed) = &self.overlay_listing {
            return Listing {
                files: listed.clone(),
                index: FileIndex::of(listed),
                roots: Vec::new(),
            };
        }
        let mut walk = Walk::new(Arc::clone(&self.dir_listings), &self.settings.packages);
        for dir in &self.dirs {
            let excluded: &'static [&'static str] = if self.promoted.contains(dir) {
                &[]
            } else {
                self.excluded_dir_names
            };
            walk.add(&self.root.join(dir), Role::Analysed, excluded);
        }
        // The vendor context was being found while the analysed files were
        // listed.
        for vendor in &self.vendors {
            let found = vendor.get();
            for dir in found.dirs.iter().filter(|d| !self.is_promoted(d)) {
                walk.add(&self.root.join(dir), Role::Context, &[]);
            }
            for file in found.files.iter().filter(|f| !self.is_promoted(f)) {
                walk.add_file(&self.root.join(file), Role::Context);
            }
        }
        for file in &self.scan_files {
            walk.add_file(&self.root.join(file), Role::Context);
        }
        for dir in &self.scan_dirs {
            walk.add(&self.root.join(dir), Role::Context, &[]);
        }
        for dir in &self.stub_dirs {
            walk.add(&self.root.join(dir), Role::Stub, &[]);
        }
        let Walked {
            files,
            index,
            roots,
        } = walk.finish(&self.pool, sink);
        Listing {
            files,
            index,
            roots,
        }
    }

    /// The resolved path of `path`, which is the key its file has in the
    /// project (see `identify_through`).
    pub(crate) fn identify(&self, path: &Path) -> PathBuf {
        identify_through(&self.aliases, path)
    }

    /// The file `path` names, resolved, and the role it plays. A file the
    /// listing holds has the role the walk gave it. Otherwise it has the
    /// strongest role of the roots that list it, a root listing it only when
    /// none of the directories between the root and the file is excluded
    /// there. A file no root lists is analysed, unless a directory the walk
    /// excludes by default holds it.
    pub(crate) fn classify(&self, path: &Path) -> Classified {
        let path = self.identify(path);
        let listing = self.listing_of();
        if let Some(at) = listing.index.position(&listing.files, &path) {
            let listed = &listing.files[at];
            return Classified {
                path,
                role: listed.role,
                is_stub: listed.stub,
            };
        }
        let mut role = None;
        let mut is_stub = false;
        for root in listing.roots.iter().filter(|root| root.contains(&path)) {
            role = Some(role.map_or(root.role, |role: Role| role.max(root.role)));
            is_stub |= root.stub;
        }
        let role = role.unwrap_or_else(|| unlisted_role(&path, Some(self.absolute_root.as_path())));
        Classified {
            path,
            role,
            is_stub,
        }
    }

    /// Whether the configuration reaches `path`, which is identified already:
    /// the listing holds it, or a root the walk gave holds it, as a file
    /// created since the walk is.
    pub(crate) fn reaches(&self, path: &Path) -> bool {
        let listing = self.listing_of();
        listing.index.position(&listing.files, path).is_some()
            || listing.roots.iter().any(|root| root.contains(path))
    }

    fn configure(&self, project: &mut Project) {
        project.apply_settings(self.settings.clone());
        project.set_source(self.source.clone());
    }

    /// Whether `path` (relative to the root) is in a package promoted to an
    /// analysed path.
    fn is_promoted(&self, path: &Path) -> bool {
        if self.promoted.is_empty() {
            return false;
        }
        let path = self.absolute_root.join(path);
        self.promoted
            .iter()
            .any(|dir| composer::manifest::path_starts_with(&path, dir))
    }

    /// `cacheable` is whether the file may be read from and stored in the
    /// per-file cache, which a context file may be in a project built once;
    /// `index_use_sites` is whether its use sites are indexed with its parse.
    /// The file's role says whether it is analysed, which keeps its ignore
    /// comments, and whether its text is kept (`Project::keeps_source`).
    fn read_and_parse_file(
        &self,
        project: &Project,
        listed: &Listed,
        worker: &mut Worker,
        cacheable: bool,
        index_use_sites: bool,
    ) -> Read {
        let path = listed.path.as_path();
        let is_stub = listed.stub;
        let analysed = listed.role == Role::Analysed;
        let Worker { names, bytes } = worker;
        if project.read_into(path, bytes).is_err() {
            return Read::Unread;
        }
        let bytes: &[u8] = bytes;
        let content_hash = project
            .retain_source
            .then(|| super::source::content_hash(bytes));
        let keeps_source = project.keeps_source(analysed);
        let root = Some(self.absolute_root.as_path());
        let cache_key = if cacheable {
            file_cache::key(bytes, path, root, is_stub, index_use_sites)
        } else {
            None
        };
        if let Some(key) = &cache_key
            && let Some(cached) = file_cache::load(key)
        {
            return Read::Loaded(Box::new(ReadFile {
                text: FileText {
                    len: cached.len,
                    source: keeps_source.then(|| super::source::decode_text(bytes).into_owned()),
                    content_hash,
                },
                parsed: cached.into_parsed(bytes),
                cache_key: None,
            }));
        }
        // Whether the file is UTF-8 is settled here, once. Its text is the bytes
        // when they are, which the lexer reads in place, and a lossy reading of
        // them when they are not, whose length is what checking weighs the file
        // by. A file that is not UTF-8 is parsed from its bytes.
        let text = super::source::decode_text(bytes);
        let len = text.len();
        // A context file's bodies are kept as text. They are not parsed
        // when the load of the file reads nothing in them, unless the use
        // sites of the file are indexed from its tree.
        let body_text = match (cacheable, index_use_sites) {
            (false, _) => super::BodyText::Parsed,
            (true, true) => super::BodyText::AsText,
            (true, false) => super::BodyText::Skipped,
        };
        let Ok(parsed) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match &text {
            Cow::Borrowed(text) => super::parse_file_with_names(
                path,
                root,
                *text,
                is_stub,
                body_text,
                Some(names),
                analysed,
            ),
            Cow::Owned(_) => super::parse_file_with_names(
                path,
                root,
                bytes,
                is_stub,
                body_text,
                Some(names),
                analysed,
            ),
        })) else {
            return Read::Unread;
        };
        let mut parsed = match parsed {
            Ok(parsed) => parsed,
            Err(error) => {
                let at = syntax::LineIndex::new(bytes).line_col(error.span.start);
                return Read::Unparsed(ParseFailure {
                    line: at.line,
                    col: at.col,
                    message: error.message,
                });
            }
        };
        // Only the reader holds the text, so the use sites are indexed here.
        project.index_use_sites(&mut parsed, &text);
        let source = keeps_source.then(|| text.into_owned());
        Read::Loaded(Box::new(ReadFile {
            text: FileText {
                len,
                source,
                content_hash,
            },
            parsed,
            cache_key,
        }))
    }

    /// `read_and_parse_file`, then the file's entry in the per-file cache.
    fn read_indexed(
        &self,
        project: &Project,
        listed: &Listed,
        worker: &mut Worker,
        cacheable: bool,
        index_use_sites: bool,
    ) -> Read {
        let mut read =
            self.read_and_parse_file(project, listed, worker, cacheable, index_use_sites);
        if worker.bytes.capacity() > KEPT_READ_CAPACITY {
            worker.bytes = Vec::new();
        }
        if let Read::Loaded(file) = &mut read {
            file.parsed.owner = listed.owner;
            if let Some(key) = file.cache_key.take()
                && file_cache::can_store()
            {
                file_cache::store(
                    &key,
                    &file_cache::CachedFile::of(&file.parsed, file.text.len),
                );
            }
            // A tree the project does not keep is dropped by the thread that
            // parsed it.
            if !project.keeps_tree(listed.role == Role::Analysed) {
                file.parsed.ast = None;
            }
        }
        read
    }

    /// Reads, parses and inserts `listed` into `project`, which may hold
    /// other files already: context files first and then analysed ones, each
    /// group in path order, as `load` loads them. The files are read and
    /// parsed in parallel on the pool. A file that cannot be read is skipped;
    /// one that does not parse is skipped, and returned with its message.
    pub(crate) fn load_listed(
        &self,
        project: &mut Project,
        listed: &[Listed],
    ) -> Vec<(PathBuf, String)> {
        debug_assert!(
            listed.windows(2).all(|pair| pair[0].path <= pair[1].path),
            "the files are given in path order, as a listing is"
        );
        let (analysed, context): (Vec<&Listed>, Vec<&Listed>) =
            listed.iter().partition(|file| file.role == Role::Analysed);
        let context_count = context.len();
        let ordered: Vec<&Listed> = context.into_iter().chain(analysed).collect();
        let index_use_sites = project.indexes_class_fn_sites();
        // A cached file has no tree. A context file keeps its tree unless the
        // project is built once, so only such a project reads the cache.
        let built_once = project.built_once;
        let shared: &Project = project;
        let workers = Workers::new(self.pool.current_num_threads());
        let (mut parsed, numbering): (Vec<Read>, _) = self.pool.install(|| {
            rayon::join(
                || {
                    ordered
                        .par_iter()
                        .map(|file| {
                            workers.with(|worker| {
                                self.read_indexed(
                                    shared,
                                    file,
                                    worker,
                                    built_once && file.role != Role::Analysed,
                                    index_use_sites,
                                )
                            })
                        })
                        .collect()
                },
                || {
                    super::file_table::FileTable::with_paths(
                        ordered
                            .iter()
                            .map(|file| (file.path.as_path(), file.path.hash())),
                    )
                },
            )
        });
        file_cache::trim();
        let numbers = project.number_files(numbering);
        bind_numbers(&mut parsed, &numbers);
        let (context_numbers, analysed_numbers) = numbers.split_at(context_count);
        project.set_path_order(&ids_in_listed_order(
            listed,
            context_numbers,
            analysed_numbers,
        ));
        project.expect_files(
            parsed.iter().filter_map(|read| match read {
                Read::Loaded(file) => Some(&file.parsed),
                _ => None,
            }),
            &self.stubs,
        );
        project.insert_declarations(
            &self.pool,
            ordered.iter().zip(&parsed).filter_map(|(file, read)| {
                declaration_of(file.path.as_path(), file.role == Role::Analysed, read)
            }),
        );
        project.insert_file_tables(
            &self.pool,
            numbers
                .iter()
                .zip(&ordered)
                .zip(parsed.iter_mut())
                .filter_map(|((&id, file), read)| match read {
                    Read::Loaded(loaded) => Some((id, file.path.as_path(), &mut loaded.parsed)),
                    _ => None,
                }),
        );
        let mut failures = Vec::new();
        for ((file, read), id) in ordered.iter().zip(parsed).zip(numbers) {
            match read {
                Read::Loaded(loaded) => {
                    let ReadFile { text, parsed, .. } = *loaded;
                    project.insert_numbered_file(
                        id,
                        file.path.as_path(),
                        parsed,
                        text,
                        file.role == Role::Analysed,
                    );
                }
                Read::Unparsed(failure) => {
                    failures.push((file.path.as_path().to_path_buf(), failure.message))
                }
                Read::Unread => {}
            }
        }
        failures
    }

    /// Loads the files this session lists into `project`, which keeps its own
    /// settings, then finishes its loading. A file that does not parse is an
    /// error. The files are read from this session's source.
    pub fn load_into(&self, project: &mut Project) -> Result<(), String> {
        project.set_source(self.source.clone());
        if let Some((path, message)) = self.load_listed(project, &self.listing_of().files).first() {
            let shown = super::shown_path(Some(self.absolute_root.as_path()), path);
            return Err(format!("{} does not parse: {message}", shown.display()));
        }
        project.finish_loading_with(&self.stubs);
        Ok(())
    }

    /// A session over `files` alone, read from memory, with no listing of
    /// the disk. Its configuration is the default one. The files load with
    /// the roles they are given, which `load` does not require to include an
    /// analysed one.
    pub fn for_overlay(files: Vec<OverlayFile>, stubs: Stubs) -> Session {
        let mut session = Session::resolve(
            &ConfigFile::empty(),
            None,
            PathBuf::new(),
            Overrides {
                no_composer: true,
                ..Overrides::default()
            },
            stubs,
            ErrorPolicy::Degrade,
        )
        .expect("the default configuration resolves");
        let mut listed = Vec::with_capacity(files.len());
        for file in files {
            session.source.set(&file.path, &file.bytes);
            let role = if file.analysed {
                Role::Analysed
            } else {
                Role::Context
            };
            listed.push(Listed::new(file.path, role, file.stub));
        }
        composer::path_order::sort_by_path(&mut listed, |file| file.path.as_path());
        session.overlay_listing = Some(listed);
        session
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
        let mut project = Project::loading();
        self.configure(&mut project);
        let workers = Workers::new(self.pool.current_num_threads());
        let index_use_sites = project.indexes_class_fn_sites();
        let streamed = StreamParse {
            session: self,
            project: &project,
            workers: &workers,
            index_use_sites,
            parsed: Mutex::default(),
        };
        let listing = self.listing_with(&streamed);
        let mut analyze_files: Vec<&Listed> = Vec::new();
        let mut context_files: Vec<&Listed> = Vec::new();
        // The listing is in path order, so each role's files are too.
        for listed in &listing.files {
            if listed.role == Role::Analysed {
                analyze_files.push(listed);
            } else {
                context_files.push(listed);
            }
        }
        if analyze_files.is_empty() && self.overlay_listing.is_none() {
            let dirs: Vec<String> = self.dirs.iter().map(|d| d.display().to_string()).collect();
            return Err(format!("no .php files found under {}", dirs.join(", ")));
        }

        let pool = &self.pool;

        let combined_files: Vec<&Listed> = context_files
            .iter()
            .chain(analyze_files.iter())
            .copied()
            .collect();
        // The files the walk handed over are parsed already, under the role
        // they have in the listing. The others are parsed below.
        let mut parsed = streamed.into_parsed();
        let mut slots: Vec<Option<Read>> = combined_files
            .iter()
            .map(|file| {
                parsed.remove(file.path.as_path()).map(|(role, read)| {
                    debug_assert_eq!(role, file.role, "a file is parsed under its listed role");
                    read
                })
            })
            .collect();
        debug_assert!(parsed.is_empty(), "the walk hands over only listed files");
        let deferred: Vec<usize> = (0..slots.len()).filter(|&at| slots[at].is_none()).collect();
        // The files are numbered while they parse.
        let (deferred_parsed, numbering): (Vec<(usize, Read)>, _) = pool.install(|| {
            rayon::join(
                || {
                    deferred
                        .par_iter()
                        .map(|&at| {
                            let read = workers.with(|worker| {
                                self.read_indexed(
                                    &project,
                                    combined_files[at],
                                    worker,
                                    // The context files come first.
                                    at < context_files.len(),
                                    index_use_sites,
                                )
                            });
                            (at, read)
                        })
                        .collect()
                },
                || {
                    super::file_table::FileTable::with_paths(
                        combined_files
                            .iter()
                            .map(|file| (file.path.as_path(), file.path.hash())),
                    )
                },
            )
        });
        for (at, read) in deferred_parsed {
            slots[at] = Some(read);
        }
        let mut combined_parsed: Vec<Read> = slots
            .into_iter()
            .map(|slot| slot.expect("every listed file is parsed"))
            .collect();
        file_cache::trim();
        let numbers = project.number_files(numbering);
        bind_numbers(&mut combined_parsed, &numbers);
        let (context_numbers, analyze_numbers) = numbers.split_at(context_files.len());
        project.set_path_order(&ids_in_listed_order(
            &listing.files,
            context_numbers,
            analyze_numbers,
        ));
        let mut analyze_parsed = combined_parsed.split_off(context_files.len());
        let mut context_parsed = combined_parsed;
        project.expect_files(
            context_parsed
                .iter()
                .chain(&analyze_parsed)
                .filter_map(|read| match read {
                    Read::Loaded(file) => Some(&file.parsed),
                    _ => None,
                }),
            &self.stubs,
        );

        // Context files first, so an analysed file's declaration wins a
        // name collision.
        project.insert_declarations(
            pool,
            context_files
                .iter()
                .zip(&context_parsed)
                .filter_map(|(file, read)| declaration_of(file.path.as_path(), false, read))
                .chain(
                    analyze_files
                        .iter()
                        .zip(&analyze_parsed)
                        .filter_map(|(file, read)| declaration_of(file.path.as_path(), true, read)),
                ),
        );
        project.insert_file_tables(
            pool,
            context_files
                .iter()
                .zip(context_numbers)
                .zip(&mut context_parsed)
                .chain(
                    analyze_files
                        .iter()
                        .zip(analyze_numbers)
                        .zip(&mut analyze_parsed),
                )
                .filter_map(|((file, &id), read)| match read {
                    Read::Loaded(loaded) => Some((id, file.path.as_path(), &mut loaded.parsed)),
                    _ => None,
                }),
        );
        let mut lex_warnings = Vec::new();
        let mut note_lex_warnings = |path: &Path, parsed: &ParsedFile| {
            lex_warnings.extend(parsed.lex_warnings().iter().map(|w| {
                format!(
                    "warning: {}:{}:{}: {}",
                    super::shown_path(Some(self.absolute_root.as_path()), path).display(),
                    parsed.line_index.line_col(w.span.start).line,
                    parsed.line_index.line_col(w.span.start).col,
                    w.message
                )
            }));
        };
        let mut skipped_context = 0usize;
        let mut parse_failure_lines = Vec::new();
        for ((file, &id), parsed) in context_files
            .iter()
            .zip(context_numbers)
            .zip(context_parsed)
        {
            let path = file.path.as_path();
            match parsed {
                Read::Loaded(loaded) => {
                    let ReadFile { text, parsed, .. } = *loaded;
                    note_lex_warnings(path, &parsed);
                    project.insert_numbered_file(id, path, parsed, text, false)
                }
                Read::Unparsed(failure) => {
                    skipped_context += 1;
                    parse_failure_lines.push(format!(
                        "warning: {}:{}:{}: {}",
                        super::shown_path(Some(self.absolute_root.as_path()), path).display(),
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
        for (index, ((file, &id), parsed)) in analyze_files
            .iter()
            .zip(analyze_numbers)
            .zip(analyze_parsed)
            .enumerate()
        {
            let path = file.path.as_path();
            match parsed {
                Read::Loaded(loaded) => {
                    let ReadFile { text, parsed, .. } = *loaded;
                    note_lex_warnings(path, &parsed);
                    analyze_sizes[index] = text.len;
                    project.insert_numbered_file(id, path, parsed, text, true)
                }
                Read::Unparsed(failure) => {
                    skipped_analyze += 1;
                    parse_failures.push((path.to_path_buf(), failure));
                }
                Read::Unread => skipped_analyze += 1,
            }
        }
        lex_warnings.extend(parse_failure_lines);
        project.finish_loading_with(&self.stubs);

        // Largest files first, so the longest checks don't start last; of
        // equal ones, the first listed first.
        let mut by_size: Vec<usize> = (0..analyze_files.len()).collect();
        by_size.sort_by_key(|&index| std::cmp::Reverse(analyze_sizes[index]));
        let analyze_files_by_size = by_size
            .into_iter()
            .map(|index| analyze_files[index].path.as_path().to_path_buf())
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

        let checks = self.check_project(&mut project, &analyze_files_by_size, dump_types);

        let mut results = checks.per_file;
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
        for (path, diags) in checks.whole_program {
            results.entry(path).or_default().extend(diags);
        }

        AnalysisResult {
            project,
            results,
            checked: analyze_files_by_size.len() - skipped_analyze - checks.check_panics.len(),
            skipped_analyze,
            skipped_context,
            context_loaded,
            check_panics: checks
                .check_panics
                .into_iter()
                .map(|(path, message)| {
                    let shown = super::shown_path(Some(self.absolute_root.as_path()), &path);
                    (shown.to_path_buf(), message)
                })
                .collect(),
            lex_warnings,
            type_dump: checks.type_dump,
        }
    }

    /// Checks `files`, the analysed paths of `project`, then runs the
    /// project-wide passes over what they found. `project` must be loaded
    /// and finished.
    pub(crate) fn check_project(
        &self,
        project: &mut Project,
        files: &[PathBuf],
        dump_types: bool,
    ) -> ProjectChecks {
        let needs_tier_2 =
            self.settings.rules.check_dead_enum_cases || self.settings.rules.check_dead_members;
        let checked_results =
            project.check_files(files, Some(&self.pool), needs_tier_2, dump_types);

        let mut per_file: FxHashMap<PathBuf, Vec<Diagnostic>> = FxHashMap::default();
        let mut check_panics: Vec<(PathBuf, String)> = Vec::new();
        // Only the rule that reads named-argument facts is given them.
        let check_renamed_arguments = self.settings.rules.check_renamed_named_arguments;
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
                    if check_renamed_arguments {
                        named_argument_calls.push((path.clone(), check.named_argument_calls));
                    }
                    if dump_types {
                        type_dumps.push((path.clone(), check.type_dump));
                    }
                    per_file.insert(path, check.diagnostics);
                }
                Err(msg) => check_panics.push((path, msg)),
            }
        }
        check_panics.sort();
        named_argument_calls.sort_by_cached_key(|(path, _)| project.path_rank(path));
        let type_dump = dump_types.then(|| {
            type_dumps.sort_by(|a, b| a.0.cmp(&b.0));
            let mut text = String::new();
            for (path, lines) in type_dumps {
                let shown = super::shown_path(Some(self.absolute_root.as_path()), &path);
                for line in lines {
                    text.push_str(&format!("{}:{line}\n", shown.display()));
                }
            }
            text
        });
        let named_argument_calls = merge_named_argument_calls(named_argument_calls);
        if needs_tier_2 {
            project.mark_member_index_built();
        }

        let mut whole_program: FxHashMap<PathBuf, Vec<Diagnostic>> = FxHashMap::default();
        let mut extend = |found: FxHashMap<PathBuf, Vec<Diagnostic>>| {
            for (path, diags) in found {
                whole_program.entry(path).or_default().extend(diags);
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

        ProjectChecks {
            per_file,
            whole_program,
            check_panics,
            type_dump,
        }
    }
}

/// The ids of the files `listed` lists, in the order it lists them. `context`
/// and `analysed` hold the ids of each role's files, in that same order.
fn ids_in_listed_order(listed: &[Listed], context: &[FileId], analysed: &[FileId]) -> Vec<FileId> {
    let mut context = context.iter();
    let mut analysed = analysed.iter();
    listed
        .iter()
        .map(|file| {
            let next = if file.role == Role::Analysed {
                analysed.next()
            } else {
                context.next()
            };
            *next.expect("every listed file has an id")
        })
        .collect()
}

/// Loads `files`, each `(path, source, analysed)`, into `project` as one
/// batch, the way `Session::load` loads a listing. Returns the files that do
/// not parse, with their messages.
#[cfg(test)]
pub(crate) fn load_test_files(
    project: &mut Project,
    files: &[(PathBuf, &str, bool)],
) -> Vec<(PathBuf, String)> {
    let mut listed = Vec::with_capacity(files.len());
    for (path, source, analysed) in files {
        project.set_document(path, source.as_bytes());
        let role = if *analysed {
            Role::Analysed
        } else {
            Role::Context
        };
        listed.push(Listed::new(path.clone(), role, false));
    }
    composer::path_order::sort_by_path(&mut listed, |file| file.path.as_path());
    Session::for_overlay(Vec::new(), Stubs::shared().clone()).load_listed(project, &listed)
}

/// Loads `source` as the file at `path` into `project`: an analysed file when
/// `analysed`, else a context file. The file must parse.
#[cfg(test)]
pub(crate) fn load_test_file(project: &mut Project, path: PathBuf, source: &str, analysed: bool) {
    if let Some((path, message)) = load_test_files(project, &[(path, source, analysed)]).first() {
        panic!("{} does not parse: {message}", path.display());
    }
}
