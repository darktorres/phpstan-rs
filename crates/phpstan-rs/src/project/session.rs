//! The `check-project` pipeline as a library: a config file's sections
//! (plus command-line overrides) resolved against an explicit root
//! directory, loaded into a
//! `Project`, checked in parallel into an `AnalysisResult`, which
//! `finalize` post-processes into the per-file diagnostics a consumer
//! reports. The CLI and the fixture harness both drive it, so a fixture
//! exercises exactly what `check-project` runs.

use rustc_hash::FxHashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

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
use crate::file_id::FileId;
use crate::parent_calls::{self, ClassShapeSettings};
use crate::registry::settings::ReflectionSettings;
use crate::rules_settings::RulesSettings;
use crate::{ext, ignore_errors};

use super::DEFAULT_SCAN_EXCLUDED_DIR_NAMES;
use super::file_cache;
use super::source::{ContentHash, Overlay};
use super::{NumberedFile, ParsedFile, Project, ProjectSettings};
use composer::dir_table::DirTable;
use diagnostics::{self, Diagnostic, DiagnosticBuilder, LevelCutoff};

/// The level reported when neither the command line nor the config sets
/// one: every level this project's diagnostics are tagged with.
pub const DEFAULT_REPORT_LEVEL: u8 = 6;

/// The cutoff of the configured level `configured`; with none, rules up
/// to `unconfigured_rule` are reported, their type acceptances judged at
/// `types::UNCONFIGURED_RULE_LEVEL`.
pub fn configured_cutoff(configured: Option<u8>, unconfigured_rule: u8) -> LevelCutoff {
    match configured {
        Some(level) => LevelCutoff::at(level),
        None => LevelCutoff {
            rule: unconfigured_rule,
            found: types::UNCONFIGURED_RULE_LEVEL,
        },
    }
}

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
    /// Reports the rules up to this level without configuring it (the
    /// level also judges the type acceptances): the fixture harness
    /// records every rule level at once this way.
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
    /// The directory names the analysed paths leave out.
    pruned: &'static [&'static str],
    scan_dirs: Vec<PathBuf>,
    scan_files: Vec<PathBuf>,
    /// The vendor files and directories of the composer roots, still being
    /// found while the analysed files are listed, and the packages
    /// promoted to analysed paths, which are left out of them.
    vendors: Vec<composer::PendingVendor>,
    promoted: Vec<PathBuf>,
    /// The directories the vendor discovery listed, which the first walk
    /// reads too; each walk after it lists the directories again, which
    /// may have changed.
    discovery_listings: Mutex<Option<Arc<composer::dir_table::DirTable>>>,
    cutoff: LevelCutoff,
    emission_order: bool,
    banned_constructs: Vec<banned::BannedConstruct>,
    banned_types: Vec<String>,
    check_closed_classes: bool,
    parent_call_rules: Vec<parent_calls::ParentCallRule>,
    architecture_rules: Vec<architecture::ArchitectureRule>,
    check_mock_hygiene: bool,
    mock_banned_classes: Vec<String>,
    /// The files read in place of the disk's.
    overlay: Overlay,
    /// Non-fatal problems found while resolving (e.g. SQL schema drift,
    /// an old config key).
    pub warnings: Vec<String>,
}

/// The most a worker keeps of its read buffer between files: a larger file's
/// bytes are given back.
const MAX_KEPT_BUFFER: usize = 1 << 20;

/// What a thread of a load keeps while it parses one file after another:
/// the names it meets, which are dropped with the load, the bytes of the
/// file it is reading, and what it found out about each directory it has
/// met.
#[derive(Default)]
struct LoadState {
    names: syntax::name::NameCache,
    /// Node and span vectors reused when a parsed file's tree is dropped.
    scratch: syntax::ast::NodeArenas,
    /// Flat token arrays and parser temporaries reused across this load.
    tokens: syntax::token::TokenStream,
    /// Per-file name table reused by the lexer across this load.
    lexer: syntax::lexer::LexerScratch,
    parser: syntax::parser::ParserScratch,
    /// The bytes of the file the thread is parsing, kept for the next.
    buf: Vec<u8>,
    dirs: rustc_hash::FxHashMap<Vec<u8>, Placed>,
}

/// How `read_and_parse_file` treats one file.
struct ParseOptions {
    /// Whether the file may be read from and stored in the per-file cache,
    /// which every file may that no editor holds.
    cacheable: bool,
    /// Whether the file keeps its tree, which an entry does not hold: a hit
    /// parses the file again and takes everything else from the entry.
    needs_tree: bool,
    /// Whether its use sites are indexed with its parse.
    index_use_sites: bool,
    /// Whether the parse takes the thread's node arenas, which a file that
    /// keeps its tree can't give back.
    reuse_scratch: bool,
}

/// The states of a load's threads, one for each thread of the pool, kept
/// by the load and dropped with it.
struct LoadStates(Vec<Mutex<LoadState>>);

impl LoadStates {
    fn new(threads: usize) -> Self {
        LoadStates((0..threads.max(1)).map(|_| Mutex::default()).collect())
    }

    /// Calls `f` with the state of the pool thread it runs on. A state that
    /// is in use, which only a job that runs inside another could find,
    /// gives way to one that is new.
    fn with<R>(&self, f: impl FnOnce(&mut LoadState) -> R) -> R {
        let slot = &self.0[rayon::current_thread_index().unwrap_or(0) % self.0.len()];
        match slot.try_lock() {
            Ok(mut state) => f(&mut state),
            Err(_) => f(&mut LoadState::default()),
        }
    }
}

/// A file listed by the walk, with its path hash for numbering.
struct Walked {
    path: PathBuf,
    /// The hash of the path in a `FileTable`, where the walk built it.
    hash: Option<u64>,
}

/// Where the walks of a listing's roots end in the list they are appended
/// to. A root's walk is in path order, and the roots are walked in path
/// order, so the list is in order wherever a root's last file is before the
/// next root's first.
#[derive(Default)]
struct Runs(Vec<usize>);

impl Runs {
    fn end(&mut self, len: usize) {
        self.0.push(len);
    }

    /// Puts `files` in path order: as it is when the runs join in order,
    /// and sorted when two do not, which a root spelled with `..` can do.
    fn join(self, files: &mut [Walked]) {
        let joined = self
            .0
            .iter()
            .all(|&end| end == 0 || end >= files.len() || files[end - 1].path < files[end].path);
        if !joined {
            composer::path_order::sort_by_path(files, |walked| &walked.path);
        }
        debug_assert!(files.windows(2).all(|pair| pair[0].path <= pair[1].path));
    }
}

/// The `explicit` files, which no walk lists, among `files`, which are in
/// path order: each is placed by a search of `files`, and one that is in
/// `files` already is left to that entry, which may hold its parse.
fn place_explicit(files: Vec<Walked>, mut explicit: Vec<Walked>) -> Vec<Walked> {
    if explicit.is_empty() {
        return files;
    }
    composer::path_order::sort_by_path(&mut explicit, |walked| &walked.path);
    explicit.dedup_by(|later, earlier| later.path == earlier.path);
    let mut at = 0;
    let mut places = Vec::with_capacity(explicit.len());
    for walked in explicit {
        at += files[at..].partition_point(|listed| listed.path < walked.path);
        if files
            .get(at)
            .is_none_or(|listed| listed.path != walked.path)
        {
            places.push((at, walked));
        }
    }
    let mut placed = Vec::with_capacity(files.len() + places.len());
    let mut rest = files.into_iter();
    let mut done = 0;
    for (at, walked) in places {
        placed.extend(rest.by_ref().take(at - done));
        done = at;
        placed.push(walked);
    }
    placed.extend(rest);
    placed
}

fn paths_of(walked: Vec<Walked>) -> Vec<PathBuf> {
    walked.into_iter().map(|walked| walked.path).collect()
}

impl Walked {
    fn bare(path: PathBuf) -> Self {
        Walked { path, hash: None }
    }
}

/// What a walk that parses what it lists parses into, and with.
#[derive(Clone, Copy)]
struct Parse<'a> {
    project: &'a Project,
    states: &'a LoadStates,
}

/// What a walk of one directory for files is: where it lists and what it
/// finds and leaves out.
#[derive(Clone, Copy)]
struct Scan<'a> {
    table: &'a DirTable,
    listed: Listed,
    pruned: &'a [&'a str],
    skip_dir: Option<&'a (dyn Fn(&Path) -> bool + Sync)>,
}

