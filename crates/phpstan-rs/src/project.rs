use rayon::prelude::*;
use rustc_hash::{FxHashMap, FxHashSet};
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use crate::checker::constant_resolver::ConstantResolver;
use crate::checker::rule_level_helper::RuleLevelHelper;
use crate::checker::settings::CheckSettings;
use crate::checker::{self, Checker, PropertyAssignFacts};
use crate::deps;
use crate::file_id::{FileId, FileIdSet};
use crate::ignore_comments;
use crate::pest_config::{PestConfig, PestFacts};
use crate::registry::scope::Scope;
use crate::registry::stubs;
use crate::registry::{
    BodySource, BuiltDecl, ClassId, DeclKind, FileBodies, FnId, GlobalFacts, Origin, Provenance,
    Registry,
};
use composer::dir_table::{DirTable, EntryKind};
use declared_classes::DeclaredClass;
use diagnostics::{self, Diagnostic, DiagnosticBuilder};
use syntax::FileFacts;
use syntax::ast::{StmtKind, StmtNode};
use syntax::declarations::Declared;
use syntax::lexer::LexWarning;
use syntax::parser::ParseError;
use syntax::span::{LineIndex, Span};
use types::Name;

mod architecture;
mod banned;
mod class_shape;
mod composer_hygiene;
pub(crate) mod declared_classes;
pub(crate) mod duplicate_class_sites;
mod duplicate_classes;
pub(crate) mod duplicate_fn_sites;
mod duplicate_functions;
mod edit_replay;
mod file_cache;
mod file_declarations;
mod file_table;
mod front_end_dump;
mod mock_hygiene;
mod registry_dump;
pub mod roles;
pub mod session;
pub mod source;
#[cfg(test)]
pub(crate) mod test_support;
pub(crate) mod use_sites;
mod whole_program;

use duplicate_class_sites::{DuplicateClassIndex, DuplicateClassSites};
use duplicate_fn_sites::{DuplicateFunctionIndex, DuplicateFunctionSites};
use use_sites::UseSiteIndex;

/// The files `Project::walk_affected` has reached, with what it has walked,
/// so that it can go on from more changed keys without starting again.
#[derive(Default)]
struct AffectedWalk {
    affected: FxHashSet<FileId>,
    seen: FxHashSet<deps::RefKey>,
    stub_files: StubFileSet,
}

/// A set of stub files, by number, which is added to a file's closure at a
/// time.
#[derive(Default)]
struct StubFileSet {
    words: Vec<u64>,
}

impl StubFileSet {
    /// Adds every stub file `file` leads to (`stubs::stub_file_closure`),
    /// calling `visit` with each that was not in the set.
    fn add_closure(&mut self, file: u32, mut visit: impl FnMut(u32)) {
        let closure = stubs::stub_file_closure(file);
        if self.words.len() < closure.len() {
            self.words.resize(closure.len(), 0);
        }
        for (at, (have, add)) in self.words.iter_mut().zip(closure).enumerate() {
            let mut new = add & !*have;
            *have |= new;
            while new != 0 {
                let bit = new.trailing_zeros();
                new &= new - 1;
                visit((at * 64) as u32 + bit);
            }
        }
    }
}

/// One file's parsed state, kept around so `Project::update_file` can undo
/// exactly what that file previously contributed - both to the shared
/// registry and to `dependents` below - before re-indexing its new parse.
/// That undo/redo pair is the difference between an incremental update and
/// a full reload.
pub(crate) struct FileState {
    // The AST and all file-local data its node IDs refer into. An analysed
    // file's tree is kept, as is a document an editor has open; a context
    // file's tree is parsed again by `ast_of` for a later reader.
    kept_tree: Option<SyntaxTree>,
    // The source's length in bytes: what checking the file costs, roughly.
    source_len: usize,
    // Where the file came in the order files were loaded, which a rebuild of
    // a name declared in several of them follows; an edited file keeps it.
    load_order: u64,
    functions: Vec<Name>,
    classes: Vec<Name>,
    // The global constants it declares (`const`, `define()`).
    constants: Vec<Name>,
    // Whether it declares or references a `PHPStan\` name.
    mentions_phpstan: bool,
    references: Box<[deps::RefKey]>,
    // The names its declarations themselves mention
    // (`deps::declaration_referenced_names`).
    declaration_references: Box<[deps::RefKey]>,
    // Inline `@phpstan-ignore*` comments, read while the file was lexed
    // (the AST carries no comment text). Only an analysed file's are read,
    // so they are kept with its tree, and a context file's of a batch run
    // are not, as its cached entry holds none.
    ignores: ignore_comments::IgnoreComments,
    // The registry names of the file's anonymous classes.
    anonymous_classes: Vec<Name>,
    // The text of the file as an editor has it, kept only while the file
    // is open and `Project::retain_source` is on (see that field's own doc
    // comment) - `None` for every other file, and in every ordinary
    // `check-project` batch run.
    source: Option<String>,
    // Whether the file is checked:
    // `true` for a real, first-party file being checked, `false` for
    // context (vendor/scan/stub, never analyzed or reported). Recorded
    // per-file so `Project::is_analyzed`/`analyzed_paths` can answer
    // without threading extra state through every caller.
    analyzed: bool,
    // The composer package this file belongs to (`Project::packages`).
    owner: Option<composer::PackageOwner>,
    // Its function and method bodies, which the declarations' provenance
    // finds.
    bodies: FileBodies,
    // What it assigns to globals, before the registry says what the values
    // are.
    global_facts: Arc<GlobalFacts>,
    // Its declarations as built, which the registry's are rebuilt from when
    // a name they declare changes.
    built_decls: Vec<BuiltDecl>,
    // Their per-file typed arena storage, retained for id-backed registry
    // access and shared with the parse cache.
    file_decls: Arc<crate::registry::FileDecls>,
    // Stable project-local class/function ids, parallel to `built_decls`.
    declaration_ids: Vec<(Option<u32>, Option<u32>)>,
    // What it binds, when it is a `Pest.php`.
    pest_facts: Option<PestFacts>,
    // The classes it declares unconditionally, as the renamed-argument
    // check reads them.
    declared_classes: Vec<DeclaredClass>,
    // What is true of the file as a whole (`syntax::FileFacts`).
    facts: FileFacts,
    // Its lines, which the lines and columns of its spans are found by.
    line_index: Arc<LineIndex>,
    // What the bytes it was parsed from are.
    content_hash: source::ContentHash,
}

/// The file-owned syntax data. Keeping the source, name table and node
/// side tables with the tree makes node IDs meaningful only inside their
/// owning file and gives the AST one storage boundary for the arena
/// migration.
#[cfg_attr(test, derive(Clone))]
pub(crate) struct SyntaxTree {
    stmts: Vec<StmtNode>,
    #[allow(dead_code)] // Read by source-backed payloads in rows 9.3-9.4.
    source: Box<[u8]>,
    #[allow(dead_code)] // Read by `NameId`-based readers in row 9.3.
    names: Box<[Name]>,
    #[allow(dead_code)] // Read by ID-based diagnostic and editor readers.
    expr_spans: Box<[Span]>,
    #[allow(dead_code)] // Read by ID-based diagnostic and editor readers.
    stmt_spans: Box<[Span]>,
}

impl SyntaxTree {
    pub(crate) fn statements(&self) -> &[StmtNode] {
        &self.stmts
    }

    #[allow(dead_code)] // Used as payloads become source ranges in row 9.4.
    pub(crate) fn source_slice(&self, span: Span) -> Option<&[u8]> {
        self.source.get(span.start as usize..span.end as usize)
    }

    #[allow(dead_code)] // Used by file-scoped NameId readers in row 9.3.
    pub(crate) fn name(&self, id: syntax::name::NameId) -> Option<&Name> {
        self.names.get(u32::from(id.into_raw()) as usize)
    }

    #[allow(dead_code)] // Used by file-scoped NameId readers in row 9.3.
    pub(crate) fn ident_name(&self, ident: &syntax::ast::Ident) -> Option<&Name> {
        self.name(ident.name_id?)
    }

    #[allow(dead_code)] // Used by file-scoped expression readers in row 9.3.
    pub(crate) fn expr_span(&self, id: syntax::ast::ExprId) -> Option<Span> {
        self.expr_spans
            .get(u32::from(id.into_raw()) as usize)
            .copied()
    }

    #[allow(dead_code)] // Used by file-scoped statement readers in row 9.3.
    pub(crate) fn stmt_span(&self, id: syntax::ast::StmtId) -> Option<Span> {
        self.stmt_spans
            .get(u32::from(id.into_raw()) as usize)
            .copied()
    }
}

impl syntax::canonical::Canonical for SyntaxTree {
    fn canonical(&self, p: &mut syntax::canonical::Printer) {
        syntax::canonical::Canonical::canonical(&self.stmts, p);
    }
}

struct DeclarationSource<'a> {
    analyzed: bool,
    load_order: u64,
    path: &'a Path,
    declarations: &'a [BuiltDecl],
    declaration_ids: &'a [(Option<u32>, Option<u32>)],
    pending: bool,
}

/// A file's tree: the one it kept, or one parsed again.
pub(crate) enum AstRef<'a> {
    Kept(&'a [StmtNode]),
    Parsed(Arc<Vec<StmtNode>>),
}

impl std::ops::Deref for AstRef<'_> {
    type Target = [StmtNode];

    fn deref(&self) -> &[StmtNode] {
        match self {
            AstRef::Kept(ast) => ast,
            AstRef::Parsed(ast) => ast,
        }
    }
}

/// The trees of the context files most recently parsed again, which a pass
/// that reads a few of them (the files that declare a class writing a
/// private property) asks for more than once.
#[derive(Default)]
struct AstCache(std::collections::VecDeque<(PathBuf, Arc<Vec<StmtNode>>)>);

impl AstCache {
    const CAPACITY: usize = 32;

    fn get(&mut self, path: &Path) -> Option<Arc<Vec<StmtNode>>> {
        let at = self.0.iter().position(|(cached, _)| cached == path)?;
        let entry = self.0.remove(at)?;
        let ast = Arc::clone(&entry.1);
        self.0.push_back(entry);
        Some(ast)
    }

    fn insert(&mut self, path: PathBuf, ast: Arc<Vec<StmtNode>>) {
        if self.0.len() == Self::CAPACITY {
            self.0.pop_front();
        }
        self.0.push_back((path, ast));
    }
}

impl FileState {
    /// The file's tree, for a reader of a file whose tree is kept: an
    /// analysed file's. A test build asserts that no reader asks for the
    /// tree of a file that has none.
    fn ast(&self) -> &[StmtNode] {
        debug_assert!(
            self.kept_tree.is_some(),
            "a reader asked for the tree of a context file that dropped it"
        );
        self.kept_tree.as_ref().map_or(&[], SyntaxTree::statements)
    }

    /// The file's tree, when it is kept.
    fn tree(&self) -> Option<&[StmtNode]> {
        self.kept_tree.as_ref().map(SyntaxTree::statements)
    }
}

/// The loaded files' function and method bodies, found by provenance.
impl BodySource for file_table::FileTable<FileState> {
    fn summary(&self, provenance: &Provenance) -> Option<&Arc<crate::registry::BodySummary>> {
        self.get_by_id(provenance.file_id()?)?
            .bodies
            .summary(provenance.declaration_span())
    }

    fn init_flow(&self, provenance: &Provenance) -> Option<&Arc<crate::registry::InitFlow>> {
        self.get_by_id(provenance.file_id()?)?
            .bodies
            .init_flow(provenance.declaration_span())
    }

    fn hook_summary(
        &self,
        provenance: &Provenance,
        set: bool,
    ) -> Option<crate::registry::bodies::HookBody<'_>> {
        self.get_by_id(provenance.file_id()?)?
            .bodies
            .hook_summary(provenance.declaration_span(), set)
    }

    fn unset_properties(&self, out: &mut crate::registry::bodies::UnsetProperties) {
        for file in self.values() {
            file.bodies.unset_properties(out);
        }
    }
}

/// A multi-file PHP project: every loaded file's own parse tree, one
/// registry merged across all of them (so a class in one file can extend a
/// class declared in another), and a reverse index - `dependents[name]` is
/// every file that *references* `name` - used to find which other files
/// need re-checking when a file's declarations change.
///
/// `Session::load_into` is for the initial, full load of a codebase; `update_file`
/// is the incremental path: re-parsing and re-checking only the file that
/// changed, plus the files reading a declaration whose resolved form the
/// change altered (see `affected_files`) - every other file's parse tree
/// and registry entries are untouched.
pub struct Project {
    files: file_table::FileTable<FileState>,
    dependents: ReferenceIndex,
    // `dependents` for the names files' declarations themselves mention
    // (`deps::declaration_referenced_names`).
    signature_dependents: ReferenceIndex,
    defer_reference_index: bool,
    pending_use_sites: Vec<(PathBuf, Vec<(Span, use_sites::RefTarget)>)>,
    pending_built_declarations: Vec<(crate::registry::BuiltDecl, bool, Option<u32>, Option<u32>)>,
    pending_global_constants: Vec<crate::registry::BuiltDecl>,
    pending_phpstan_class_files: Vec<(u64, u32, Name, PathBuf)>,
    pending_stale_files: Vec<FileId>,
    registry: Registry,
    sql: sql::context::SqlContext,
    // Directory-scoped Pest `$this`-binding config, rebuilt from every
    // currently-loaded `Pest.php` file whenever the loaded file set could
    // have changed (`finish_loading`/`update_file`) - see `pest_config`'s
    // own module doc comment for the full resolution algorithm.
    pest_config: PestConfig,
    // The directory relative include paths resolve against first, as PHP
    // resolves them against the working directory. `None` means the
    // process's own working directory.
    working_dir: Option<PathBuf>,
    // `working_dir` canonicalized, which
    // `absolute_path` resolves analysed paths against.
    absolute_root: Option<PathBuf>,
    // What a file is, by the roots it is under (`classify`).
    roles: roles::FileRoles,
    // The composer packages and which directories they own; each file's
    // owner is kept in its `FileState`.
    packages: composer::PackageIndex,
    // What the checks are configured with (`apply_settings`).
    settings: CheckSettings,
    // Whether the project serves an editor: the text of each open document
    // is kept in its `FileState` (see that field's own doc comment) and
    // every file's use sites are indexed as it is parsed - off by default.
    // Only `lsp.rs`'s `Server` turns this on: hover/definition's own
    // text-split technique (`lsp::text_split`) needs a known span's exact
    // source text, including whatever an editor's `didChange` most
    // recently sent - not necessarily what's on disk.
    retain_source: bool,
    // `ProjectSettings::keeps_as_built`.
    keeps_as_built: bool,
    // The documents an editor has open, whose trees and text are kept
    // whatever the role of the file (`open_file`).
    open: FxHashSet<PathBuf>,
    // Whether the finishing passes have run.
    finished: bool,
    // Whether PHPStan's source stays merged in whatever the loaded files
    // mention: for the project building the registry that has it.
    phpstan_pinned: bool,
    // How many files hold a place in the load order, which is dense: the
    // places are `0..next_load_order`, the context files' first.
    next_load_order: u64,
    // Path-order rank, indexed by the stable FileId slot. Rebuilt once after
    // a load or edit so whole-program merges compare integers.
    path_ranks: Vec<usize>,
    // Monotonic high-water marks for project-local declaration ids. Deleted
    // ids are never recycled, so stale handles can never alias new symbols.
    next_project_class_local: u32,
    next_project_function_local: u32,
    // The files loaded or edited since the finishing passes last walked
    // them for global variables.
    globals_stale: FxHashSet<FileId>,
    // The files a cancelled recheck never reached, whose last diagnostics
    // are therefore stale (`take_unchecked`).
    unchecked: Vec<PathBuf>,
    // Every top-level function's own unconditional declaration site(s) -
    // always populated (no `retain_source` gate), the same indexing-side/
    // check-side split
    // `duplicate_class_sites` below uses: its own gate is at the check
    // side, `RulesSettings::check_duplicate_function_declarations`, not the
    // indexing side. See `DuplicateFunctionSites`'s own doc comment.
    duplicate_fn_sites: DuplicateFunctionIndex,
    // The class-like counterpart to `duplicate_fn_sites` above - always
    // populated too (its own gate is at the check side,
    // `RulesSettings::check_duplicate_class_declarations`, not the indexing
    // side, so an opt-out project pays only the cost of an unused table,
    // not a missing one it might later want turned on incrementally).
    // See `DuplicateClassSites`'s own doc comment.
    duplicate_class_sites: DuplicateClassIndex,
    // Initial loads collect parse-local duplicate sites and build the
    // project-wide, FileId-keyed indexes in parallel at the batch boundary.
    pending_duplicate_fn_sites: Vec<(FileId, DuplicateFunctionSites)>,
    pending_duplicate_class_sites: Vec<(FileId, DuplicateClassSites)>,
    // `textDocument/references`/`textDocument/rename`'s own whole-project
    // use-site index - see `use_sites`'s own module doc comment for the
    // two-tier split. Tier 1 (class/function references) is one product of
    // each file's parse, indexed as the files load when the project
    // retains source or checks dead code (`indexes_use_sites`). Tier 2
    // (member references) is NOT populated by ordinary indexing at all -
    // see `member_index`.
    use_sites: UseSiteIndex,
    // Which files Tier 2 (member references) of `use_sites` and
    // `escapes` cover - none until the first `ensure_member_index_built`
    // call or the analysis seeds them. Deliberately NOT built as part
    // of `finish_loading` or a load: Tier 2 needs one real `Checker`
    // pass per file, which would make every workspace load pay a cost
    // only `textDocument/references`/`textDocument/rename` actually
    // need - see `ensure_member_index_built`'s own doc comment.
    member_index: MemberIndex,
    // The trees of context files re-parsed on demand, the latest few.
    ast_cache: Mutex<AstCache>,
    // Each file's places its last check looked for a file an `include` or
    // `require` names, made absolute (`absolute_path`), sorted: a file
    // appearing or vanishing at one changes what the check reports. Kept
    // only for a project that is changed after it is built.
    include_probes: Mutex<FxHashMap<PathBuf, Vec<PathBuf>>>,
    // What the last check of each file the member index covers let
    // escape, for the files where it let anything: kept with Tier 2 of
    // `use_sites`, and replaced with it at each recheck.
    escapes: FxHashMap<PathBuf, Escapes>,
    // Each analysed file's named-argument call sites against overridable
    // methods, from its last check, for the files that have any: kept by
    // the analysis (`keep_analysed_check`), `ensure_member_index_built` and
    // each recheck, so whole only once one of the first two has run.
    named_argument_calls: FxHashMap<PathBuf, checker::NamedArgumentCallFacts>,
}

/// The files a project's member index covers: Tier 2 of its use sites and
/// what its checks let escape.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MemberIndex {
    /// None: nothing has asked for it yet.
    Unbuilt,
    /// The analysed files, as the analysis seeds it.
    Analysed,
    /// Every loaded file, as `ensure_member_index_built` builds it.
    Every,
}

syntax::canonical_enum!(MemberIndex {
    Unbuilt,
    Analysed,
    Every,
});

/// What one check let escape the names it reads: the `Enum` classes whose
/// cases (`Checker::escaped_enums`) and the classes whose members
/// (`Checker::escaped_member_classes`) it may reach by a name it does not
/// write, which the dead enum case and dead member checks count as used.
#[derive(Debug, Default)]
struct Escapes {
    enums: FxHashSet<String>,
    member_classes: FxHashSet<String>,
}

syntax::canonical_struct!(Escapes {
    enums,
    member_classes
});

/// The stack each checking thread starts with. Deep walks grow their stack
/// on demand (`syntax::stack`), so this only spares ordinary code those
/// growths.
pub const WORKER_STACK_SIZE: usize = 64 * 1024 * 1024;

/// The process's pool of one `WORKER_STACK_SIZE` thread per core (or
/// `PHPSTAN_RS_THREADS`, for a harness running many projects at once,
/// where the cores are already busy), which runs every parallel parse,
/// check and registry pass - one pool, so concurrent sessions and
/// projects share the cores rather than each starting threads of its own.
pub fn worker_pool() -> Arc<rayon::ThreadPool> {
    static POOL: std::sync::LazyLock<Arc<rayon::ThreadPool>> = std::sync::LazyLock::new(|| {
        let threads = std::env::var("PHPSTAN_RS_THREADS")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(0);
        Arc::new(
            rayon::ThreadPoolBuilder::new()
                .num_threads(threads)
                .stack_size(WORKER_STACK_SIZE)
                .build()
                .expect("failed to build the worker pool"),
        )
    });
    Arc::clone(&POOL)
}

