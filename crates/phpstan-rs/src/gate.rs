//! Reading what the gate records back: the templates in scope where a
//! type was dumped or asserted, so a displayed template name reads as
//! that template rather than a class.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use rustc_hash::FxHashMap;
use syntax::ast::{ClassDecl, FunctionDecl, Stmt, StmtKind};
use syntax::span::LineCol;
use syntax::visit::{self, Visitor};
use types::Type;

use crate::Registry;
use crate::project::session::Session;
use crate::registry::stub_set::Stubs;

mod type_drift;
pub use type_drift::{Drift, Facet, Verdict, type_drift};

/// The templates in scope at a point, by name, as the checker binds them:
/// the enclosing class's, then the enclosing function's or method's own,
/// which shadow a class template of the same name.
#[derive(Debug, Clone, Default)]
pub struct TemplateScope {
    templates: FxHashMap<String, Type>,
}

impl FromIterator<(String, Type)> for TemplateScope {
    fn from_iter<I: IntoIterator<Item = (String, Type)>>(templates: I) -> TemplateScope {
        TemplateScope {
            templates: templates.into_iter().collect(),
        }
    }
}

impl TemplateScope {
    /// The template `name` names here, if any.
    pub fn resolve(&self, name: &str) -> Option<Type> {
        self.templates.get(name).cloned()
    }
}

/// The templates in scope at the byte offset `at` of a file whose
/// statements are `stmts`, with class templates from `registry`.
pub fn template_scope_at(registry: &Registry, stmts: &[Stmt], at: u32) -> TemplateScope {
    let mut finder = Enclosing {
        at,
        class: None,
        function: None,
    };
    visit::walk_stmts(&mut finder, stmts);
    let mut templates = FxHashMap::default();
    if let Some(class) = finder.class
        && let Some(info) = registry.classes.get(class.fq_name())
    {
        for (name, arg) in info
            .templates()
            .iter()
            .zip(info.own_template_args(class.fq_name()))
        {
            templates.insert(name.to_string(), arg);
        }
    }
    if let Some(function) = finder.function
        && let Some(doc) = function.doc_comment.as_deref()
    {
        let ctx = syntax::name_resolution::context_at(stmts, function.span.start);
        let own = crate::registry::build_template_params(
            &phpdoc::docblock::parse(doc),
            &function.name,
            &ctx,
        );
        for (name, param) in own {
            templates.insert(name, Type::template_param(param));
        }
    }
    TemplateScope { templates }
}

/// The innermost class and function or method declarations containing a
/// position.
struct Enclosing<'ast> {
    at: u32,
    class: Option<&'ast ClassDecl>,
    function: Option<&'ast FunctionDecl>,
}

impl Enclosing<'_> {
    fn contains(&self, start: u32, end: u32) -> bool {
        start <= self.at && self.at <= end
    }
}

impl<'ast> Visitor<'ast> for Enclosing<'ast> {
    fn visit_stmt(&mut self, stmt: &'ast Stmt) {
        if !self.contains(stmt.span.start, stmt.span.end) {
            return;
        }
        if let StmtKind::ClassDecl(class) = &stmt.kind {
            self.class = Some(class);
            self.function = None;
        }
        visit::walk_stmt(self, stmt);
    }

    fn visit_function_decl(&mut self, function: &'ast FunctionDecl) {
        if self.contains(function.span.start, function.span.end) {
            self.function = Some(function);
            visit::walk_function_decl(self, function);
        }
    }
}

/// A fixture's `X.toml`: the analysis config, and the harness's own keys,
/// which are removed before the config is checked against the schema.
pub struct FixtureConfig {
    pub config: config::ConfigFile,
    /// Cargo features the fixture needs (`requires_feature`).
    pub requires_feature: Vec<String>,
    /// Whether every runtime warning must line up with an expected
    /// diagnostic (`runtime_warnings = "exact"`).
    pub runtime_warnings_exact: bool,
    /// Libraries the fixture needs at runtime (`runtime_packages`).
    pub runtime_packages: Vec<String>,
}

impl FixtureConfig {
    /// The PHP version the fixture is analysed for, as written
    /// (`php_version = "8.0"`).
    pub fn php_version(&self) -> Option<String> {
        self.config
            .section::<crate::registry::settings::ReflectionSettings>()
            .ok()
            .and_then(|settings| settings.php_version)
    }
}

