//! Checks of the analysed code against the root package's
//! `composer.json`:
//!
//! - which installed package every referenced symbol comes from:
//!   `undeclared.composerDependency` (not required at all),
//!   `misplaced.composerDependency` (only in `require-dev`, used from
//!   production-autoloaded code), `devOnly.composerDependency` (a
//!   `require` package only code autoload-dev loads uses), and
//!   `unused.composerDependency` (a `require`/`require-dev` entry nothing
//!   references, a package run through its binaries or registered with
//!   a framework's auto-discovery exempt);
//! - `misplaced.autoloadDevSymbol`: production code using a class only
//!   `autoload-dev` can load;
//! - `psr4.pathMismatch`/`psr4.caseMismatch`: a class declared where its
//!   PSR-4 prefix doesn't map it;
//! - `provide.unimplemented`: a provided PSR virtual package with no
//!   implementing class;
//! - `ext.notRequired`: a built-in from an optional extension with no
//!   `ext-*` requirement or required polyfill;
//! - `composerScript.notFound`/`composerScript.notPublicStatic`: a
//!   `Class::method` script that Composer can't call;
//! - `composer.platformPhpInconsistentWithRequire`: `config.platform.php`
//!   outside the `require.php` constraint.

use rustc_hash::{FxHashMap, FxHashSet};
use std::path::{Path, PathBuf};

use crate::registry::stubs;
use composer::PackageOwner;
use composer::constraint;
use composer::manifest::{ComposerManifest, InstalledPackage, path_starts_with};
use syntax::ast::*;
use syntax::declarations::Declared;
use syntax::span::{LineCol, LineIndex, Range, Span};
use syntax::visit::{Visitor, walk_expr, walk_stmts};

use super::use_sites::{RefTarget, index_class_fn_refs_without_source};
use super::{FileState, Project};
use diagnostics::{Diagnostic, DiagnosticBuilder};

/// PSR virtual packages and the interfaces a provider must implement.
const PSR_VIRTUAL_PACKAGES: &[(&str, &[&str])] = &[
    ("psr/log-implementation", &["Psr\\Log\\LoggerInterface"]),
    (
        "psr/container-implementation",
        &["Psr\\Container\\ContainerInterface"],
    ),
    (
        "psr/cache-implementation",
        &["Psr\\Cache\\CacheItemPoolInterface"],
    ),
    (
        "psr/simple-cache-implementation",
        &["Psr\\SimpleCache\\CacheInterface"],
    ),
    (
        "psr/event-dispatcher-implementation",
        &["Psr\\EventDispatcher\\EventDispatcherInterface"],
    ),
    (
        "psr/http-client-implementation",
        &["Psr\\Http\\Client\\ClientInterface"],
    ),
    ("psr/link-implementation", &["Psr\\Link\\LinkInterface"]),
    ("psr/clock-implementation", &["Psr\\Clock\\ClockInterface"]),
    (
        "psr/http-message-implementation",
        &[
            "Psr\\Http\\Message\\RequestInterface",
            "Psr\\Http\\Message\\ResponseInterface",
            "Psr\\Http\\Message\\ServerRequestInterface",
            "Psr\\Http\\Message\\StreamInterface",
            "Psr\\Http\\Message\\UploadedFileInterface",
            "Psr\\Http\\Message\\UriInterface",
        ],
    ),
    (
        "psr/http-factory-implementation",
        &[
            "Psr\\Http\\Message\\RequestFactoryInterface",
            "Psr\\Http\\Message\\ResponseFactoryInterface",
            "Psr\\Http\\Message\\ServerRequestFactoryInterface",
            "Psr\\Http\\Message\\StreamFactoryInterface",
            "Psr\\Http\\Message\\UploadedFileFactoryInterface",
            "Psr\\Http\\Message\\UriFactoryInterface",
        ],
    ),
];