/// A file's, or a directory's, absolute path and package owner.
struct Placed {
    absolute: PathBuf,
    owner: Option<composer::PackageOwner>,
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

/// What `Session::load_into` found in the files it was given.
pub(crate) struct Added {
    /// How long each analysed file is, by its place in the list given.
    pub(crate) analyze_sizes: Vec<usize>,
    pub(crate) skipped_analyze: usize,
    pub(crate) skipped_context: usize,
    /// The analysed files that do not parse, which are reported.
    pub(crate) parse_failures: Vec<(PathBuf, ParseFailure)>,
    /// The context files that do not parse, which are not.
    pub(crate) context_failures: Vec<(PathBuf, ParseFailure)>,
    /// What the lexer skipped over in the loaded files, one line each.
    pub(crate) lex_warnings: Vec<String>,
}

/// What one run's analysis produced, before post-processing: every
/// check's diagnostics, and the project they came from, which
/// `finalize` needs for inline ignores. Finalizing moves the findings into
/// the report, so an analysis is finalized once.
pub struct AnalysisResult {
    project: Project,
    finalized: bool,
    /// Per-file diagnostics as the checks emitted them, and a parse error
    /// for each analysed file that does not parse.
    results: FxHashMap<PathBuf, Vec<Diagnostic>>,
    /// The whole-program checks' diagnostics (`Session::whole_program`).
    whole_program: FxHashMap<PathBuf, Vec<Diagnostic>>,
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
    /// The expression-type record when requested: `path:start..end [host]
    /// inferred | native` for every expression of each analysed file, in
    /// the final pass, files in path order.
    pub expr_type_dump: Option<String>,
    /// The PHPDoc resolution record when requested: `path:[class] function
    /// site: written => resolved`, sorted within each file, files in path
    /// order.
    pub doc_resolution_dump: Option<String>,
}

/// What `Session::check_loaded` found.
pub(crate) struct Checked {
    /// Per-file diagnostics as the checks emitted them, and a parse error
    /// for each analysed file that does not parse.
    pub(crate) results: FxHashMap<PathBuf, Vec<Diagnostic>>,
    /// The whole-program checks' diagnostics (`Session::whole_program`).
    pub(crate) whole_program: FxHashMap<PathBuf, Vec<Diagnostic>>,
    /// Files whose check panicked, with the panic message, sorted.
    pub(crate) check_panics: Vec<(PathBuf, String)>,
    pub(crate) type_dump: Option<String>,
    pub(crate) expr_type_dump: Option<String>,
    pub(crate) doc_resolution_dump: Option<String>,
}

/// The post-processing a finalized report gets, in this order: the level
/// cutoff, inline ignores, `ignore_errors`, the baseline, then the order.
#[derive(Clone, PartialEq)]
pub struct Finalize {
    pub cutoff: LevelCutoff,
    pub report_unmatched_ignores: bool,
    pub ignore_errors: Vec<ignore_errors::IgnoreErrorEntry>,
    pub baseline: Option<crate::baseline::Baseline>,
    pub order: DiagnosticOrder,
    /// The directory `ignore_errors` and the baseline match a path from.
    pub root: PathBuf,
}

impl Finalize {
    /// Post-processes `results` in place, the diagnostics of the files of
    /// `project`: the cutoff, inline ignores, `ignore_errors`, the baseline,
    /// then the order. Gives the baseline's entries that matched no
    /// diagnostic, which are the whole report's only when `results` is.
    pub fn apply(
        &self,
        project: &Project,
        results: &mut FxHashMap<PathBuf, Vec<Diagnostic>>,
    ) -> Vec<String> {
        for diagnostics in results.values_mut() {
            let made = diagnostics.len();
            diagnostics.retain(|d| self.cutoff.reports(d));
            syntax::stats::add(syntax::stats::Counter::DiagnosticsBuilt, made as u64);
            syntax::stats::add(
                syntax::stats::Counter::DiagnosticsBelowLevel,
                (made - diagnostics.len()) as u64,
            );
        }
        project.apply_inline_ignores(results, self.report_unmatched_ignores);
        let stale = self.filter_by_path(results);
        if self.order == DiagnosticOrder::Sorted {
            for diagnostics in results.values_mut() {
                diagnostics::sort_diagnostics(diagnostics);
            }
        }
        stale
    }

    /// `ignore_errors` and the baseline, which match a path from the root:
    /// a path under the root, spelled absolutely, is matched as spelled
    /// from it.
    fn filter_by_path(&self, results: &mut FxHashMap<PathBuf, Vec<Diagnostic>>) -> Vec<String> {
        if self.ignore_errors.is_empty() && self.baseline.is_none() {
            return Vec::new();
        }
        let from_root = |path: &Path| {
            path.strip_prefix(&self.root)
                .map_or_else(|_| path.to_path_buf(), Path::to_path_buf)
        };
        let rooted = results
            .keys()
            .any(|path| path.is_absolute() && path.starts_with(&self.root));
        let mut spelled: FxHashMap<PathBuf, PathBuf> = FxHashMap::default();
        if rooted {
            let mut relative = FxHashMap::default();
            for (path, diagnostics) in results.drain() {
                let key = from_root(&path);
                spelled.insert(key.clone(), path);
                relative.insert(key, diagnostics);
            }
            *results = relative;
        }
        ignore_errors::filter(results, &self.ignore_errors);
        let stale = match &self.baseline {
            Some(baseline) => crate::baseline::filter(results, baseline),
            None => Vec::new(),
        };
        if rooted {
            *results = results
                .drain()
                .map(|(key, diagnostics)| (spelled.remove(&key).unwrap_or(key), diagnostics))
                .collect();
        }
        stale
    }
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
    /// Every field of every diagnostic of `report` (`syntax::canonical`),
    /// one line each - `path`, the UTF-16 columns of its start and end, and
    /// the diagnostic - files in path order, each file's in the report's:
    /// the view the gate compares, which no output format shows whole.
    pub fn diagnostic_view(&self, report: &Report) -> String {
        let mut files: Vec<(&PathBuf, &Vec<Diagnostic>)> = report.results.iter().collect();
        files.sort_by(|a, b| a.0.cmp(b.0));
        let mut text = String::new();
        for (path, diagnostics) in files {
            let line_index = self.project.line_index(path);
            let utf16 = |at: syntax::span::LineCol| {
                line_index
                    .and_then(|index| {
                        let offset = index.offset(at)?;
                        Some(index.column(offset, syntax::span::Unit::Utf16).to_string())
                    })
                    .unwrap_or_else(|| "-".to_string())
            };
            for diagnostic in diagnostics {
                text.push_str(&format!(
                    "{}\t{}-{}\t{}\n",
                    path.display(),
                    utf16(diagnostic.span.start),
                    utf16(diagnostic.span.end),
                    syntax::canonical::print_line(diagnostic)
                ));
            }
        }
        text
    }

    /// What the analysis says beyond its diagnostics, one line each.
    pub fn notices(&self) -> Vec<String> {
        self.lex_warnings.clone()
    }

    /// The report `options` make of this analysis: each file's diagnostics
    /// followed by the whole-program checks' about it. The findings move
    /// into the report.
    pub fn finalize(&mut self, options: &Finalize) -> Report {
        assert!(!self.finalized, "an analysis is finalized once");
        self.finalized = true;
        let results = std::mem::take(&mut self.results);
        let whole_program = std::mem::take(&mut self.whole_program);
        self.report(results, whole_program, options)
    }

    /// `finalize` that keeps the findings, for the views of one analysis
    /// that are more than one.
    pub fn finalize_copy(&self, options: &Finalize) -> Report {
        assert!(!self.finalized, "an analysis is finalized once");
        self.report(self.results.clone(), self.whole_program.clone(), options)
    }

    fn report(
        &self,
        mut results: FxHashMap<PathBuf, Vec<Diagnostic>>,
        whole_program: FxHashMap<PathBuf, Vec<Diagnostic>>,
        options: &Finalize,
    ) -> Report {
        for (path, diagnostics) in whole_program {
            results.entry(path).or_default().extend(diagnostics);
        }
        let stale_baseline_entries = options.apply(&self.project, &mut results);
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
        let start = dunce::canonicalize(root.join(dir)).unwrap_or_else(|_| root.join(dir));
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
    /// Where the parse is stored once it is complete, for a file that was
    /// not read back from the per-file cache.
    cache_key: Option<file_cache::Key>,
}

/// Which files a listing holds: those a `.php` name gives, or every file
/// PHP may be run from (`collect_php_run_files_excluding`).
#[derive(Clone, Copy)]
enum Listed {
    Php,
    Run,
}

/// What decides which files a session loads, to tell whether a
/// configuration changes the set.
#[derive(Clone, Debug, PartialEq)]
pub struct FileSet {
    roles: super::roles::FileRoles,
    pruned: &'static [&'static str],
    scan_dirs: Vec<PathBuf>,
    scan_files: Vec<PathBuf>,
    vendor: Vec<composer::vendor::VendorContext>,
}

/// What resolving does with a configuration that is wrong.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Policy {
    /// Fails with the first problem, as a command line run does.
    Fail,
    /// Takes what is wrong as unset and reports it in `Session::warnings`,
    /// as an editor does, which a malformed configuration must not stop.
    /// Without paths, the whole root is analysed, less the directories
    /// `DEFAULT_SCAN_EXCLUDED_DIR_NAMES` names.
    Degrade,
}

