//! The edit replay: a script of edits, over files drawn with a seed,
//! applied to an analysed project the way the language server applies
//! them (`update_file`, `remove_file`, `recheck_including`), each file's
//! diagnostics kept as the server keeps them - a file's last check stands
//! until an edit rechecks it. After each step the registry dump, those
//! diagnostics and the whole-program checks' are compared with those of a
//! fresh analysis of the tree as the step leaves it, or, for a step that
//! leaves the project as it was, with the step before. As PHPStan's
//! result-cache end-to-end tests do, it catches what an edit leaves stale,
//! which no full run sees, at the step that leaves it.
//!
//! The script makes each edit the edit path tracks a dependency for, where
//! the project has what it edits: a file saved unchanged, a method
//! deleted, a class renamed, a parameter's type (a signature), a body
//! alone, a doc type, a constant's value, a class a trait's user is checked
//! as, a class declared twice, a class an `@see` names renamed, a file
//! added declaring a class named nowhere declared, an include's target
//! appearing and vanishing, a parse error made and mended, a file renamed
//! and a file deleted. Then the same edits in a seeded random order, so
//! that orders the script does not try are tried too.
//!
//! A file the replay adds is named to come in the middle of the analysed
//! files, where a fresh load puts it. The edits are made to the files on
//! disk too, so the tree must be a copy: the replay refuses a tree without
//! a `.phpstan-rs-replay` file at its root.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use diagnostics::Diagnostic;
use syntax::ParsedFile;
use syntax::ast::{ClassDef, FunctionDecl, Param, StmtKind, StmtNode};
use syntax::span::Span;
use syntax::visit::{Visitor, walk_class_def, walk_function_decl, walk_stmt};

use crate::project::Project;

/// The file at the root of a tree the replay may edit.
pub(crate) const MARKER: &str = ".phpstan-rs-replay";

/// How many edits the replay makes at random after its script.
const RANDOM_STEPS: usize = 8;

/// Each analysed file's diagnostics, as the session holds them.
pub(crate) type HeldDiagnostics = BTreeMap<PathBuf, Vec<Diagnostic>>;

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

/// What a step's state is compared with.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Against {
    /// A fresh analysis of the tree as the step leaves it.
    Fresh,
    /// The state before the step, which the step leaves as it was: a text
    /// that does not parse leaves the file's last parse in place, as the
    /// server keeps it, and a step with nothing to edit edits nothing.
    Previous,
}

/// One step of the replay: what it did, and to which file.
pub(crate) struct Step {
    pub(crate) label: &'static str,
    pub(crate) description: String,
    pub(crate) against: Against,
}

/// The edits, each a dependency of the edit path's.
#[derive(Clone, Copy)]
enum Edit {
    SaveUnchanged,
    DeleteMethod,
    RenameClass,
    ChangeParamType,
    ChangeBody,
    ChangeDocType,
    ChangeConstValue,
    ChangeTraitUser,
    DeclareTwice,
    RenameSeeTarget,
    AddMissingClass,
    IncludeTarget,
    ParseError,
    RenameFile,
    DeleteFile,
}

/// The script, in its order.
const SCRIPT: [Edit; 15] = [
    Edit::SaveUnchanged,
    Edit::DeleteMethod,
    Edit::RenameClass,
    Edit::ChangeParamType,
    Edit::ChangeBody,
    Edit::ChangeDocType,
    Edit::ChangeConstValue,
    Edit::ChangeTraitUser,
    Edit::DeclareTwice,
    Edit::RenameSeeTarget,
    Edit::AddMissingClass,
    Edit::IncludeTarget,
    Edit::ParseError,
    Edit::RenameFile,
    Edit::DeleteFile,
];