/// The result of `Project::check_paths_isolating_panics`: successful
/// per-file diagnostics, plus every path whose own check panicked
/// (message, not a diagnostic list) instead of completing.
pub type CheckPathsIsolatingPanics = (FxHashMap<PathBuf, Vec<Diagnostic>>, Vec<(PathBuf, String)>);

/// Everything one main-pass check of a file produces.
pub(crate) struct FileCheck {
    pub(crate) diagnostics: Vec<Diagnostic>,
    /// Member-reference sites; empty unless member references were
    /// collected.
    pub(crate) member_refs: Vec<(Span, String, String)>,
    /// The `Enum` classes the file let escape (`Checker::escaped_enums`).
    pub(crate) escaped_enums: FxHashSet<String>,
    /// The classes the file let escape through a dynamic member access
    /// (`Checker::escaped_member_classes`).
    pub(crate) escaped_member_classes: FxHashSet<String>,
    /// Named-argument call sites against overridable methods.
    pub(crate) named_argument_calls: checker::NamedArgumentCallFacts,
    /// The `--dump-types` lines (`line:col [host] vars`, one per
    /// statement); empty unless requested.
    pub(crate) type_dump: Vec<String>,
    /// The expression-type record's lines (`Checker::take_expr_types`);
    /// empty unless requested.
    pub(crate) expr_types: Vec<String>,
    /// The PHPDoc resolution record's lines
    /// (`Checker::take_doc_resolutions`); empty unless requested.
    pub(crate) doc_resolutions: Vec<String>,
    /// The file's writes to private properties.
    property_assign_facts: PropertyAssignFacts,
    /// The file's private property types, until judged against the
    /// writes of every file (`Project::judge_property_types`).
    property_type_sites: Vec<checker::PropertyTypeSite>,
    /// The places the file's includes looked for a file
    /// (`Checker::include_probes`).
    include_probes: Vec<PathBuf>,
}

impl FileCheck {
    fn empty() -> FileCheck {
        FileCheck {
            diagnostics: Vec::new(),
            member_refs: Vec::new(),
            escaped_enums: FxHashSet::default(),
            escaped_member_classes: FxHashSet::default(),
            named_argument_calls: FxHashMap::default(),
            type_dump: Vec::new(),
            expr_types: Vec::new(),
            doc_resolutions: Vec::new(),
            property_assign_facts: FxHashMap::default(),
            property_type_sites: Vec::new(),
            include_probes: Vec::new(),
        }
    }
}

/// The records a check keeps of each file, beside its diagnostics.
#[derive(Debug, Clone, Copy, Default)]
pub struct Records {
    /// `--dump-types`: every tracked variable's type after each statement.
    pub types: bool,
    /// Every expression's inferred and native type in the final pass.
    pub expr_types: bool,
    /// Every PHPDoc resolution.
    pub doc_resolutions: bool,
}

/// What checking a file came to: its check, or the message it panicked
/// with.
pub(crate) type CheckOutcome = Result<FileCheck, String>;

/// A panic payload's message.
pub(crate) fn panic_message(payload: Box<dyn std::any::Any + Send>) -> String {
    payload
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| payload.downcast_ref::<&str>().map(|s| s.to_string()))
        .unwrap_or_else(|| "unknown panic".to_string())
}

/// `f` over `items`, in parallel on `pool` when there is one, the results
/// in `items`' order.
fn map_on_pool<T: Sync, R: Send>(
    pool: Option<&rayon::ThreadPool>,
    items: &[T],
    f: impl Fn(&T) -> R + Sync + Send,
) -> Vec<R> {
    match pool {
        Some(pool) => pool.install(|| items.par_iter().map(&f).collect()),
        None => items.iter().map(f).collect(),
    }
}

/// `map_on_pool`, with the costliest items started first: a file as large
/// as the rest of a run's combined, started last, would run alone after the
/// others are done. The results are still in `items`' order.
fn map_on_pool_largest_first<T: Sync, R: Send>(
    pool: Option<&rayon::ThreadPool>,
    items: &[T],
    cost: impl Fn(&T) -> usize,
    f: impl Fn(&T) -> R + Sync + Send,
) -> Vec<R> {
    let Some(pool) = pool else {
        return items.iter().map(f).collect();
    };
    let mut order: Vec<usize> = (0..items.len()).collect();
    order.sort_by_key(|&i| std::cmp::Reverse(cost(&items[i])));
    let done: Vec<(usize, R)> = pool.install(|| {
        order
            .par_iter()
            .with_max_len(1)
            .map(|&i| (i, f(&items[i])))
            .collect()
    });
    let mut results: Vec<Option<R>> = items.iter().map(|_| None).collect();
    for (i, result) in done {
        results[i] = Some(result);
    }
    results
        .into_iter()
        .map(|result| result.expect("every item was mapped"))
        .collect()
}

/// Everything a project is configured with, applied together
/// (`Project::apply_settings`).
#[derive(Default, Clone, PartialEq)]
pub struct ProjectSettings {
    /// The directory relative include paths resolve against first, as PHP
    /// resolves them against the working directory; `None` means the
    /// process's own.
    pub working_dir: Option<PathBuf>,
    /// `working_dir` canonicalized, which analysed paths resolve against.
    pub absolute_root: Option<PathBuf>,
    /// The roots that decide each file's role.
    pub roles: roles::FileRoles,
    /// The composer packages, and which directories each owns.
    pub packages: composer::PackageIndex,
    pub php_versions: Option<types::PhpVersions>,
    /// Whether the project consumes PHPStan as a library, which the
    /// PHPStan-API rules apply to.
    pub phpstan_api_check_enabled: bool,
    /// Keeps the text of each open document (the language server's hover
    /// and definition need it) and indexes every file's use sites as it is
    /// parsed.
    pub retain_source: bool,
    /// Whether the project keeps a copy of its declarations as built, for
    /// recomputing them after a change: the language server's, not a batch
    /// run's, which is built once and never changed.
    pub keeps_as_built: bool,
    pub rules: crate::rules_settings::RulesSettings,
    /// Which extensions are active; each extension's checks default to it.
    pub extensions: crate::ext::ActiveExtensions,
    pub extras: crate::ext::extras::ExtrasSettings,
    pub strict_rules: crate::ext::strict_rules::StrictRulesSettings,
    pub shipmonk: crate::ext::shipmonk::ShipmonkSettings,
    pub voku: crate::ext::voku::VokuSettings,
    pub slam: crate::ext::slam::SlamSettings,
    pub psr3: crate::ext::psr3::Psr3Settings,
    pub sidz: crate::ext::sidz::SidzSettings,
    pub phpunit: crate::ext::phpunit::PhpunitSettings,
    /// Every restricted symbol, the presets' included
    /// (`DisallowedCallsSettings::entries`).
    pub restricted_symbols: Vec<crate::restricted_symbols::RestrictedSymbolEntry>,
    pub doctrine: crate::ext::doctrine::DoctrineSettings,
    /// `EngineSettings::dynamic_constant_entries`.
    pub dynamic_constant_names: Vec<(String, Option<String>)>,
    pub overlays: crate::assertion_tables::OverlaySettings,
    pub sql_reflection: Option<sql::config::SqlReflectionConfig>,
    pub sql_schema: Option<sql::schema::SchemaCatalog>,
}

impl Project {
    /// A project holding the built-in stubs, resolved: a clone of a
    /// registry resolved once per process, sharing its tables.
    pub fn new() -> Self {
        Project::with_registry(Project::plain_base().clone())
    }

    /// The registry dump (`registry_dump`) of the project, its registry
    /// over the stub base it stands on.
    pub fn registry_dump(&self) -> String {
        self.registry_dump_comparing(crate::registry::canonical::CompareAt::Layers)
    }

    /// `registry_dump`, the registry compared with its base as `compare`
    /// says.
    fn registry_dump_comparing(&self, compare: crate::registry::canonical::CompareAt) -> String {
        registry_dump::registry_dump(self, self.dump_base(), compare)
    }

    /// `registry_dump`, written into `out` as it is printed.
    pub fn write_registry_dump(&self, out: &mut dyn std::io::Write) -> std::io::Result<()> {
        registry_dump::write_registry_dump(
            self,
            self.dump_base(),
            crate::registry::canonical::CompareAt::Layers,
            out,
        )
    }

    /// The stub base the project's registry stands on, by name.
    fn dump_base(&self) -> Option<(&'static str, &'static Registry)> {
        Project::loaded_bases()
            .into_iter()
            .find(|(_, base)| self.registry.shares_class_base(base))
    }

    /// Both stub bases whole, by name (`registry_dump::stub_bases_dump`).
    pub fn stub_bases_dump() -> String {
        registry_dump::stub_bases_dump(&Project::stub_bases())
    }

    /// `stub_bases_dump`, written into `out` as it is printed.
    pub fn write_stub_bases_dump(out: &mut dyn std::io::Write) -> std::io::Result<()> {
        registry_dump::write_stub_bases_dump(&Project::stub_bases(), out)
    }

    fn stub_bases() -> [(&'static str, &'static Registry); 2] {
        [
            (PLAIN_BASE_NAME, Project::plain_base()),
            (PHPSTAN_BASE_NAME, Project::phpstan_base()),
        ]
    }

    /// The stub bases this process has made so far, by name: a project's
    /// base is one of them, and none is made to look.
    fn loaded_bases() -> Vec<(&'static str, &'static Registry)> {
        [
            (PLAIN_BASE_NAME, PLAIN_BASE.get()),
            (PHPSTAN_BASE_NAME, PHPSTAN_BASE.get()),
        ]
        .into_iter()
        .filter_map(|(name, base)| Some((name, base?)))
        .collect()
    }

