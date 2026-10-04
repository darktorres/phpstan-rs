//! `registry-dump`: what a load leaves, after the finishing passes and
//! before any check, printed canonically (`syntax::canonical`). It only
//! reads: lazy records are decoded, and what a pass defers is printed as
//! it is stored.
//!
//! The text has, in order:
//! - the file table, by path: each file's path, role, load order,
//!   absolute path, package and what the load keeps of it, and each of its
//!   declarations with its guard, decided on the configured PHP versions
//!   and on each end of their range;
//! - both reverse indexes, the use-site index and the duplicate-site
//!   tables;
//! - the registry, by name, over the stub base it stands on, which it
//!   names: each table, map and set as what the project adds, changes and
//!   drops of the base's (`CanonicalOver`), the rest whole;
//! - what the project records of the load.
//!
//! The stub bases print whole in a dump of their own (`stub_bases_dump`).
//!
//! A file's number prints as its path, so that another numbering prints
//! the same.

use std::path::{Path, PathBuf};

use syntax::canonical::{Canonical, Printer};

use crate::project::{FileState, Project};
use crate::registry::Registry;
use crate::registry::canonical::CanonicalOver;

/// The dump of `project`, its registry over `base` (the base's name, and
/// the base) when it stands on one.
pub(crate) fn registry_dump(project: &Project, base: Option<(&str, &Registry)>) -> String {
    let Project {
        files,
        dependents,
        signature_dependents,
        registry,
        // The configuration, given; what it makes is in the tables.
        sql: _,
        pest_config: _,
        settings: _,
        working_dir,
        absolute_root,
        packages,
        defer_inline_ignores,
        retain_source,
        built_once,
        finished,
        phpstan_pinned,
        next_load_order,
        globals_stale,
        unchecked,
        duplicate_fn_sites,
        duplicate_class_sites,
        use_sites,
        member_index_built,
        // The trees last read again for checks.
        ast_cache: _,
        include_probes,
        escaped_enum_cases,
        escaped_member_classes,
    } = project;
    crate::file_id::printing_paths(files.paths_by_id(), || {
        let mut p = Printer::default();
        section(&mut p, "files");
        let versions = registry.php_versions;
        let mut by_path: Vec<(&PathBuf, &FileState)> = files.iter().collect();
        by_path.sort_by_key(|(path, _)| *path);
        for (path, state) in by_path {
            p.line();
            write_file(project, path, state, versions, packages, &mut p);
        }
        section(&mut p, "dependents");
        dependents.canonical(&mut p);
        section(&mut p, "signature dependents");
        signature_dependents.canonical(&mut p);
        section(&mut p, "use sites");
        use_sites.canonical(&mut p);
        section(&mut p, "duplicate function sites");
        duplicate_fn_sites.canonical(&mut p);
        section(&mut p, "duplicate class sites");
        duplicate_class_sites.canonical(&mut p);
        section(&mut p, "include probes");
        include_probes
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .canonical(&mut p);
        match base {
            Some((name, base)) => {
                section(&mut p, &format!("registry over {name}"));
                registry.canonical_over(base, &mut p);
            }
            None => {
                section(&mut p, "registry");
                registry.canonical(&mut p);
            }
        }
        section(&mut p, "project");
        p.node(
            "Project",
            &[
                ("working_dir", working_dir),
                ("absolute_root", absolute_root),
                ("defer_inline_ignores", defer_inline_ignores),
                ("retain_source", retain_source),
                ("built_once", built_once),
                ("finished", finished),
                ("phpstan_pinned", phpstan_pinned),
                ("next_load_order", next_load_order),
                ("globals_stale", globals_stale),
                ("unchecked", unchecked),
                ("member_index_built", member_index_built),
                ("escaped_enum_cases", escaped_enum_cases),
                ("escaped_member_classes", escaped_member_classes),
            ],
        );
        p.line();
        p.finish()
    })
}

/// Writes the file at `path`, which the load keeps as `state`.
fn write_file(
    project: &Project,
    path: &Path,
    state: &FileState,
    versions: Option<types::PhpVersions>,
    packages: &composer::PackageIndex,
    p: &mut Printer,
) {
    let FileState {
        kept_ast,
        source_len,
        load_order,
        functions,
        classes,
        constants,
        mentions_phpstan,
        references,
        declaration_references,
        ignores,
        anonymous_classes,
        source,
        analyzed,
        owner,
        bodies,
        global_facts,
        built_decls,
        pest_facts,
        declared_classes,
        facts,
        // Printed as the positions it gives by the front end's dump.
        line_index: _,
    } = state;
    let role = if *analyzed { "analysed" } else { "context" };
    let package = owner.map(|owner| packages.name(owner).map(str::to_string));
    let guards: Vec<GuardedDecl<'_>> = built_decls
        .iter()
        .map(|decl| GuardedDecl {
            decl,
            on: decl.guard_on_range_and_ends(versions),
        })
        .collect();
    p.node(
        "File",
        &[
            ("path", &path),
            ("role", &role),
            ("load_order", load_order),
            ("absolute_path", &project.absolute_path(path)),
            ("owner", owner),
            ("package", &package),
            ("mentions_phpstan", mentions_phpstan),
            // The tree and the text, which the front end's dump prints; whether
            // they are kept.
            ("kept_ast", &kept_ast.is_some()),
            ("source", &source.is_some()),
            ("source_len", source_len),
            ("functions", functions),
            ("classes", classes),
            ("constants", constants),
            ("references", references),
            ("declaration_references", declaration_references),
            ("ignores", ignores),
            ("anonymous_classes", anonymous_classes),
            ("bodies", bodies),
            ("global_facts", global_facts),
            ("built_decls", &guards),
            ("pest_facts", pest_facts),
            ("declared_classes", declared_classes),
            ("facts", facts),
        ],
    );
}

/// A declaration, and whether it runs on the configured PHP versions and
/// on each end of their range.
struct GuardedDecl<'a> {
    decl: &'a crate::registry::BuiltDecl,
    on: (
        crate::registry::Guard,
        crate::registry::Guard,
        crate::registry::Guard,
    ),
}

impl Canonical for GuardedDecl<'_> {
    fn canonical(&self, p: &mut Printer) {
        let GuardedDecl { decl, on } = self;
        p.node("GuardedDecl", &[("decl", decl), ("on", on)]);
    }
}

/// Starts the section `name` on a line of its own.
fn section(p: &mut Printer, name: &str) {
    p.line();
    p.atom("== ");
    p.atom(name);
    p.line();
}

/// Each of `bases` whole, after its name: what a registry dump prints its
/// project over.
pub(crate) fn stub_bases_dump(bases: &[(&str, &Registry)]) -> String {
    let mut p = Printer::default();
    for (name, base) in bases {
        section(&mut p, &format!("stub base {name}"));
        base.canonical(&mut p);
    }
    p.line();
    p.finish()
}