impl Edit {
    /// The label of its step, or of its first when it makes two.
    fn label(self) -> &'static str {
        match self {
            Edit::SaveUnchanged => "save unchanged",
            Edit::DeleteMethod => "delete a method",
            Edit::RenameClass => "rename a class",
            Edit::ChangeParamType => "change a parameter type",
            Edit::ChangeBody => "change a body alone",
            Edit::ChangeDocType => "change a doc type",
            Edit::ChangeConstValue => "change a constant's value",
            Edit::ChangeTraitUser => "change a class using a trait",
            Edit::DeclareTwice => "declare a class twice",
            Edit::RenameSeeTarget => "rename a class an @see names",
            Edit::AddMissingClass => "add a file declaring a class named nowhere declared",
            Edit::IncludeTarget => "an include's target appears",
            Edit::ParseError => "make a parse error",
            Edit::RenameFile => "rename a file",
            Edit::DeleteFile => "delete a file",
        }
    }
}

/// How the files an edit is made to are drawn.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Draw {
    /// In the order drawn at the start, each file edited once at most.
    Script,
    /// From a place drawn for each edit, any file.
    Random,
}

/// Applies the script's edits and then random ones to `project` and the
/// tree under `root`, drawing with `seed`, and calls `after` with each
/// step, the project and the diagnostics the session then holds. `list`
/// lists the analysed files as a fresh load finds them.
pub(crate) fn replay(
    project: &mut Project,
    root: &Path,
    seed: u64,
    held: &mut HeldDiagnostics,
    list: &dyn Fn() -> Vec<PathBuf>,
    after: impl FnMut(&Step, &mut Project, &HeldDiagnostics) -> Result<(), String>,
) -> Result<(), String> {
    let mut order = project.analyzed_paths();
    order.sort();
    let mut rng = Rng(seed);
    for i in (1..order.len()).rev() {
        order.swap(i, rng.below(i + 1));
    }
    let mut replay = Replay {
        project,
        root,
        held,
        list,
        after,
        rng,
        order,
        used: Vec::new(),
        draw: Draw::Script,
        added: 0,
    };
    for edit in SCRIPT {
        replay.run(edit)?;
    }
    replay.draw = Draw::Random;
    for _ in 0..RANDOM_STEPS {
        let edit = SCRIPT[replay.rng.below(SCRIPT.len())];
        replay.run(edit)?;
    }
    Ok(())
}

/// A file drawn for an edit: its path and text, the span the edit replaces
/// and what it writes there.
struct Picked {
    path: PathBuf,
    text: String,
    span: Span,
    replacement: String,
}

impl Picked {
    /// The text with the edit made.
    fn edited(&self) -> String {
        let mut edited = self.text.clone();
        edited.replace_range(
            self.span.start as usize..self.span.end as usize,
            &self.replacement,
        );
        edited
    }

    /// What the edit did, for the script.
    fn description(&self) -> String {
        format!(
            "{} {}..{}: {:?} -> {:?}",
            self.path.display(),
            self.span.start,
            self.span.end,
            &self.text[self.span.start as usize..self.span.end as usize],
            self.replacement
        )
    }
}

struct Replay<'a, F> {
    project: &'a mut Project,
    root: &'a Path,
    held: &'a mut HeldDiagnostics,
    list: &'a dyn Fn() -> Vec<PathBuf>,
    after: F,
    rng: Rng,
    // The analysed files in the order the script draws them.
    order: Vec<PathBuf>,
    // The files the script has edited.
    used: Vec<PathBuf>,
    draw: Draw,
    // How many files the replay has added.
    added: usize,
}