    /// The registry of the built-in stubs, resolved once per process.
    fn plain_base() -> &'static Registry {
        PLAIN_BASE.get_or_init(|| Project::resolved_stubs(false))
    }

    /// Starts loading the built-in stubs' registry and index on threads of
    /// their own and returns at once, for a caller that has other work to
    /// do before its first project: the loads are then done, or under
    /// way, when `Project::new` first asks. The registry with PHPStan's
    /// source is not loaded: only a project whose files mention PHPStan
    /// needs it.
    pub fn warm_stubs() {
        let spawn = |name: &str, load: fn()| {
            // A failure to start a thread only loses the head start.
            let _ = std::thread::Builder::new()
                .name(name.to_string())
                .stack_size(WORKER_STACK_SIZE)
                .spawn(load);
        };
        spawn("warm-stub-registry", || {
            Project::plain_base();
        });
        spawn("warm-stub-index", stubs::warm_index);
    }

    /// The registry of the built-in stubs, with PHPStan's own source when
    /// `with_phpstan`, resolved: loaded from the stub cache when it holds
    /// it, else built and stored there. Frozen, to share.
    fn resolved_stubs(with_phpstan: bool) -> Registry {
        let cache_name = if with_phpstan {
            "stub-base-phpstan"
        } else {
            "stub-base"
        };
        if let Some(stored) = Registry::load_cached(cache_name) {
            match stored {
                Ok(mut registry) => {
                    registry.freeze();
                    return registry;
                }
                Err(error) => {
                    debug_assert!(false, "the stored {cache_name} is inconsistent: {error}");
                    eprintln!(
                        "phpstan-rs: the stored {cache_name} is inconsistent ({error}); building it again"
                    );
                }
            }
        }
        let mut registry = Project::built_stubs(with_phpstan);
        if let Err(error) = registry.store_cached(cache_name) {
            eprintln!("phpstan-rs: the {cache_name} can't be stored ({error})");
        }
        registry.freeze();
        registry
    }

    /// The registry of the built-in stubs, with PHPStan's own source when
    /// `with_phpstan`, built and resolved: what the stub cache stores.
    pub(crate) fn built_stubs(with_phpstan: bool) -> Registry {
        let mut registry = Registry::default();
        if with_phpstan {
            registry.phpstan_stubs_loaded = true;
            registry.merge_stubs(stubs::generated_stubs());
            registry.merge_stub_classes(stubs::generated_stub_classes());
            registry.merge_stub_constants(stubs::generated_stub_constants());
        } else {
            registry.merge_stubs(stubs::builtin_stubs());
            registry.merge_stub_classes(stubs::builtin_stub_classes());
            registry.merge_stub_constants(stubs::builtin_stub_constants());
        }
        let mut base = Project::with_registry(registry);
        base.phpstan_pinned = with_phpstan;
        base.finish_loading();
        base.registry.resolve_stub_deferred_types();
        debug_assert!(
            !base.registry.has_deferred_types(),
            "the embedded stub registry must not store deferred types"
        );
        base.registry
    }

    fn with_registry(registry: Registry) -> Self {
        Project {
            files: file_table::FileTable::default(),
            dependents: ReferenceIndex::default(),
            signature_dependents: ReferenceIndex::default(),
            defer_reference_index: false,
            pending_use_sites: Vec::new(),
            pending_built_declarations: Vec::new(),
            pending_global_constants: Vec::new(),
            pending_phpstan_class_files: Vec::new(),
            pending_stale_files: Vec::new(),
            registry,
            sql: sql::context::SqlContext::default(),
            pest_config: PestConfig::default(),
            working_dir: None,
            absolute_root: None,
            roles: roles::FileRoles::default(),
            packages: composer::PackageIndex::default(),
            settings: CheckSettings::default(),
            retain_source: false,
            keeps_as_built: true,
            open: FxHashSet::default(),
            finished: false,
            unchecked: Vec::new(),
            phpstan_pinned: false,
            next_load_order: 0,
            path_ranks: Vec::new(),
            next_project_class_local: 0,
            next_project_function_local: 0,
            globals_stale: FxHashSet::default(),
            duplicate_fn_sites: DuplicateFunctionIndex::default(),
            duplicate_class_sites: DuplicateClassIndex::default(),
            pending_duplicate_fn_sites: Vec::new(),
            pending_duplicate_class_sites: Vec::new(),
            use_sites: UseSiteIndex::default(),
            member_index: MemberIndex::Unbuilt,
            ast_cache: Mutex::default(),
            include_probes: Mutex::default(),
            escapes: FxHashMap::default(),
            named_argument_calls: FxHashMap::default(),
        }
    }

    /// Configures the project with `settings`, replacing what it had.
    /// Each loaded file's package owner is recomputed.
    pub fn apply_settings(&mut self, settings: ProjectSettings) {
        let ProjectSettings {
            working_dir,
            absolute_root,
            roles,
            packages,
            php_versions,
            phpstan_api_check_enabled,
            retain_source,
            keeps_as_built,
            rules,
            extensions: active,
            extras,
            strict_rules,
            shipmonk,
            voku,
            slam,
            psr3,
            sidz,
            phpunit,
            restricted_symbols,
            doctrine,
            dynamic_constant_names,
            overlays,
            sql_reflection,
            sql_schema,
        } = settings;
        self.working_dir = working_dir;
        self.absolute_root = absolute_root;
        self.roles = roles;
        self.retain_source = retain_source;
        self.keeps_as_built = keeps_as_built;

        self.registry.php_versions = php_versions;
        self.registry
            .set_assertion_tables(overlays.assertions, overlays.class_argument_returns);

        self.settings = CheckSettings {
            rule_levels: RuleLevelHelper {
                strict_mixed_arrays: rules.strict_mixed_arrays,
                check_benevolent_union_types: rules.check_benevolent_union_types,
            },
            constants: ConstantResolver::new(&dynamic_constant_names),
            rules,
            phpstan_api: phpstan_api_check_enabled,
            extras: extras.resolve(active.extras),
            strict_rules: strict_rules.resolve(active.strict_rules),
            shipmonk: shipmonk.resolve(active.shipmonk),
            voku: voku.resolve(active.voku),
            slam: slam.resolve(active.slam),
            psr3_context_key_pattern: psr3.context_key_pattern().ok().flatten(),
            psr3: psr3.resolve(active.psr3),
            sidz: sidz.resolve(active.sidz),
            phpunit: phpunit.resolve(active.phpunit),
            deprecation_rules: active.deprecation_rules,
            restricted_symbols: crate::restricted_symbols::RestrictedSymbols::compile(
                &restricted_symbols,
            ),
            doctrine: active.doctrine,
            doctrine_custom_functions: doctrine
                .custom_functions
                .iter()
                .map(|e| (e.name.to_ascii_lowercase(), e.kind))
                .collect(),
            language_extensions: active.language_extensions,
            pest: active.pest,
        };

        self.sql.set_config(sql_reflection);
        self.sql.set_schema(sql_schema);

        self.packages = packages;
        let owners: Vec<(PathBuf, Option<composer::PackageOwner>)> = self
            .files
            .keys()
            .map(|path| (path.clone(), self.packages.owner(&self.absolute_path(path))))
            .collect();
        for (path, owner) in owners {
            if let Some(file) = self.files.get_mut(&path) {
                file.owner = owner;
            }
        }
    }

    pub(crate) fn use_sites(&self) -> &UseSiteIndex {
        &self.use_sites
    }

    /// Rebuilds `pest_config` from every currently-loaded `Pest.php` file
    /// (`PestConfig::build` itself filters for that filename) - cheap
    /// regardless of corpus size, since it's proportional only to the
    /// small number of `Pest.php` files a real project has, never the
    /// whole loaded file set. Called unconditionally from both
    /// `finish_loading` and `update_file`, rather than only when the
    /// specific edited file is itself a `Pest.php`, to keep the LSP
    /// incremental-edit path correct if a `Pest.php` config itself is
    /// what just changed.
    fn rebuild_pest_config(&mut self) {
        self.pest_config = PestConfig::build(
            self.files
                .iter()
                .filter_map(|(_, state)| state.pest_facts.as_ref()),
        );
    }

    /// The project's SQL reflection service.
    pub fn sql(&self) -> &sql::context::SqlContext {
        &self.sql
    }

    /// What the checks are configured with.
    pub fn check_settings(&self) -> &CheckSettings {
        &self.settings
    }

    /// The loaded files' function and method bodies.
    pub(crate) fn bodies(&self) -> &dyn BodySource {
        &self.files
    }

    /// The lines of the loaded file `path`.
    pub(crate) fn line_index(&self, path: &Path) -> Option<&Arc<LineIndex>> {
        self.files.get(path).map(|file| &file.line_index)
    }

    /// The 1-based line of the byte offset `offset` of the loaded file
    /// `path`: the first line for a file that is not loaded.
    pub(crate) fn line_in(&self, path: &Path, offset: u32) -> u32 {
        self.files
            .get(path)
            .map_or(1, |file| file.line_index.line(offset))
    }

    /// Applies the inline ignore comments of every analysed file `results`
    /// holds to its diagnostics and, with `report_unmatched`, reports each
    /// ignore that suppressed nothing (`ignore.unmatchedLine`/
    /// `ignore.unmatchedIdentifier`), except inside a trait, whose
    /// diagnostics depend on its users.
    pub fn apply_inline_ignores(
        &self,
        results: &mut FxHashMap<PathBuf, Vec<Diagnostic>>,
        report_unmatched: bool,
    ) {
        for (path, diagnostics) in results.iter_mut() {
            let Some(file) = self.files.get(path).filter(|file| file.analyzed) else {
                continue;
            };
            let ignores = &file.ignores;
            let mut matched_lines: FxHashSet<u32> = FxHashSet::default();
            let mut matched_ids: FxHashSet<(u32, &str)> = FxHashSet::default();
            diagnostics.retain(|d| {
                if d.non_ignorable {
                    return true;
                }
                let line = d.span.start.line;
                if ignores.lines.contains(&line) {
                    matched_lines.insert(line);
                    return false;
                }
                if let Some(ids) = ignores.identifiers.get(&line)
                    && let Some(id) = ids.iter().find(|id| ***id == *d.identifier)
                {
                    matched_ids.insert((line, id.as_str()));
                    return false;
                }
                true
            });
            if !report_unmatched {
                continue;
            }
            let trait_lines = trait_line_ranges(file.ast(), &file.line_index);
            let in_trait = |line: u32| trait_lines.iter().any(|(a, b)| (*a..=*b).contains(&line));
            let mut unmatched: Vec<Diagnostic> = Vec::new();
            let mut unmatched_line = |line: u32, message: String, identifier: &'static str| {
                unmatched.push(
                    diagnostics::DiagnosticBuilder::new(message, Span::default(), 0, identifier)
                        .non_ignorable()
                        .build_at(line_start_range(line)),
                );
            };
            for &line in &ignores.lines {
                if !matched_lines.contains(&line) && !in_trait(line) {
                    unmatched_line(
                        line,
                        format!("No error to ignore is reported on line {line}."),
                        "ignore.unmatchedLine",
                    );
                }
            }
            for (&line, ids) in &ignores.identifiers {
                if ignores.lines.contains(&line) || in_trait(line) {
                    continue;
                }
                for id in ids {
                    if !matched_ids.contains(&(line, id.as_str())) {
                        unmatched_line(
                            line,
                            format!("No error with identifier {id} is reported on line {line}."),
                            "ignore.unmatchedIdentifier",
                        );
                    }
                }
            }
            if !unmatched.is_empty() {
                diagnostics.extend(unmatched);
                diagnostics.sort_by(|a, b| {
                    (a.span.start.line, a.span.start.col, &a.message).cmp(&(
                        b.span.start.line,
                        b.span.start.col,
                        &b.message,
                    ))
                });
            }
        }
    }

    /// `path`, spelled from the working directory, as an absolute path:
    /// the working directory swapped for `absolute_root`, then `.` and
    /// `..` resolved lexically. Without an absolute root, only the latter.
    pub(crate) fn absolute_path(&self, path: &Path) -> PathBuf {
        absolute_spelling(
            self.absolute_root.as_deref(),
            self.working_dir.as_deref(),
            path,
        )
    }

    /// What `path`, spelled from the working directory, is to the project:
    /// the role and stub status its roots give it.
    pub fn classify(&self, path: &Path) -> roles::Classified {
        self.roles.classify(&self.absolute_path(path))
    }

    /// Gives a file not yet loaded the place in the load order a load would
    /// give it, and moves the places after it down by one: the context
    /// files come first and each group is in path order. Returns the place.
    fn make_room_in_load_order(&mut self, path: &Path, analyzed: bool) -> u64 {
        let mut context = 0;
        let mut before = 0;
        for (other, state) in self.files.iter() {
            context += u64::from(!state.analyzed);
            before += u64::from(state.analyzed == analyzed && other.as_path() < path);
        }
        let place = before + if analyzed { context } else { 0 };
        for state in self.files.values_mut() {
            if state.load_order >= place {
                state.load_order += 1;
            }
        }
        self.next_load_order += 1;
        place
    }

    /// Gives every loaded file the place a load would: the context files
    /// first, each group in path order. The names more than one file
    /// declares are rebuilt, which the registry builds in that order.
    pub fn restore_load_order(&mut self) {
        let mut order: Vec<(bool, PathBuf)> = self
            .files
            .iter()
            .map(|(path, state)| (state.analyzed, path.clone()))
            .collect();
        order.sort();
        let mut changed = false;
        for (place, (_, path)) in order.iter().enumerate() {
            let state = self.files.get_mut(path).expect("the file is held");
            changed |= state.load_order != place as u64;
            state.load_order = place as u64;
        }
        self.next_load_order = order.len() as u64;
        if !changed {
            return;
        }
        let mut declared: FxHashMap<String, (usize, Name)> = FxHashMap::default();
        let mut shared = DeclaredNames::default();
        let mut note = |kind: u8, name: &Name, key: String, list: &mut Vec<Name>| {
            let entry = declared
                .entry(format!("{kind}{key}"))
                .or_insert((0, name.clone()));
            entry.0 += 1;
            if entry.0 == 2 {
                list.push(entry.1.clone());
            }
        };
        for state in self.files.values() {
            for name in &state.functions {
                note(0, name, name.to_ascii_lowercase(), &mut shared.functions);
            }
            for name in &state.classes {
                note(1, name, name.to_ascii_lowercase(), &mut shared.classes);
            }
            for name in &state.constants {
                note(2, name, name.as_str().to_string(), &mut shared.constants);
            }
        }
        if !(shared.functions.is_empty()
            && shared.classes.is_empty()
            && shared.constants.is_empty())
        {
            self.rebuild_declarations(&shared, None);
        }
    }

    /// Closes the places in the load order that `removed` held, which are
    /// in no file now.
    fn close_load_order(&mut self, mut removed: Vec<u64>) {
        removed.sort_unstable();
        for state in self.files.values_mut() {
            state.load_order -= removed.partition_point(|&place| place < state.load_order) as u64;
        }
        self.next_load_order -= removed.len() as u64;
    }

    /// The name of the composer package `path` belongs to.
    pub(crate) fn package_name(&self, path: &Path) -> Option<&str> {
        let owner = self.files.get(path)?.owner?;
        self.packages.name(owner)
    }

    /// The text of a loaded file as an editor has it, when `retain_source`
    /// is on and the file is open - `None` otherwise.
    pub fn source(&self, path: &Path) -> Option<&str> {
        self.files.get(path)?.source.as_deref()
    }

    /// The tree of a loaded file: the one it kept, or, for a file that kept
    /// none, one parsed again from the file as it was read: `None` when
    /// the file has changed since.
    pub(crate) fn ast_of(&self, path: &Path) -> Option<AstRef<'_>> {
        let file = self.files.get(path)?;
        if let Some(ast) = file.tree() {
            return Some(AstRef::Kept(ast));
        }
        if let Some(ast) = self.ast_cache.lock().ok()?.get(path) {
            return Some(AstRef::Parsed(ast));
        }
        let full = match &self.working_dir {
            Some(root) => root.join(path),
            None => path.to_path_buf(),
        };
        let bytes = std::fs::read(full).ok()?;
        if source::ContentHash::of(&bytes) != file.content_hash {
            return None;
        }
        let parsed = match String::from_utf8(bytes) {
            Ok(text) => parse_with_ignores(path, &text, None),
            Err(error) => parse_with_ignores(path, &error.into_bytes(), None),
        }
        .ok()?;
        let ast = Arc::new(parsed.ast);
        self.ast_cache
            .lock()
            .ok()?
            .insert(path.to_path_buf(), Arc::clone(&ast));
        Some(AstRef::Parsed(ast))
    }

    /// What the bytes `path` was loaded or last edited from are.
    pub fn content_hash(&self, path: &Path) -> Option<source::ContentHash> {
        Some(self.files.get(path)?.content_hash)
    }

    /// Notes that an editor has `path` open with `text`: the file's tree
    /// and text are kept from now on, whatever its role, and read from the
    /// editor's text rather than the disk's. A loaded file that has not
    /// changed since is parsed here; an edit's own `update_file` keeps its
    /// own.
    pub(crate) fn open_file(&mut self, path: &Path, text: &str) {
        self.open.insert(path.to_path_buf());
        let retain_source = self.retain_source;
        let Some(file) = self.files.get_mut(path) else {
            return;
        };
        if file.content_hash != source::ContentHash::of(text.as_bytes()) {
            return;
        }
        if retain_source {
            file.source = Some(text.to_string());
        }
        if file.kept_tree.is_none()
            && let Ok(parsed) = parse_with_ignores(path, text, None)
        {
            file.kept_tree = Some(SyntaxTree {
                stmts: parsed.ast,
                source: parsed.source,
                names: parsed.names,
                expr_spans: parsed.expr_spans,
                stmt_spans: parsed.stmt_spans,
            });
            file.ignores = parsed.ignores;
        }
    }

    /// Notes that the editor no longer has `path` open: its text is
    /// dropped, and a context file's tree.
    pub(crate) fn close_file(&mut self, path: &Path) {
        self.open.remove(path);
        if let Some(file) = self.files.get_mut(path) {
            file.source = None;
            if !file.analyzed {
                file.kept_tree = None;
                file.ignores = Default::default();
            }
        }
    }

    /// Numbers `paths` before any is inserted, which a caller can do while
    /// it parses them. The number of each place in the list, for
    /// `insert_numbered_file`.
    pub(crate) fn number_files(
        &mut self,
        numbering: (file_table::FileTable<FileState>, Vec<FileId>),
    ) -> Vec<FileId> {
        let (table, numbers) = numbering;
        if self.files.has_numbers() {
            return numbers
                .iter()
                .map(|&id| self.files.id_for(table.path(id)))
                .collect();
        }
        self.files = table;
        numbers
    }

    pub(crate) fn path_for_file(&self, id: FileId) -> &Path {
        self.files.path(id)
    }

    /// `install_parsed_file`, for the file `id` numbers `path` as, `len`
    /// bytes long: `source` is empty unless the project keeps the text.
    pub(crate) fn insert_numbered_file(
        &mut self,
        id: FileId,
        path: &Path,
        parsed: ParsedFile,
        source: String,
        len: usize,
        analyzed: bool,
        declaration_ids: Vec<(Option<u32>, Option<u32>)>,
    ) {
        self.begin_declaration_changes();
        let load_order = self.next_load_order;
        self.next_load_order += 1;
        debug_assert_eq!(parsed.built_decls.len(), declaration_ids.len());
        for (decl, &(class_local, function_local)) in
            parsed.built_decls.iter().zip(&declaration_ids)
        {
            self.advance_project_declaration_ids(class_local, function_local);
            if self.defer_reference_index {
                match decl {
                    crate::registry::BuiltDecl::GlobalConst { .. } => {
                        self.pending_global_constants.push(decl.clone());
                    }
                    _ => self.pending_built_declarations.push((
                        decl.clone(),
                        analyzed,
                        class_local,
                        function_local,
                    )),
                }
            } else {
                self.registry.insert_built_decl_with_ids(
                    decl.clone(),
                    analyzed,
                    class_local,
                    function_local,
                );
            }
        }
        // `record_phpstan_class_file` no-ops unless the name is
        // `PHPStan\*`-namespaced, so this is a zero-cost loop for every
        // ordinary corpus.
        if self.defer_reference_index {
            self.pending_phpstan_class_files.extend(
                parsed
                    .classes
                    .iter()
                    .enumerate()
                    .filter(|(_, name)| crate::registry::is_phpstan_namespace(name.as_str()))
                    .map(|(sequence, name)| {
                        (
                            load_order,
                            u32::try_from(sequence).expect("a file has too many classes"),
                            name.clone(),
                            path.to_path_buf(),
                        )
                    }),
            );
        } else {
            for name in &parsed.classes {
                self.registry.record_phpstan_class_file(name, path);
            }
        }
        let placed = Placed {
            id,
            path,
            analyzed,
            load_order,
        };
        self.install_parsed_file(placed, parsed, declaration_ids, source, len);
    }

    /// Holds `parsed`, the parse of the file `placed` says, and indexes
    /// everything it gives but its declarations in the registry: its use
    /// sites, its declaration sites, its references and its owner. Where
    /// its declarations go is the caller's.
    fn install_parsed_file(
        &mut self,
        placed: Placed<'_>,
        parsed: ParsedFile,
        declaration_ids: Vec<(Option<u32>, Option<u32>)>,
        source: String,
        len: usize,
    ) {
        let Placed {
            id,
            path,
            analyzed,
            load_order,
        } = placed;
        let ParsedFile {
            tree,
            lex_warnings: _,
            functions,
            classes,
            constants,
            references,
            declaration_references,
            ignores,
            built_decls,
            file_decls,
            anonymous_classes,
            duplicate_fn_sites,
            duplicate_class_sites,
            bodies,
            global_facts,
            pest_facts,
            declared_classes,
            facts,
            line_index,
            class_fn_refs,
            owner,
            content_hash,
            ..
        } = parsed;
        let references = sorted_ref_keys(references);
        let declaration_references = sorted_ref_keys(declaration_references);
        let keeps_tree = self.keeps_tree(path, analyzed);
        debug_assert!(
            tree.is_some() || !keeps_tree,
            "a file whose tree is needed was read back from the cache"
        );
        let class_fn_refs = class_fn_refs.unwrap_or_else(|| {
            tree.as_ref().map_or_else(Vec::new, |tree| {
                self.class_fn_refs_of(tree.statements(), &source)
            })
        });
        if self.defer_reference_index {
            self.pending_use_sites
                .push((path.to_path_buf(), class_fn_refs));
        } else {
            for (span, target) in class_fn_refs {
                self.use_sites.insert(path, target, span);
            }
        }
        if self.defer_reference_index {
            self.pending_duplicate_fn_sites
                .push((id, duplicate_fn_sites));
            self.pending_duplicate_class_sites
                .push((id, duplicate_class_sites));
        } else {
            self.duplicate_fn_sites.merge_file(id, duplicate_fn_sites);
            self.duplicate_class_sites
                .merge_file(id, duplicate_class_sites);
        }
        if !self.defer_reference_index {
            register_references(&mut self.dependents, id, &references);
            register_references(&mut self.signature_dependents, id, &declaration_references);
        }
        let owner = owner.unwrap_or_else(|| self.packages.owner(&self.absolute_path(path)));
        if self.defer_reference_index {
            self.pending_stale_files.push(id);
        } else {
            self.globals_stale.insert(id);
        }
        self.files.insert_at(
            id,
            FileState {
                kept_tree: tree.filter(|_| keeps_tree),
                source_len: len,
                load_order,
                mentions_phpstan: mentions_phpstan(&functions, &classes, references.iter()),
                functions,
                classes,
                constants,
                references,
                declaration_references,
                ignores: if keeps_tree {
                    ignores
                } else {
                    Default::default()
                },
                anonymous_classes,
                source: (self.retain_source && self.open.contains(path)).then_some(source),
                analyzed,
                owner,
                bodies,
                global_facts,
                built_decls,
                file_decls,
                declaration_ids,
                pest_facts,
                declared_classes,
                facts,
                line_index,
                content_hash,
            },
        );
    }

    fn advance_project_declaration_ids(
        &mut self,
        class_local: Option<u32>,
        function_local: Option<u32>,
    ) {
        if let Some(local) = class_local {
            self.next_project_class_local = self.next_project_class_local.max(
                local
                    .checked_add(1)
                    .expect("project class id range exhausted"),
            );
        }
        if let Some(local) = function_local {
            self.next_project_function_local = self.next_project_function_local.max(
                local
                    .checked_add(1)
                    .expect("project function id range exhausted"),
            );
        }
    }

    /// Reuses each declaration's old id for the same kind, folded name and
    /// occurrence in this file. New declarations receive ids above the
    /// project's high-water marks; removals never make an id reusable.
    fn declaration_ids_for_edit(
        &mut self,
        declarations: &[BuiltDecl],
        previous: Option<&UnindexedFile>,
    ) -> Vec<(Option<u32>, Option<u32>)> {
        let mut old_by_key: FxHashMap<(u8, String), Vec<u32>> = FxHashMap::default();
        if let Some(previous) = previous {
            for (decl, &(class_local, function_local)) in
                previous.built_decls.iter().zip(&previous.declaration_ids)
            {
                let (tag, name, local) = match decl {
                    BuiltDecl::Class { name, .. } => (0, name, class_local),
                    BuiltDecl::Function { name, .. } => (1, name, function_local),
                    BuiltDecl::GlobalConst { .. } => continue,
                };
                if let Some(local) = local {
                    old_by_key
                        .entry((tag, name.to_ascii_lowercase()))
                        .or_default()
                        .push(local);
                }
            }
        }
        let mut occurrences: FxHashMap<(u8, String), usize> = FxHashMap::default();
        declarations
            .iter()
            .map(|decl| {
                let (tag, name) = match decl {
                    BuiltDecl::Class { name, .. } => (0, name),
                    BuiltDecl::Function { name, .. } => (1, name),
                    BuiltDecl::GlobalConst { .. } => return (None, None),
                };
                let key = (tag, name.to_ascii_lowercase());
                let occurrence = occurrences.entry(key.clone()).or_default();
                let old = old_by_key
                    .get(&key)
                    .and_then(|locals| locals.get(*occurrence))
                    .copied();
                *occurrence += 1;
                match tag {
                    0 => {
                        let local = old.unwrap_or_else(|| {
                            let local = self.next_project_class_local;
                            self.next_project_class_local = local
                                .checked_add(1)
                                .expect("project class id range exhausted");
                            local
                        });
                        self.advance_project_declaration_ids(Some(local), None);
                        (Some(local), None)
                    }
                    _ => {
                        let local = old.unwrap_or_else(|| {
                            let local = self.next_project_function_local;
                            self.next_project_function_local = local
                                .checked_add(1)
                                .expect("project function id range exhausted");
                            local
                        });
                        self.advance_project_declaration_ids(None, Some(local));
                        (None, Some(local))
                    }
                }
            })
            .collect()
    }

    /// Whether the tree of the file `path` is kept: an analysed file's is,
    /// and so is any file's that an editor has open. The tree of any other
    /// is dropped, on the thread that parsed it where a load parses it.
    pub(crate) fn keeps_tree(&self, path: &Path, analyzed: bool) -> bool {
        analyzed || self.open.contains(path)
    }

    pub(crate) fn rebuild_path_ranks(&mut self) {
        let mut ordered: Vec<(FileId, PathBuf)> = self
            .files
            .iter_with_ids()
            .map(|(id, path, _)| (id, path.clone()))
            .collect();
        composer::path_order::sort_by_path(&mut ordered, |(_, path)| path);
        self.path_ranks
            .resize(self.files.paths_by_id().len(), usize::MAX);
        for (rank, (id, _)) in ordered.into_iter().enumerate() {
            self.path_ranks[id.index()] = rank;
        }
    }

    pub(crate) fn begin_reference_index_batch(&mut self) {
        self.defer_reference_index = true;
        self.pending_use_sites.clear();
        self.pending_built_declarations.clear();
        self.pending_global_constants.clear();
        self.pending_phpstan_class_files.clear();
        self.pending_stale_files.clear();
        self.pending_duplicate_fn_sites.clear();
        self.pending_duplicate_class_sites.clear();
    }

    pub(crate) fn finish_reference_index_batch(&mut self) {
        let files: Vec<_> = self
            .files
            .iter_with_ids()
            .map(|(file, _, state)| {
                (
                    file,
                    state.references.as_ref(),
                    state.declaration_references.as_ref(),
                )
            })
            .collect();
        let functions: Vec<_> = files
            .par_iter()
            .map(|(file, references, _)| (*file, references.to_vec()))
            .collect();
        let signatures: Vec<_> = files
            .par_iter()
            .map(|(file, _, references)| (*file, references.to_vec()))
            .collect();
        self.dependents = ReferenceIndex::from_file_refs(functions);
        self.signature_dependents = ReferenceIndex::from_file_refs(signatures);
        self.use_sites
            .extend_files(std::mem::take(&mut self.pending_use_sites));
        self.duplicate_fn_sites
            .extend_files(std::mem::take(&mut self.pending_duplicate_fn_sites));
        self.duplicate_class_sites
            .extend_files(std::mem::take(&mut self.pending_duplicate_class_sites));
        self.registry
            .insert_built_declarations_batch(std::mem::take(&mut self.pending_built_declarations));
        self.registry
            .insert_built_global_constants_batch(std::mem::take(
                &mut self.pending_global_constants,
            ));
        self.registry
            .record_phpstan_class_files_batch(std::mem::take(
                &mut self.pending_phpstan_class_files,
            ));
        self.globals_stale
            .extend(std::mem::take(&mut self.pending_stale_files));
        self.defer_reference_index = false;
    }

    fn path_rank(&self, path: &Path) -> usize {
        self.files
            .id_of(path)
            .and_then(|id| self.path_ranks.get(id.index()).copied())
            .unwrap_or(usize::MAX)
    }

    /// Whether a file's class and function use sites are indexed as it
    /// is parsed: for the language server's references and rename, and for
    /// the dead code check. The walk is too expensive to pay for every
    /// loaded file (including every vendor/context one) when nothing
    /// consumes it.
    pub(crate) fn indexes_use_sites(&self) -> bool {
        self.retain_source || self.settings.rules.check_dead_code
    }

    /// The class and function use sites of a file being loaded, from its
    /// `source`: each at the span of the identifier itself
    /// (`use_sites::index_class_fn_refs`).
    fn class_fn_refs_of(
        &self,
        ast: &[StmtNode],
        source: &str,
    ) -> Vec<(Span, use_sites::RefTarget)> {
        if self.indexes_use_sites() {
            use_sites::index_class_fn_refs(ast, source)
        } else {
            Vec::new()
        }
    }

    /// Indexes `parsed`'s use sites ahead of `install_parsed_file`, so a
    /// caller parsing many files in parallel indexes them there too.
    pub(crate) fn index_use_sites(&self, parsed: &mut ParsedFile, source: &str) {
        if let Some(tree) = &parsed.tree {
            parsed.class_fn_refs = Some(self.class_fn_refs_of(tree.statements(), source));
        }
    }

    /// Finds the composer package `parsed`, the file at `path`, belongs to
    /// ahead of `install_parsed_file`, so a caller parsing many files in
    /// parallel does the path work there too.
    pub(crate) fn note_owner(&self, parsed: &mut ParsedFile, path: &Path) {
        parsed.owner = Some(self.packages.owner(&self.absolute_path(path)));
    }

    /// Merges PHPStan's own source in with the built-in stubs while some
    /// loaded file mentions a `PHPStan\` name, and takes it out again once
    /// none does: most projects never name it, and it is over half of the
    /// stub classes. The names it declares are rebuilt, from the stubs
    /// first and then every file declaring them, as `Project::new`'s merge
    /// and the loads after it would have built them.
    fn sync_phpstan_stubs(&mut self) {
        let wanted = self.phpstan_pinned || self.files.values().any(|f| f.mentions_phpstan);
        if wanted == self.registry.phpstan_stubs_loaded {
            return;
        }
        if wanted && !self.finished {
            self.switch_to_phpstan_base();
            return;
        }
        self.registry.phpstan_stubs_loaded = wanted;
        self.registry.adopt_stub_tables(if wanted {
            Project::phpstan_base()
        } else {
            Project::plain_base()
        });
        let names = stubs::phpstan_stub_names();
        self.rebuild_declarations(
            &DeclaredNames {
                functions: names.functions.into_iter().map(Name::from).collect(),
                classes: names.classes.into_iter().map(Name::from).collect(),
                constants: names.constants.into_iter().map(Name::from).collect(),
            },
            None,
        );
    }

    /// The registry of the built-in stubs and PHPStan's own source,
    /// resolved once per process.
    fn phpstan_base() -> &'static Registry {
        PHPSTAN_BASE.get_or_init(|| Project::resolved_stubs(true))
    }

    /// Before the files `parsed` are inserted into a project that has none
    /// yet: starts from the registry with PHPStan's source when one of
    /// them mentions it, so that no declaration is inserted twice.
    pub(crate) fn expect_files<'a>(&mut self, parsed: impl IntoIterator<Item = &'a ParsedFile>) {
        if self.finished || !self.files.is_empty() || self.registry.phpstan_stubs_loaded {
            return;
        }
        if parsed
            .into_iter()
            .any(|p| mentions_phpstan(&p.functions, &p.classes, &p.references))
        {
            self.registry.switch_base(Project::phpstan_base().clone());
        }
    }

    /// Starts again from the registry resolved once per process with
    /// PHPStan's own source in it, instead of resolving that source for
    /// this project alone: the loaded files' declarations go into it as
    /// they went into the registry without it, from the built-in stubs
    /// first, context files before analyzed ones and each group in load order.
    /// Only for a project that hasn't finished yet, whose resolved
    /// declarations are all the shared base's.
    fn switch_to_phpstan_base(&mut self) {
        self.registry.switch_base(Project::phpstan_base().clone());
        self.begin_declaration_changes();
        let mut sources: Vec<DeclarationSource<'_>> = self
            .files
            .iter()
            .map(|(path, state)| DeclarationSource {
                analyzed: state.analyzed,
                load_order: state.load_order,
                path: path.as_path(),
                declarations: &state.built_decls,
                declaration_ids: &state.declaration_ids,
                pending: false,
            })
            .collect();
        sources.sort_by_key(|source| (source.analyzed, source.load_order));
        for source in sources {
            for (decl, &(class_local, function_local)) in
                source.declarations.iter().zip(source.declaration_ids)
            {
                if let BuiltDecl::Class { name, .. } = decl {
                    self.registry.record_phpstan_class_file(name, source.path);
                }
                self.registry.insert_built_decl_with_ids(
                    decl.clone(),
                    source.analyzed,
                    class_local,
                    function_local,
                );
            }
        }
    }

    /// Makes the registry's classes and functions the declarations as
    /// built, for declarations to change on; the finishing passes then
    /// resolve again whatever the changes can have affected.
    fn begin_declaration_changes(&mut self) {
        debug_assert!(
            self.keeps_as_built || !self.finished,
            "a project built once can't change after its finishing passes ran"
        );
        self.registry.begin_declaration_changes();
    }

    /// The classes and functions the finishing passes must run over after
    /// `changed`, as two scopes: what resolution reads from a
    /// declaration (its signature, hierarchy, PHPDoc and constant values)
    /// and what purity and property writes read from a body too, a
    /// superset. Each is the changed names plus every class and function
    /// declared where a name in it is referenced - in declarations only,
    /// or anywhere - transitively, in a loaded file or a built-in stub
    /// file.
    fn finishing_scopes(&self, changed: &crate::registry::ChangedNames) -> (Scope, Scope) {
        // The two walks read the project alike and share nothing.
        rayon::join(
            || self.closure_scope(changed, &self.signature_dependents),
            || self.closure_scope(changed, &self.dependents),
        )
    }

    fn closure_scope(
        &self,
        changed: &crate::registry::ChangedNames,
        index: &ReferenceIndex,
    ) -> Scope {
        let mut classes: FxHashSet<ClassId> = changed
            .classes
            .iter()
            .filter_map(|name| self.registry.class_id(name))
            .collect();
        let mut functions: FxHashSet<FnId> = changed
            .functions
            .iter()
            .filter_map(|name| self.registry.function_id(name))
            .collect();
        let mut removed_classes: FxHashSet<String> = changed
            .classes
            .iter()
            .filter(|name| self.registry.class_id(name).is_none())
            .cloned()
            .collect();
        let mut removed_functions: FxHashSet<String> = changed
            .functions
            .iter()
            .filter(|name| self.registry.function_id(name).is_none())
            .cloned()
            .collect();
        let mut frontier = self.registry.changed_frontier(changed);
        let mut seen: FxHashSet<deps::RefKey> = FxHashSet::default();
        // Each file is expanded once: what it contributes to the scope is
        // its declared names, whichever name led to it.
        let mut expanded = FileIdSet::default();
        let mut stub_files = StubFileSet::default();
        while let Some(key) = frontier.pop() {
            if seen.contains(&key) {
                continue;
            }
            // A project file's declared name is a key of its kind, walked
            // when something references it.
            for file in index.get(&key).into_iter().flatten() {
                if !expanded.insert(*file) {
                    continue;
                }
                let Some(state) = self.files.get_by_id(*file) else {
                    continue;
                };
                for name in state.classes.iter().chain(&state.anonymous_classes) {
                    let text = name.lower().trim_start_matches('\\');
                    if let Some(id) = self.registry.class_id(text) {
                        if classes.insert(id) {
                            frontier.push(deps::RefKey::class(name));
                        }
                    } else if removed_classes.insert(text.to_string()) {
                        // A changed declaration may have disappeared between
                        // the dependency index and the active registry.
                        // Keep its key for cleanup and still expand its
                        // dependents so removed symbols propagate transitively.
                        frontier.push(deps::RefKey::class(name));
                    }
                }
                for name in &state.functions {
                    let text = name.lower().trim_start_matches('\\');
                    if let Some(id) = self.registry.function_id(text) {
                        if functions.insert(id) {
                            frontier.push(deps::RefKey::function(name));
                        }
                    } else if removed_functions.insert(text.to_string()) {
                        // Keep the removed declaration's name for stale state
                        // cleanup and continue expanding its dependents.
                        frontier.push(deps::RefKey::function(name));
                    }
                }
            }
            // A stub file is reached through every key it references, and
            // with it every stub file that references what it declares;
            // the names of each are walked the first time. A stub's name is
            // a key already, and is walked only if a project file
            // references it.
            if let Some(stub_key) = key.stub_key() {
                for file in stubs::stub_dependent_files(stub_key) {
                    stub_files.add_closure(file, |file| {
                        for (is_class, name) in stubs::stub_file_names(file) {
                            let inserted = if is_class {
                                if let Some(id) = self.registry.class_id(name) {
                                    classes.insert(id)
                                } else {
                                    removed_classes.insert(name.to_string())
                                }
                            } else if let Some(id) = self.registry.function_id(name) {
                                functions.insert(id)
                            } else {
                                removed_functions.insert(name.to_string())
                            };
                            if !inserted {
                                continue;
                            }
                            let key = if is_class {
                                deps::RefKey::class_text(name)
                            } else {
                                deps::RefKey::function_text(name)
                            };
                            if index.contains_key(&key) {
                                frontier.push(key);
                            }
                        }
                    });
                }
            }
            seen.insert(key);
        }
        // A class in scope is resolved from the classes it copies from as
        // they are mid-way through the passes, as a load resolves them
        // together, never from their finished entries: those are in scope
        // too, read from the declarations as built.
        let mut copied_from: Vec<ClassId> = classes.iter().copied().collect();
        while let Some(class_id) = copied_from.pop() {
            let Some(info) = self.registry.class_by_id(class_id) else {
                continue;
            };
            for name in info.copies_from() {
                let text = name.lower().trim_start_matches('\\');
                if let Some(id) = self.registry.class_id(text)
                    && classes.insert(id)
                {
                    copied_from.push(id);
                } else if self.registry.class_id(text).is_none() {
                    removed_classes.insert(text.to_string());
                }
            }
        }
        #[cfg(debug_assertions)]
        self.report_stub_scope(changed, index);
        Scope::from_ids(classes, functions, removed_classes, removed_functions)
    }

    /// Reports, for each changed declaration, the stub keys pulled into this
    /// finishing-pass scope. This second walk is debug-only and does not
    /// affect the scope that the pass runs on.
    #[cfg(debug_assertions)]
    fn report_stub_scope(&self, changed: &crate::registry::ChangedNames, index: &ReferenceIndex) {
        let mut frontier = Vec::new();
        frontier.extend(
            changed
                .classes
                .iter()
                .map(|name| (deps::RefKey::class_text(name), name.clone())),
        );
        frontier.extend(
            changed
                .functions
                .iter()
                .map(|name| (deps::RefKey::function_text(name), name.clone())),
        );
        frontier.extend(
            changed
                .constants
                .keys()
                .map(|name| (deps::RefKey::constant_text(name), name.clone())),
        );

        let mut seen: FxHashSet<(deps::RefKey, String)> = FxHashSet::default();
        let mut expanded: FxHashSet<(FileId, String)> = FxHashSet::default();
        let mut stub_files: FxHashMap<String, StubFileSet> = FxHashMap::default();
        let mut keys_by_cause: FxHashMap<String, FxHashSet<String>> = FxHashMap::default();

        while let Some((key, cause)) = frontier.pop() {
            if !seen.insert((key.clone(), cause.clone())) {
                continue;
            }
            for file in index.get(&key).into_iter().flatten() {
                if !expanded.insert((*file, cause.clone())) {
                    continue;
                }
                let Some(state) = self.files.get_by_id(*file) else {
                    continue;
                };
                for name in state.classes.iter().chain(&state.anonymous_classes) {
                    let key = deps::RefKey::class(name);
                    frontier.push((key, cause.clone()));
                }
                for name in &state.functions {
                    frontier.push((deps::RefKey::function(name), cause.clone()));
                }
            }
            if let Some(stub_key) = key.stub_key() {
                for file in stubs::stub_dependent_files(stub_key) {
                    stub_files
                        .entry(cause.clone())
                        .or_default()
                        .add_closure(file, |file| {
                            for (is_class, name) in stubs::stub_file_names(file) {
                                let key = if is_class {
                                    deps::RefKey::class_text(name)
                                } else {
                                    deps::RefKey::function_text(name)
                                };
                                keys_by_cause
                                    .entry(cause.clone())
                                    .or_default()
                                    .insert(format!("{key:?}"));
                                if index.contains_key(&key) {
                                    frontier.push((key, cause.clone()));
                                }
                            }
                        });
                }
            }
        }

        if keys_by_cause.is_empty() {
            return;
        }
        let mut causes: Vec<_> = keys_by_cause.into_iter().collect();
        causes.sort_by(|(left, _), (right, _)| left.cmp(right));
        eprintln!("finishing scope stub keys by changed name:");
        for (cause, keys) in causes {
            let mut keys: Vec<_> = keys.into_iter().collect();
            keys.sort();
            eprintln!("  {cause}: {} keys", keys.len());
            for key in keys {
                eprintln!("    {key}");
            }
        }
    }

    /// Finalizes an initial load (trait flattening).
    #[hotpath::measure]
    pub fn finish_loading(&mut self) {
        self.finish_declarations();
    }

    /// Settles which stubs the project has, then runs the finishing passes
    /// on the worker pool. The stubs are settled on the calling thread: a
    /// registry resolved once per process may have to be built first, on
    /// the pool, which a pool thread waiting for it would starve.
    fn finish_declarations(&mut self) -> FxHashSet<PathBuf> {
        self.begin_declaration_changes();
        self.sync_phpstan_stubs();
        worker_pool().install(|| self.run_finishing_passes())
    }

    /// Runs the finishing passes, returning the `deps::dependency_key`s of
    /// the classes, functions and constants whose resolved form they
    /// changed (none the first time, when everything is new).
    fn run_finishing_passes(&mut self) -> FxHashSet<PathBuf> {
        self.begin_declaration_changes();
        let (scope, body_scope, changed) = if self.registry.has_resolved_declarations() {
            let changed = self.registry.take_changed();
            let (scope, body_scope) = self.finishing_scopes(&changed);
            (scope, body_scope, Some(changed))
        } else {
            (Scope::all(), Scope::all(), None)
        };
        if !body_scope.is_all() {
            self.registry.warm(&body_scope);
        }
        let before = self.registry.end_declaration_changes(
            &scope,
            changed.as_ref().map(|_| &body_scope),
            self.keeps_as_built,
            changed.as_ref(),
        );
        if !body_scope.is_all() {
            self.registry.clear_purity(&body_scope.minus(&scope));
        }
        self.registry.set_scope(scope.clone());
        self.finished = true;
        self.registry.advance_relation_generation();
        self.registry.canonicalize_class_name_case();
        self.registry.advance_relation_generation();
        self.registry.flatten_traits();
        self.registry.advance_relation_generation();
        self.registry.resolve_method_tags();
        // `flatten_phpdoc_inheritance` consumes `lookup_ancestor_method_
        // with_owner`/`lookup_method_with_owner` internally - see
        // `Registry::index_and_flatten`'s own identical two-precompute-
        // calls comment for why a single call at the very end (like
        // `precompute_ancestor_names`) isn't enough here.
        self.registry.precompute_member_owners();
        self.registry.advance_relation_generation();
        // Aliases and deferred value types resolve first, so inherited
        // PHPDoc copies an ancestor's finished types.
        self.registry.resolve_deferred_type_imports();
        self.registry.advance_relation_generation();
        self.registry.resolve_deferred_value_of();
        self.registry.advance_relation_generation();
        self.registry.flatten_phpdoc_inheritance();
        self.registry.advance_relation_generation();
        self.registry.inherit_property_hooks();
        self.registry.advance_relation_generation();
        self.registry.apply_assertion_table();
        self.registry.advance_relation_generation();
        // Adds more method/property names (mixin members) -
        // `resolve_transitive_purity` right after this also consumes
        // `lookup_method_with_owner`/`lookup_ancestor_method_with_owner`
        // internally, so the member-owner maps take in the mixin members
        // here, before it runs.
        self.registry.resolve_deferred_mixins();
        self.registry.advance_relation_generation();
        self.registry.precompute_member_owners_after_mixins();
        self.registry.advance_relation_generation();
        // The class graph is final here; the effects bound a call by the
        // classes descending from its receiver's, which this indexes.
        self.registry.precompute_ancestor_names();
        self.registry.advance_relation_generation();
        // The declared types are final after these two, and purity and
        // the effects read them: a call through a property is resolved by
        // the type the property has, the one PHP enforces.
        self.registry.resolve_constant_named_types();
        self.registry.advance_relation_generation();
        self.registry.drop_doc_members_excluded_by_native();
        self.registry.advance_relation_generation();
        // Neither `resolve_transitive_purity` nor anything below it adds
        // or removes a method/property/const name anywhere, so the
        // member-owner maps built just above stay correct through this
        // point and for the checker's own later use - no third rebuild
        // needed.
        self.registry.set_scope(body_scope);
        self.registry.resolve_transitive_purity(&self.files);
        self.registry.advance_relation_generation();
        self.registry.set_scope(scope);
        self.registry.index_assert_tagged_names();
        self.registry.advance_relation_generation();
        self.registry.index_doctrine_entities();
        self.registry.advance_relation_generation();
        let changed_keys = match (&before, &changed) {
            (Some(before), Some(changed)) => self.registry.changed_since(before, changed),
            _ => FxHashSet::default(),
        };
        // What a file contributes to a global's type reads what it
        // references, so a file is walked again when one of those changed.
        let mut stale = std::mem::take(&mut self.globals_stale);
        let mut walk = AffectedWalk::default();
        self.walk_affected(&mut walk, changed_keys.iter().cloned());
        // A trait is checked as each class using it: its file is checked
        // again when one of them changed, came or went.
        if let Some(before) = &before {
            for name in self
                .registry
                .traits_of_changed_classes(before, changed.as_ref())
            {
                let file = self
                    .registry
                    .class_by_name(&name)
                    .and_then(|info| info.provenance().file_id());
                walk.affected.extend(file);
            }
        }
        stale.extend(walk.affected.iter().cloned());
        let files: Vec<(FileId, &Path, &GlobalFacts)> = self
            .files
            .iter_with_ids()
            .map(|(id, p, f)| (id, p.as_path(), &*f.global_facts))
            .collect();
        self.registry.infer_global_variables_of(&files, &stale);
        self.registry.set_scope(Scope::all());
        self.rebuild_pest_config();
        // The walk goes on from where it stopped with the globals that
        // changed too, which `stale` has no use for.
        if let Some(before) = &before {
            self.walk_affected(&mut walk, self.registry.changed_global_variables(before));
        }
        walk.affected
            .into_iter()
            .map(|id| self.files.path(id).to_path_buf())
            .collect()
    }

    /// Re-parses one already-loaded file, patches just its own registry
    /// contributions and `dependents` entries, and re-checks it plus every
    /// other file that references a symbol it declared (before or after
    /// the change - a symbol the new version dropped still needs its old
    /// dependents re-checked, since they'll now see it as undefined).
    /// Returns every re-checked file's fresh diagnostics, keyed by path -
    /// the changed file is always included, even with zero diagnostics.
    /// Unwinds `path`'s prior contributions to the shared registry/
    /// `dependents` - the same undo both `update_file` (before
    /// re-indexing a new parse) and `remove_files` (permanently) need.
    /// Returns what the removed file was, or `None` when `path` wasn't
    /// loaded (a no-op - both callers treat that as "nothing to undo").
    fn unindex_file(&mut self, path: &Path) -> Option<UnindexedFile> {
        self.begin_declaration_changes();
        let id = self.files.id_of(path)?;
        let old = self.files.remove(path)?;
        let classes: Vec<Name> = old
            .classes
            .iter()
            .chain(&old.anonymous_classes)
            .cloned()
            .collect();
        self.registry.remove_declarations(&old.functions, &classes);
        self.use_sites.remove_file(path);
        self.duplicate_fn_sites.remove_file(id, &old.functions);
        self.duplicate_class_sites.remove_file(id, &old.classes);
        unregister_references(&mut self.dependents, id, &old.references);
        unregister_references(
            &mut self.signature_dependents,
            id,
            &old.declaration_references,
        );
        Some(UnindexedFile {
            analyzed: old.analyzed,
            load_order: old.load_order,
            functions: old.functions,
            classes: old.classes,
            constants: old.constants,
            built_decls: old.built_decls,
            declaration_ids: old.declaration_ids,
        })
    }

    /// Rebuilds the registry entries of `functions` and `classes` from
    /// every declaration of them still loaded: the built-in stub's first,
    /// as `Project::new` merges it, then each file's, context files before
    /// analyzed ones and each group in load order - the order a project loads
    /// them in. `pending` is a file whose parse isn't in `self.files` yet;
    /// all of its declarations are inserted, at its place in that order.
    fn rebuild_declarations(
        &mut self,
        names: &DeclaredNames,
        pending: Option<DeclarationSource<'_>>,
    ) {
        self.begin_declaration_changes();
        self.registry
            .remove_declarations(&names.functions, &names.classes);
        let function_keys: FxHashSet<String> = names
            .functions
            .iter()
            .map(|n| n.to_ascii_lowercase())
            .collect();
        let class_keys: FxHashSet<String> = names
            .classes
            .iter()
            .map(|n| n.to_ascii_lowercase())
            .collect();
        let constant_keys: FxHashSet<&str> = names.constants.iter().map(Name::as_str).collect();
        let with_phpstan = self.registry.phpstan_stubs_loaded;
        for name in &constant_keys {
            self.registry.remove_global_constant(name);
        }
        for name in &function_keys {
            self.registry
                .merge_stubs(stubs::generated_stubs_named(name, with_phpstan));
        }
        for name in &class_keys {
            self.registry
                .merge_stub_classes(stubs::generated_stub_classes_named(name, with_phpstan));
        }
        for name in &constant_keys {
            self.registry
                .merge_stub_constants(stubs::generated_stub_constants_named(name, with_phpstan));
        }
        let declares_one = |state: &FileState| {
            state
                .functions
                .iter()
                .any(|n| function_keys.contains(&n.to_ascii_lowercase()))
                || state
                    .classes
                    .iter()
                    .any(|n| class_keys.contains(&n.to_ascii_lowercase()))
                || state
                    .constants
                    .iter()
                    .any(|n| constant_keys.contains(n.as_str()))
        };
        let mut sources: Vec<DeclarationSource<'_>> = self
            .files
            .iter()
            .filter(|(_, state)| declares_one(state))
            .map(|(path, state)| DeclarationSource {
                analyzed: state.analyzed,
                load_order: state.load_order,
                path: path.as_path(),
                declarations: &state.built_decls,
                declaration_ids: &state.declaration_ids,
                pending: false,
            })
            .collect();
        if let Some(pending) = pending {
            sources.push(pending);
        }
        sources.sort_by_key(|source| (source.analyzed, source.load_order));
        for source in sources {
            // Of a file already loaded, only the declarations being
            // rebuilt are inserted again; the pending file's all are.
            let rebuilt = |kind: DeclKind, name: &str| match kind {
                DeclKind::Class => class_keys.contains(&name.to_ascii_lowercase()),
                DeclKind::Function => function_keys.contains(&name.to_ascii_lowercase()),
                DeclKind::Constant => constant_keys.contains(name),
            };
            for (decl, &(class_local, function_local)) in
                source.declarations.iter().zip(source.declaration_ids)
            {
                let (kind, name) = decl.kind_and_name();
                if !source.pending && !rebuilt(kind, name) {
                    continue;
                }
                if let BuiltDecl::Class { name, .. } = decl {
                    self.registry.record_phpstan_class_file(name, source.path);
                }
                self.registry.insert_built_decl_with_ids(
                    decl.clone(),
                    source.analyzed,
                    class_local,
                    function_local,
                );
            }
        }
    }

    /// Builds Tier 2 (member-reference) of `use_sites` for every
    /// currently-loaded file, exactly once - idempotent, a no-op on every
    /// call after the first. Deliberately NOT run as part of
    /// `finish_loading`/`load_workspace`: unlike Tier 1 (a product of each
    /// file's parse), Tier 2 needs one real `Checker`
    /// pass per file, which would make every workspace open pay a cost
    /// only `textDocument/references`/`textDocument/rename` actually
    /// need. `Server::references`/`Server::rename` call this first; every
    /// other request path (hover, definition, diagnostics, ...) never
    /// triggers it at all. The FIRST call after a workspace loads is
    /// therefore the one place this cost is paid, on the worker pool while
    /// the caller waits. Each check keeps only what the index and the
    /// whole-program checks read of it.
    pub(crate) fn ensure_member_index_built(&mut self) {
        if self.member_index != MemberIndex::Unbuilt {
            return;
        }
        let mut paths: Vec<PathBuf> = self.files.keys().cloned().collect();
        paths.sort_unstable();
        let pool = worker_pool();
        let checks = map_on_pool_largest_first(
            Some(&pool),
            &paths,
            |path| self.files.get(path).map_or(0, |file| file.source_len),
            |path| {
                let check = self.run_checker(path, true, Records::default());
                FileCheck {
                    member_refs: check.member_refs,
                    escaped_enums: check.escaped_enums,
                    escaped_member_classes: check.escaped_member_classes,
                    named_argument_calls: check.named_argument_calls,
                    ..FileCheck::empty()
                }
            },
        );
        for (path, mut check) in paths.iter().zip(checks) {
            self.use_sites
                .replace_members(path, std::mem::take(&mut check.member_refs));
            self.keep_check_facts(path, &mut check, true);
        }
        self.member_index = MemberIndex::Every;
    }

    /// Keeps what the analysis's check of `path` found for the use-site
    /// index and the whole-program checks: its member references too where
    /// the analysis builds the member index (`index_members`), which is
    /// marked built (`mark_member_index_built`) once every analysed file's
    /// check is kept - instead of `ensure_member_index_built` re-deriving
    /// them via a second, redundant pass.
    pub(crate) fn keep_analysed_check(
        &mut self,
        path: &Path,
        check: &mut FileCheck,
        index_members: bool,
    ) {
        if index_members {
            self.use_sites
                .replace_members(path, std::mem::take(&mut check.member_refs));
        }
        self.keep_check_facts(path, check, index_members);
    }

    /// Keeps what `check`, the last check of `path`, found for the
    /// whole-program checks, in place of what an earlier check of it
    /// kept: what it let escape when the member index covers the file
    /// (`covered`), and its named-argument calls when it is analysed.
    fn keep_check_facts(&mut self, path: &Path, check: &mut FileCheck, covered: bool) {
        let escapes = Escapes {
            enums: std::mem::take(&mut check.escaped_enums),
            member_classes: std::mem::take(&mut check.escaped_member_classes),
        };
        if covered && !(escapes.enums.is_empty() && escapes.member_classes.is_empty()) {
            self.escapes.insert(path.to_path_buf(), escapes);
        } else {
            self.escapes.remove(path);
        }
        let calls = std::mem::take(&mut check.named_argument_calls);
        if self.settings.rules.check_renamed_named_arguments
            && self.is_analyzed(path)
            && !calls.is_empty()
        {
            self.named_argument_calls.insert(path.to_path_buf(), calls);
        } else {
            self.named_argument_calls.remove(path);
        }
    }

    /// See `keep_analysed_check`'s own doc comment - marks Tier 2 as built
    /// over the analysed files without `ensure_member_index_built`'s own
    /// per-file loop ever running, once each of them has been seeded
    /// that way.
    pub(crate) fn mark_member_index_built(&mut self) {
        self.member_index = MemberIndex::Analysed;
    }

    /// Whether the member index covers `path`, so that its recheck
    /// collects its member references and escapes.
    fn member_index_covers(&self, path: &Path) -> bool {
        match self.member_index {
            MemberIndex::Unbuilt => false,
            MemberIndex::Analysed => self.is_analyzed(path),
            MemberIndex::Every => self.is_loaded(path),
        }
    }

    /// The always-run-to-completion convenience wrapper every existing
    /// caller uses - `update_file_cancellable` with a `should_abort`
    /// that never fires.
    pub fn update_file<'a>(
        &mut self,
        path: &Path,
        new_source: impl Into<syntax::Source<'a>>,
    ) -> Result<FxHashMap<PathBuf, Vec<Diagnostic>>, ParseError> {
        self.update_file_cancellable(path, new_source, || false)
    }

    /// The debounce worker's own entry point - identical to
    /// `update_file` except that the recheck of the affected
    /// files asks `should_abort()` before each file's check starts, from
    /// the worker pool's threads, and leaves unchecked (`take_unchecked`)
    /// every file whose check had not started when it answered `true`, so
    /// a generation bump made by a concurrently-running reader thread
    /// (recorded via `lsp::debounce::DebounceQueue::record_edit`) can
    /// stop an in-flight cascade partway through rather than only ever
    /// being observable *between* separate `update_file` calls.
    pub fn update_file_cancellable<'a>(
        &mut self,
        path: &Path,
        new_source: impl Into<syntax::Source<'a>>,
        should_abort: impl Fn() -> bool + Sync,
    ) -> Result<FxHashMap<PathBuf, Vec<Diagnostic>>, ParseError> {
        let new_source = new_source.into();
        let classified = self.classify(path);
        // Parse BEFORE unindexing anything - on a parse failure (the
        // normal, common state while a file is mid-edit, not an edge
        // case), this must leave every existing registry/
        // `self.files` entry for `path` completely untouched, not
        // half-torn-down: `Project::ast`/`Project::source` read straight
        // from `self.files`, with no other place a "last known good"
        // version could still be found, and every last-good-AST-dependent
        // LSP feature (hover/definition/documentSymbol, and
        // `lsp::completion`'s own member-completion probe, which
        // specifically needs the AST from *before* the very edit that
        // introduced its own trigger) reads it for as long as the file
        // keeps failing to parse.
        let file_id = self.files.id_for(path);
        let parsed =
            parse_file_with_names_and_file_id(path, new_source, classified.is_stub, None, file_id)?;
        // An identical edit cannot change what another file reads from
        // this declaration. Keep the registry IDs and result overlays
        // stable, and recheck only the edited file for the caller's
        // diagnostics payload.
        if self
            .files
            .get(path)
            .is_some_and(|previous| previous.content_hash == parsed.content_hash)
        {
            self.unchecked.clear();
            return Ok(self.recheck([path.to_path_buf()], &should_abort));
        }
        let text = match new_source {
            syntax::Source::Text(text) => std::borrow::Cow::Borrowed(text),
            syntax::Source::Bytes(bytes) => String::from_utf8_lossy(bytes),
        };
        let previous = self.unindex_file(path);
        let appeared = previous.is_none();
        let declaration_ids = self.declaration_ids_for_edit(&parsed.built_decls, previous.as_ref());

        // A file already loaded keeps its role, which an edit does not
        // change; a new one takes the role its roots give it, as a load
        // would.
        let analyzed = previous.as_ref().map_or_else(
            || classified.role == roles::Role::Analysed,
            |previous| previous.analyzed,
        );
        let load_order = match previous.as_ref() {
            Some(previous) => previous.load_order,
            None => self.make_room_in_load_order(path, analyzed),
        };
        let mut rebuilt = DeclaredNames {
            functions: parsed.functions.clone(),
            classes: parsed.classes.clone(),
            constants: parsed.constants.clone(),
        };
        if let Some(previous) = previous {
            rebuilt.functions.extend(previous.functions);
            rebuilt.classes.extend(previous.classes);
            rebuilt.constants.extend(previous.constants);
        }
        self.rebuild_declarations(
            &rebuilt,
            Some(DeclarationSource {
                analyzed,
                load_order,
                path,
                declarations: &parsed.built_decls,
                declaration_ids: &declaration_ids,
                pending: true,
            }),
        );
        let id = self.files.id_for(path);
        let len = text.len();
        let source = if self.retain_source && self.open.contains(path) {
            text.into_owned()
        } else {
            String::new()
        };
        let placed = Placed {
            id,
            path,
            analyzed,
            load_order,
        };
        self.install_parsed_file(placed, parsed, declaration_ids, source, len);
        self.rebuild_path_ranks();
        let mut affected = self.finish_declarations();

        affected.insert(path.to_path_buf());
        affected.extend(self.declaring_the_same(&rebuilt));
        if appeared {
            affected.extend(self.including(&[path.to_path_buf()]));
        }
        self.unchecked.clear();
        Ok(self.recheck(affected, &should_abort))
    }

    /// The files the last cancelled `update_file_cancellable` affected but
    /// never rechecked; their previous diagnostics no longer hold.
    pub fn take_unchecked(&mut self) -> Vec<PathBuf> {
        std::mem::take(&mut self.unchecked)
    }

    /// Unwinds `path`'s prior contributions to the shared registry/
    /// `dependents` - the same undo `update_file` performs before
    /// re-indexing a new parse - but with no re-index step afterward:
    /// `path` is gone, not changed. Reruns the finishing passes, then
    /// re-checks the files reading a declaration they changed or removed
    /// (`affected_files`), as `update_file` does.
    ///
    /// The finishing passes start again from every remaining class's
    /// declaration as built, so nothing the removed files contributed
    /// survives in the classes using them (trait and `@mixin` members,
    /// inherited PHPDoc); a class still naming a removed one directly
    /// (`extends Base`) reads as it would with `Base` never loaded.
    ///
    /// `path` itself is always a key in the returned map, with an empty `Vec` when `path` was never loaded in the first
    /// place - a harmless no-op a caller doesn't need to guard with a
    /// prior loaded-state check, and exactly the empty
    /// `publishDiagnostics` payload needed to clear whatever the editor
    /// was last showing for a file that no longer exists.
    pub fn remove_file(&mut self, path: &Path) -> FxHashMap<PathBuf, Vec<Diagnostic>> {
        self.remove_files(std::slice::from_ref(&path.to_path_buf()))
    }

    /// Batch counterpart to `remove_file` - unwinds every one of `paths`
    /// (each via `unindex_file`) and runs the shared registry recompute
    /// (`finish_loading`) at most ONCE for the whole batch, not once per
    /// path - the same "one recompute per batch, not per item" discipline
    /// `Server::add_workspace_folders` (3.2) needs on the load side.
    /// Every one of `paths` is always a key in the result (generalizing
    /// `remove_file`'s own guarantee - `check_file` already returns an
    /// empty `Vec` for an unloaded/just-removed path, exactly the
    /// clearing `publishDiagnostics` payload a caller needs for a file
    /// that no longer exists), even when none of them were actually
    /// loaded - but the recompute/BFS itself is skipped entirely in that
    /// case, since there is nothing for it to discover.
    pub fn remove_files(&mut self, paths: &[PathBuf]) -> FxHashMap<PathBuf, Vec<Diagnostic>> {
        let mut removed = DeclaredNames::default();
        let mut places = Vec::new();
        for path in paths {
            if let Some(file) = self.unindex_file(path) {
                removed.functions.extend(file.functions);
                removed.classes.extend(file.classes);
                removed.constants.extend(file.constants);
                places.push(file.load_order);
            }
        }
        let any_removed = !places.is_empty();
        self.close_load_order(places);
        if any_removed {
            self.rebuild_path_ranks();
        }
        let mut affected: FxHashSet<PathBuf> = paths.iter().cloned().collect();
        affected.extend(self.including(paths));
        if any_removed {
            self.rebuild_declarations(&removed, None);
            affected.extend(self.finish_declarations());
            affected.extend(self.declaring_the_same(&removed));
        }

        self.recheck(affected, &|| false)
    }

    /// The files declaring a function or a class of `names`: what reports a
    /// declaration as declared more than once in each reads the others.
    fn declaring_the_same(&self, names: &DeclaredNames) -> Vec<PathBuf> {
        let functions = names
            .functions
            .iter()
            .flat_map(|name| self.duplicate_fn_sites.paths_of(name));
        let classes = names
            .classes
            .iter()
            .flat_map(|name| self.duplicate_class_sites.paths_of(name));
        functions
            .chain(classes)
            .filter(|&id| self.files.get_by_id(id).is_some())
            .map(|id| self.files.path(id).to_path_buf())
            .collect()
    }

    /// The files to recheck once the classes, functions and constants
    /// `changed` names (`deps::dependency_key`s) took a new resolved form:
    /// every file referencing one of them, or a declaration whose own
    /// signature, hierarchy or PHPDoc mentions one of them, transitively.
    /// A declaration taking its shape from a changed one is itself among
    /// `changed` when its resolved form changed; the signature closure
    /// reaches the files that read a changed declaration only through
    /// another's types (`f(A $a) { $a->b()->c(); }` where `A::b()`
    /// returns the changed `B`).
    fn walk_affected(
        &self,
        walk: &mut AffectedWalk,
        changed: impl IntoIterator<Item = deps::RefKey>,
    ) {
        let AffectedWalk {
            affected,
            seen,
            stub_files,
        } = walk;
        let mut frontier: Vec<deps::RefKey> = changed.into_iter().collect();
        while let Some(key) = frontier.pop() {
            if seen.contains(&key) {
                continue;
            }
            affected.extend(self.dependents.get(&key).into_iter().flatten().cloned());
            for file in self.signature_dependents.get(&key).into_iter().flatten() {
                let Some(state) = self.files.get_by_id(*file) else {
                    continue;
                };
                // The names the file declares, as the keys they are
                // referenced by, unless they were reached already or are
                // `key`, which is being walked.
                let reach = |next: deps::RefKey, frontier: &mut Vec<deps::RefKey>| {
                    if next != key && !seen.contains(&next) {
                        frontier.push(next);
                    }
                };
                for name in &state.functions {
                    reach(deps::RefKey::function(name), &mut frontier);
                }
                for name in &state.classes {
                    reach(deps::RefKey::class(name), &mut frontier);
                }
            }
            // A stub's name is a key already, and only a file that reads it
            // leads further: through the files that reference it, which
            // its stub file's closure holds.
            if let Some(stub_key) = key.stub_key() {
                for file in stubs::stub_dependent_files(stub_key) {
                    stub_files.add_closure(file, |file| {
                        for (is_class, name) in stubs::stub_file_names(file) {
                            if name == stub_key {
                                continue;
                            }
                            let next = if is_class {
                                deps::RefKey::class_text(name)
                            } else {
                                deps::RefKey::function_text(name)
                            };
                            if !seen.contains(&next)
                                && (self.dependents.contains_key(&next)
                                    || self.signature_dependents.contains_key(&next))
                            {
                                frontier.push(next);
                            }
                        }
                    });
                }
            }
            seen.insert(key);
        }
    }

    /// The checking settings, to change directly.
    #[cfg(test)]
    pub(crate) fn check_settings_mut(&mut self) -> &mut CheckSettings {
        &mut self.settings
    }

    /// `path`'s type dump: every statement's recorded variable types.
    #[cfg(test)]
    pub(crate) fn type_dump(&self, path: &Path) -> Vec<String> {
        match self
            .check_files(
                &[path.to_path_buf()],
                None,
                false,
                Records {
                    types: true,
                    expr_types: false,
                    doc_resolutions: false,
                },
            )
            .pop()
        {
            Some((_, Ok(check))) => check.type_dump,
            Some((_, Err(message))) => panic!("{message}"),
            None => Vec::new(),
        }
    }

    /// Type-checks one already-loaded file against the project's current
    /// registry, returning its diagnostics. Its private property types are
    /// judged against the writes of every file making them - its own, and
    /// those declaring its classes' traits (`check_files`).
    #[hotpath::measure]
    pub fn check_file(&self, path: &Path) -> Vec<Diagnostic> {
        let paths = [path.to_path_buf()];
        match self
            .check_files(&paths, None, false, Records::default())
            .pop()
        {
            Some((_, Ok(check))) => check.diagnostics,
            Some((_, Err(message))) => panic!("{message}"),
            None => Vec::new(),
        }
    }

    /// Checks `paths` (in parallel on `pool` when there is one), each
    /// file's panic caught as its outcome. With `collect_member_refs`, the
    /// Tier-2 member-reference data too (`UseSiteIndex::replace_members`);
    /// and the records `records` asks for. The private property
    /// types each file declares are judged once every file is checked,
    /// against the writes all of them made plus those of any other file
    /// writing the same properties (`judge_property_types`).
    pub(crate) fn check_files(
        &self,
        paths: &[PathBuf],
        pool: Option<&rayon::ThreadPool>,
        collect_member_refs: bool,
        records: Records,
    ) -> Vec<(PathBuf, CheckOutcome)> {
        self.check_files_until(paths, pool, &|_| collect_member_refs, records, &|| false)
            .0
    }

    /// `check_files`, the member references collected for the files
    /// `collect_member_refs` accepts, each file's check started only while
    /// `should_abort` answers `false`; the files it stopped are returned
    /// apart, unchecked, in `paths`' order.
    fn check_files_until(
        &self,
        paths: &[PathBuf],
        pool: Option<&rayon::ThreadPool>,
        collect_member_refs: &(dyn Fn(&Path) -> bool + Sync),
        records: Records,
        should_abort: &(dyn Fn() -> bool + Sync),
    ) -> (Vec<(PathBuf, CheckOutcome)>, Vec<PathBuf>) {
        let outcomes = map_on_pool_largest_first(
            pool,
            paths,
            |path| self.files.get(path).map_or(0, |file| file.source_len),
            |path| {
                if should_abort() {
                    return None;
                }
                let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    self.run_checker(path, collect_member_refs(path), records)
                }))
                .map_err(panic_message);
                crate::heap::file_checked();
                Some(outcome)
            },
        );
        let mut checks = Vec::with_capacity(paths.len());
        let mut stopped = Vec::new();
        for (path, outcome) in paths.iter().zip(outcomes) {
            match outcome {
                Some(outcome) => checks.push((path.clone(), outcome)),
                None => stopped.push(path.clone()),
            }
        }
        self.judge_property_types(&mut checks, pool);
        self.keep_include_probes(&mut checks);
        // Each file's diagnostics are finished from the project as it is,
        // which nothing here changes, so the files are finished side by
        // side; they stay where they are in `checks`.
        let finish = |(path, outcome): &mut (PathBuf, CheckOutcome)| {
            if let Ok(check) = outcome {
                let diagnostics = std::mem::take(&mut check.diagnostics);
                check.diagnostics = self.finish_diagnostics(path, diagnostics);
            }
        };
        match pool {
            Some(pool) => pool.install(|| checks.par_iter_mut().for_each(finish)),
            None => checks.iter_mut().for_each(finish),
        }
        (checks, stopped)
    }

    /// Keeps the places each of `checks` looked for an included file, for
    /// a project that is changed after it is built. A file that panicked
    /// keeps what its check before looked at.
    fn keep_include_probes(&self, checks: &mut [(PathBuf, CheckOutcome)]) {
        if !self.keeps_as_built {
            return;
        }
        let mut kept = self
            .include_probes
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for (path, outcome) in checks.iter_mut() {
            let Ok(check) = outcome else { continue };
            let mut probes: Vec<PathBuf> = std::mem::take(&mut check.include_probes)
                .iter()
                .map(|probe| self.absolute_path(probe))
                .collect();
            probes.sort_unstable();
            probes.dedup();
            if probes.is_empty() {
                kept.remove(path);
            } else {
                kept.insert(path.clone(), probes);
            }
        }
    }

    /// The files whose last check looked for an included file at one of
    /// `paths` or under one: a directory appearing or vanishing makes what
    /// it holds appear or vanish.
    fn including(&self, paths: &[PathBuf]) -> Vec<PathBuf> {
        let paths: Vec<PathBuf> = paths.iter().map(|path| self.absolute_path(path)).collect();
        let kept = self
            .include_probes
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        kept.iter()
            .filter(|(_, probes)| {
                probes
                    .iter()
                    .any(|probe| paths.iter().any(|path| probe.starts_with(path)))
            })
            .map(|(file, _)| file.clone())
            .collect()
    }

    /// Rechecks the files whose last check looked for an included file at
    /// one of `paths` or under one, where a file or a directory the project
    /// does not load appeared or vanished.
    pub fn recheck_including(&mut self, paths: &[PathBuf]) -> FxHashMap<PathBuf, Vec<Diagnostic>> {
        let including = self.including(paths);
        self.recheck(including, &|| false)
    }

    /// Checks `paths` again as an edit leaves the project, in one
    /// `check_files` over the worker pool, so their private property types
    /// are judged once for all of them; then keeps what each check found
    /// for the use-site index and the whole-program checks, the one part
    /// that changes the project. A file whose check had not started when
    /// `should_abort` answered `true` is left unchecked
    /// (`take_unchecked`). A check's panic propagates, before anything is
    /// kept.
    fn recheck(
        &mut self,
        paths: impl IntoIterator<Item = PathBuf>,
        should_abort: &(dyn Fn() -> bool + Sync),
    ) -> FxHashMap<PathBuf, Vec<Diagnostic>> {
        let mut paths: Vec<PathBuf> = paths.into_iter().collect();
        paths.sort_unstable();
        paths.dedup();
        let covered: FxHashSet<PathBuf> = paths
            .iter()
            .filter(|path| self.member_index_covers(path))
            .cloned()
            .collect();
        let pool = worker_pool();
        let (checks, stopped) = self.check_files_until(
            &paths,
            Some(&pool),
            &|path| covered.contains(path),
            Records::default(),
            should_abort,
        );
        let checks: Vec<(PathBuf, FileCheck)> = checks
            .into_iter()
            .map(|(path, outcome)| match outcome {
                Ok(check) => (path, check),
                Err(message) => panic!("checking {} panicked: {message}", path.display()),
            })
            .collect();
        self.unchecked.extend(stopped);
        let mut results = FxHashMap::default();
        for (path, mut check) in checks {
            self.use_sites
                .replace_members(&path, std::mem::take(&mut check.member_refs));
            self.keep_check_facts(&path, &mut check, covered.contains(&path));
            results.insert(path, check.diagnostics);
        }
        results
    }

    /// Judges the private property types `checks` recorded against the
    /// writes made in the scope of their classes: those in `checks`, and
    /// those of the other files declaring one of those classes or a trait
    /// they use (transitively), which are checked for their writes alone.
    /// Facts merge in path order, so a union's member order doesn't
    /// depend on scheduling. A file that panicked contributes nothing.
    fn judge_property_types(
        &self,
        checks: &mut [(PathBuf, CheckOutcome)],
        pool: Option<&rayon::ThreadPool>,
    ) {
        let mut writers: FxHashSet<ClassId> = FxHashSet::default();
        for (_, outcome) in checks.iter() {
            if let Ok(check) = outcome {
                for site in &check.property_type_sites {
                    writers.extend(site.writers().iter().copied());
                }
            }
        }
        if writers.is_empty() {
            return;
        }
        let mut scopes: FxHashSet<ClassId> = FxHashSet::default();
        let mut stack: Vec<ClassId> = writers.iter().copied().collect();
        while let Some(class) = stack.pop() {
            if let Some(references) = self.registry.class_references(class) {
                stack.extend(
                    references
                        .uses
                        .iter()
                        .filter_map(|used| self.registry.class(used.as_ref()).map(|(id, _, _)| id)),
                );
            }
            scopes.insert(class);
        }
        let checked: FxHashSet<&PathBuf> = checks.iter().map(|(path, _)| path).collect();
        let mut others: Vec<PathBuf> = self
            .files
            .iter()
            .filter(|(path, file)| {
                !checked.contains(path)
                    && file
                        .classes
                        .iter()
                        .filter_map(|class| {
                            self.registry.class(class.as_str()).map(|(id, _, _)| id)
                        })
                        .any(|class| scopes.contains(&class))
            })
            .map(|(path, _)| path.clone())
            .collect();
        others.sort();
        let other_facts = map_on_pool(pool, &others, |path| {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                self.run_checker(path, false, Records::default())
                    .property_assign_facts
            }))
            .ok()
        });
        let mut sources: Vec<(&PathBuf, PropertyAssignFacts)> = others
            .iter()
            .zip(other_facts)
            .filter_map(|(path, facts)| Some((path, facts?)))
            .collect();
        for (path, outcome) in checks.iter_mut() {
            if let Ok(check) = outcome {
                sources.push((path, std::mem::take(&mut check.property_assign_facts)));
            }
        }
        sources.sort_by_key(|(path, _)| self.path_rank(path));
        let mut merged: PropertyAssignFacts = FxHashMap::default();
        for (_, facts) in sources {
            for (writer, props) in facts {
                if !writers.contains(&writer) {
                    continue;
                }
                let entry = merged.entry(writer).or_default();
                for (property, mut types) in props {
                    entry.entry(property).or_default().append(&mut types);
                }
            }
        }
        let relation = self.settings.rule_levels.relation(&self.registry);
        for (path, outcome) in checks.iter_mut() {
            if let Ok(check) = outcome
                && let Some(file) = self.files.get(path.as_path())
            {
                let sites = std::mem::take(&mut check.property_type_sites);
                check
                    .diagnostics
                    .extend(checker::check_too_wide_property_types(
                        &sites,
                        &merged,
                        &relation,
                        &file.line_index,
                    ));
            }
        }
    }

    /// A bare, un-run `Checker` for `path` plus its AST, for an LSP
    /// feature that needs to drive `check_program` itself with extra
    /// collection flags set first (e.g. `textDocument/inlayHint`'s
    /// `collect_inlay_hints`) rather than through `check_files`'
    /// fixed diagnostic pipeline. Mirrors `run_checker`'s setup exactly.
    pub(crate) fn checker_for(&self, path: &Path) -> Option<(Checker<'_>, AstRef<'_>)> {
        let file = self.files.get(path)?;
        let ast = self.ast_of(path)?;
        let mut checker = Checker::new(&self.registry)
            .with_sql(&self.sql)
            .with_settings(&self.settings)
            .with_bodies(&self.files)
            .with_line_index(&file.line_index);
        checker.set_pest_this_type(self.pest_config.resolve_this_type(path, &self.registry));
        checker.set_pest_hook_properties(self.pest_config.resolve_hook_properties(path));
        checker.set_current_file(path);
        if let Some(id) = self.files.id_of(path) {
            checker.set_current_file_id(id);
        }
        checker.set_current_file_absolute(self.absolute_path(path));
        checker.set_working_dir(self.working_dir.clone());
        checker.set_closure_rebind_possible(
            file.facts.mentions & checker::unused::MENTIONS_REBIND != 0,
        );
        checker.set_include_guards_possible(
            file.facts.mentions & checker::includes::MENTIONS_GUARD != 0,
        );
        checker.set_syntax_marks(file.facts.syntax);
        Some((checker, ast))
    }

    /// `textDocument/semanticTokens/full`'s own independent re-walk of
    /// `path` - no cache, no shared pass with `check_file`/`update_file`'s
    /// own diagnostics walk (see this item's own open-risks note on that
    /// cost). Built on `checker_for` exactly like `inlay_hints::collect`.
    pub(crate) fn semantic_tokens_for_file(
        &self,
        path: &Path,
    ) -> Vec<checker::semantic_tokens::UsageSemanticToken> {
        let Some((mut checker, ast)) = self.checker_for(path) else {
            return Vec::new();
        };
        checker.check_program(&ast);
        checker.take_semantic_tokens()
    }

    /// Runs the checker over `path`, leaving its private property types
    /// unjudged and its diagnostics unfiltered (`finish_diagnostics`).
    fn run_checker(&self, path: &Path, collect_member_refs: bool, records: Records) -> FileCheck {
        #[cfg(test)]
        tests::panic_injection::panic_if_injected(path);
        let Some((mut checker, ast)) = self.checker_for(path) else {
            return FileCheck::empty();
        };
        checker.set_defer_property_type_checks(true);
        checker.set_collect_member_refs(collect_member_refs);
        checker.set_records(records);
        checker.check_program(&ast);
        if collect_member_refs {
            checker.collect_default_and_const_value_member_refs(&ast);
        }
        FileCheck {
            member_refs: checker.take_member_ref_sites(),
            escaped_enums: checker.take_escaped_enums(),
            escaped_member_classes: checker.take_escaped_member_classes(),
            named_argument_calls: checker.take_named_argument_call_facts(),
            type_dump: checker.take_type_dump(),
            expr_types: checker.take_expr_types(),
            doc_resolutions: checker.take_doc_resolutions(),
            property_assign_facts: checker.take_property_assign_facts(),
            property_type_sites: checker.take_property_type_sites(),
            include_probes: checker.take_include_probes(),
            diagnostics: checker.into_diagnostics(),
        }
    }

    /// `path`'s checker `diagnostics` with the project-level ones added,
    /// sorted.
    fn finish_diagnostics(&self, path: &Path, mut diagnostics: Vec<Diagnostic>) -> Vec<Diagnostic> {
        let Some(file) = self.files.get(path) else {
            return diagnostics;
        };
        let Some(file_id) = self.files.id_of(path) else {
            return diagnostics;
        };
        if self.settings.rules.check_duplicate_function_declarations {
            diagnostics.extend(duplicate_functions::check_duplicate_function_declarations(
                file_id,
                &file.line_index,
                &|path, offset| self.line_in(path, offset),
                &|id| Some(self.files.path(id).to_path_buf()),
                &self.duplicate_fn_sites,
            ));
        }
        if self.settings.rules.check_duplicate_class_declarations {
            diagnostics.extend(duplicate_classes::check_duplicate_class_declarations(
                file_id,
                &file.line_index,
                &|path, offset| self.line_in(path, offset),
                &|id| Some(self.files.path(id).to_path_buf()),
                &self.duplicate_class_sites,
            ));
        }
        for (line, message) in &file.ignores.parse_errors {
            diagnostics.push(
                diagnostics::DiagnosticBuilder::new(
                    format!("Parse error in @phpstan-ignore: {message}"),
                    Span::default(),
                    0,
                    "ignore.parseError",
                )
                .non_ignorable()
                .build_at(line_start_range(*line)),
            );
        }
        diagnostics.sort_by_key(|d| (d.span.start.line, d.span.start.col));
        if self.settings.phpstan_api {
            diagnostics.extend(checker::phpstan_api::check_phpstan_api_usage(
                file.ast(),
                &file.line_index,
                &self.registry,
                Some(self.package_name(path)),
            ));
            // `into_diagnostics` above already sorted the checker's own
            // diagnostics into span order - re-sort once more now that
            // `check_phpstan_api_usage`'s own (already span-ordered
            // amongst themselves, but not yet interleaved) diagnostics
            // have been appended, so every caller still sees one
            // consistent, fully span-ordered list regardless of whether
            // this gate is on.
            diagnostics.sort_by_key(|d| (d.span.start.line, d.span.start.col));
        }
        diagnostics
    }

    pub fn registry(&self) -> &Registry {
        &self.registry
    }

    /// Runs every `rules` entry's own `check_class`/
    /// `check_function` against every class-like/plain-function this
    /// project's own registry currently knows about - the single entry
    /// point `reflection_rule::ReflectionRule`'s own doc comment
    /// promises. Iteration order follows `FxHashMap`'s own (unspecified,
    /// load-order-dependent) iteration - a caller that needs a stable
    /// output order should sort the returned `Vec` itself, the same way
    /// `main/check_project.rs`'s own batch-checking path already sorts
    /// before printing.
    pub fn run_reflection_rules(
        &self,
        rules: &[Box<dyn crate::reflection_rule::ReflectionRule>],
    ) -> Vec<Diagnostic> {
        let mut out = Vec::new();
        for (_, fqn, info) in self.registry.classes_by_id() {
            for rule in rules {
                out.extend(rule.check_class(info, fqn));
            }
        }
        for (_, fqn, sig) in self.registry.functions_by_id() {
            for rule in rules {
                out.extend(rule.check_function(sig, fqn));
            }
        }
        out
    }

    /// Runs the checker over exactly `paths` (which must already be
    /// loaded), on `pool`. Panic-transparent - a real checker panic in
    /// `paths` propagates to the caller, which is what a unit test wants;
    /// see `check_paths_isolating_panics` for the batch-safe counterpart.
    pub fn check_paths(
        &self,
        paths: &[PathBuf],
        pool: &rayon::ThreadPool,
    ) -> FxHashMap<PathBuf, Vec<Diagnostic>> {
        self.check_files(paths, Some(pool), false, Records::default())
            .into_iter()
            .map(|(path, outcome)| match outcome {
                Ok(check) => (path, check.diagnostics),
                Err(message) => panic!("checking {} panicked: {message}", path.display()),
            })
            .collect()
    }

    /// Same as `check_paths`, but a panic while checking one file is
    /// caught and reported rather than aborting every other file's
    /// result, with the panic hook silenced meanwhile, since a
    /// whole-workspace pass runs over uncurated code the caller doesn't
    /// control, where one file's latent checker bug must not take down
    /// every other file's diagnostics. `Registry.sql_reflector`'s only interior mutability is
    /// a plain `OnceLock` - if its lazy-init closure panics mid-connection
    /// attempt, the cell is left simply uninitialized (not corrupted;
    /// `OnceLock::get_or_init`'s own contract), so a later observer sees
    /// either "not yet initialized" or a real, valid reflector - never a
    /// torn/partial one. `AssertUnwindSafe` in `check_files` is that
    /// specific, checked claim, not a blanket "trust me".
    pub fn check_paths_isolating_panics(
        &self,
        paths: &[PathBuf],
        pool: &rayon::ThreadPool,
    ) -> CheckPathsIsolatingPanics {
        let default_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let outcomes = self.check_files(paths, Some(pool), false, Records::default());
        std::panic::set_hook(default_hook);

        let mut results = FxHashMap::default();
        let mut panics = Vec::new();
        for (path, outcome) in outcomes {
            match outcome {
                Ok(check) => {
                    results.insert(path, check.diagnostics);
                }
                Err(msg) => panics.push((path, msg)),
            }
        }
        panics.sort();
        (results, panics)
    }

    /// Runs the checker over every currently-loaded file, not just one -
    /// the whole-project counterpart to `check_file`. `--check-project`
    /// no longer uses this directly (it needs to check only the files it
    /// was asked to analyze, not every file loaded for context - see
    /// `main.rs::run_check_project`), but it's still the right tool for
    /// tests/tooling that need to see every diagnostic across a real
    /// corpus at once, rather than one file (and its
    /// transitively-affected dependents) at a time the way `update_file`
    /// does.
    pub fn check_all(&self, pool: &rayon::ThreadPool) -> FxHashMap<PathBuf, Vec<Diagnostic>> {
        self.check_paths(&self.files.keys().cloned().collect::<Vec<_>>(), pool)
    }

    /// Resolves one whole-program finding's own reportable location: only
    /// first-party files report, never vendor/context ones swept in only
    /// because the registry records every loaded file's declarations,
    /// analysed or not (a vendor-only dead enum case used to create a brand
    /// new, never-otherwise-checked entry in `check-project`'s own `results`
    /// map). `None` for an unloaded or context path.
    fn whole_program_diagnostic(
        &self,
        path: &Path,
        span: Span,
        message: String,
        identifier: &'static str,
    ) -> Option<Diagnostic> {
        let file = self.files.get(path)?;
        if !file.analyzed {
            return None;
        }
        Some(DiagnosticBuilder::new(message, span, 5, identifier).build(&file.line_index))
    }

    /// The function and class-like declarations of the analysed files,
    /// anonymous classes included, in path order and then the order of
    /// each file's declarations.
    fn analysed_declarations(
        &self,
    ) -> (
        Vec<whole_program::Declaration<'_>>,
        Vec<(
            whole_program::Declaration<'_>,
            &crate::registry::ClassReflection,
        )>,
    ) {
        let mut files: Vec<(&PathBuf, &FileState)> = self
            .files
            .iter()
            .filter(|(_, file)| file.analyzed)
            .collect();
        files.sort_by(|a, b| a.0.cmp(b.0));
        let mut functions = Vec::new();
        let mut classes = Vec::new();
        for (path, file) in files {
            for decl in &file.built_decls {
                match decl {
                    BuiltDecl::Function { name, sig, .. } => {
                        functions.push(whole_program::Declaration {
                            name,
                            path,
                            span: sig.provenance().declaration_span(),
                        });
                    }
                    BuiltDecl::Class { name, info, .. } => classes.push((
                        whole_program::Declaration {
                            name,
                            path,
                            span: info.provenance().declaration_span(),
                        },
                        info.as_ref(),
                    )),
                    BuiltDecl::GlobalConst { .. } => {}
                }
            }
        }
        (functions, classes)
    }

    /// Every class-like declaration of a project file, analysed or
    /// context, as the namespace graph reads it, in path order.
    fn project_class_declarations(
        &self,
    ) -> Vec<whole_program::circular_namespace_deps::ClassDeclaration<'_>> {
        let mut files: Vec<(&PathBuf, &FileState)> = self.files.iter().collect();
        files.sort_by(|a, b| a.0.cmp(b.0));
        files
            .into_iter()
            .flat_map(|(path, file)| {
                file.built_decls.iter().filter_map(move |decl| match decl {
                    BuiltDecl::Class {
                        name,
                        info,
                        namespace,
                        ..
                    } => Some(whole_program::circular_namespace_deps::ClassDeclaration {
                        declaration: whole_program::Declaration {
                            name,
                            path,
                            span: info.provenance().declaration_span(),
                        },
                        namespace,
                        analysed: file.analyzed,
                    }),
                    _ => None,
                })
            })
            .collect()
    }

    /// The registry's names of the class-likes `names` names, once each, in
    /// the order first named: a name declared twice has one entry.
    fn registry_names<'n>(&self, names: impl IntoIterator<Item = &'n str>) -> Vec<&str> {
        let mut seen: FxHashSet<&str> = FxHashSet::default();
        names
            .into_iter()
            .filter_map(|name| {
                self.registry
                    .lookup_class(name)
                    .map(|(declared, _)| declared)
            })
            .filter(|declared| seen.insert(declared))
            .collect()
    }

    /// Runs every opt-in whole-program diagnostic check (currently
    /// `check_dead_enum_cases`, `check_dead_code`, `check_circular_
    /// dependencies`, `check_dead_abstract_classes`, and `check_dead_
    /// members`) - see `project::whole_program`'s own module doc comment
    /// for why these can't run through the ordinary per-file `Checker`
    /// path `check_file`/`check_all` above use. `check-project` calls
    /// this once, after its own primary per-file pass has populated
    /// `results`, merging this method's own return value in before
    /// level-filtering/`ignore_errors`/baseline (so `@phpstan-ignore`/
    /// baseline suppression apply to a whole-program finding exactly
    /// like an ordinary one). Each check reads what the project keeps of
    /// every file's last check (`keep_check_facts`), so an edited project
    /// answers as a fresh one does.
    ///
    /// `check_dead_enum_cases` and `check_dead_members` both need
    /// `ensure_member_index_built`'s own Tier-2 use-site index - a real
    /// cost when it has to build it itself (a second, redundant
    /// `Checker` pass per file), avoided entirely when the caller
    /// already seeded it via `keep_analysed_check`/`mark_member_index_
    /// built` (`check-project`'s own primary pass does exactly this,
    /// whenever either flag is on - see its own doc comment) before
    /// calling here, in which case `ensure_member_index_built` is a
    /// complete no-op. `check_dead_code` needs only Tier 1 (`use_sites`'s
    /// class/function reference sites) - cheaper than Tier 2 (a plain
    /// syntactic walk, not a real `Checker` pass), but still real per-
    /// loaded-file cost, so `Project::install_parsed_file` itself gates
    /// populating it in batch mode on this exact flag - see that call
    /// site's own doc comment. `check_circular_dependencies` and
    /// `check_dead_abstract_classes` need neither Tier - both read
    /// directly off `Registry.classes`' own already-resolved fields, so
    /// each costs nothing beyond its own traversal regardless of how
    /// large the project is.
    pub(crate) fn check_whole_program(&mut self) -> FxHashMap<PathBuf, Vec<Diagnostic>> {
        let mut results: FxHashMap<PathBuf, Vec<Diagnostic>> = FxHashMap::default();

        if self.settings.rules.check_dead_enum_cases || self.settings.rules.check_dead_members {
            self.ensure_member_index_built();
        }
        let (functions, classes) = self.analysed_declarations();

        if self.settings.rules.check_dead_enum_cases {
            let escaped: FxHashSet<String> = self
                .escapes
                .values()
                .flat_map(|escapes| escapes.enums.iter().cloned())
                .collect();
            let dead = whole_program::dead_enum_cases::find_dead_cases(
                self.registry_names(classes.iter().map(|(d, _)| d.name)),
                &self.registry,
                &self.use_sites,
                &escaped,
            );
            for (fqn, case) in dead {
                let Some((file_id, span)) = self.registry.constant_declaration(&fqn, &case) else {
                    continue;
                };
                let path = self.files.path(file_id).to_path_buf();
                if let Some(d) = self.whole_program_diagnostic(
                    &path,
                    span,
                    format!("Enum case {fqn}::{case} is never used."),
                    "enumCase.unused",
                ) {
                    results.entry(path).or_default().push(d);
                }
            }
        }

        if self.settings.rules.check_dead_code {
            let dead_functions = whole_program::dead_code::find_dead_functions(
                functions.iter().copied(),
                &self.use_sites,
            );
            for declaration in dead_functions {
                let name = declaration.name;
                if let Some(d) = self.whole_program_diagnostic(
                    declaration.path,
                    declaration.span,
                    format!("Function {name} is never referenced."),
                    "function.unused",
                ) {
                    results
                        .entry(declaration.path.to_path_buf())
                        .or_default()
                        .push(d);
                }
            }
            let dead_classes = whole_program::dead_code::find_dead_classes(
                classes
                    .iter()
                    .filter(|(_, info)| !info.is_anonymous())
                    .map(|(d, _)| *d),
                &self.use_sites,
            );
            for declaration in dead_classes {
                let name = declaration.name;
                if let Some(d) = self.whole_program_diagnostic(
                    declaration.path,
                    declaration.span,
                    format!("Class {name} is never referenced."),
                    "class.unused",
                ) {
                    results
                        .entry(declaration.path.to_path_buf())
                        .or_default()
                        .push(d);
                }
            }
        }

        if self.settings.rules.check_circular_dependencies {
            let declarations = self.project_class_declarations();
            let cycles = whole_program::circular_namespace_deps::find_namespace_cycles(
                &declarations,
                &self.registry,
                |name| self.registry.is_builtin_class(name),
            );
            for cycle in cycles {
                let namespaces = cycle.namespaces.join(", ");
                let declaration = cycle.representative;
                if let Some(d) = self.whole_program_diagnostic(
                    declaration.path,
                    declaration.span,
                    format!("Circular namespace dependency: {namespaces}."),
                    "namespace.circularDependency",
                ) {
                    results
                        .entry(declaration.path.to_path_buf())
                        .or_default()
                        .push(d);
                }
            }
        }

        if self.settings.rules.check_dead_abstract_classes {
            let dead_abstract_classes =
                whole_program::dead_abstract_classes::find_dead_abstract_classes(
                    classes.iter().map(|(d, info)| (*d, *info)),
                    &self.registry,
                );
            for declaration in dead_abstract_classes {
                let name = declaration.name;
                if let Some(d) = self.whole_program_diagnostic(
                    declaration.path,
                    declaration.span,
                    format!(
                        "Abstract class {name} has no concrete subclass anywhere in the project."
                    ),
                    "class.deadAbstract",
                ) {
                    results
                        .entry(declaration.path.to_path_buf())
                        .or_default()
                        .push(d);
                }
            }
        }

        if self.settings.rules.check_dead_members {
            let escaped: FxHashSet<String> = self
                .escapes
                .values()
                .flat_map(|escapes| escapes.member_classes.iter().cloned())
                .collect();
            let hosts: Vec<Name> = classes
                .iter()
                .filter(|(_, info)| info.kind() == syntax::ast::ClassKind::Trait)
                .flat_map(|(d, _)| self.registry.classes_using_trait(d.name))
                .collect();
            let dead_members = whole_program::dead_members::find_dead_members(
                self.registry_names(
                    classes
                        .iter()
                        .map(|(d, _)| d.name)
                        .chain(hosts.iter().map(|name| name.as_ref())),
                ),
                &self.registry,
                &self.use_sites,
                &escaped,
            );
            for (owner, name, kind) in dead_members {
                let declaration = match kind {
                    whole_program::dead_members::MemberKind::Method => {
                        self.registry.method_declaration(&owner, &name)
                    }
                    whole_program::dead_members::MemberKind::Property => {
                        self.registry.property_declaration(&owner, &name)
                    }
                    whole_program::dead_members::MemberKind::Const => {
                        self.registry.constant_declaration(&owner, &name)
                    }
                };
                let Some((file_id, span)) = declaration else {
                    continue;
                };
                let path = self.files.path(file_id).to_path_buf();
                let (noun, identifier) = match kind {
                    whole_program::dead_members::MemberKind::Method => ("Method", "method.unused"),
                    whole_program::dead_members::MemberKind::Property => {
                        ("Property", "property.unused")
                    }
                    whole_program::dead_members::MemberKind::Const => {
                        ("Constant", "classConstant.unused")
                    }
                };
                if let Some(d) = self.whole_program_diagnostic(
                    &path,
                    span,
                    format!("{noun} {owner}::{name} is never used."),
                    identifier,
                ) {
                    results.entry(path).or_default().push(d);
                }
            }
        }

        if self.settings.rules.check_renamed_named_arguments {
            let mut programs: Vec<(&PathBuf, &[DeclaredClass])> = self
                .files
                .iter()
                .map(|(path, file)| (path, file.declared_classes.as_slice()))
                .collect();
            programs.sort_by_key(|(path, _)| self.path_rank(path));
            let overrides = whole_program::renamed_named_arguments::renamed_parameter_overrides(
                &self.registry,
                programs.into_iter().flat_map(|(_, classes)| classes),
            );
            let mut calls: Vec<(&PathBuf, &checker::NamedArgumentCallFacts)> =
                self.named_argument_calls.iter().collect();
            calls.sort_by_key(|(path, _)| self.path_rank(path));
            let calls = whole_program::renamed_named_arguments::merge_named_argument_calls(calls);
            let collisions =
                whole_program::renamed_named_arguments::find_renamed_named_argument_collisions(
                    &calls, &overrides,
                );
            for (path, span, message) in collisions {
                if let Some(d) = self.whole_program_diagnostic(
                    &path,
                    span,
                    message,
                    "argument.parameterRenamedInSubtype",
                ) {
                    results.entry(path).or_default().push(d);
                }
            }
        }

        results
    }

    pub fn file_count(&self) -> usize {
        self.files.len()
    }

    /// Every path currently loaded, analyzed or context alike - the
    /// `Server::remove_workspace_folders` "which of my loaded files are
    /// under this now-removed root" query needs every one of them, not
    /// just `analyzed_paths`' first-party subset.
    pub fn loaded_paths(&self) -> impl Iterator<Item = &Path> {
        self.files.keys().map(PathBuf::as_path)
    }

    /// Every loaded path whose own `analyzed` flag is `true` - a real,
    /// first-party file, as opposed to context (vendor/scan/stub).
    pub fn analyzed_paths(&self) -> Vec<PathBuf> {
        self.files
            .iter()
            .filter(|(_, state)| state.analyzed)
            .map(|(path, _)| path.clone())
            .collect()
    }

    /// Every loaded path, with whether it is analysed or context, sorted.
    pub(crate) fn file_roles(&self) -> Vec<(PathBuf, bool)> {
        let mut roles: Vec<(PathBuf, bool)> = self
            .files
            .iter()
            .map(|(path, state)| (path.clone(), state.analyzed))
            .collect();
        roles.sort();
        roles
    }

    /// Whether `path` is loaded, analysed or as context.
    pub fn is_loaded(&self, path: &Path) -> bool {
        self.files.get(path).is_some()
    }

    /// `true` if `path` is loaded and analyzed; `false` for an unloaded
    /// path or one loaded only as context.
    pub fn is_analyzed(&self, path: &Path) -> bool {
        self.files.get(path).is_some_and(|state| state.analyzed)
    }
}