/// Package types that contribute no symbols to code by design.
const SYMBOL_LESS_PACKAGE_TYPES: &[&str] = &[
    "metapackage",
    "composer-plugin",
    "composer-installer",
    "phpstan-extension",
    "project",
];

/// The Composer platform package (`ext-<name>`) a phpstorm-stubs
/// directory documents, `None` for what can't be required: core
/// functionality always compiled in, a SAPI, or a non-extension
/// directory.
pub(crate) fn composer_extension_name(stub_dir: &str) -> Option<String> {
    let lower = stub_dir.to_ascii_lowercase();
    let not_requirable = [
        "core",
        "standard",
        "spl",
        "reflection",
        "date",
        "hash",
        "json",
        "pcre",
        "random",
        "meta",
        "superglobals",
        "apache",
        "fpm",
        "frankenphp",
        "litespeed",
        "phpdbg",
        "zend",
        "zendcache",
        "zenddebugger",
        "zendutils",
    ];
    if not_requirable.contains(&lower.as_str()) {
        return None;
    }
    let name = match lower.as_str() {
        "zend opcache" => "zend-opcache".to_string(),
        other => other.trim_end_matches("_v2").to_string(),
    };
    Some(name)
}

fn is_platform_package(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    !lower.contains('/')
        || lower == "php"
        || lower.starts_with("ext-")
        || lower.starts_with("lib-")
        || lower.starts_with("composer-")
}

/// The empty range at the start of `line` of a file that is not PHP.
fn at_line(line: u32) -> Range {
    let at = LineCol { line, col: 1 };
    Range { start: at, end: at }
}

fn diagnostic(
    message: String,
    span: Span,
    identifier: &'static str,
    index: &LineIndex,
) -> Diagnostic {
    DiagnosticBuilder::new(message, span, 0, identifier).build(index)
}

fn manifest_diagnostic(message: String, range: Range, identifier: &'static str) -> Diagnostic {
    DiagnosticBuilder::new(message, Span::default(), 0, identifier).build_at(range)
}

/// What an analysed file's code mentions besides resolved references:
/// class-name-shaped strings, and the extensions/functions/classes it
/// probes for before using them.
#[derive(Default)]
struct FileFacts {
    strings: Vec<String>,
    probed_extensions: FxHashSet<String>,
    probed_symbols: FxHashSet<String>,
}

fn collect_facts_in_stmts(stmts: &[Stmt], facts: &mut FileFacts) {
    walk_stmts(&mut Facts(facts), stmts);
}

/// Every expression in the file: closures, declarations, parameter
/// defaults, attributes, property hooks and enum case values included.
struct Facts<'f>(&'f mut FileFacts);

impl<'ast> Visitor<'ast> for Facts<'_> {
    fn visit_expr(&mut self, e: &'ast Expr) {
        collect_facts_in_expr(e, self.0);
        walk_expr(self, e);
    }
}

/// `e`'s own facts, not its operands'.
fn collect_facts_in_expr(e: &Expr, facts: &mut FileFacts) {
    match &e.kind {
        ExprKind::Literal(Literal::String(s)) => facts.strings.push(s.to_string()),
        ExprKind::Call(callee, args) => {
            if let ExprKind::Name(name) = &callee.kind
                && let Some(ExprKind::Literal(Literal::String(arg))) =
                    args.first().map(|a| &a.value.kind)
            {
                match name.trim_start_matches('\\').to_ascii_lowercase().as_str() {
                    "extension_loaded" => {
                        facts
                            .probed_extensions
                            .insert(arg.to_str_lossy().to_ascii_lowercase());
                    }
                    "function_exists" | "class_exists" | "interface_exists" | "enum_exists"
                    | "is_callable" => {
                        facts.probed_symbols.insert(
                            arg.to_str_lossy()
                                .trim_start_matches('\\')
                                .to_ascii_lowercase(),
                        );
                    }
                    _ => {}
                }
            }
        }
        _ => {}
    }
}