impl<F> Replay<'_, F>
where
    F: FnMut(&Step, &mut Project, &HeldDiagnostics) -> Result<(), String>,
{
    fn run(&mut self, edit: Edit) -> Result<(), String> {
        let label = edit.label();
        match edit {
            Edit::SaveUnchanged => {
                self.splice(label, &|_, _| Some((Span::new(0, 0), String::new())))
            }
            Edit::DeleteMethod => self.splice(label, &|parsed, text| {
                Finder::of(parsed, text)
                    .method
                    .map(|span| (span, String::new()))
            }),
            Edit::RenameClass => self.splice(label, &|parsed, text| {
                Finder::of(parsed, text).class.map(|span| {
                    let name = &text[span.start as usize..span.end as usize];
                    (span, format!("{name}Replayed"))
                })
            }),
            Edit::ChangeParamType => self.splice(label, &|parsed, text| {
                Finder::of(parsed, text).param_type.map(|span| {
                    let hint = &text[span.start as usize..span.end as usize];
                    (span, other_type(hint).to_string())
                })
            }),
            Edit::ChangeBody => self.splice(label, &|parsed, text| {
                Finder::of(parsed, text)
                    .body
                    .map(|open| (Span::new(open.end, open.end), " echo '';".to_string()))
            }),
            Edit::ChangeDocType => self.splice(label, &|parsed, text| {
                Finder::of(parsed, text).doc_type.map(|span| {
                    let written = &text[span.start as usize..span.end as usize];
                    (span, other_type(written).to_string())
                })
            }),
            Edit::ChangeConstValue => self.splice(label, &|parsed, text| {
                Finder::of(parsed, text).const_value.map(|span| {
                    let value = &text[span.start as usize..span.end as usize];
                    let other = if value.starts_with(['\'', '"']) {
                        "1"
                    } else {
                        "'replayed'"
                    };
                    (span, other.to_string())
                })
            }),
            Edit::ChangeTraitUser => self.splice(label, &|parsed, text| {
                Finder::of(parsed, text).trait_use.map(|span| {
                    (
                        Span::new(span.end, span.end),
                        "\n    public function replayedMember(): int\n    {\n        return 1;\n    }\n"
                            .to_string(),
                    )
                })
            }),
            Edit::DeclareTwice => self.declare_twice(label),
            Edit::RenameSeeTarget => self.rename_see_target(label),
            Edit::AddMissingClass => self.add_missing_class(label),
            Edit::IncludeTarget => self.include_target(label),
            Edit::ParseError => self.parse_error(label),
            Edit::RenameFile => self.rename_file(label),
            Edit::DeleteFile => self.delete_file(label),
        }
    }

    /// Calls `after` with the step.
    fn step(
        &mut self,
        label: &'static str,
        description: String,
        against: Against,
    ) -> Result<(), String> {
        let step = Step {
            label,
            description,
            against,
        };
        (self.after)(&step, self.project, self.held)
    }

    /// The step of an edit that found nothing to edit.
    fn nothing(&mut self, label: &'static str, why: &str) -> Result<(), String> {
        self.step(label, why.to_string(), Against::Previous)
    }

    /// The analysed files an edit may be made to, in the order it tries
    /// them.
    fn candidates(&mut self) -> Vec<PathBuf> {
        match self.draw {
            Draw::Script => self
                .order
                .iter()
                .filter(|path| !self.used.contains(path) && self.project.is_analyzed(path))
                .cloned()
                .collect(),
            Draw::Random => {
                let mut paths = self.project.analyzed_paths();
                paths.sort();
                if !paths.is_empty() {
                    let start = self.rng.below(paths.len());
                    paths.rotate_left(start);
                }
                paths
            }
        }
    }

    fn read(&self, path: &Path) -> Result<String, String> {
        let bytes =
            std::fs::read(self.root.join(path)).map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }

    /// The first candidate for which `find` finds a span to edit, with its
    /// text, taken as used.
    fn pick(&mut self, find: &Find) -> Result<Option<Picked>, String> {
        for path in self.candidates() {
            let text = self.read(&path)?;
            let Ok(parsed) = syntax::parse(text.as_bytes()) else {
                continue;
            };
            if let Some((span, replacement)) = find(&parsed, &text) {
                self.used.push(path.clone());
                return Ok(Some(Picked {
                    path,
                    text,
                    span,
                    replacement,
                }));
            }
        }
        Ok(None)
    }

    /// Writes `text` to `path` and gives it to the project as the server
    /// gives it an edit.
    fn write(&mut self, path: &Path, text: &str) -> Result<(), String> {
        std::fs::write(self.root.join(path), text)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        let rechecked = self
            .project
            .update_file(path, text)
            .map_err(|e| format!("{}: {}", path.display(), e.message))?;
        hold(self.project, self.held, rechecked);
        Ok(())
    }

    /// Removes `path` from the tree and the project.
    fn remove(&mut self, path: &Path) -> Result<(), String> {
        std::fs::remove_file(self.root.join(path))
            .map_err(|e| format!("{}: {e}", path.display()))?;
        let rechecked = self.project.remove_file(path);
        self.held.remove(path);
        hold(self.project, self.held, rechecked);
        Ok(())
    }

    /// Whether a fresh load lists `path` among the analysed files.
    fn listed(&self, path: &Path) -> bool {
        (self.list)().iter().any(|listed| listed == path)
    }

    /// A path for a new file that a fresh load lists among the analysed
    /// files: beside the middle one in path order, its name begun by that
    /// file's.
    fn placed(&mut self) -> Option<PathBuf> {
        let mut paths = self.project.analyzed_paths();
        paths.sort();
        let middle = paths.get(paths.len() / 2)?;
        let name = middle.file_name()?.to_string_lossy().into_owned();
        self.added += 1;
        Some(middle.with_file_name(format!("{name}-replay{}.php", self.added)))
    }

    /// Adds the file `text` at a path `placed` gives, when a fresh load
    /// lists it there; the path, or `None` when it would not.
    fn add(&mut self, text: &str) -> Result<Option<PathBuf>, String> {
        let Some(path) = self.placed() else {
            return Ok(None);
        };
        std::fs::write(self.root.join(&path), text)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        if !self.listed(&path) {
            std::fs::remove_file(self.root.join(&path))
                .map_err(|e| format!("{}: {e}", path.display()))?;
            return Ok(None);
        }
        self.write(&path, text)?;
        Ok(Some(path))
    }

    /// Edits the first candidate in which `find` finds a span.
    fn splice(&mut self, label: &'static str, find: &Find) -> Result<(), String> {
        let Some(picked) = self.pick(find)? else {
            return self.nothing(label, "no file has what the step edits");
        };
        self.write(&picked.path, &picked.edited())?;
        self.step(label, picked.description(), Against::Fresh)
    }

    /// A new file declaring a class an analysed file declares.
    fn declare_twice(&mut self, label: &'static str) -> Result<(), String> {
        let class = self.candidates_unfiltered().into_iter().find_map(|path| {
            let state = self.project.files.get(&path)?;
            let class = state
                .classes
                .iter()
                .find(|name| is_plain_class_name(name))?;
            Some((path, class.to_string()))
        });
        let Some((declared_in, class)) = class else {
            return self.nothing(label, "no analysed file declares a class");
        };
        match self.add(&declaring(&class))? {
            Some(path) => self.step(
                label,
                format!(
                    "{} declares {class}, as {} does",
                    path.display(),
                    declared_in.display()
                ),
                Against::Fresh,
            ),
            None => self.nothing(label, "a fresh load would not list an added file"),
        }
    }

    /// A class an `@see` of another file names, renamed where it is
    /// declared.
    fn rename_see_target(&mut self, label: &'static str) -> Result<(), String> {
        for naming in self.candidates_unfiltered() {
            let text = self.read(&naming)?;
            let Ok(parsed) = syntax::parse(text.as_bytes()) else {
                continue;
            };
            let targets: Vec<String> = crate::deps::doc_site_references(&parsed.references)
                .into_iter()
                .flat_map(|(_, keys)| keys.see)
                .filter_map(|key| match key {
                    crate::deps::RefKey::Class(name) => Some(name.to_string()),
                    _ => None,
                })
                .collect();
            for target in targets {
                let Some((declared, info)) = self.project.registry.lookup_class(&target) else {
                    continue;
                };
                let declared = declared.to_string();
                let Some(file_id) = info.provenance().file_id() else {
                    continue;
                };
                let path = self.project.files.path(file_id).to_path_buf();
                if path == naming
                    || !self.project.is_analyzed(&path)
                    || self.draw == Draw::Script && self.used.contains(&path)
                {
                    continue;
                }
                let declaring = self.read(&path)?;
                let Ok(parsed) = syntax::parse(declaring.as_bytes()) else {
                    continue;
                };
                let Some(span) = class_name_span(&parsed.stmts, &declared) else {
                    continue;
                };
                let picked = Picked {
                    path,
                    text: declaring,
                    span,
                    replacement: format!(
                        "{}Replayed",
                        &declared[declared.rfind('\\').map_or(0, |at| at + 1)..]
                    ),
                };
                if self.draw == Draw::Script {
                    self.used.push(picked.path.clone());
                }
                self.write(&picked.path, &picked.edited())?;
                return self.step(
                    label,
                    format!("{}, named by {}", picked.description(), naming.display()),
                    Against::Fresh,
                );
            }
        }
        self.nothing(
            label,
            "no @see names a class another analysed file declares",
        )
    }

    /// The analysed files in the order `candidates` gives, used ones too.
    fn candidates_unfiltered(&mut self) -> Vec<PathBuf> {
        match self.draw {
            Draw::Script => self
                .order
                .iter()
                .filter(|path| self.project.is_analyzed(path))
                .cloned()
                .collect(),
            Draw::Random => self.candidates(),
        }
    }

    /// A new file declaring a class some file names and none declares.
    fn add_missing_class(&mut self, label: &'static str) -> Result<(), String> {
        let missing = self.candidates_unfiltered().into_iter().find_map(|path| {
            let state = self.project.files.get(&path)?;
            let mut names: Vec<&syntax::name::Name> = state
                .references
                .iter()
                .filter_map(|key| match key {
                    crate::deps::RefKey::Class(name) => Some(name),
                    _ => None,
                })
                .filter(|name| {
                    self.project.registry.lookup_class(name).is_none()
                        && is_plain_class_name(name)
                        && !types::is_reserved_type_word(name)
                })
                .collect();
            names.sort_by(|a, b| a.as_str().cmp(b.as_str()));
            names.first().map(|name| (path.clone(), name.to_string()))
        });
        let Some((naming, class)) = missing else {
            return self.nothing(label, "no file names a class none declares");
        };
        match self.add(&declaring(&class))? {
            Some(path) => self.step(
                label,
                format!(
                    "{} declares {class}, which {} names",
                    path.display(),
                    naming.display()
                ),
                Against::Fresh,
            ),
            None => self.nothing(label, "a fresh load would not list an added file"),
        }
    }

    /// A file appearing where an include looked for one and found none,
    /// then vanishing: two steps. A PHP file there must be one a fresh load
    /// analyses, as the server would load it.
    fn include_target(&mut self, label: &'static str) -> Result<(), String> {
        let tree = std::fs::canonicalize(self.root)
            .map_err(|e| format!("{}: {e}", self.root.display()))?;
        let mut probes: Vec<PathBuf> = self
            .project
            .include_probes
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .values()
            .flatten()
            .filter(|probe| {
                probe.starts_with(&tree)
                    && !probe.exists()
                    && probe.parent().is_some_and(Path::is_dir)
            })
            .cloned()
            .collect();
        probes.sort();
        probes.dedup();
        if !probes.is_empty() {
            let start = self.rng.below(probes.len());
            probes.rotate_left(start);
        }
        let text = "<?php\n";
        for probe in probes {
            std::fs::write(&probe, text).map_err(|e| format!("{}: {e}", probe.display()))?;
            if probe.extension().is_some_and(|e| e == "php") {
                let Some(path) = (self.list)()
                    .into_iter()
                    .find(|path| self.project.absolute_path(path) == probe)
                else {
                    std::fs::remove_file(&probe)
                        .map_err(|e| format!("{}: {e}", probe.display()))?;
                    continue;
                };
                let rechecked = self
                    .project
                    .update_file(&path, text)
                    .map_err(|e| format!("{}: {}", path.display(), e.message))?;
                hold(self.project, self.held, rechecked);
                self.step(label, path.display().to_string(), Against::Fresh)?;
                self.remove(&path)?;
                return self.step(
                    "an include's target vanishes",
                    path.display().to_string(),
                    Against::Fresh,
                );
            }
            let rechecked = self.project.recheck_including(std::slice::from_ref(&probe));
            hold(self.project, self.held, rechecked);
            self.step(label, probe.display().to_string(), Against::Fresh)?;
            std::fs::remove_file(&probe).map_err(|e| format!("{}: {e}", probe.display()))?;
            let rechecked = self.project.recheck_including(std::slice::from_ref(&probe));
            hold(self.project, self.held, rechecked);
            return self.step(
                "an include's target vanishes",
                probe.display().to_string(),
                Against::Fresh,
            );
        }
        self.nothing(
            label,
            "no include looked for a file in the tree that is not there",
        )
    }

    /// A text that does not parse, which leaves the project as it was,
    /// then the file's text again: two steps.
    fn parse_error(&mut self, label: &'static str) -> Result<(), String> {
        let Some(picked) = self.pick(&|parsed, _| {
            parsed
                .stmts
                .iter()
                .find(|stmt| !matches!(stmt.kind, StmtKind::InlineHtml(_)))
                .map(|stmt| {
                    (
                        Span::new(stmt.span.start, stmt.span.start),
                        ") ".to_string(),
                    )
                })
        })?
        else {
            return self.nothing(label, "no file has PHP code");
        };
        let broken = picked.edited();
        let path = &picked.path;
        std::fs::write(self.root.join(path), &broken)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        if self.project.update_file(path, &broken).is_ok() {
            return Err(format!(
                "{}: the text made not to parse parsed",
                path.display()
            ));
        }
        self.step(label, picked.description(), Against::Previous)?;
        self.write(&picked.path, &picked.text)?;
        self.step(
            "mend the parse error",
            picked.path.display().to_string(),
            Against::Fresh,
        )
    }

    /// An analysed file moved to a path `placed` gives, as the server
    /// sees a rename: the file deleted, and a file added.
    fn rename_file(&mut self, label: &'static str) -> Result<(), String> {
        let Some(picked) = self.pick(&|_, _| Some((Span::new(0, 0), String::new())))? else {
            return self.nothing(label, "no file left to rename");
        };
        let Some(to) = self.placed() else {
            return self.nothing(label, "no file left to rename");
        };
        std::fs::rename(self.root.join(&picked.path), self.root.join(&to))
            .map_err(|e| format!("{}: {e}", picked.path.display()))?;
        if !self.listed(&to) {
            std::fs::rename(self.root.join(&to), self.root.join(&picked.path))
                .map_err(|e| format!("{}: {e}", to.display()))?;
            return self.nothing(label, "a fresh load would not list an added file");
        }
        let rechecked = self.project.remove_file(&picked.path);
        self.held.remove(&picked.path);
        hold(self.project, self.held, rechecked);
        let rechecked = self
            .project
            .update_file(&to, &picked.text)
            .map_err(|e| format!("{}: {}", to.display(), e.message))?;
        hold(self.project, self.held, rechecked);
        self.step(
            label,
            format!("{} -> {}", picked.path.display(), to.display()),
            Against::Fresh,
        )
    }

    fn delete_file(&mut self, label: &'static str) -> Result<(), String> {
        let Some(picked) = self.pick(&|_, _| Some((Span::new(0, 0), String::new())))? else {
            return self.nothing(label, "no file left to delete");
        };
        self.remove(&picked.path)?;
        self.step(label, picked.path.display().to_string(), Against::Fresh)
    }
}