/// The fixture config at `path`, empty when there's none. An old key a
/// schema moves is an error: a fixture is written in the current layout.
pub fn fixture_config(path: &Path) -> Result<FixtureConfig, String> {
    if !path.is_file() {
        return Ok(FixtureConfig {
            config: config::ConfigFile::empty(),
            requires_feature: Vec::new(),
            runtime_warnings_exact: false,
            runtime_packages: Vec::new(),
        });
    }
    let schema = fixture_schema();
    let mut config = config::ConfigFile::load(path, schema)?;
    let strings = |value: Option<toml::Value>, key: &str| -> Result<Vec<String>, String> {
        let Some(value) = value else {
            return Ok(Vec::new());
        };
        value
            .as_array()
            .and_then(|list| {
                list.iter()
                    .map(|v| v.as_str().map(str::to_string))
                    .collect::<Option<Vec<_>>>()
            })
            .ok_or_else(|| format!("{}: {key}: expected a list of strings", path.display()))
    };
    let requires_feature = strings(config.remove("requires_feature"), "requires_feature")?;
    let runtime_packages = strings(config.remove("runtime_packages"), "runtime_packages")?;
    let runtime_warnings_exact = match config.remove("runtime_warnings") {
        None => false,
        Some(toml::Value::String(mode)) if mode == "exact" => true,
        Some(other) => {
            return Err(format!(
                "{}: runtime_warnings: expected \"exact\", got {other}",
                path.display()
            ));
        }
    };
    config.validate(schema)?;
    if let Some(warning) = config.warnings().first() {
        return Err(format!("{}: {warning}", path.display()));
    }
    Ok(FixtureConfig {
        config,
        requires_feature,
        runtime_warnings_exact,
        runtime_packages,
    })
}

/// The config schema every fixture is read against, built once.
fn fixture_schema() -> &'static config::Schema {
    static SCHEMA: std::sync::OnceLock<config::Schema> = std::sync::OnceLock::new();
    SCHEMA.get_or_init(crate::app::config_schema)
}

/// Whether the config at `path` sets `requires_feature`: a plain parse of
/// the file, without the schema, for a caller that reads the rest of a
/// fixture's config only when it does. A file that can't be read or
/// parsed sets nothing.
pub fn declares_required_feature(path: &Path) -> bool {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| toml::from_str::<toml::Table>(&text).ok())
        .is_some_and(|table| table.contains_key("requires_feature"))
}

/// A fixture's session and the directory its paths are relative to:
/// `stem.php`, or the directory `stem/`, analysed under its config.
pub fn load_fixture(
    stem: &Path,
    config: &FixtureConfig,
    stubs: Stubs,
) -> Result<(Session, PathBuf), String> {
    Session::for_fixture(stem, &config.config, stubs)
}

/// The drift from `old` to `new` with no project: classes are related by
/// name only and template names read as classes.
pub fn type_drift_structural(old: &str, new: &str) -> Drift {
    type_drift(old, new, &types::Relation::structural(), &mut |_, _, _| {
        TemplateScope::default()
    })
}

/// The drift from `old` to `new`, dumped from `project`, related with its
/// class hierarchy and each dump point's templates.
pub fn type_drift_in_project(project: &crate::Project, old: &str, new: &str) -> Drift {
    let registry = project.registry();
    let mut scopes = TemplateScopes::new(registry);
    type_drift(old, new, &registry.relation(), &mut |path, line, col| {
        scopes.at(Path::new(path), line, col)
    })
}