impl Default for Project {
    fn default() -> Self {
        Self::new()
    }
}

/// The empty range at the start of `line`.
fn line_start_range(line: u32) -> syntax::span::Range {
    let at = syntax::span::LineCol { line, col: 1 };
    syntax::span::Range { start: at, end: at }
}

/// First and last line of every trait declared in `ast`.
fn trait_line_ranges(ast: &[StmtNode], line_index: &LineIndex) -> Vec<(u32, u32)> {
    let mut ranges = Vec::new();
    for stmt in ast {
        syntax::visit::any_stmt_in_body(stmt, |s| {
            if let StmtKind::ClassDecl(c) = &s.kind
                && c.def.kind == syntax::ast::ClassKind::Trait
            {
                ranges.push((line_index.line(s.span.start), line_index.line(s.span.end)));
            }
            false
        });
    }
    ranges
}

/// The registry of the built-in stubs, made once per process
/// (`Project::plain_base`).
static PLAIN_BASE: std::sync::OnceLock<Registry> = std::sync::OnceLock::new();

/// The registry of the built-in stubs with PHPStan's own source, made once
/// per process when a project needs it (`Project::phpstan_base`).
static PHPSTAN_BASE: std::sync::OnceLock<Registry> = std::sync::OnceLock::new();

/// What the dumps call each stub base.
const PLAIN_BASE_NAME: &str = "stub-base";
const PHPSTAN_BASE_NAME: &str = "stub-base-phpstan";