/// What finds, in a file's parse and text, the span an edit replaces and
/// what it writes there.
type Find = dyn Fn(&ParsedFile, &str) -> Option<(Span, String)>;

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

/// A type other than `written`.
fn other_type(written: &str) -> &'static str {
    if written.eq_ignore_ascii_case("int") {
        "string"
    } else {
        "int"
    }
}

/// Whether `name` is a class name a declaration can be written for:
/// identifiers between its separators.
fn is_plain_class_name(name: &str) -> bool {
    name.split('\\').all(|part| {
        part.chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
            && part.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    })
}

/// A file declaring the empty class `class`.
fn declaring(class: &str) -> String {
    let (namespace, short) = match class.rsplit_once('\\') {
        Some((namespace, short)) => (format!("namespace {namespace};\n\n"), short),
        None => (String::new(), class),
    };
    format!("<?php\n{namespace}class {short}\n{{\n}}\n")
}

/// The span of the name of the class `fqn` declares in `stmts`.
fn class_name_span(stmts: &[StmtNode], fqn: &str) -> Option<Span> {
    struct Named<'a> {
        fqn: &'a str,
        found: Option<Span>,
    }
    impl<'ast> Visitor<'ast> for Named<'_> {
        fn visit_stmt(&mut self, stmt: &'ast StmtNode) {
            if let StmtKind::ClassDecl(decl) = &stmt.kind
                && self.found.is_none()
                && decl.fq_name().eq_ignore_ascii_case(self.fqn)
            {
                self.found = Some(decl.name.span);
            }
            walk_stmt(self, stmt);
        }
    }
    let mut named = Named { fqn, found: None };
    stmts.iter().for_each(|stmt| named.visit_stmt(stmt));
    named.found
}