fn last_segment(name: &str) -> &str {
    name.rsplit('\\').next().unwrap_or(name)
}

/// The expected file of `fqn` under PSR-4 `prefix` mapped to `dir`.
fn psr4_expected_path(dir: &Path, prefix: &str, fqn: &str) -> PathBuf {
    let rest = &fqn[prefix.len()..];
    let mut path = dir.to_path_buf();
    for segment in rest.split('\\') {
        path.push(segment);
    }
    path.set_extension("php");
    path
}

impl Project {
    /// Every composer-hygiene diagnostic for the analysed files of the
    /// composer root `root` (an index into `packages().manifests()`),
    /// keyed by the file each belongs to (`composer.json` itself for the
    /// manifest-level ones).
    pub fn check_composer_hygiene(&self, root: usize) -> FxHashMap<PathBuf, Vec<Diagnostic>> {
        let manifest = &self.packages.manifests()[root];
        let mut out: FxHashMap<PathBuf, Vec<Diagnostic>> = FxHashMap::default();
        // Every loaded file declaring each class and function, whichever
        // declaration the registry keeps (a built-in stub can win a name a
        // package also declares): the first such file by path.
        let mut declaring_files: Vec<(&PathBuf, &FileState)> = self.files.iter().collect();
        declaring_files.sort_by_key(|(path, _)| *path);
        let mut classes_by_lower: FxHashMap<String, (&str, &Path)> = FxHashMap::default();
        let mut functions_by_lower: FxHashMap<String, (&str, &Path)> = FxHashMap::default();
        for (path, file) in &declaring_files {
            for name in &file.classes {
                classes_by_lower
                    .entry(name.to_ascii_lowercase())
                    .or_insert((name.as_str(), path.as_path()));
            }
            for name in &file.functions {
                functions_by_lower
                    .entry(name.to_ascii_lowercase())
                    .or_insert((name.as_str(), path.as_path()));
            }
        }
        // The package installed under this root that `path`, a loaded
        // file, belongs to.
        let installed_owner = |path: &Path| -> Option<&InstalledPackage> {
            match self.files.get(path)?.owner? {
                owner @ PackageOwner::Installed(i, _) if i == root => {
                    self.packages.installed(owner)
                }
                _ => None,
            }
        };

        let mut used_packages: FxHashSet<String> = FxHashSet::default();
        let mut used_outside_dev: FxHashSet<String> = FxHashSet::default();
        let mut analyzed: Vec<(&PathBuf, &super::FileState)> =
            self.files.iter().filter(|(_, f)| f.analyzed).collect();
        analyzed.sort_by(|a, b| a.0.cmp(b.0));

        analyzed.retain(|(_, file)| file.owner == Some(PackageOwner::Root(root)));
        for (path, file) in &analyzed {
            let absolute = self.absolute_path(path);
            let production = manifest.covers(&manifest.autoload, &absolute);
            let dev_only = !production && manifest.covers(&manifest.autoload_dev, &absolute);
            let used_before: FxHashSet<String> = used_packages.clone();
            let mut facts = FileFacts::default();
            collect_facts_in_stmts(file.ast(), &mut facts);
            // Class names written as strings (service wiring, config)
            // count as using their package.
            for s in &facts.strings {
                let trimmed = s.trim_start_matches('\\');
                if trimmed.contains('\\')
                    && let Some((_, declared_in)) =
                        classes_by_lower.get(&trimmed.to_ascii_lowercase())
                    && let Some(package) = installed_owner(declared_in)
                {
                    used_packages.insert(package.name.to_ascii_lowercase());
                }
            }
            let mut reported_packages: FxHashSet<String> = FxHashSet::default();
            let mut reported_extensions: FxHashSet<String> = FxHashSet::default();
            let mut reported_dev_symbols: FxHashSet<String> = FxHashSet::default();
            let diagnostics = out.entry((*path).clone()).or_default();
            for (span, target) in index_class_fn_refs_without_source(file.ast()) {
                let (kind, display, declared_in, builtin_extension) = match &target {
                    RefTarget::Class(fqn) => {
                        let lower = fqn.to_ascii_lowercase();
                        let declared = classes_by_lower.get(&lower);
                        (
                            "Class",
                            declared
                                .map(|(n, _)| n.to_string())
                                .unwrap_or_else(|| fqn.clone()),
                            declared.map(|(_, p)| *p),
                            stubs::builtin_class_extension(fqn),
                        )
                    }
                    // A qualified name reaches the function it names alone.
                    RefTarget::Function(fqn) => {
                        let lower = fqn.to_ascii_lowercase();
                        let declared = functions_by_lower.get(&lower);
                        let builtin = if lower.contains('\\') {
                            None
                        } else {
                            stubs::builtin_function_extension(&lower)
                        };
                        (
                            "Function",
                            declared
                                .map(|(n, _)| format!("{n}()"))
                                .unwrap_or_else(|| format!("{}()", last_segment(fqn))),
                            declared.map(|(_, p)| *p),
                            builtin,
                        )
                    }
                    RefTarget::UnqualifiedFunction(fqn) => {
                        let lower = fqn.to_ascii_lowercase();
                        let global = last_segment(&lower).to_string();
                        let namespaced_declaration =
                            lower.contains('\\') && functions_by_lower.contains_key(&lower);
                        let declared = functions_by_lower
                            .get(&lower)
                            .or_else(|| functions_by_lower.get(&global));
                        let builtin = if namespaced_declaration {
                            None
                        } else {
                            stubs::builtin_function_extension(&global)
                        };
                        (
                            "Function",
                            declared
                                .map(|(n, _)| format!("{n}()"))
                                .unwrap_or_else(|| format!("{}()", last_segment(fqn))),
                            declared.map(|(_, p)| *p),
                            builtin,
                        )
                    }
                    RefTarget::Member(..) => continue,
                };

                if let Some(stub_dir) = builtin_extension {
                    let polyfill = declared_in.and_then(&installed_owner);
                    if let Some(package) = polyfill {
                        used_packages.insert(package.name.to_ascii_lowercase());
                    }
                    let required_polyfill = polyfill.is_some_and(|p| {
                        manifest.requires(&p.name) || manifest.requires_dev(&p.name)
                    });
                    let Some(extension) = composer_extension_name(stub_dir) else {
                        continue;
                    };
                    let requirement = format!("ext-{extension}");
                    let probed = facts.probed_extensions.contains(&extension)
                        || facts
                            .probed_symbols
                            .contains(&display.trim_end_matches("()").to_ascii_lowercase());
                    if manifest.requires(&requirement)
                        || manifest.suggests(&requirement)
                        || manifest.requires_dev(&requirement) && !production
                        || probed
                        || required_polyfill
                        || !reported_extensions.insert(extension.clone())
                    {
                        continue;
                    }
                    diagnostics.push(diagnostic(
                        format!(
                            "{kind} {display} belongs to the {extension} extension, but composer.json doesn't require {requirement}."
                        ),
                        span,
                        "ext.notRequired",
&file.line_index,
));
                    continue;
                }

                let Some(declared_in) = declared_in else {
                    continue;
                };
                if self.files.get(declared_in).is_some_and(|f| f.analyzed) {
                    if production
                        && kind == "Class"
                        && manifest.covers(&manifest.autoload_dev, &self.absolute_path(declared_in))
                        && !manifest.covers(&manifest.autoload, &self.absolute_path(declared_in))
                        && reported_dev_symbols.insert(display.clone())
                    {
                        diagnostics.push(diagnostic(
                            format!(
                                "Class {display} is only autoloaded through autoload-dev, but this file is autoloaded in production; `composer install --no-dev` can't load it."
                            ),
                            span,
                            "misplaced.autoloadDevSymbol",
&file.line_index,
));
                    }
                    continue;
                }
                let Some(package) = installed_owner(declared_in) else {
                    continue;
                };
                let package_name = package.name.clone();
                used_packages.insert(package_name.to_ascii_lowercase());
                if facts
                    .probed_symbols
                    .contains(&display.trim_end_matches("()").to_ascii_lowercase())
                {
                    continue;
                }
                if manifest
                    .name
                    .as_deref()
                    .is_some_and(|n| n.eq_ignore_ascii_case(&package_name))
                {
                    continue;
                }
                let in_require = manifest.requires(&package_name);
                let in_require_dev = manifest.requires_dev(&package_name);
                if !in_require && !in_require_dev && !manifest.suggests(&package_name) {
                    if reported_packages.insert(package_name.clone()) {
                        diagnostics.push(diagnostic(
                            format!(
                                "{kind} {display} is declared in package {package_name}, which composer.json doesn't require; it's only installed as another package's dependency."
                            ),
                            span,
                            "undeclared.composerDependency",
&file.line_index,
));
                    }
                } else if !in_require
                    && production
                    && reported_packages.insert(package_name.clone())
                {
                    diagnostics.push(diagnostic(
                        format!(
                            "{kind} {display} is declared in package {package_name}, which composer.json only requires in require-dev, but this file is autoloaded in production."
                        ),
                        span,
                        "misplaced.composerDependency",
&file.line_index,
));
                }
            }
            self.check_psr4_locations(manifest, path, file, diagnostics);
            if !dev_only {
                used_outside_dev.extend(used_packages.difference(&used_before).cloned());
            }
        }

        let composer_json = manifest.path.clone();
        let mut manifest_diagnostics = Vec::new();
        let sections = [
            (&manifest.require, "require"),
            (&manifest.require_dev, "require-dev"),
        ];
        for (entries, section) in sections {
            for entry in entries {
                let name = entry.value.to_ascii_lowercase();
                if is_platform_package(&name) {
                    continue;
                }
                let Some(installed) = manifest.installed_package(&name) else {
                    continue;
                };
                if SYMBOL_LESS_PACKAGE_TYPES.contains(&installed.kind.as_str()) {
                    continue;
                }
                if !used_packages.contains(&name) {
                    if installed.has_bin || installed.auto_discovered {
                        continue;
                    }
                    manifest_diagnostics.push(manifest_diagnostic(
                        format!(
                            "Package {} is in {section}, but the analysed code uses none of its symbols.",
                            entry.value
                        ),
                        at_line(entry.line),
                        "unused.composerDependency",
                    ));
                } else if section == "require"
                    && !used_outside_dev.contains(&name)
                    && !manifest.autoload_dev.psr4.is_empty()
                {
                    manifest_diagnostics.push(manifest_diagnostic(
                        format!(
                            "Package {} is in require, but only code autoloaded through autoload-dev uses it; it belongs in require-dev.",
                            entry.value
                        ),
                        at_line(entry.line),
                        "devOnly.composerDependency",
                    ));
                }
            }
        }
        self.check_provided_implementations(manifest, &mut manifest_diagnostics);
        self.check_composer_scripts(manifest, &mut manifest_diagnostics);
        check_platform_php(manifest, &mut manifest_diagnostics);
        if !manifest_diagnostics.is_empty() {
            out.entry(composer_json)
                .or_default()
                .extend(manifest_diagnostics);
        }
        out.retain(|_, diagnostics| !diagnostics.is_empty());
        for diagnostics in out.values_mut() {
            diagnostics.sort_by_key(|d| (d.span.start.line, d.span.start.col));
        }
        out
    }