/// The drift between two fixture dumps (`types-fixtures`), whose lines
/// start `<fixture>: `: each fixture's bindings related within that
/// fixture's own project, loaded from `root` with its `X.toml`. A fixture
/// in one dump only was renamed or deleted; its bindings are added or
/// removed, unrelated.
pub fn type_drift_of_fixtures(
    root: &Path,
    old: &str,
    new: &str,
    stubs: Stubs,
) -> Result<Drift, String> {
    let by_fixture = |text: &str| {
        let mut split: std::collections::BTreeMap<String, String> = Default::default();
        for line in text.lines() {
            let Some((fixture, rest)) = line.split_once(": ") else {
                continue;
            };
            let part = split.entry(fixture.to_string()).or_default();
            part.push_str(rest);
            part.push('\n');
        }
        split
    };
    let (mut old, new) = (by_fixture(old), by_fixture(new));
    let mut drift = Drift::default();
    for (fixture, new_part) in new {
        let old_part = old.remove(&fixture).unwrap_or_default();
        let tag = |d: Drift| d.tagged(&fixture);
        if old_part.is_empty() {
            drift.merge(tag(type_drift_structural("", &new_part)).in_one_dump());
            continue;
        }
        let stem = root.join(&fixture);
        let config = fixture_config(&stem.with_extension("toml"))?;
        let (session, base) = load_fixture(&stem, &config, stubs.clone())?;
        let loaded = session.load()?;
        let registry = loaded.project.registry();
        let mut scopes = TemplateScopes::new(registry);
        let part = type_drift(
            &old_part,
            &new_part,
            &registry.relation(),
            &mut |path, line, col| scopes.at(&base.join(path), line, col),
        );
        drift.merge(tag(part));
    }
    for (fixture, old_part) in old {
        drift.merge(
            type_drift_structural(&old_part, "")
                .tagged(&fixture)
                .in_one_dump(),
        );
    }
    Ok(drift)
}

/// A file's statements and lines.
type ParsedScopeFile = (Vec<Stmt>, Arc<syntax::span::LineIndex>);

/// `template_scope_at` over files read and parsed once each.
pub struct TemplateScopes<'r> {
    registry: &'r Registry,
    files: FxHashMap<PathBuf, Option<ParsedScopeFile>>,
}

impl<'r> TemplateScopes<'r> {
    pub fn new(registry: &'r Registry) -> TemplateScopes<'r> {
        TemplateScopes {
            registry,
            files: FxHashMap::default(),
        }
    }

    /// The templates in scope at `line`:`col` of `path`; none when the
    /// file can't be read or parsed.
    pub fn at(&mut self, path: &Path, line: u32, col: u32) -> TemplateScope {
        let stmts = self.files.entry(path.to_path_buf()).or_insert_with(|| {
            let source = std::fs::read(path).ok()?;
            syntax::parse(&source)
                .ok()
                .map(|parsed| (parsed.stmts, parsed.line_index))
        });
        match stmts {
            Some((stmts, index)) => match index.offset(LineCol { line, col }) {
                Some(at) => template_scope_at(self.registry, stmts, at),
                None => TemplateScope::default(),
            },
            None => TemplateScope::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SOURCE: &str = "<?php
namespace App;
/** @template T of \\Countable */
class Box {
    /**
     * @template U of int
     * @template T
     */
    public function map(): void {
        $a = 1;
    }
    public function get(): void {
        $b = 1;
    }
}
function free(): void {
    $c = 1;
}
";

    fn scope_at(line: u32) -> TemplateScope {
        let parsed = syntax::parse(SOURCE.as_bytes()).unwrap();
        let registry = Registry::build(&parsed.stmts);
        let at = parsed
            .line_index
            .offset(LineCol { line, col: 9 })
            .expect("in the source");
        template_scope_at(&registry, &parsed.stmts, at)
    }

    fn bound(scope: &TemplateScope, name: &str) -> Option<String> {
        scope
            .resolve(name)
            .and_then(|t| t.template_bound().map(Type::to_string))
    }

    #[test]
    fn a_method_sees_its_class_templates_and_its_own() {
        let scope = scope_at(13);
        assert_eq!(bound(&scope, "T").as_deref(), Some("Countable"));
        assert_eq!(bound(&scope, "U"), None);
    }

    #[test]
    fn a_method_template_shadows_a_class_template_of_the_same_name() {
        let scope = scope_at(10);
        assert_eq!(bound(&scope, "U").as_deref(), Some("int"));
        assert_eq!(bound(&scope, "T").as_deref(), Some("mixed"));
    }

    #[test]
    fn outside_the_class_its_templates_are_not_in_scope() {
        let outside = scope_at(17);
        assert_eq!(outside.resolve("T"), None);
        let inside = scope_at(13);
        let read = |scope: &TemplateScope| {
            types::try_parse_displayed_with("list<T>", &|n| scope.resolve(n)).unwrap()
        };
        assert!(read(&inside).references_template());
        assert!(!read(&outside).references_template());
    }
}