/// The type `doc`, a doc comment, gives first in a `@return`, `@param` or
/// `@var` tag, where it is one word with its brackets closed: its range in
/// `doc`.
fn doc_type(doc: &str) -> Option<(usize, usize)> {
    let mut first: Option<(usize, usize)> = None;
    for tag in ["@return", "@param", "@var"] {
        for (at, _) in doc.match_indices(tag) {
            let rest = &doc[at + tag.len()..];
            let skipped = rest.len() - rest.trim_start_matches([' ', '\t']).len();
            if skipped == 0 {
                continue;
            }
            let start = at + tag.len() + skipped;
            let word = doc[start..]
                .split(|c: char| c.is_whitespace())
                .next()
                .unwrap_or_default();
            if word.is_empty() || word.starts_with(['$', '*']) || !balanced(word) {
                continue;
            }
            if first.is_none_or(|(earlier, _)| start < earlier) {
                first = Some((start, start + word.len()));
            }
            break;
        }
    }
    first
}

/// Whether every bracket `word` opens it closes.
fn balanced(word: &str) -> bool {
    let mut depth = 0i32;
    for c in word.chars() {
        match c {
            '<' | '(' | '{' | '[' => depth += 1,
            '>' | ')' | '}' | ']' => depth -= 1,
            _ => {}
        }
        if depth < 0 {
            return false;
        }
    }
    depth == 0
}