/// The spellings a file's facts note, each list with the bits of the walk
/// it gates.
static WATCHED: syntax::WatchList = syntax::WatchList(&[
    crate::stub_attributes::WATCH,
    file_declarations::WATCH,
    checker::unused::REBIND_WATCH,
    checker::includes::WATCH,
]);

/// What the tree `ast` refers to, and what its declarations do: from what
/// the parse recorded, or by walking the tree when a pass changed it after.
fn file_references(
    ast: &[StmtNode],
    recorded: Option<syntax::references::FileReferences>,
) -> (FxHashSet<deps::RefKey>, FxHashSet<deps::RefKey>) {
    let Some(recorded) = recorded else {
        return deps::both_referenced_names(ast);
    };
    deps::recorded_references(recorded)
}

/// A file parsed, with its inline ignore comments.
struct Parsed {
    ast: Vec<StmtNode>,
    source: Box<[u8]>,
    names: Box<[Name]>,
    expr_spans: Box<[Span]>,
    stmt_spans: Box<[Span]>,
    // What the tree refers to, as the parse recorded it; none when a pass
    // changed the tree after.
    references: Option<syntax::references::FileReferences>,
    ignores: ignore_comments::IgnoreComments,
    warnings: Vec<LexWarning>,
    facts: FileFacts,
    line_index: Arc<LineIndex>,
}

