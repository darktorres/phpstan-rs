//! The edit replay: a fixed script of edits, over files drawn with a fixed
//! seed, applied to a loaded project the way the language server applies
//! them (`update_file`, `remove_file`), each file's diagnostics kept as the
//! server keeps them - a file's last check stands until an edit rechecks
//! it - with the registry dump and the diagnostics taken after each step,
//! then a fresh load of the edited tree, which must give the same. As
//! PHPStan's result-cache end-to-end tests do, it catches what an edit
//! leaves stale, which no full run sees.
//!
//! The script: save a file unchanged, delete a method, rename a class,
//! change a parameter's type, add a file declaring a class that is named
//! but declared nowhere, delete a file. The edits are made to the files on
//! disk too, so the tree must be a copy: the replay refuses a tree without
//! a `.phpstan-rs-replay` file at its root.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use diagnostics::Diagnostic;
use rayon::prelude::*;
use syntax::ast::{ClassDef, FunctionDecl, Param, Stmt, StmtKind};
use syntax::span::Span;
use syntax::visit::{Visitor, walk_class_def, walk_function_decl, walk_stmt};

use crate::project::Project;

/// The file at the root of a tree the replay may edit.
pub(crate) const MARKER: &str = ".phpstan-rs-replay";

/// Each analysed file's diagnostics, as the session holds them.
pub(crate) type HeldDiagnostics = BTreeMap<PathBuf, Vec<Diagnostic>>;

/// Every analysed file of `project` checked, on `pool`.
pub(crate) fn check_all(project: &Project, pool: &rayon::ThreadPool) -> HeldDiagnostics {
    let paths = project.analyzed_paths();
    pool.install(|| {
        paths
            .par_iter()
            .map(|path| (path.clone(), project.check_file(path)))
            .collect()
    })
}

/// `held`, one line for each diagnostic: the path and its canonical print.
pub(crate) fn print_held(held: &HeldDiagnostics) -> String {
    let mut text = String::new();
    for (path, diagnostics) in held {
        for diagnostic in diagnostics {
            text.push_str(&format!(
                "{}\t{}\n",
                path.display(),
                syntax::canonical::print_line(diagnostic)
            ));
        }
    }
    text
}

/// What finds, in a file's statements and text, the span a step edits
/// and what it writes there.
type Find = dyn Fn(&[Stmt], &str) -> Option<(Span, String)>;

/// One step of the script: what it did, and to which file.
pub(crate) struct Step {
    pub(crate) label: &'static str,
    pub(crate) description: String,
}