/// The first of each thing an edit replaces in a file, in source order: a
/// method with a span of its own to delete, a class name, a typed
/// parameter, a function body's opening brace, a doc type, a constant's
/// value and a class's first `use` of traits.
struct Finder<'a> {
    text: &'a str,
    method: Option<Span>,
    class: Option<Span>,
    param_type: Option<Span>,
    body: Option<Span>,
    doc_type: Option<Span>,
    const_value: Option<Span>,
    trait_use: Option<Span>,
}

impl<'a> Finder<'a> {
    fn of(parsed: &ParsedFile, text: &'a str) -> Finder<'a> {
        let mut finder = Finder {
            text,
            method: None,
            class: None,
            param_type: None,
            body: None,
            doc_type: None,
            const_value: None,
            trait_use: None,
        };
        parsed.stmts.iter().for_each(|stmt| finder.visit_stmt(stmt));
        finder
    }

    fn param(&mut self, params: &[Param]) {
        if self.param_type.is_none() {
            self.param_type = params
                .iter()
                .find_map(|param| param.type_hint.as_ref().map(|hint| hint.span));
        }
    }
}

impl<'ast> Visitor<'ast> for Finder<'_> {
    fn visit_stmt(&mut self, stmt: &'ast StmtNode) {
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
        if self.const_value.is_none() {
            self.const_value = def.consts.first().map(|constant| constant.value.span);
        }
        if self.trait_use.is_none() {
            self.trait_use = def.trait_uses.first().map(|trait_use| trait_use.span);
        }
        walk_class_def(self, def);
    }

    fn visit_function_decl(&mut self, f: &'ast FunctionDecl) {
        self.param(&f.params);
        if self.body.is_none() {
            self.body = f.body.as_ref().map(|body| body.open);
        }
        if self.doc_type.is_none()
            && let Some(doc) = &f.doc_comment
            && let Some(at) = self.text[..f.name.span.start as usize].rfind(&**doc)
            && let Some((start, end)) = doc_type(doc)
        {
            self.doc_type = Some(Span::new((at + start) as u32, (at + end) as u32));
        }
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