fn parse_with_ignores<'a>(
    path: &Path,
    source: impl Into<syntax::Source<'a>>,
    names: Option<&mut syntax::name::NameCache>,
) -> Result<Parsed, ParseError> {
    parse_recording(path, source, names, None)
}

/// What a file's parse gives that `ParsedFile` does not keep, recorded for
/// the front-end dump.
#[derive(Default)]
pub(crate) struct ParseRecord {
    pub(crate) ignore_marks: Vec<syntax::lexer::IgnoreMark>,
    pub(crate) goto_labels: Box<[syntax::ast::GotoLabel]>,
    // What the parse recorded of the file's references; none when a pass
    // changed the tree after it.
    pub(crate) references: Option<syntax::references::FileReferences>,
}

/// `parse_with_ignores`, filling `record` when given.
fn parse_recording<'a>(
    path: &Path,
    source: impl Into<syntax::Source<'a>>,
    names: Option<&mut syntax::name::NameCache>,
    record: Option<&mut ParseRecord>,
) -> Result<Parsed, ParseError> {
    let parsed = syntax::parse_watching(source, names, Some(&WATCHED))?;
    let ignores = ignore_comments::scan_parsed(&parsed);
    let mut ast = parsed.stmts;
    // What the file does not name, or hold, there is nothing to find of by
    // walking it.
    let changed = parsed.facts.mentions & crate::stub_attributes::MENTIONS != 0
        && crate::stub_attributes::apply_stub_attributes(&mut ast);
    if parsed.facts.has_anonymous_class {
        crate::registry::name_anonymous_classes(
            &mut ast,
            &path.display().to_string(),
            &parsed.line_index,
        );
    }
    let references = (!changed).then_some(parsed.references);
    if let Some(record) = record {
        *record = ParseRecord {
            ignore_marks: parsed.ignore_marks,
            goto_labels: parsed.goto_labels,
            references: references.clone(),
        };
    }
    Ok(Parsed {
        ast,
        source: parsed.source,
        names: parsed.names,
        expr_spans: parsed.expr_spans,
        stmt_spans: parsed.stmt_spans,
        references,
        ignores,
        warnings: parsed.warnings,
        facts: parsed.facts,
        line_index: parsed.line_index,
    })
}