/// Applies the script's steps to `project` and the tree under `root`,
/// drawing its files with `seed`, and calls `after` with each step and the
/// diagnostics the session then holds.
pub(crate) fn replay(
    project: &mut Project,
    root: &Path,
    seed: u64,
    held: &mut HeldDiagnostics,
    mut after: impl FnMut(&Step, &Project, &HeldDiagnostics) -> Result<(), String>,
) -> Result<(), String> {
    let mut order = project.analyzed_paths();
    order.sort();
    let mut rng = Rng(seed);
    for i in (1..order.len()).rev() {
        order.swap(i, rng.below(i + 1));
    }
    let mut used: Vec<PathBuf> = Vec::new();
    let read = |path: &Path| -> Result<String, String> {
        let bytes =
            std::fs::read(root.join(path)).map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    };
    // The first file in the drawn order, not used by a step before, for
    // which `find` finds a span to edit, with its text.
    let pick = |used: &mut Vec<PathBuf>,
                find: &Find|
     -> Result<Option<(PathBuf, String, Span, String)>, String> {
        for path in &order {
            if used.contains(path) {
                continue;
            }
            let text = read(path)?;
            let Ok(parsed) = syntax::parse(text.as_bytes()) else {
                continue;
            };
            if let Some((span, replacement)) = find(&parsed.stmts, &text) {
                used.push(path.clone());
                return Ok(Some((path.clone(), text, span, replacement)));
            }
        }
        Ok(None)
    };

    let unchanged = pick(&mut used, &|_, _| Some((Span::new(0, 0), String::new())))?;
    let deleted_method = pick(&mut used, &|stmts, _| {
        let mut found = Finder::default();
        stmts.iter().for_each(|stmt| found.visit_stmt(stmt));
        found.method.map(|span| (span, String::new()))
    })?;
    let renamed_class = pick(&mut used, &|stmts, text| {
        let mut found = Finder::default();
        stmts.iter().for_each(|stmt| found.visit_stmt(stmt));
        found.class.map(|span| {
            let name = &text[span.start as usize..span.end as usize];
            (span, format!("{name}Replayed"))
        })
    })?;
    let changed_param = pick(&mut used, &|stmts, text| {
        let mut found = Finder::default();
        stmts.iter().for_each(|stmt| found.visit_stmt(stmt));
        found.param_type.map(|span| {
            let hint = &text[span.start as usize..span.end as usize];
            let other = if hint.eq_ignore_ascii_case("int") {
                "string"
            } else {
                "int"
            };
            (span, other.to_string())
        })
    })?;

    for (label, picked) in [
        ("save unchanged", unchanged),
        ("delete a method", deleted_method),
        ("rename a class", renamed_class),
        ("change a parameter type", changed_param),
    ] {
        let Some((path, text, span, replacement)) = picked else {
            after(
                &Step {
                    label,
                    description: "no file has what the step edits".to_string(),
                },
                project,
                held,
            )?;
            continue;
        };
        let mut edited = text.clone();
        edited.replace_range(span.start as usize..span.end as usize, &replacement);
        std::fs::write(root.join(&path), &edited)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        let rechecked = project
            .update_file(&path, &edited)
            .map_err(|e| format!("{}: {}", path.display(), e.message))?;
        hold(project, held, rechecked);
        let step = Step {
            label,
            description: format!(
                "{} {}..{}: {:?} -> {replacement:?}",
                path.display(),
                span.start,
                span.end,
                &text[span.start as usize..span.end as usize]
            ),
        };
        after(&step, project, held)?;
    }

    // A class some file names and none declares, declared in a new file
    // beside the first file naming one.
    let missing = order.iter().find_map(|path| {
        let state = project.files.get(path)?;
        let mut names: Vec<&syntax::name::Name> = state
            .references
            .iter()
            .filter_map(|key| match key {
                crate::deps::RefKey::Class(name) => Some(name),
                _ => None,
            })
            .filter(|name| {
                project.registry.lookup_class(name).is_none()
                    && name.split('\\').all(|part| {
                        part.chars()
                            .next()
                            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
                            && part.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                    })
                    && !types::is_reserved_type_word(name)
            })
            .collect();
        names.sort_by(|a, b| a.as_str().cmp(b.as_str()));
        names.first().map(|name| (path.clone(), name.to_string()))
    });
    match missing {
        Some((beside, class)) => {
            let path = beside.with_file_name("__replay_added.php");
            let (namespace, short) = match class.rsplit_once('\\') {
                Some((namespace, short)) => (format!("namespace {namespace};\n\n"), short),
                None => (String::new(), class.as_str()),
            };
            let text = format!("<?php\n{namespace}class {short}\n{{\n}}\n");
            std::fs::write(root.join(&path), &text)
                .map_err(|e| format!("{}: {e}", path.display()))?;
            let rechecked = project
                .update_file(&path, &text)
                .map_err(|e| format!("{}: {}", path.display(), e.message))?;
            hold(project, held, rechecked);
            let step = Step {
                label: "add a file declaring a class named nowhere declared",
                description: format!("{} declares {class}", path.display()),
            };
            after(&step, project, held)?;
        }
        None => after(
            &Step {
                label: "add a file declaring a class named nowhere declared",
                description: "no file names a class none declares".to_string(),
            },
            project,
            held,
        )?,
    }

    let removed = order.iter().find(|path| !used.contains(path)).cloned();
    match removed {
        Some(path) => {
            std::fs::remove_file(root.join(&path))
                .map_err(|e| format!("{}: {e}", path.display()))?;
            let rechecked = project.remove_file(&path);
            held.remove(&path);
            hold(project, held, rechecked);
            let step = Step {
                label: "delete a file",
                description: path.display().to_string(),
            };
            after(&step, project, held)
        }
        None => after(
            &Step {
                label: "delete a file",
                description: "no file left to delete".to_string(),
            },
            project,
            held,
        ),
    }
}

/// Keeps what an edit rechecked: each analysed file's new diagnostics.
fn hold(
    project: &Project,
    held: &mut HeldDiagnostics,
    rechecked: rustc_hash::FxHashMap<PathBuf, Vec<Diagnostic>>,
) {
    for (path, diagnostics) in rechecked {
        if project.is_analyzed(&path) {
            held.insert(path, diagnostics);
        } else {
            held.remove(&path);
        }
    }
}

/// The first method with a span of its own to delete, class name and typed
/// parameter of a file, in source order.
#[derive(Default)]
struct Finder {
    method: Option<Span>,
    class: Option<Span>,
    param_type: Option<Span>,
}

impl Finder {
    fn param(&mut self, params: &[Param]) {
        if self.param_type.is_none() {
            self.param_type = params
                .iter()
                .find_map(|param| param.type_hint.as_ref().map(|hint| hint.span));
        }
    }
}

impl<'ast> Visitor<'ast> for Finder {
    fn visit_stmt(&mut self, stmt: &'ast Stmt) {
        if let StmtKind::ClassDecl(decl) = &stmt.kind
            && self.class.is_none()
        {
            self.class = Some(decl.name.span);
        }
        walk_stmt(self, stmt);
    }

    fn visit_class_def(&mut self, def: &'ast ClassDef) {
        if self.method.is_none() {
            self.method = def.methods.iter().find_map(|method| method.removal_span);
        }
        walk_class_def(self, def);
    }

    fn visit_function_decl(&mut self, f: &'ast FunctionDecl) {
        self.param(&f.params);
        walk_function_decl(self, f);
    }
}

/// A fixed sequence of numbers from a seed (xorshift64*).
struct Rng(u64);

impl Rng {
    /// A number below `bound`, which is not zero.
    fn below(&mut self, bound: usize) -> usize {
        let mut x = self.0.max(1);
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        (x.wrapping_mul(0x2545_f491_4f6c_dd1d) % bound as u64) as usize
    }
}