/// `result`, or under `Policy::Degrade` the default for what it would have
/// given, with the problem added to `warnings`.
fn recovered<T: Default>(
    result: Result<T, String>,
    policy: Policy,
    warnings: &mut Vec<String>,
) -> Result<T, String> {
    match result {
        Err(problem) if policy == Policy::Degrade => {
            warnings.push(problem);
            Ok(T::default())
        }
        other => other,
    }
}

/// The configuration's section `S`, read under `policy`.
fn section<S: config::Section>(
    config: &ConfigFile,
    policy: Policy,
    warnings: &mut Vec<String>,
) -> Result<S, String> {
    recovered(config.section::<S>(), policy, warnings)
}

/// A file that was not loaded because it does not parse, where, and why.
#[derive(Clone)]
pub(crate) struct ParseFailure {
    pub(crate) line: u32,
    pub(crate) col: u32,
    pub(crate) message: String,
}

impl ParseFailure {
    /// `error`, which the parse of `source` gave.
    pub(crate) fn of(error: super::ParseError, source: &[u8]) -> ParseFailure {
        let at = syntax::LineIndex::new(source).line_col(error.span.start);
        ParseFailure {
            line: at.line,
            col: at.col,
            message: error.message.clone(),
        }
    }

    /// The report of the failure: a file that does not parse is reported
    /// where it fails, as PHP refuses to run it.
    pub(crate) fn diagnostic(&self) -> Diagnostic {
        let at = syntax::span::LineCol {
            line: self.line,
            col: self.col,
        };
        DiagnosticBuilder::new(self.message.clone(), Span::default(), 0, "parse.error")
            .non_ignorable()
            .build_at(syntax::span::Range { start: at, end: at })
    }
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
        policy: Policy,
        pool: Arc<rayon::ThreadPool>,
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
        let (dirs, defaulted_dirs) = if dirs.is_empty() {
            match policy {
                Policy::Fail => {
                    return Err(
                        "no paths given on the command line or in the config file".to_string()
                    );
                }
                // Everything under the root, less what no analysed file is
                // in.
                Policy::Degrade => (vec![root.clone()], true),
            }
        } else {
            (dirs, false)
        };
        let mut cutoff = configured_cutoff(overrides.level.or(session.level), DEFAULT_REPORT_LEVEL);
        if let Some(rule) = overrides.report_level {
            cutoff.rule = rule;
        }
        recovered(psr3.context_key_pattern().map(drop), policy, &mut warnings)?;
        let php_releases = recovered(
            match &overrides.php_version {
                Some(version) => types::parse_php_release_range(version).map(Some),
                None => reflection
                    .php_version
                    .as_ref()
                    .map(crate::registry::settings::PhpVersionSetting::releases)
                    .transpose(),
            },
            policy,
            &mut warnings,
        )?;
        let php_target_profile = recovered(
            reflection
                .php_target_profile
                .as_ref()
                .map(crate::registry::settings::PhpTargetProfileSetting::target_profile)
                .transpose(),
            policy,
            &mut warnings,
        )?;
        let restricted_symbols = recovered(disallowed_calls.entries(), policy, &mut warnings)?;
        let banned_constructs = recovered(
            banned_settings
                .constructs
                .iter()
                .map(|name| banned::BannedConstruct::parse(name))
                .collect::<Result<Vec<_>, _>>(),
            policy,
            &mut warnings,
        )?;
        recovered(overlays.validate(), policy, &mut warnings)?;
        let parent_call_rules = class_shape.rules();
        let mut architecture_rules = architecture.0;
        let rules_valid = recovered(
            architecture_rules
                .iter()
                .try_for_each(|rule| rule.validate())
                .map(|()| true),
            policy,
            &mut warnings,
        )?;
        if !rules_valid {
            architecture_rules.clear();
        }
        let sql_reflection = sql_settings.reflection;
        let sql_schema = match sql_reflection
            .as_ref()
            .and_then(|c| c.schema_file.as_ref().map(|f| (f.clone(), c.driver)))
        {
            Some((path, driver)) => {
                let path = resolve_against_config(path);
                recovered(
                    sql::schema::parse_ddl_file(&root.join(path), driver).map(Some),
                    policy,
                    &mut warnings,
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
        scan_dirs.extend(stub_dirs.iter().cloned());

        let mut scan_files: Vec<PathBuf> = Vec::new();
        let mut vendors = Vec::new();
        let mut promoted: Vec<PathBuf> = Vec::new();
        let mut phpstan_api_check_enabled = false;
        let mut composer_manifests = Vec::new();
        let absolute_root = dunce::canonicalize(&root).unwrap_or_else(|_| root.clone());
        let mut composer_php_versions = None;
        let mut root_manifest = None;
        let listings = Arc::new(composer::dir_table::DirTable::default());
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
                let package = composer::PackageContext::load_in(
                    &composer_root,
                    &manifest_root,
                    Arc::clone(&listings),
                    super::file_cache::vendor_directory(),
                );
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

        let php_releases = php_releases
            .or_else(|| composer_php_versions.map(types::PhpReleaseRange::from_language_versions));
        let php_versions = php_releases.and_then(types::PhpReleaseRange::language_versions);
        let extensions = recovered(
            ext::ActiveExtensions::resolve(config, &|p| {
                ext::installed_in(root_manifest.as_ref(), p)
            }),
            policy,
            &mut warnings,
        )?;

        let roles = {
            let absolute =
                |path: &Path| super::absolute_spelling(Some(&absolute_root), Some(&root), path);
            let mut roles = super::roles::FileRoles::default();
            for dir in &dirs {
                roles = if defaulted_dirs {
                    roles.with_pruned_analysed(&absolute(dir), DEFAULT_SCAN_EXCLUDED_DIR_NAMES)
                } else {
                    roles.with_analysed(&absolute(dir))
                };
            }
            for dir in &stub_dirs {
                roles = roles.with_stubs(&absolute(dir));
            }
            roles
        };
        let settings = ProjectSettings {
            working_dir: Some(root.clone()),
            absolute_root: Some(absolute_root),
            roles,
            packages: composer::PackageIndex::new(composer_manifests),
            php_versions,
            php_releases,
            php_target_profile,
            phpstan_api_check_enabled,
            retain_source: false,
            keeps_as_built: false,
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
            report_cutoff: None,
        };

        Ok(Session {
            pool,
            root,
            session,
            settings,
            dirs,
            pruned: if defaulted_dirs {
                DEFAULT_SCAN_EXCLUDED_DIR_NAMES
            } else {
                &[]
            },
            scan_dirs,
            scan_files,
            vendors,
            promoted,
            discovery_listings: Mutex::new(Some(listings)),
            cutoff,
            emission_order: overrides.emission_order,
            banned_constructs,
            banned_types: banned_settings.types,
            check_closed_classes: class_shape.check_closed_classes,
            parent_call_rules,
            architecture_rules,
            check_mock_hygiene: mockstan.check_mock_hygiene.unwrap_or(extensions.mockstan),
            mock_banned_classes: mockstan.mock_banned_classes,
            overlay: Overlay::default(),
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
        let session = Session::resolve(config, None, base.clone(), overrides, Policy::Fail, pool)?;
        Ok((session, base))
    }

    /// The post-processing this session's configuration asks for, without
    /// a baseline.
    pub fn finalize_options(&self) -> Finalize {
        Finalize {
            cutoff: self.cutoff,
            report_unmatched_ignores: self.session.report_unmatched_ignored_errors,
            ignore_errors: self.session.ignore_errors.clone(),
            baseline: None,
            order: if self.emission_order {
                DiagnosticOrder::Emission
            } else {
                DiagnosticOrder::Sorted
            },
            root: self.root.clone(),
        }
    }

    /// The session for a run whose report is final: the checks leave a
    /// finding below the session's cutoff unmade, which nothing after them
    /// would have read (the cutoff is the first step of `Finalize::apply`).
    pub fn reporting_only_at_cutoff(mut self) -> Self {
        self.settings.report_cutoff = Some(self.cutoff);
        self
    }

    /// The session for a project kept for edits, as the language server
    /// keeps it: with each file's source, and every file's tree.
    pub fn for_editing(mut self) -> Self {
        self.settings.retain_source = true;
        self.settings.keeps_as_built = true;
        self
    }

    /// The session, configuring its project with `settings` instead, kept
    /// for edits.
    #[cfg(test)]
    pub(crate) fn with_settings(mut self, settings: ProjectSettings) -> Session {
        self.settings = ProjectSettings {
            keeps_as_built: true,
            ..settings
        };
        self
    }

    /// What the session configures its project with.
    pub fn settings(&self) -> &ProjectSettings {
        &self.settings
    }

    /// The directory the session's relative paths are relative to.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The session reading `files` in place of what the disk has at their
    /// paths, which are spelled from the root: they exist whether or not
    /// the disk has them, and are listed where a walk of the paths would
    /// find them.
    pub fn with_overlay(mut self, files: impl IntoIterator<Item = (PathBuf, Vec<u8>)>) -> Session {
        for (path, bytes) in files {
            self.set_overlay(&path, bytes);
        }
        self
    }

    /// Reads `bytes` as the file at `path`, spelled from the root, from
    /// now on.
    pub fn set_overlay(&mut self, path: &Path, bytes: Vec<u8>) {
        self.overlay.set(self.absolute(path), bytes);
    }

    /// Reads the file at `path`, spelled from the root, from the disk
    /// again.
    pub fn clear_overlay(&mut self, path: &Path) {
        self.overlay.remove(&self.absolute(path));
    }

    /// The bytes of the file at `path`, spelled from the root: the
    /// overlay's, or the disk's. Every read of a file goes through here.
    pub fn read(&self, path: &Path) -> std::io::Result<Vec<u8>> {
        self.read_at(path, &self.absolute(path))
    }

    /// `read` of the file at `path`, which is `absolute` as an absolute
    /// path.
    fn read_at(&self, path: &Path, absolute: &Path) -> std::io::Result<Vec<u8>> {
        let mut bytes = Vec::new();
        self.read_into(path, absolute, &mut bytes)?;
        Ok(bytes)
    }

    /// `read_at` into `buf`, which holds the file's bytes alone afterwards.
    fn read_into(&self, path: &Path, absolute: &Path, buf: &mut Vec<u8>) -> std::io::Result<()> {
        use std::io::Read as _;
        buf.clear();
        match self.overlay.get(absolute) {
            Some(bytes) => buf.extend_from_slice(bytes),
            None => {
                let mut file = std::fs::File::open(self.root.join(path))?;
                match file.metadata() {
                    Ok(meta) => {
                        // One byte past the length tells a file that grew
                        // since the length was read; reading to the end then
                        // takes the rest.
                        let len = meta.len().saturating_add(1);
                        buf.reserve(usize::try_from(len).unwrap_or(0));
                        let read = (&mut file).take(len).read_to_end(buf)?;
                        if read as u64 == len {
                            file.read_to_end(buf)?;
                        }
                    }
                    Err(_) => {
                        file.read_to_end(buf)?;
                    }
                }
            }
        }
        Ok(())
    }

    /// The settings of the session as a whole: the baseline, the level and
    /// the ignored errors.
    pub fn session_settings(&self) -> &SessionSettings {
        &self.session
    }

    /// What decides which files the session loads.
    pub fn file_set(&self) -> FileSet {
        FileSet {
            roles: self.settings.roles.clone(),
            pruned: self.pruned,
            scan_dirs: self.scan_dirs.clone(),
            scan_files: self.scan_files.clone(),
            vendor: self.vendors.iter().map(|v| v.get().clone()).collect(),
        }
    }

    /// The files the session loads: those it analyses, then the context
    /// files, each in the order a load takes them.
    pub fn listing(&self) -> (Vec<PathBuf>, Vec<PathBuf>) {
        let table = self.listings();
        (
            paths_of(self.analysed_listing(&table, Listed::Php)),
            paths_of(self.context_listing(&table, Listed::Php)),
        )
    }

    /// The directories analysed (after composer path-repository
    /// promotion), relative to the root as given.
    pub fn dirs(&self) -> &[PathBuf] {
        &self.dirs
    }

    /// Configures `project` with the session's settings, built once or
    /// kept for edits as `keeps_as_built` says.
    fn configure(&self, project: &mut Project, keeps_as_built: bool) {
        let mut settings = self.settings.clone();
        settings.keeps_as_built = keeps_as_built;
        project.apply_settings(settings);
    }

    /// The directory listings a walk of this session reads through: the
    /// ones vendor discovery made, for the first walk, and none after it.
    fn listings(&self) -> Arc<DirTable> {
        self.discovery_listings
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
            .unwrap_or_default()
    }

    /// `path`, spelled from the root, as an absolute path.
    fn absolute(&self, path: &Path) -> PathBuf {
        super::absolute_spelling(
            self.settings.absolute_root.as_deref(),
            self.settings.working_dir.as_deref(),
            path,
        )
    }

    /// What `path`, spelled from the root, is to the session: analysed when
    /// an analysed path contains it, context otherwise, and a stub when a
    /// stub path does. The load and every edit take a file's role from
    /// here.
    pub fn classify(&self, path: &Path) -> super::roles::Classified {
        self.settings.roles.classify(&self.absolute(path))
    }

    /// Whether `path`, spelled from the root, is under a stub path.
    fn is_stub(&self, path: &Path) -> bool {
        self.is_stub_at(&self.absolute(path))
    }

    /// `is_stub` of the file at `absolute`.
    fn is_stub_at(&self, absolute: &Path) -> bool {
        self.settings.roles.has_stubs() && self.settings.roles.is_stub(absolute)
    }

    /// What a load finds out about a file's place: its absolute path and
    /// its package's owner. The directory's are found once for the files
    /// `state` meets in it, and the file's are the directory's with its
    /// name.
    fn place(&self, state: &mut LoadState, path: &Path) -> Placed {
        let (Some(dir), Some(name)) = (path.parent(), path.file_name()) else {
            let absolute = self.absolute(path);
            let owner = self.settings.packages.owner_of_dir(&absolute);
            return Placed { absolute, owner };
        };
        let here = state
            .dirs
            .entry(dir.as_os_str().as_encoded_bytes().to_vec())
            .or_insert_with(|| {
                let absolute = self.absolute(dir);
                let owner = self.settings.packages.owner_of_dir(&absolute);
                Placed { absolute, owner }
            });
        Placed {
            absolute: here.absolute.join(name),
            owner: here.owner,
        }
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

    /// The files `listed` finds under `dir` (or `dir` itself), leaving out
    /// what is under a directory named in `pruned`, spelled as
    /// `dir` joined with its path below it - the same spelling the command
    /// line gave, built as the walk goes down - while the walk itself
    /// happens under the root.
    fn collect_files(&self, scan: Scan, dir: &Path, out: &mut Vec<Walked>) {
        let Scan {
            table,
            listed,
            pruned,
            skip_dir,
        } = scan;
        let walked = self.root.join(dir);
        let wanted = |dir: &Path, name: &std::ffi::OsStr| match listed {
            Listed::Php => super::is_php_file(Path::new(name)),
            Listed::Run => super::is_php_run_file(dir, name),
        };
        let skip = skip_dir.map(|skip| move |path: &Path| skip(&self.absolute(path)));
        // A walk that goes into a root with analysed files below it finds
        // them too, and parses only the files that are context.
        let make = |_path: &Path| None::<Read>;
        let found = self.pool.install(|| {
            super::Walk {
                table,
                excluded_dir_names: pruned,
                wanted: &wanted,
                skip_dir: skip
                    .as_ref()
                    .map(|skip| skip as &(dyn Fn(&Path) -> bool + Sync)),
                make: &make,
            }
            .files(&walked, dir)
        });
        let start = out.len();
        out.extend(found.into_iter().map(|(path, hash, _read)| Walked {
            path,
            hash: Some(hash),
        }));
        if self.overlay.is_empty() {
            return;
        }
        // The overlay's files the walk did not find.
        let absolute = self.absolute(dir);
        let held: Vec<PathBuf> = self
            .overlay
            .under(&absolute)
            .filter(|path| {
                let below = path.strip_prefix(&absolute).unwrap_or(path);
                !super::path_under_excluded_dir(below, pruned)
                    && match listed {
                        Listed::Php => path.extension().is_some_and(|e| e == "php"),
                        Listed::Run => {
                            super::has_php_run_extension(path)
                                || self
                                    .overlay
                                    .get(path)
                                    .is_some_and(|bytes| super::opens_php(bytes))
                        }
                    }
            })
            .map(|path| {
                let below = path.strip_prefix(&absolute).unwrap_or(path);
                if below.as_os_str().is_empty() {
                    dir.to_path_buf()
                } else {
                    dir.join(below)
                }
            })
            .collect();
        let found = out.len();
        for path in held {
            if !out[start..found].iter().any(|walked| walked.path == path) {
                out.push(Walked::bare(path));
            }
        }
        if out.len() > found {
            composer::path_order::sort_by_path(&mut out[start..], |walked| &walked.path);
        }
    }

    /// Reads and parses the file at `path`, as a load does each file it
    /// lists: a context file when `context`, which the per-file cache
    /// serves, and everything its use sites, owner and cache entry need.
    fn parse_listed(&self, parse: Parse, path: &Path, context: bool, file_id: FileId) -> Read {
        let shared = parse.project;
        let index_use_sites = shared.indexes_use_sites();
        parse.states.with(|state| {
            let placed = self.place(state, path);
            // A file the overlay holds or an editor has open keeps its
            // tree, which no cache entry holds.
            let held = self.overlay.get(&placed.absolute).is_some();
            let keeps_tree = shared.keeps_tree(path, !context);
            let mut read = self.read_and_parse_file(
                path,
                &placed.absolute,
                self.is_stub_at(&placed.absolute),
                state,
                ParseOptions {
                    cacheable: !held && (!context || !keeps_tree),
                    needs_tree: keeps_tree,
                    index_use_sites,
                    reuse_scratch: !keeps_tree,
                },
                file_id,
            );
            if let Read::Loaded(file) = &mut read {
                shared.index_use_sites(&mut file.parsed, &file.source);
                if !keeps_tree && let Some(scratch) = file.parsed.drop_tree() {
                    state.scratch = scratch;
                }
                if !(self.settings.retain_source && held) {
                    file.source = String::new();
                }
                file.parsed.owner = Some(placed.owner);
                if let Some(key) = file.cache_key.take() {
                    file_cache::store(&key, &file_cache::CachedFile::of(&file.parsed, file.len));
                }
            }
            if state.buf.capacity() > MAX_KEPT_BUFFER {
                state.buf = Vec::new();
            }
            read
        })
    }

    fn read_and_parse_file(
        &self,
        path: &Path,
        absolute: &Path,
        is_stub: bool,
        state: &mut LoadState,
        options: ParseOptions,
        file_id: FileId,
    ) -> Read {
        let ParseOptions {
            cacheable,
            needs_tree,
            index_use_sites,
            reuse_scratch,
        } = options;
        let LoadState {
            names,
            buf,
            scratch: arenas,
            tokens,
            lexer,
            parser,
            ..
        } = state;
        if self.read_into(path, absolute, buf).is_err() {
            return Read::Unread;
        }
        let bytes: &[u8] = buf;
        let content_hash = ContentHash::of(bytes);
        let cache_key = if cacheable {
            file_cache::key(content_hash, path, is_stub, index_use_sites)
        } else {
            None
        };
        let mut cached_derivations = None;
        let mut had_entry = false;
        if let Some(key) = &cache_key
            && let Some(cached) = file_cache::load(key, file_id)
        {
            if needs_tree {
                had_entry = true;
                cached_derivations = Some(cached);
            } else {
                let len = cached.len;
                if let Some(parsed) = cached.into_parsed(path, content_hash) {
                    return Read::Loaded(Box::new(ReadFile {
                        len,
                        source: String::new(),
                        parsed,
                        cache_key: None,
                    }));
                }
            }
        }
        // Whether the file is UTF-8 is settled here, once, and the lexer
        // takes the text as it is. What checking costs is told by the
        // length of the text as a lossy reading of the bytes has it, which
        // is the bytes' own for a file that is UTF-8.
        let text = std::str::from_utf8(bytes);
        let len = match text {
            Ok(text) => text.len(),
            Err(_) => String::from_utf8_lossy(bytes).len(),
        };
        let mut local_arenas = syntax::ast::NodeArenas::default();
        let node_scratch = if reuse_scratch {
            arenas
        } else {
            &mut local_arenas
        };
        let scratch = syntax::WorkerScratch {
            arenas: node_scratch,
            tokens,
            lexer,
            parser,
        };
        let Ok(parsed) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match text {
            Ok(text) => super::parse_file_with_names_and_file_id_and_worker_scratch(
                path,
                text,
                is_stub,
                Some(names),
                file_id,
                scratch,
                content_hash,
                cached_derivations,
            ),
            Err(_) => super::parse_file_with_names_and_file_id_and_worker_scratch(
                path,
                bytes,
                is_stub,
                Some(names),
                file_id,
                scratch,
                content_hash,
                cached_derivations,
            ),
        })) else {
            return Read::Unread;
        };
        let parsed = match parsed {
            Ok(parsed) => parsed,
            Err(error) => return Read::Unparsed(ParseFailure::of(error, bytes)),
        };
        // The text of the file is kept only while its use sites are
        // indexed from it.
        let source = match text {
            Ok(text) if index_use_sites => text.to_owned(),
            Err(_) if index_use_sites => String::from_utf8_lossy(bytes).into_owned(),
            _ => String::new(),
        };
        // An entry that was read is not written again.
        let cache_key = if had_entry { None } else { cache_key };
        Read::Loaded(Box::new(ReadFile {
            len,
            source,
            parsed,
            cache_key,
        }))
    }

    /// Writes O1, the front end's dump (`front_end_dump`), to `out`, a dump
    /// of files (`dumps`) with its manifest beside it: each file PHP may be
    /// run from under the analysed paths, the context paths and the stub
    /// paths, parsed as the load parses it, with the names a job of the
    /// load shares, at `<role>/<path>`; each embedded stub, as the stub
    /// build reads it, at `embedded/<path>`; a file that cannot be read,
    /// empty, at `unread/<role>/<path>`; and the unions built while the
    /// files were read, one a line, sorted, at `unions`. Given `reference`,
    /// the manifest of another run, writes the text of a file only where it
    /// differs from that run's.
    pub fn dump_front_end(
        &self,
        out: &Path,
        reference: Option<&Path>,
    ) -> Result<FrontEndDump, String> {
        let table = self.listings();
        let analysed = paths_of(self.analysed_listing(&table, Listed::Run));
        let context = paths_of(self.context_listing(&table, Listed::Run));
        let mut files: Vec<(&'static str, &PathBuf)> = context
            .iter()
            .map(|path| ("context", path))
            .chain(analysed.iter().map(|path| ("analysed", path)))
            .collect();
        files.sort_unstable();

        let mut project = Project::new();
        self.configure(&mut project, self.settings.keeps_as_built);
        let index_use_sites = project.indexes_use_sites();
        let context_of = |e: std::io::Error| format!("{}: {e}", out.display());
        let writer = std::sync::Mutex::new(
            dumps::DumpWriter::create(out, dumps::Kind::Files, reference).map_err(context_of)?,
        );
        let write = |role: &str, path: &[u8], text: Option<&[u8]>| -> std::io::Result<()> {
            let path = path.split(|&byte| byte == b'/');
            let entity = match text {
                Some(text) => {
                    dumps::FileEntity::new(std::iter::once(role.as_bytes()).chain(path), text)
                }
                None => dumps::FileEntity::new(
                    [b"unread".as_slice(), role.as_bytes()]
                        .into_iter()
                        .chain(path),
                    b"",
                ),
            };
            writer
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .file(entity)
        };
        let embedded = crate::registry::stubs::embedded_stub_files();
        types::record_unions();
        let (dumped, stubbed) = self.pool.install(|| {
            rayon::join(
                || {
                    files.par_iter().try_for_each_init(
                        syntax::name::NameCache::default,
                        |names, (role, path)| {
                            let is_stub = self.is_stub(path);
                            let role = if is_stub {
                                format!("{role} stub")
                            } else {
                                role.to_string()
                            };
                            let text = self.read(path).ok().map(|bytes| {
                                let source = if index_use_sites {
                                    String::from_utf8_lossy(&bytes).into_owned()
                                } else {
                                    String::new()
                                };
                                super::front_end_dump::dump_file(
                                    path,
                                    &bytes,
                                    is_stub,
                                    names,
                                    &project,
                                    index_use_sites,
                                    &source,
                                )
                            });
                            write(
                                &role,
                                path.as_os_str().as_encoded_bytes(),
                                text.as_deref().map(str::as_bytes),
                            )
                        },
                    )
                },
                || {
                    embedded.par_iter().try_for_each(|(path, source, origin)| {
                        let text = super::front_end_dump::dump_embedded_stub(source, origin);
                        write("embedded", path.as_bytes(), Some(text.as_bytes()))
                    })
                },
            )
        });
        dumped.and(stubbed).map_err(context_of)?;
        let unions = types::recorded_unions();
        let mut union_text = unions.join("\n");
        union_text.push('\n');
        let mut writer = writer
            .into_inner()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        writer
            .file(dumps::FileEntity::new(
                [b"unions".as_slice()],
                union_text.as_bytes(),
            ))
            .and_then(|()| writer.finish())
            .map_err(context_of)?;
        Ok(FrontEndDump {
            files: files.len() + embedded.len(),
            unions: unions.len(),
        })
    }

    /// Runs the edit replay (`edit_replay`) over an analysis of the
    /// project kept for edits, its files drawn with `seed`, and writes into
    /// `out` the registry dump without the stub bases, the diagnostics held
    /// and the whole-program checks' after the analysis and after each
    /// step (`step-<n>.registry.txt`, `step-<n>.diagnostics.txt`,
    /// `step-<n>.whole-program.txt`, dumps with their manifests, `dumps`),
    /// the same of a fresh analysis of the tree as each step that is
    /// compared with one leaves it (`fresh-<n>.*`); and `script.txt`, the
    /// steps, and `verdict.txt`, for each step which sections of its dumps
    /// and those it is compared with differ. Each dump is written where it
    /// differs from the same dump of `reference`, another replay's
    /// directory, when that has it, and otherwise from this replay's step
    /// before, or for a fresh analysis from its step. Edits the tree it
    /// reads: refuses one without the replay's marker file at its root.
    /// Gives the verdict, or an error with it when a step differs.
    pub fn edit_replay(
        &self,
        out: &Path,
        seed: u64,
        reference: Option<&Path>,
    ) -> Result<String, String> {
        let marker = self.root.join(super::edit_replay::MARKER);
        if !marker.is_file() {
            return Err(format!(
                "the edit replay edits the files it reads; run it on a copy, with {} in it",
                marker.display()
            ));
        }
        std::fs::create_dir_all(out).map_err(|e| format!("{}: {e}", out.display()))?;
        let write_state = |name: &str,
                           base: Option<&str>,
                           project: &Project,
                           held: &super::edit_replay::HeldDiagnostics,
                           whole_program: &super::edit_replay::HeldDiagnostics|
         -> Result<(), String> {
            for dump in [REPLAY_REGISTRY, REPLAY_DIAGNOSTICS, REPLAY_WHOLE_PROGRAM] {
                let text = out.join(format!("{name}.{dump}.txt"));
                let given = reference
                    .map(|dir| dumps::manifest_path(&dir.join(format!("{name}.{dump}.txt"))))
                    .filter(|given| given.is_file())
                    .or_else(|| {
                        base.map(|base| {
                            dumps::manifest_path(&out.join(format!("{base}.{dump}.txt")))
                        })
                    });
                if dump == REPLAY_REGISTRY {
                    write_dump(&text, dumps::Kind::Tree, given.as_deref(), |w| {
                        project.write_registry_dump(w)
                    })?;
                } else {
                    let held = if dump == REPLAY_DIAGNOSTICS {
                        held
                    } else {
                        whole_program
                    };
                    write_dump(&text, dumps::Kind::ByPrefix(b'\t'), given.as_deref(), |w| {
                        w.write_all(super::edit_replay::print_held(held).as_bytes())
                    })?;
                }
            }
            Ok(())
        };
        let analyse = || -> Result<AnalysisResult, String> {
            Ok(self.analyse(self.load_editable()?, super::Records::default()))
        };
        let AnalysisResult {
            mut project,
            results,
            whole_program,
            ..
        } = analyse()?;
        let mut held: super::edit_replay::HeldDiagnostics = results.into_iter().collect();
        write_state(
            "step-0",
            None,
            &project,
            &held,
            &whole_program.into_iter().collect(),
        )?;
        let mut script = String::from("0\tload\n");
        let mut verdict = String::new();
        let mut differing: Vec<usize> = Vec::new();
        let mut number = 0;
        let list = || self.analysed_files();
        super::edit_replay::replay(
            &mut project,
            &self.root,
            seed,
            &mut held,
            &list,
            |step, project, held| {
                number += 1;
                script.push_str(&format!("{number}\t{}\t{}\n", step.label, step.description));
                let name = format!("step-{number}");
                let previous = format!("step-{}", number - 1);
                let whole_program = self.whole_program(project).into_iter().collect();
                write_state(&name, Some(&previous), project, held, &whole_program)?;
                let against = match step.against {
                    super::edit_replay::Against::Fresh => {
                        let fresh = analyse()?;
                        let fresh_name = format!("fresh-{number}");
                        write_state(
                            &fresh_name,
                            Some(&name),
                            &fresh.project,
                            &fresh.results.into_iter().collect(),
                            &fresh.whole_program.into_iter().collect(),
                        )?;
                        fresh_name
                    }
                    super::edit_replay::Against::Previous => previous,
                };
                let states = replay_verdict(out, &name, &against)
                    .map_err(|e| format!("{}: {e}", out.display()))?;
                let differs: Vec<String> = states
                    .iter()
                    .filter(|(_, state)| *state != SAME)
                    .map(|(section, state)| format!("{section} {state}"))
                    .collect();
                if states.iter().any(|(_, state)| *state == DIFFERS) {
                    differing.push(number);
                }
                let summary = if differs.is_empty() {
                    SAME.to_string()
                } else {
                    differs.join(", ")
                };
                verdict.push_str(&format!("{number}\t{against}\t{summary}\n"));
                Ok(())
            },
        )?;
        for (name, text) in [("script.txt", &script), ("verdict.txt", &verdict)] {
            std::fs::write(out.join(name), text).map_err(|e| format!("{name}: {e}"))?;
        }
        if differing.is_empty() {
            Ok(verdict)
        } else {
            let steps: Vec<String> = differing.iter().map(usize::to_string).collect();
            Err(format!(
                "{verdict}the edited project differs from what it is compared with at step {}",
                steps.join(", ")
            ))
        }
    }

    /// Loads every file, runs the finishing passes, and writes the registry
    /// dump (`registry_dump`) of what the load leaves to `out`, a dump with
    /// its manifest (`dumps`), where it differs from `reference`'s.
    pub fn dump_registry(&self, out: &Path, reference: Option<&Path>) -> Result<(), String> {
        let loaded = self.load()?;
        write_dump(out, dumps::Kind::Tree, reference, |w| {
            loaded.project.write_registry_dump(w)
        })
    }

    /// Writes both stub bases whole (`Project::stub_bases_dump`), what a
    /// registry dump prints its project over, to `out`, a dump with its
    /// manifest (`dumps`), where it differs from `reference`'s.
    pub fn dump_stub_bases(&self, out: &Path, reference: Option<&Path>) -> Result<(), String> {
        write_dump(
            out,
            dumps::Kind::Tree,
            reference,
            Project::write_stub_bases_dump,
        )
    }

    /// Loads and checks every analysed file, with the `--dump-types`
    /// output when `dump_types` is set. Fails when the analysed paths hold
    /// no `.php` file.
    pub fn run(&self, records: super::Records) -> Result<AnalysisResult, String> {
        let loaded = self.load()?;
        Ok(self.analyse(loaded, records))
    }

    /// Parses every context and analysed file into a configured,
    /// finished `Project`, ready for `analyse`.
    pub fn load(&self) -> Result<Loaded, String> {
        self.load_project(self.settings.keeps_as_built)
    }

    /// `load`, the project kept for edits as the language server keeps it,
    /// with its declarations as built.
    fn load_editable(&self) -> Result<Loaded, String> {
        self.load_project(true)
    }

    /// The files the load analyses, in path order.
    fn analysed_files(&self) -> Vec<PathBuf> {
        paths_of(self.analysed_listing(&self.listings(), Listed::Php))
    }

    /// The files under the analysed paths that `listed` finds, in path order,
    /// each once: a path another contains is not walked again. They come
    /// parsed when `parse` is given.
    fn analysed_listing(&self, table: &DirTable, listed: Listed) -> Vec<Walked> {
        let mut files = Vec::new();
        let mut runs = Runs::default();
        let mut roots = super::roles::outermost(&self.dirs, |dir| self.absolute(dir));
        composer::path_order::sort_by_path(&mut roots, |dir| dir.as_path());
        for dir in roots {
            let scan = Scan {
                table,
                listed,
                pruned: self.pruned,
                skip_dir: None,
            };
            self.collect_files(scan, dir, &mut files);
            runs.end(files.len());
        }
        runs.join(&mut files);
        files
    }

    /// The files the context paths and the vendor context reach that `listed`
    /// finds, in path order, each once and none that an analysed path
    /// contains: such a file is analysed, and listed by `analysed_listing`.
    /// They come parsed when `parse` is given, those a walk found.
    fn context_listing(&self, table: &DirTable, listed: Listed) -> Vec<Walked> {
        // The vendor context was being found while the analysed files were
        // listed.
        let mut scan_dirs = self.scan_dirs.clone();
        let mut explicit: Vec<Walked> = self.scan_files.iter().cloned().map(Walked::bare).collect();
        for vendor in &self.vendors {
            let found = vendor.get();
            scan_dirs.extend(found.dirs.iter().filter(|d| !self.is_promoted(d)).cloned());
            explicit.extend(
                found
                    .files
                    .iter()
                    .filter(|f| !self.is_promoted(f))
                    .cloned()
                    .map(Walked::bare),
            );
        }
        let roles = &self.settings.roles;
        let is_context = |walked: &Walked| {
            roles.classify(&self.absolute(&walked.path)).role == super::roles::Role::Context
        };
        explicit.retain(is_context);
        let mut files = Vec::new();
        let mut runs = Runs::default();
        let mut roots = super::roles::outermost(&scan_dirs, |dir| self.absolute(dir));
        composer::path_order::sort_by_path(&mut roots, |dir| dir.as_path());
        for dir in roots {
            let absolute = self.absolute(dir);
            if roles.classify(&absolute).role == super::roles::Role::Analysed {
                continue;
            }
            let start = files.len();
            // What an analysed root holds, in full, is analysed, and listed
            // by the walk of that root: a walk of this directory goes no
            // further into it.
            let holds_analysed = roles.holds_analysed(&absolute);
            let skip = |absolute: &Path| roles.holds_only_analysed(absolute);
            let scan = Scan {
                table,
                listed,
                pruned: &[],
                skip_dir: holds_analysed.then_some(&skip),
            };
            self.collect_files(scan, dir, &mut files);
            if holds_analysed {
                let walked = files.split_off(start);
                files.extend(walked.into_iter().filter(is_context));
            }
            runs.end(files.len());
        }
        runs.join(&mut files);
        // A file both listed and walked keeps the walk's path hash.
        place_explicit(files, explicit)
    }

    /// `load`, the project built once or kept for edits as `keeps_as_built`
    /// says. Each file is parsed as its directory is walked, in the pool
    /// the walk runs in.
    fn load_project(&self, keeps_as_built: bool) -> Result<Loaded, String> {
        let table = self.listings();
        let mut project = Project::new();
        self.configure(&mut project, keeps_as_built);
        let states = LoadStates::new(self.pool.current_num_threads());
        let analyze_files = self.analysed_listing(&table, Listed::Php);
        let context_files = self.context_listing(&table, Listed::Php);
        if analyze_files.is_empty() {
            let dirs: Vec<String> = self.dirs.iter().map(|d| d.display().to_string()).collect();
            return Err(format!("no .php files found under {}", dirs.join(", ")));
        }
        Ok(self.load_walked(project, &states, context_files, analyze_files))
    }

    /// `load_project` of the files given, which are the listing's, and
    /// that may have no analysed one.
    #[cfg(test)]
    pub(crate) fn load_listed(
        &self,
        keeps_as_built: bool,
        context_files: &[PathBuf],
        analyze_files: Vec<PathBuf>,
    ) -> Loaded {
        let mut project = Project::new();
        self.configure(&mut project, keeps_as_built);
        let states = LoadStates::new(self.pool.current_num_threads());
        self.load_walked(
            project,
            &states,
            context_files.iter().cloned().map(Walked::bare).collect(),
            analyze_files.into_iter().map(Walked::bare).collect(),
        )
    }

    /// The configured `project` with the files listed loaded into it.
    fn load_walked(
        &self,
        mut project: Project,
        states: &LoadStates,
        context_files: Vec<Walked>,
        analyze_files: Vec<Walked>,
    ) -> Loaded {
        let context_count = context_files.len();
        let analyze_paths: Vec<PathBuf> = analyze_files
            .iter()
            .map(|walked| walked.path.clone())
            .collect();
        let Added {
            analyze_sizes,
            skipped_analyze,
            skipped_context,
            parse_failures,
            context_failures,
            mut lex_warnings,
        } = self.add_walked(&mut project, states, context_files, analyze_files);
        project.finish_loading();
        lex_warnings.extend(context_failures.into_iter().map(|(path, failure)| {
            format!(
                "warning: {}:{}:{}: {}",
                path.display(),
                failure.line,
                failure.col,
                failure.message
            )
        }));

        // Largest files first, so the longest checks don't start last; of
        // equal ones, the first listed first.
        let mut by_size: Vec<usize> = (0..analyze_paths.len()).collect();
        by_size.sort_by_key(|&index| std::cmp::Reverse(analyze_sizes[index]));
        let mut analyze_paths: Vec<Option<PathBuf>> = analyze_paths.into_iter().map(Some).collect();
        let analyze_files_by_size = by_size
            .into_iter()
            .filter_map(|index| analyze_paths[index].take())
            .collect();
        Loaded {
            project,
            analyze_files_by_size,
            skipped_analyze,
            skipped_context,
            context_loaded: context_count - skipped_context,
            parse_failures,
            lex_warnings,
        }
    }

    /// Reads, parses and inserts into `project` the files `context_files`
    /// and `analyze_files`, which are spelled from the root: the context
    /// files first, so an analysed file's declaration wins a name
    /// collision, each group in the order given. The load of a whole
    /// session and the files an edit of the configuration adds are loaded
    /// by this. The caller finishes loading.
    pub(crate) fn load_into(
        &self,
        project: &mut Project,
        context_files: &[PathBuf],
        analyze_files: &[PathBuf],
    ) -> Added {
        let states = LoadStates::new(self.pool.current_num_threads());
        self.add_walked(
            project,
            &states,
            context_files.iter().cloned().map(Walked::bare).collect(),
            analyze_files.iter().cloned().map(Walked::bare).collect(),
        )
    }

    /// `load_into` of files that may come parsed, which are used as they
    /// are.
    fn add_walked(
        &self,
        project: &mut Project,
        states: &LoadStates,
        context: Vec<Walked>,
        analyze: Vec<Walked>,
    ) -> Added {
        let pool = &self.pool;
        let context_len = context.len();
        let total = context_len + analyze.len();
        let mut combined_files = Vec::with_capacity(total);
        let mut hashes = Vec::with_capacity(total);
        for walked in context.into_iter().chain(analyze) {
            combined_files.push(walked.path);
            hashes.push(walked.hash);
        }
        let context_files = &combined_files[..context_len];
        let analyze_files = &combined_files[context_len..];
        // Assign IDs before parsing so each declaration records its file
        // directly. The ID list is in the same context-then-analysed order
        // used by the merge and declaration prefix sums below.
        let numbering = super::file_table::FileTable::with_paths(
            combined_files
                .iter()
                .zip(&hashes)
                .map(|(path, hash)| (path.as_path(), *hash)),
        );
        let numbers = project.number_files(numbering);
        let parse = Parse { project, states };
        let mut combined_parsed: Vec<Read> = pool.install(|| {
            combined_files
                .par_iter()
                .zip(numbers.par_iter())
                .enumerate()
                .map(|(index, (path, &file_id))| {
                    self.parse_listed(parse, path, index < context_len, file_id)
                })
                .collect()
        });
        file_cache::trim();
        // Reserve declaration ids by file before the serial merge. Each
        // file owns a contiguous class range and a contiguous function
        // range, both in the established context-then-analysed load order.
        // Slots for declarations that lose collision/conditional selection
        // remain unused, preserving the merge policy while making every
        // file's reservation independent of worker completion order.
        let mut next_class_local = 0u32;
        let mut next_function_local = 0u32;
        let mut declaration_ids: Vec<Vec<(Option<u32>, Option<u32>)>> = combined_parsed
            .iter()
            .map(|read| {
                let Read::Loaded(file) = read else {
                    return Vec::new();
                };
                file.parsed
                    .built_decls
                    .iter()
                    .map(|decl| match decl {
                        crate::registry::BuiltDecl::Class { .. } => {
                            let local = next_class_local;
                            next_class_local = next_class_local
                                .checked_add(1)
                                .expect("project class id range exhausted");
                            (Some(local), None)
                        }
                        crate::registry::BuiltDecl::Function { .. } => {
                            let local = next_function_local;
                            next_function_local = next_function_local
                                .checked_add(1)
                                .expect("project function id range exhausted");
                            (None, Some(local))
                        }
                        crate::registry::BuiltDecl::GlobalConst { .. } => (None, None),
                    })
                    .collect()
            })
            .collect();
        let (context_numbers, analyze_numbers) = numbers.split_at(context_files.len());
        let analyze_parsed = combined_parsed.split_off(context_files.len());
        let analyze_declaration_ids = declaration_ids.split_off(context_files.len());
        let context_declaration_ids = declaration_ids;
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
        let mut context_failures = Vec::new();
        project.begin_reference_index_batch();
        for (((path, &id), parsed), declaration_ids) in context_files
            .iter()
            .zip(context_numbers)
            .zip(context_parsed)
            .zip(context_declaration_ids)
        {
            match parsed {
                Read::Loaded(file) => {
                    let ReadFile {
                        len,
                        source,
                        parsed,
                        cache_key: _,
                    } = *file;
                    note_lex_warnings(path, &parsed);
                    project.insert_numbered_file(
                        NumberedFile {
                            id,
                            path,
                            analyzed: false,
                            declaration_ids,
                        },
                        parsed,
                        source,
                        len,
                    )
                }
                Read::Unparsed(failure) => {
                    skipped_context += 1;
                    context_failures.push((path.clone(), failure));
                }
                Read::Unread => skipped_context += 1,
            }
        }
        // How long each analysed file is, by its place in `analyze_files`.
        let mut analyze_sizes: Vec<usize> = vec![0; analyze_files.len()];
        let mut skipped_analyze = 0usize;
        let mut parse_failures = Vec::new();
        for (index, (((path, &id), parsed), declaration_ids)) in analyze_files
            .iter()
            .zip(analyze_numbers)
            .zip(analyze_parsed)
            .zip(analyze_declaration_ids)
            .enumerate()
        {
            match parsed {
                Read::Loaded(file) => {
                    let ReadFile {
                        len,
                        source,
                        parsed,
                        cache_key: _,
                    } = *file;
                    note_lex_warnings(path, &parsed);
                    analyze_sizes[index] = len;
                    project.insert_numbered_file(
                        NumberedFile {
                            id,
                            path,
                            analyzed: true,
                            declaration_ids,
                        },
                        parsed,
                        source,
                        len,
                    )
                }
                Read::Unparsed(failure) => {
                    skipped_analyze += 1;
                    parse_failures.push((path.clone(), failure));
                }
                Read::Unread => skipped_analyze += 1,
            }
        }
        project.finish_reference_index_batch();
        project.rebuild_path_ranks();
        Added {
            analyze_sizes,
            skipped_analyze,
            skipped_context,
            parse_failures,
            context_failures,
            lex_warnings,
        }
    }

    /// Checks every analysed file of `loaded` and runs the project-wide
    /// passes.
    pub fn analyse(&self, loaded: Loaded, records: super::Records) -> AnalysisResult {
        let Loaded {
            mut project,
            analyze_files_by_size,
            skipped_analyze,
            skipped_context,
            context_loaded,
            parse_failures,
            lex_warnings,
        } = loaded;

        crate::heap::check_started();
        let Checked {
            results,
            whole_program,
            check_panics,
            type_dump,
            expr_type_dump,
            doc_resolution_dump,
        } = self.check_loaded(
            &mut project,
            &analyze_files_by_size,
            parse_failures,
            records,
        );
        crate::heap::check_finished();
        AnalysisResult {
            project,
            finalized: false,
            results,
            whole_program,
            checked: analyze_files_by_size.len() - skipped_analyze - check_panics.len(),
            skipped_analyze,
            skipped_context,
            context_loaded,
            check_panics,
            lex_warnings,
            type_dump,
            expr_type_dump,
            doc_resolution_dump,
        }
    }

    /// Checks the analysed `files` of `project`, which keeps what its
    /// whole-program checks read of them, reports the `parse_failures` and
    /// runs the project-wide passes.
    pub(crate) fn check_loaded(
        &self,
        project: &mut Project,
        files: &[PathBuf],
        parse_failures: impl IntoIterator<Item = (PathBuf, ParseFailure)>,
        records: super::Records,
    ) -> Checked {
        let needs_tier_2 =
            self.settings.rules.check_dead_enum_cases || self.settings.rules.check_dead_members;
        let checked_results = project.check_files(files, Some(&self.pool), needs_tier_2, records);

        let mut results: FxHashMap<PathBuf, Vec<Diagnostic>> = FxHashMap::default();
        let mut check_panics: Vec<(PathBuf, String)> = Vec::new();
        let mut type_dumps = Vec::new();
        let mut expr_type_dumps = Vec::new();
        let mut doc_resolution_dumps = Vec::new();
        for (path, outcome) in checked_results {
            match outcome {
                Ok(mut check) => {
                    project.keep_analysed_check(&path, &mut check, needs_tier_2);
                    if records.types {
                        type_dumps.push((path.clone(), check.type_dump));
                    }
                    if records.expr_types {
                        expr_type_dumps.push((path.clone(), check.expr_types));
                    }
                    if records.doc_resolutions {
                        doc_resolution_dumps.push((path.clone(), check.doc_resolutions));
                    }
                    results.insert(path, check.diagnostics);
                }
                Err(msg) => check_panics.push((path, msg)),
            }
        }
        check_panics.sort();
        let type_dump = records.types.then(|| by_path(type_dumps));
        let expr_type_dump = records.expr_types.then(|| by_path(expr_type_dumps));
        let doc_resolution_dump = records
            .doc_resolutions
            .then(|| by_path(doc_resolution_dumps));
        if needs_tier_2 {
            project.mark_member_index_built();
        }

        for (path, failure) in parse_failures {
            results.entry(path).or_default().push(failure.diagnostic());
        }
        let whole_program = self.whole_program(project);
        Checked {
            results,
            whole_program,
            check_panics,
            type_dump,
            expr_type_dump,
            doc_resolution_dump,
        }
    }

    /// The diagnostics of the checks that read the whole of `project` as
    /// it is, where no file's check sees enough: the dead code, the
    /// namespace cycles, the renamed named arguments, the mock and
    /// Composer hygiene, the class shapes, the banned constructs and the
    /// architecture rules.
    pub(crate) fn whole_program(
        &self,
        project: &mut Project,
    ) -> FxHashMap<PathBuf, Vec<Diagnostic>> {
        let mut results: FxHashMap<PathBuf, Vec<Diagnostic>> = FxHashMap::default();
        let mut extend = |found: FxHashMap<PathBuf, Vec<Diagnostic>>| {
            for (path, diags) in found {
                results.entry(path).or_default().extend(diags);
            }
        };
        extend(project.check_whole_program());
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
        results
    }
}

/// Each file's record lines, each after the file's path, files in path
/// order.
fn by_path(mut files: Vec<(PathBuf, Vec<String>)>) -> String {
    files.sort_by(|a, b| a.0.cmp(&b.0));
    let mut text = String::new();
    for (path, lines) in files {
        for line in lines {
            text.push_str(&format!("{}:{line}\n", path.display()));
        }
    }
    text
}

/// What `Session::dump_front_end` wrote.
pub struct FrontEndDump {
    /// One for each file and role.
    pub files: usize,
    /// The unions recorded.
    pub unions: usize,
}

/// The edit replay's dump of the registry, of the diagnostics held, and of
/// the whole-program checks' diagnostics.
const REPLAY_REGISTRY: &str = "registry";
const REPLAY_DIAGNOSTICS: &str = "diagnostics";
const REPLAY_WHOLE_PROGRAM: &str = "whole-program";

/// What a section of a replay's dumps is beside the dump it is compared
/// with.
const SAME: &str = "same";
const DIFFERS: &str = "differs";

/// Writes the dump `kind` (`dumps`) to `text`, its manifest beside it, as
/// `write` prints it; given `reference`, the manifest of another run of it,
/// only the entities that differ from that run's.
pub fn write_dump(
    text: &Path,
    kind: dumps::Kind,
    reference: Option<&Path>,
    write: impl FnOnce(&mut dyn std::io::Write) -> std::io::Result<()>,
) -> Result<(), String> {
    let context = |e: std::io::Error| format!("{}: {e}", text.display());
    let mut writer = dumps::DumpWriter::create(text, kind, reference).map_err(context)?;
    write(&mut writer).map_err(context)?;
    writer.finish().map_err(context)?;
    Ok(())
}

/// What each section of the dumps `step` in `out` is beside the same dumps
/// of `against`: each section of the registry dump, the diagnostics held
/// and the whole-program checks' (`SAME`, `DIFFERS`). Read from the
/// manifests.
fn replay_verdict(
    out: &Path,
    step: &str,
    against: &str,
) -> std::io::Result<Vec<(String, &'static str)>> {
    let open = |name: &str, dump: &str| {
        dumps::Dump::open(&dumps::manifest_path(
            &out.join(format!("{name}.{dump}.txt")),
        ))
    };
    let (step_registry, against_registry) = (
        open(step, REPLAY_REGISTRY)?,
        open(against, REPLAY_REGISTRY)?,
    );
    let sections = |dump: &dumps::Dump| -> Vec<(String, Vec<usize>)> {
        let mut sections: Vec<(String, Vec<usize>)> = Vec::new();
        for (index, entry) in dump.manifest.entries.iter().enumerate() {
            let name = entry.path.split('/').next().unwrap_or_default();
            if name.starts_with('.') {
                continue;
            }
            match sections.last_mut() {
                Some((last, entries)) if last == name => entries.push(index),
                _ => sections.push((name.to_string(), vec![index])),
            }
        }
        sections
    };
    let against_sections = sections(&against_registry);
    let mut states = Vec::new();
    for (name, entries) in sections(&step_registry) {
        let against_entries = against_sections
            .iter()
            .find(|(against_name, _)| *against_name == name)
            .map(|(_, entries)| entries.clone());
        let alike = |a: usize, b: usize, step: &dumps::Dump, against: &dumps::Dump| {
            let (a, b) = (&step.manifest.entries[a], &against.manifest.entries[b]);
            (a.path == b.path, a.id == b.id)
        };
        let state = match against_entries {
            Some(against_entries)
                if against_entries.len() == entries.len()
                    && entries
                        .iter()
                        .zip(&against_entries)
                        .all(|(&a, &b)| alike(a, b, &step_registry, &against_registry).0) =>
            {
                if entries
                    .iter()
                    .zip(&against_entries)
                    .all(|(&a, &b)| alike(a, b, &step_registry, &against_registry).1)
                {
                    SAME
                } else {
                    DIFFERS
                }
            }
            _ => DIFFERS,
        };
        states.push((format!("registry {name}"), state));
    }
    for (name, dump) in [
        ("diagnostics", REPLAY_DIAGNOSTICS),
        ("whole-program", REPLAY_WHOLE_PROGRAM),
    ] {
        let state = if open(step, dump)?.manifest.root == open(against, dump)?.manifest.root {
            SAME
        } else {
            DIFFERS
        };
        states.push((name.to_string(), state));
    }
    Ok(states)
}