/// Everything about loading one file that's pure - parsing, plus every
/// fact derived only from the parsed AST or the raw source text, with no
/// dependency on (or mutation of) a `Project`'s shared registry. Computing
/// this for many files is embarrassingly parallel - `install_parsed_file`
/// below does the remaining, order-sensitive registry-mutating half,
/// meant to stay sequential and run in whatever order a caller's own
/// collision-resolution policy needs (see that method's own doc comment).
#[cfg_attr(test, derive(Clone))]
pub struct ParsedFile {
    // The tree and all file-local data its IDs address; absent for a
    // cache-only context entry, which stores the facts derived from it.
    tree: Option<SyntaxTree>,
    // What the lexer skipped over (see `ParsedFile::lex_warnings`).
    lex_warnings: Vec<LexWarning>,
    functions: Vec<Name>,
    classes: Vec<Name>,
    constants: Vec<Name>,
    references: FxHashSet<deps::RefKey>,
    declaration_references: FxHashSet<deps::RefKey>,
    ignores: ignore_comments::IgnoreComments,
    // This file's own declarations, already built into
    // registry-independent `ClassReflection`/`FunctionReflection`/`Type` values (see
    // `registry::build_declarations`'s own doc comment for why that
    // split is pure and safe to compute here, alongside everything else
    // in this struct) - `install_parsed_file` feeds these into
    // `Registry::insert_built_decl` one at a time instead of re-deriving
    // them from `ast` via `Registry::index`, so the expensive "turn an
    // AST node into a ClassReflection/FunctionReflection" work runs here, in
    // parallel, rather than in the sequential insert loop.
    built_decls: Vec<crate::registry::BuiltDecl>,
    // The declarations' file-grouped arena layout. BuiltDecl remains the
    // registry's input while its name tables move to declaration ids.
    file_decls: Arc<crate::registry::FileDecls>,
    // What the file binds, when it is a `Pest.php`.
    pest_facts: Option<PestFacts>,
    // The classes the file declares unconditionally.
    declared_classes: Vec<DeclaredClass>,
    // This file's own top-level function declaration sites, already
    // built (see `DuplicateFunctionSites::merge`'s own doc comment) -
    // `install_parsed_file` merges these into the project-wide table
    // instead of re-walking `ast` via `DuplicateFunctionSites::index`,
    // moving the per-function type-hint cloning `record` does into this
    // already-parallel step. Unlike `built_decls`, this needs `path`
    // (baked into every `FnDeclSite`), which is why `parse_file` below
    // takes one now.
    duplicate_fn_sites: DuplicateFunctionSites,
    // The registry names of the file's anonymous classes.
    anonymous_classes: Vec<Name>,
    // The class-like counterpart to `duplicate_fn_sites` above, built the
    // same way and for the same reason - `parse_file`'s own `is_stub`
    // parameter is baked into every `ClassDeclSite` here (see that
    // field's own doc comment for why it's needed at all).
    duplicate_class_sites: DuplicateClassSites,
    bodies: FileBodies,
    // What it assigns to globals.
    global_facts: Arc<GlobalFacts>,
    // What is true of the file as a whole (`syntax::FileFacts`).
    facts: FileFacts,
    // Its lines, which the lines and columns of its spans are found by.
    line_index: Arc<LineIndex>,
    // The file's class and function use sites, when indexed alongside the
    // parse (`Project::index_use_sites`); `install_parsed_file` indexes
    // them itself otherwise.
    class_fn_refs: Option<Vec<(Span, use_sites::RefTarget)>>,
    // The composer package that owns the file, when found alongside the
    // parse (`Project::note_owner`); `install_parsed_file` finds it itself
    // otherwise.
    owner: Option<Option<composer::PackageOwner>>,
    // What the bytes the file was parsed from are.
    content_hash: source::ContentHash,
}

/// Computes a `ParsedFile` from `source` - see that type's own doc
/// comment. Safe to call concurrently for many files at once (no shared
/// state touched at all). `path` is used only to tag
/// `duplicate_fn_sites`'s own records (see `ParsedFile`'s doc comment on
/// that field) - never read to make any decision, so it's fine that a
/// caller building many `ParsedFile`s concurrently passes a different
/// `path` to each call with no coordination needed between them.
#[hotpath::measure]
pub fn parse_file<'a>(
    path: &Path,
    source: impl Into<syntax::Source<'a>>,
    is_stub: bool,
) -> Result<ParsedFile, ParseError> {
    parse_file_with_names(path, source, is_stub, None)
}

/// `parse_file`, with the names the files of a load share: the cache is
/// asked for a name before the interner is.
pub fn parse_file_with_names<'a>(
    path: &Path,
    source: impl Into<syntax::Source<'a>>,
    is_stub: bool,
    names: Option<&mut syntax::name::NameCache>,
) -> Result<ParsedFile, ParseError> {
    parse_file_recording(path, source, is_stub, names, None)
}

pub(crate) fn parse_file_with_names_and_file_id<'a>(
    path: &Path,
    source: impl Into<syntax::Source<'a>>,
    is_stub: bool,
    names: Option<&mut syntax::name::NameCache>,
    file_id: FileId,
) -> Result<ParsedFile, ParseError> {
    parse_file_recording_with_file_id(path, source, is_stub, names, None, file_id)
}

/// `parse_file_with_names`, filling `record` when given.
pub(crate) fn parse_file_recording<'a>(
    path: &Path,
    source: impl Into<syntax::Source<'a>>,
    is_stub: bool,
    names: Option<&mut syntax::name::NameCache>,
    record: Option<&mut ParseRecord>,
) -> Result<ParsedFile, ParseError> {
    parse_file_recording_with_origin(
        path,
        source,
        is_stub,
        names,
        record,
        Origin::Path(Arc::from(path)),
    )
}

pub(crate) fn parse_file_recording_with_file_id<'a>(
    path: &Path,
    source: impl Into<syntax::Source<'a>>,
    is_stub: bool,
    names: Option<&mut syntax::name::NameCache>,
    record: Option<&mut ParseRecord>,
    file_id: FileId,
) -> Result<ParsedFile, ParseError> {
    parse_file_recording_with_origin(path, source, is_stub, names, record, Origin::File(file_id))
}

fn parse_file_recording_with_origin<'a>(
    path: &Path,
    source: impl Into<syntax::Source<'a>>,
    is_stub: bool,
    names: Option<&mut syntax::name::NameCache>,
    record: Option<&mut ParseRecord>,
    origin: Origin,
) -> Result<ParsedFile, ParseError> {
    let source = source.into();
    let content_hash = source::ContentHash::of_source(source);
    let Parsed {
        ast,
        source: parsed_source,
        names: parsed_names,
        expr_spans,
        stmt_spans,
        references,
        ignores,
        warnings: lex_warnings,
        facts,
        line_index,
    } = parse_recording(path, source, names, record)?;
    let file_declarations::FileDeclarations {
        functions,
        classes,
        constants,
        built_decls,
        file_decls,
        duplicate_fn_sites,
        duplicate_class_sites,
        declared_classes,
    } = file_declarations::collect(&ast, path, is_stub, &origin, &facts);
    let (references, declaration_references) = file_references(&ast, references);
    // The anonymous classes are the declarations built last, one per
    // `new class`, so the file's own walk need not be repeated for them.
    let anonymous_classes = built_decls
        .iter()
        .filter_map(|decl| match decl {
            BuiltDecl::Class { name, info, .. } if info.is_anonymous() => Some(name.clone()),
            _ => None,
        })
        .collect();
    let bodies = FileBodies::of_file(&ast, &facts);
    let global_facts = Arc::new(GlobalFacts::of(&ast));
    let pest_facts = PestFacts::of(path, &ast);
    Ok(ParsedFile {
        tree: Some(SyntaxTree {
            stmts: ast,
            source: parsed_source,
            names: parsed_names,
            expr_spans,
            stmt_spans,
        }),
        lex_warnings,
        functions,
        classes,
        constants,
        references,
        declaration_references,
        built_decls,
        file_decls: Arc::new(file_decls),
        anonymous_classes,
        duplicate_fn_sites,
        duplicate_class_sites,
        ignores,
        bodies,
        global_facts,
        pest_facts,
        declared_classes,
        facts,
        line_index,
        class_fn_refs: None,
        owner: None,
        content_hash,
    })
}