    fn check_psr4_locations(
        &self,
        manifest: &ComposerManifest,
        path: &Path,
        file: &super::FileState,
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        let mut declared: Vec<(String, Span)> = Vec::new();
        collect_declared_classes(file.ast(), &mut declared);
        if declared.is_empty() {
            return;
        }
        // Compared against the manifest's paths resolved; reported as the
        // file's own spelling.
        let shown = path;
        let path = self.absolute_path(path);
        let path = path.as_path();
        let sections = [&manifest.autoload, &manifest.autoload_dev];
        if sections.iter().any(|s| manifest.excludes(s, path)) {
            return;
        }
        if sections.iter().any(|s| {
            s.classmap
                .iter()
                .chain(&s.files)
                .any(|dir| path_starts_with(path, &manifest.absolute(dir)))
        }) {
            return;
        }
        let actual = path.to_string_lossy();
        for (fqn, span) in declared {
            let candidates: Vec<(&String, &PathBuf)> = sections
                .iter()
                .flat_map(|s| s.psr4.iter())
                .filter(|(prefix, _)| prefix.is_empty() || fqn.starts_with(prefix.as_str()))
                .flat_map(|(prefix, dirs)| dirs.iter().map(move |d| (prefix, d)))
                .collect();
            let longest = candidates.iter().map(|(p, _)| p.len()).max();
            let Some(longest) = longest else {
                // No prefix covers the class; flag it only when the file
                // sits inside a PSR-4 root, where it can't be autoloaded.
                if let Some((prefix, _)) =
                    sections
                        .iter()
                        .flat_map(|s| s.psr4.iter())
                        .find(|(_, dirs)| {
                            dirs.iter()
                                .any(|d| path_starts_with(path, &manifest.absolute(d)))
                        })
                {
                    diagnostics.push(diagnostic(
                        format!(
                            "Class {fqn} is declared under the PSR-4 root of prefix {prefix}, but its namespace doesn't start with that prefix; the autoloader can't find it."
                        ),
                        span,
                        "psr4.pathMismatch",
&file.line_index,
));
                }
                continue;
            };
            let expected: Vec<(&String, PathBuf)> = candidates
                .into_iter()
                .filter(|(p, _)| p.len() == longest)
                .map(|(p, dir)| (p, psr4_expected_path(dir, p, &fqn)))
                .collect();
            if expected
                .iter()
                .any(|(_, e)| manifest.absolute(e).to_string_lossy() == actual)
            {
                continue;
            }
            let (prefix, first) = &expected[0];
            if let Some((_, case_variant)) = expected.iter().find(|(_, e)| {
                manifest
                    .absolute(e)
                    .to_string_lossy()
                    .eq_ignore_ascii_case(&actual)
            }) {
                diagnostics.push(diagnostic(
                    format!(
                        "Class {fqn} is declared in {}, whose letter case differs from {} that PSR-4 prefix {prefix} maps it to; a case-sensitive filesystem won't find it.",
                        shown.display(),
                        case_variant.display()
                    ),
                    span,
                    "psr4.caseMismatch",
&file.line_index,
));
                continue;
            }
            diagnostics.push(diagnostic(
                format!(
                    "Class {fqn} is declared in {}, but PSR-4 prefix {prefix} maps it to {}.",
                    shown.display(),
                    first.display()
                ),
                span,
                "psr4.pathMismatch",
                &file.line_index,
            ));
        }
    }

