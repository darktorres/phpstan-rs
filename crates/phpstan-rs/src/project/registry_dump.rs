//! `registry-dump`: what a load leaves, after the finishing passes and
//! before any check, printed canonically (`syntax::canonical`). It only
//! reads: lazy records are decoded, and what a pass defers is printed as
//! it is stored.
//!
//! The text has, in order:
//! - the file table, a map by path: each file's role, load order,
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

use std::io;
use std::path::{Path, PathBuf};

use syntax::canonical::{Canonical, Printer};

use crate::project::{FileState, Project};
use crate::registry::Registry;
use crate::registry::canonical::{CanonicalOver, CompareAt};

/// The dump of `project`, its registry over `base` (the base's name, and
/// the base) when it stands on one, compared with it as `compare` says.
pub(crate) fn registry_dump(
    project: &Project,
    base: Option<(&str, &Registry)>,
    compare: CompareAt,
) -> String {
    let mut p = Printer::default();
    print_registry_dump(project, base, compare, &mut p);
    p.finish()
}

/// `registry_dump`, written into `out` as it is printed.
pub(crate) fn write_registry_dump(
    project: &Project,
    base: Option<(&str, &Registry)>,
    compare: CompareAt,
    out: &mut dyn io::Write,
) -> io::Result<()> {
    let mut p = Printer::to(out);
    print_registry_dump(project, base, compare, &mut p);
    p.close()
}

fn print_registry_dump(
    project: &Project,
    base: Option<(&str, &Registry)>,
    compare: CompareAt,
    p: &mut Printer,
) {
    let Project {
        files,
        dependents,
        signature_dependents,
        registry,
        // The configuration, given; what it makes is in the tables.
        sql: _,
        pest_config: _,
        settings: _,
        roles: _,
        working_dir,
        absolute_root,
        packages,
        retain_source,
        keeps_as_built,
        open,
        finished,
        phpstan_pinned,
        next_load_order,
        globals_stale,
        unchecked,
        duplicate_fn_sites,
        duplicate_class_sites,
        use_sites,
        member_index,
        // The trees last read again for checks.
        ast_cache: _,
        include_probes,
        escapes,
        named_argument_calls,
    } = project;
    crate::file_id::printing_paths(files.paths_by_id(), || {
        section(p, "files");
        let versions = registry.php_versions;
        let mut by_path: Vec<(&PathBuf, &FileState)> = files.iter().collect();
        by_path.sort_by_key(|(path, _)| *path);
        p.map_in_order(by_path.into_iter().map(|(path, state)| {
            (
                path,
                FileEntry {
                    project,
                    path,
                    state,
                    versions,
                    packages,
                },
            )
        }));
        section(p, "dependents");
        dependents.canonical(p);
        section(p, "signature dependents");
        signature_dependents.canonical(p);
        section(p, "use sites");
        use_sites.canonical(p);
        section(p, "duplicate function sites");
        duplicate_fn_sites.canonical(p);
        section(p, "duplicate class sites");
        duplicate_class_sites.canonical(p);
        section(p, "include probes");
        include_probes
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .canonical(p);
        match base {
            Some((name, base)) => {
                section(p, &format!("registry over {name}"));
                registry.canonical_over(base, compare, p);
            }
            None => {
                section(p, "registry");
                registry.canonical(p);
            }
        }
        section(p, "project");
        p.node(
            "Project",
            &[
                ("working_dir", working_dir),
                ("absolute_root", absolute_root),
                ("retain_source", retain_source),
                ("keeps_as_built", keeps_as_built),
                ("open", open),
                ("finished", finished),
                ("phpstan_pinned", phpstan_pinned),
                ("next_load_order", next_load_order),
                ("globals_stale", globals_stale),
                ("unchecked", unchecked),
                ("member_index", member_index),
                ("escapes", escapes),
                ("named_argument_calls", named_argument_calls),
            ],
        );
        p.line();
    });
}

/// The file at `path`, which the load keeps as `state`: the file table's
/// entry for it.
struct FileEntry<'a> {
    project: &'a Project,
    path: &'a Path,
    state: &'a FileState,
    versions: Option<types::PhpVersions>,
    packages: &'a composer::PackageIndex,
}

impl Canonical for FileEntry<'_> {
    fn canonical(&self, p: &mut Printer) {
        let FileEntry {
            project,
            path,
            state,
            versions,
            packages,
        } = *self;
        write_file(project, path, state, versions, packages, p);
    }
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
        content_hash: _,
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
    print_stub_bases(bases, &mut p);
    p.finish()
}

/// `stub_bases_dump`, written into `out` as it is printed.
pub(crate) fn write_stub_bases_dump(
    bases: &[(&str, &Registry)],
    out: &mut dyn io::Write,
) -> io::Result<()> {
    let mut p = Printer::to(out);
    print_stub_bases(bases, &mut p);
    p.close()
}

fn print_stub_bases(bases: &[(&str, &Registry)], p: &mut Printer) {
    for (name, base) in bases {
        section(p, &format!("stub base {name}"));
        base.canonical(p);
    }
    p.line();
}