impl ParsedFile {
    #[cfg(test)]
    pub(crate) fn built_declarations(&self) -> &[crate::registry::BuiltDecl] {
        &self.built_decls
    }

    /// Drops the tree and the ignore comments, which only a file whose tree
    /// is kept has; a file read back from the per-file cache has neither.
    pub(crate) fn drop_tree(&mut self) {
        self.tree = None;
        self.ignores = Default::default();
    }

    /// What the lexer skipped over rather than failing on.
    pub fn lex_warnings(&self) -> &[LexWarning] {
        &self.lex_warnings
    }
}

/// Every product of the file; its lines are printed as the positions they
/// give (`front_end_dump`).
impl syntax::canonical::Canonical for ParsedFile {
    fn canonical(&self, p: &mut syntax::canonical::Printer) {
        let ParsedFile {
            tree,
            lex_warnings,
            functions,
            classes,
            constants,
            references,
            declaration_references,
            ignores,
            built_decls,
            file_decls: _,
            pest_facts,
            declared_classes,
            duplicate_fn_sites,
            anonymous_classes,
            duplicate_class_sites,
            bodies,
            global_facts,
            facts,
            line_index: _,
            class_fn_refs,
            owner,
            content_hash: _,
        } = self;
        p.node(
            "ParsedFile",
            &[
                ("ast", tree),
                ("lex_warnings", lex_warnings),
                ("functions", functions),
                ("classes", classes),
                ("constants", constants),
                ("references", references),
                ("declaration_references", declaration_references),
                ("ignores", ignores),
                ("built_decls", built_decls),
                ("pest_facts", pest_facts),
                ("declared_classes", declared_classes),
                ("duplicate_fn_sites", duplicate_fn_sites),
                ("anonymous_classes", anonymous_classes),
                ("duplicate_class_sites", duplicate_class_sites),
                ("bodies", bodies),
                ("global_facts", global_facts),
                ("facts", facts),
                ("class_fn_refs", class_fn_refs),
                ("owner", owner),
            ],
        );
    }
}

/// Recursively collects every `.php` file under `path` - shared by the CLI
/// (`main.rs`'s own subcommands) and the LSP's own workspace-loading path
/// (`lsp.rs`'s `load_workspace`). Deliberately NOT `#[hotpath::measure]`d
/// itself - it's self-recursive (one call per directory in the tree), and
/// hotpath's per-call "Total" sums every call's own measured span, so a
/// recursive function's aggregate massively over-counts its true unique
/// wall-clock contribution (a parent directory's own span already
/// contains every descendant's separately-measured span too). `check_
/// project.rs`'s `collect_analyze_and_context_files` wraps the *whole*
/// top-level scan in one non-recursive, correctly-measured call instead -
/// see that function's own doc comment for the real number this found.
pub fn collect_php_files(path: &Path, out: &mut Vec<PathBuf>) {
    let found = Walk {
        table: &DirTable::default(),
        excluded_dir_names: &[],
        wanted: &|_, name| is_php_file(Path::new(name)),
        skip_dir: None,
        make: &|_| (),
    }
    .files(path, path);
    out.extend(found.into_iter().map(|(file, _, ())| file));
}

/// Whether the file `name` in `dir` is one PHP may be run from: one named with an
/// extension PHP sources have (`.php`, `.inc`, `.module`, `.install`,
/// `.theme`, `.engine`, `.profile`), or one that starts with `<?php` or
/// `<?=`.
pub(crate) fn is_php_run_file(dir: &Path, name: &std::ffi::OsStr) -> bool {
    let path = dir.join(name);
    if has_php_run_extension(&path) {
        return true;
    }
    let mut start = [0u8; 5];
    let Ok(mut file) = fs::File::open(&path) else {
        return false;
    };
    let mut read = 0;
    while read < start.len() {
        match std::io::Read::read(&mut file, &mut start[read..]) {
            Ok(0) | Err(_) => break,
            Ok(n) => read += n,
        }
    }
    opens_php(&start[..read])
}

/// Whether `path` has an extension PHP sources have.
fn has_php_run_extension(path: &Path) -> bool {
    const EXTENSIONS: [&str; 7] = [
        "php", "inc", "module", "install", "theme", "engine", "profile",
    ];
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| EXTENSIONS.iter().any(|known| e.eq_ignore_ascii_case(known)))
}

/// Whether a file that starts with `start` is PHP: it opens with `<?php`
/// or `<?=`.
fn opens_php(start: &[u8]) -> bool {
    start.starts_with(b"<?=")
        || start
            .get(..5)
            .is_some_and(|open| open.eq_ignore_ascii_case(b"<?php"))
}

/// Whether `path` is named as a `.php` file.
pub(crate) fn is_php_file(path: &Path) -> bool {
    path.extension().is_some_and(|e| e == "php")
}

/// A walk for files: what it finds, where it reads directories, what it
/// leaves out, and what it makes of each file it finds.
pub(crate) struct Walk<'a, T> {
    pub(crate) table: &'a DirTable,
    /// A directory whose name is in it is left out.
    pub(crate) excluded_dir_names: &'a [&'a str],
    /// Which files it finds, by the directory they are read from and their
    /// name.
    pub(crate) wanted: &'a (dyn Fn(&Path, &std::ffi::OsStr) -> bool + Sync),
    /// Which directories below the one it starts from it does not go into,
    /// by their spelling.
    pub(crate) skip_dir: Option<&'a (dyn Fn(&Path) -> bool + Sync)>,
    /// What each file found comes with, made by the task that lists its
    /// directory, from the file's spelling.
    pub(crate) make: &'a (dyn Fn(&Path) -> T + Sync),
}

/// A file a walk found: its spelling, the hash of that spelling in a
/// `FileTable`, and what the walk made of it.
pub(crate) type WalkedFile<T> = (PathBuf, u64, T);

/// One entry of a directory, as its walk settles it.
enum Found<T> {
    File(WalkedFile<T>),
    Files(Vec<WalkedFile<T>>),
}

impl<T: Send> Walk<'_, T> {
    /// The files found under `disk`, or `disk` itself when it is one, with
    /// what `make` made of each, spelled as `spelled` is for `disk` and
    /// each name below it joined to it, and the hash of each spelling,
    /// built as the walk goes down. They come in the order paths
    /// compare in: each directory's entries are by name before the walk
    /// goes into them, so a name is before the longer ones it begins, and
    /// what is in a directory is before what comes after the directory's
    /// name. Each directory is a task of the pool the caller runs in, and
    /// its subdirectories' results join in its entries' order.
    pub(crate) fn files(&self, disk: &Path, spelled: &Path) -> Vec<WalkedFile<T>> {
        let hasher = file_table::PathHasher::of(spelled);
        if disk.is_dir() {
            self.directory(disk, spelled, &hasher)
        } else if (self.wanted)(
            disk.parent().unwrap_or(Path::new("")),
            disk.file_name().unwrap_or_default(),
        ) {
            vec![(spelled.to_path_buf(), hasher.finish(), (self.make)(spelled))]
        } else {
            Vec::new()
        }
    }

    fn directory(
        &self,
        disk: &Path,
        spelled: &Path,
        hasher: &file_table::PathHasher,
    ) -> Vec<WalkedFile<T>> {
        if disk
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| self.excluded_dir_names.contains(&n))
        {
            return Vec::new();
        }
        let Some(listing) = self.table.list(disk) else {
            return Vec::new();
        };
        let found: Vec<Found<T>> = listing
            .entries()
            .par_iter()
            .filter_map(|entry| {
                // Only a link needs a `stat` to tell what it leads to.
                let is_dir = match entry.kind {
                    EntryKind::Dir => true,
                    EntryKind::File => false,
                    EntryKind::Link => disk.join(&entry.name).is_dir(),
                };
                let spelled = spelled.join(&entry.name);
                let hasher = hasher.child(&entry.name);
                if is_dir {
                    if self.skip_dir.is_some_and(|skip| skip(&spelled)) {
                        return None;
                    }
                    Some(Found::Files(self.directory(
                        &disk.join(&entry.name),
                        &spelled,
                        &hasher,
                    )))
                } else if (self.wanted)(disk, &entry.name) {
                    let made = (self.make)(&spelled);
                    Some(Found::File((spelled, hasher.finish(), made)))
                } else {
                    None
                }
            })
            .collect();
        let mut files = Vec::new();
        for found in found {
            match found {
                Found::File(file) => files.push(file),
                Found::Files(mut inner) => files.append(&mut inner),
            }
        }
        files
    }
}

/// Directory names a workspace-wide scan skips by default when no real
/// config (1.1's own `phpstan-rs.toml` `paths`) says otherwise - a
/// hardcoded floor, not a substitute for real exclusion config. Matches
/// exactly one path component, not a prefix (`vendor-utils/` is
/// unaffected).
pub const DEFAULT_SCAN_EXCLUDED_DIR_NAMES: &[&str] = &["vendor", ".git", "node_modules"];

/// `true` if any component of `path` is named exactly one of
/// `excluded_dir_names` - a pure name check, no filesystem access. Used
/// both to prune a walk and by `FileRoles::classify` for a project with no
/// analysed root.
pub fn path_under_excluded_dir(path: &Path, excluded_dir_names: &[&str]) -> bool {
    path.components().any(|c| {
        c.as_os_str()
            .to_str()
            .is_some_and(|s| excluded_dir_names.contains(&s))
    })
}

/// `path`, spelled from `working_dir`, as an absolute path: `working_dir`
/// swapped for `absolute_root`, then `.` and `..` resolved lexically.
/// Without an absolute root, only the latter.
pub(crate) fn absolute_spelling(
    absolute_root: Option<&Path>,
    working_dir: Option<&Path>,
    path: &Path,
) -> PathBuf {
    let joined = match (absolute_root, working_dir) {
        (Some(absolute_root), Some(working_dir)) => match path.strip_prefix(working_dir) {
            Ok(rest) => absolute_root.join(rest),
            Err(_) => absolute_root.join(path),
        },
        (Some(absolute_root), None) => absolute_root.join(path),
        (None, _) => path.to_path_buf(),
    };
    composer::manifest::lexically_normalized(&joined)
}

/// Where a file is among the files a project holds.
struct Placed<'a> {
    id: FileId,
    path: &'a Path,
    analyzed: bool,
    load_order: u64,
}

/// What `Project::unindex_file` removed.
struct UnindexedFile {
    analyzed: bool,
    load_order: u64,
    functions: Vec<Name>,
    classes: Vec<Name>,
    constants: Vec<Name>,
    built_decls: Vec<BuiltDecl>,
    declaration_ids: Vec<(Option<u32>, Option<u32>)>,
}

/// Whether a file declaring `functions` and `classes` and referencing
/// `references` names anything in `PHPStan\`.
fn mentions_phpstan<'a>(
    functions: &[Name],
    classes: &[Name],
    references: impl IntoIterator<Item = &'a deps::RefKey>,
) -> bool {
    let is_phpstan = |name: &Name| name.lower().starts_with("phpstan\\");
    references.into_iter().any(|r| is_phpstan(r.name()))
        || functions.iter().chain(classes).any(is_phpstan)
}

/// Names a file declares, by kind.
#[derive(Default)]
struct DeclaredNames {
    functions: Vec<Name>,
    classes: Vec<Name>,
    constants: Vec<Name>,
}

/// The function and class names one file's parse tree declares - exactly
/// what a registry entry needs to be identified by to be removed again
/// later. Enums share the registry's `classes` map (see
/// `Registry::index`), so they're collected into the same list here.
/// Walks the declarations `registry::build_declarations` builds entries
/// from, so an incremental update's removal step finds every key the
/// registry holds.
pub(crate) fn declared_names(stmts: &[StmtNode]) -> (Vec<Name>, Vec<Name>) {
    let mut functions = Vec::new();
    let mut classes = Vec::new();
    syntax::declarations::for_each_declaration(stmts, |declaration, _, _| {
        match declaration.declared {
            Declared::Function(_) => functions.push(Name::from(declaration.name())),
            Declared::Class(_) => classes.push(Name::from(declaration.name())),
            Declared::Constant(_) | Declared::Define { .. } => {}
        }
    });
    (functions, classes)
}

/// A sorted reverse-index posting list. Most names are referenced by one
/// file, so keep that common case inline and allocate only for shared refs.
enum FileIdPostingList {
    One(FileId),
    Many(Vec<FileId>),
}

const REFERENCE_INDEX_SHARDS: usize = 32;

/// The reverse-reference map split by the hash of its own `RefKey`. A
/// load can prepare each file's shard buckets concurrently and merge those
/// buckets independently; incremental edits route directly to one shard.
struct ReferenceIndex {
    shards: Vec<FxHashMap<deps::RefKey, FileIdPostingList>>,
}

impl Default for ReferenceIndex {
    fn default() -> Self {
        Self {
            shards: (0..REFERENCE_INDEX_SHARDS)
                .map(|_| FxHashMap::default())
                .collect(),
        }
    }
}

impl ReferenceIndex {
    fn shard(key: &deps::RefKey) -> usize {
        let mut hasher = rustc_hash::FxHasher::default();
        key.hash(&mut hasher);
        (hasher.finish() >> (u64::BITS - 5)) as usize
    }

    fn get(&self, key: &deps::RefKey) -> Option<&FileIdPostingList> {
        self.shards[Self::shard(key)].get(key)
    }

    fn contains_key(&self, key: &deps::RefKey) -> bool {
        self.get(key).is_some()
    }

    fn insert(&mut self, key: deps::RefKey, file: FileId) {
        let shard = &mut self.shards[Self::shard(&key)];
        match shard.get_mut(&key) {
            Some(files) => files.insert(file),
            None => {
                shard.insert(key, FileIdPostingList::One(file));
            }
        }
    }

    fn remove(&mut self, key: &deps::RefKey, file: FileId) {
        let shard = &mut self.shards[Self::shard(key)];
        if shard.get_mut(key).is_some_and(|files| files.remove(file)) {
            shard.remove(key);
        }
    }

    fn from_file_refs(files: Vec<(FileId, Vec<deps::RefKey>)>) -> Self {
        let per_file: Vec<[Vec<(deps::RefKey, FileId)>; REFERENCE_INDEX_SHARDS]> = files
            .into_par_iter()
            .map(|(file, references)| {
                let mut buckets = std::array::from_fn(|_| Vec::new());
                for key in references {
                    buckets[Self::shard(&key)].push((key, file));
                }
                buckets
            })
            .collect();
        let mut buckets: [Vec<(deps::RefKey, FileId)>; REFERENCE_INDEX_SHARDS] =
            std::array::from_fn(|_| Vec::new());
        for mut file_buckets in per_file {
            for (bucket, file_bucket) in buckets.iter_mut().zip(&mut file_buckets) {
                bucket.append(file_bucket);
            }
        }
        let shards: Vec<_> = buckets
            .into_par_iter()
            .map(|entries| {
                let mut shard: FxHashMap<deps::RefKey, FileIdPostingList> = FxHashMap::default();
                shard.reserve(entries.len());
                for (key, file) in entries {
                    match shard.get_mut(&key) {
                        Some(files) => files.insert(file),
                        None => {
                            shard.insert(key, FileIdPostingList::One(file));
                        }
                    }
                }
                shard
            })
            .collect();
        Self { shards }
    }
}

impl syntax::canonical::Canonical for ReferenceIndex {
    fn canonical(&self, p: &mut syntax::canonical::Printer) {
        p.node("ReferenceIndex", &[("shards", &self.shards)]);
    }
}

impl FileIdPostingList {
    fn insert(&mut self, file: FileId) {
        match self {
            Self::One(existing) if *existing == file => {}
            Self::One(existing) => {
                let mut files = vec![*existing, file];
                files.sort_unstable();
                *self = Self::Many(files);
            }
            Self::Many(files) => match files.binary_search(&file) {
                Ok(_) => {}
                Err(at) => files.insert(at, file),
            },
        }
    }

    /// Removes `file`, returning true when the posting list is now empty.
    fn remove(&mut self, file: FileId) -> bool {
        let (empty, only) = match self {
            Self::One(existing) => return *existing == file,
            Self::Many(files) => {
                if let Ok(at) = files.binary_search(&file) {
                    files.remove(at);
                }
                (files.is_empty(), (files.len() == 1).then(|| files[0]))
            }
        };
        if empty {
            true
        } else {
            if let Some(only) = only {
                *self = Self::One(only);
            }
            false
        }
    }

    fn iter(&self) -> FileIdPostingIter<'_> {
        match self {
            Self::One(file) => FileIdPostingIter::One(Some(file)),
            Self::Many(files) => FileIdPostingIter::Many(files.iter()),
        }
    }
}

enum FileIdPostingIter<'a> {
    One(Option<&'a FileId>),
    Many(std::slice::Iter<'a, FileId>),
}

impl<'a> Iterator for FileIdPostingIter<'a> {
    type Item = &'a FileId;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::One(file) => file.take(),
            Self::Many(files) => files.next(),
        }
    }
}

impl<'a> IntoIterator for &'a FileIdPostingList {
    type Item = &'a FileId;
    type IntoIter = FileIdPostingIter<'a>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl syntax::canonical::Canonical for FileIdPostingList {
    fn canonical(&self, p: &mut syntax::canonical::Printer) {
        let files: Vec<_> = self.iter().copied().collect();
        p.node("FileIdPostingList", &[("files", &files)]);
    }
}

fn register_references(dependents: &mut ReferenceIndex, file: FileId, references: &[deps::RefKey]) {
    for name in references {
        dependents.insert(name.clone(), file);
    }
}

fn unregister_references(
    dependents: &mut ReferenceIndex,
    file: FileId,
    references: &[deps::RefKey],
) {
    for name in references {
        dependents.remove(name, file);
    }
}

fn sorted_ref_keys(keys: FxHashSet<deps::RefKey>) -> Box<[deps::RefKey]> {
    let mut keys: Vec<_> = keys.into_iter().collect();
    syntax::references::sort_ref_keys(&mut keys);
    keys.into_boxed_slice()
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod posting_list_tests {
    use super::*;

    #[test]
    fn reverse_posting_lists_keep_one_inline_and_many_sorted() {
        let first = FileId::new(4);
        let second = FileId::new(2);
        let mut files = FileIdPostingList::One(first);
        files.insert(second);
        files.insert(first);
        assert_eq!(files.iter().copied().collect::<Vec<_>>(), [second, first]);
        assert!(!files.remove(second));
        assert_eq!(files.iter().copied().collect::<Vec<_>>(), [first]);
        assert!(files.remove(first));
    }

    #[test]
    fn reference_keys_have_a_stable_equality_compatible_order() {
        let mut keys = vec![
            deps::RefKey::function(&Name::from("zeta")),
            deps::RefKey::class(&Name::from("Zulu")),
            deps::RefKey::class(&Name::from("alpha")),
            deps::RefKey::class(&Name::from("ALPHA")),
        ];
        keys.pop(); // Simulate the parse-time set's equality-based deduplication.
        syntax::references::sort_ref_keys(&mut keys);
        assert!(syntax::references::ref_keys_are_sorted(&keys));
        assert_eq!(keys[0].name().as_str(), "alpha");
        assert_eq!(keys[1].name().as_str(), "Zulu");
        assert!(matches!(keys[2], deps::RefKey::Function(_)));
    }

    #[test]
    fn parallel_reference_index_build_and_edits_keep_sorted_file_postings() {
        let first = FileId::new(1);
        let second = FileId::new(7);
        let class = deps::RefKey::class(&Name::from("Demo"));
        let folded = deps::RefKey::class(&Name::from("dEMO"));
        let mut index = ReferenceIndex::from_file_refs(vec![
            (first, vec![class.clone()]),
            (second, vec![folded]),
        ]);
        assert_eq!(
            index
                .get(&class)
                .unwrap()
                .iter()
                .copied()
                .collect::<Vec<_>>(),
            [first, second]
        );
        let new_file = FileId::new(9);
        index.insert(class.clone(), new_file);
        assert_eq!(
            index
                .get(&class)
                .unwrap()
                .iter()
                .copied()
                .collect::<Vec<_>>(),
            [first, second, new_file]
        );
        index.remove(&class, second);
        assert_eq!(
            index
                .get(&class)
                .unwrap()
                .iter()
                .copied()
                .collect::<Vec<_>>(),
            [first, new_file]
        );
    }
}