    fn check_provided_implementations(
        &self,
        manifest: &ComposerManifest,
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        let first_party: Vec<&str> = self
            .registry
            .classes_by_id()
            .filter(|(_, _, info)| {
                info.provenance()
                    .file()
                    .and_then(|path| self.files.get(path))
                    .is_some_and(|f| f.analyzed)
            })
            .filter(|(_, _, info)| info.kind() == ClassKind::Class && !info.is_abstract())
            .map(|(_, name, _)| name.as_ref())
            .collect();
        for entry in &manifest.provide {
            let Some((_, interfaces)) = PSR_VIRTUAL_PACKAGES
                .iter()
                .find(|(name, _)| name.eq_ignore_ascii_case(&entry.value))
            else {
                continue;
            };
            let missing: Vec<&str> = interfaces
                .iter()
                .copied()
                .filter(|iface| {
                    !first_party
                        .iter()
                        .any(|class| self.registry.is_subclass_of(class, iface))
                })
                .collect();
            if missing.is_empty() {
                continue;
            }
            diagnostics.push(manifest_diagnostic(
                format!(
                    "composer.json provides {}, but no class implements {}.",
                    entry.value,
                    missing.join(", ")
                ),
                at_line(entry.line),
                "provide.unimplemented",
            ));
        }
    }

    fn check_composer_scripts(
        &self,
        manifest: &ComposerManifest,
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        for entry in &manifest.scripts {
            let (script, command) = &entry.value;
            let Some((class, method)) = command.split_once("::") else {
                continue;
            };
            let class = class.trim_start_matches('\\');
            let is_identifier = |s: &str| {
                !s.is_empty()
                    && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                    && !s.starts_with(|c: char| c.is_ascii_digit())
            };
            if !class.split('\\').all(is_identifier) || !is_identifier(method) {
                continue;
            }
            let span = at_line(entry.line);
            let Some((canonical, _)) = self.registry.lookup_class(class) else {
                diagnostics.push(manifest_diagnostic(
                    format!(
                        "Composer script {script} calls {command}, but class {class} doesn't exist."
                    ),
                    span,
                    "composerScript.notFound",
                ));
                continue;
            };
            let canonical = canonical.to_string();
            let found = self
                .registry
                .lookup_method_with_owner(&canonical, method)
                .or_else(|| {
                    self.registry
                        .lookup_method_with_owner(&canonical, &method.to_ascii_lowercase())
                });
            let Some((owner, sig)) = found else {
                if self
                    .registry
                    .lookup_method_with_owner(&canonical, "__callStatic")
                    .or_else(|| {
                        self.registry
                            .lookup_method_with_owner(&canonical, "__callstatic")
                    })
                    .is_some()
                {
                    continue;
                }
                diagnostics.push(manifest_diagnostic(
                    format!("Composer script {script} calls {command}, but method {canonical}::{method}() doesn't exist."),
                    span,
                    "composerScript.notFound",
                ));
                continue;
            };
            let is_static = self.registry.method_is_static(owner.as_ref(), method);
            let is_public = sig.visibility() == Visibility::Public;
            if is_static && is_public {
                continue;
            }
            let problem = match (is_public, is_static) {
                (false, false) => "neither public nor static",
                (false, true) => "not public",
                _ => "not static",
            };
            diagnostics.push(manifest_diagnostic(
                format!(
                    "Composer script {script} calls {command}, but {owner}::{method}() is {problem}; Composer calls it statically from outside the class."
                ),
                span,
                "composerScript.notPublicStatic",
            ));
        }
    }
}

fn check_platform_php(manifest: &ComposerManifest, diagnostics: &mut Vec<Diagnostic>) {
    let Some(platform) = &manifest.platform_php else {
        return;
    };
    let Some(required) = manifest.constraints.get("php") else {
        return;
    };
    if constraint::satisfies(required, &platform.value) == Some(false) {
        diagnostics.push(manifest_diagnostic(
            format!(
                "config.platform.php {} doesn't satisfy require.php {required}; Composer resolves dependencies for a PHP version the package doesn't support.",
                platform.value
            ),
            at_line(platform.line),
            "composer.platformPhpInconsistentWithRequire",
        ));
    }
}

/// Every class-like `stmts` declares unconditionally, with its name's
/// span.
fn collect_declared_classes(stmts: &[Stmt], out: &mut Vec<(String, Span)>) {
    syntax::declarations::for_each_declaration(stmts, |declaration, _, _| {
        if let Declared::Class(c) = declaration.declared
            && !declaration.conditional
        {
            out.push((declaration.name(), c.name.span));
        }
    });
}
