//! What a file is to the project: analysed or context, and whether it is a
//! stub. One function decides it for the load and for every edit.

use std::path::{Path, PathBuf};

use composer::manifest::lexically_normalized;
use rustc_hash::FxHashMap;

/// Whether a file is checked (`Analysed`) or only read for the
/// declarations it gives the files that are (`Context`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Analysed,
    Context,
}

/// What `FileRoles::classify` decides about one file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Classified {
    pub role: Role,
    /// Whether the file is in a stub directory, whichever its role: its
    /// declarations are stubs of the PHP version they name.
    pub is_stub: bool,
}

/// The roots a project's files are classified by.
///
/// A file's role is that of the strongest root that contains it: analysed
/// when an analysed root does, context otherwise. A project with no
/// analysed root, a bare `Project`, takes every file as analysed unless a
/// directory below it is named in `DEFAULT_SCAN_EXCLUDED_DIR_NAMES`.
///
/// Every path given or asked about is absolute and lexically normalized.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FileRoles {
    analysed: Vec<AnalysedRoot>,
    /// The `pruned` lists of the analysed roots, by directory: a path is
    /// under a root exactly when that root is one of its ancestors.
    analysed_by_dir: FxHashMap<PathBuf, Vec<&'static [&'static str]>>,
    stubs: Vec<PathBuf>,
}

#[derive(Clone, Debug, PartialEq)]
struct AnalysedRoot {
    dir: PathBuf,
    /// Directory names below `dir` that hold no analysed file.
    pruned: &'static [&'static str],
}

impl FileRoles {
    /// Adds an analysed root: a directory, or a file.
    pub fn with_analysed(self, dir: &Path) -> Self {
        self.with_pruned_analysed(dir, &[])
    }

    /// Adds an analysed root that leaves out what is under a directory
    /// named in `pruned`.
    pub fn with_pruned_analysed(mut self, dir: &Path, pruned: &'static [&'static str]) -> Self {
        let dir = lexically_normalized(dir);
        self.analysed_by_dir
            .entry(dir.clone())
            .or_default()
            .push(pruned);
        self.analysed.push(AnalysedRoot { dir, pruned });
        self
    }

    /// Adds a stub root: a directory, or a file.
    pub fn with_stubs(mut self, dir: &Path) -> Self {
        self.stubs.push(lexically_normalized(dir));
        self
    }

    pub fn classify(&self, path: &Path) -> Classified {
        Classified {
            role: if self.is_analysed(path) {
                Role::Analysed
            } else {
                Role::Context
            },
            is_stub: self.is_stub(path),
        }
    }

    fn is_analysed(&self, path: &Path) -> bool {
        if self.analysed.is_empty() {
            return !crate::project::path_under_excluded_dir(
                path,
                crate::project::DEFAULT_SCAN_EXCLUDED_DIR_NAMES,
            );
        }
        let found = path.ancestors().any(|ancestor| {
            self.analysed_by_dir.get(ancestor).is_some_and(|lists| {
                path.strip_prefix(ancestor).is_ok_and(|below| {
                    lists
                        .iter()
                        .any(|pruned| !crate::project::path_under_excluded_dir(below, pruned))
                })
            })
        });
        #[cfg(debug_assertions)]
        {
            let scanned = self.analysed.iter().any(|root| {
                path.strip_prefix(&root.dir)
                    .is_ok_and(|below| !crate::project::path_under_excluded_dir(below, root.pruned))
            });
            debug_assert_eq!(found, scanned, "{path:?}");
        }
        found
    }

    pub fn is_stub(&self, path: &Path) -> bool {
        self.stubs.iter().any(|dir| path.starts_with(dir))
    }

    /// Whether any stub root is configured.
    pub fn has_stubs(&self) -> bool {
        !self.stubs.is_empty()
    }

    /// Whether every file under `dir` is analysed: an analysed root with
    /// no directory names left out is `dir` or holds it.
    pub fn holds_only_analysed(&self, dir: &Path) -> bool {
        self.analysed
            .iter()
            .any(|root| root.pruned.is_empty() && dir.starts_with(&root.dir))
    }

    /// Whether an analysed root is `dir` or lies under it: a walk of `dir`
    /// may list files of that role.
    pub fn holds_analysed(&self, dir: &Path) -> bool {
        self.analysed.iter().any(|root| root.dir.starts_with(dir))
    }
}

/// The directories of `dirs`, in order, that no other of them contains,
/// compared as `absolute` spells them: walking those lists every file the
/// others would. Of equal directories, the first stays.
pub fn outermost(dirs: &[PathBuf], absolute: impl Fn(&Path) -> PathBuf) -> Vec<&PathBuf> {
    let spelled: Vec<PathBuf> = dirs.iter().map(|dir| absolute(dir)).collect();
    dirs.iter()
        .enumerate()
        .filter(|&(index, _)| {
            !spelled.iter().enumerate().any(|(other, outer)| {
                other != index
                    && spelled[index].starts_with(outer)
                    && (spelled[index] != *outer || other < index)
            })
        })
        .map(|(_, dir)| dir)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roles() -> FileRoles {
        FileRoles::default()
            .with_analysed(Path::new("/w/src"))
            .with_analysed(Path::new("/w/tests/./unit"))
            .with_stubs(Path::new("/w/stubs"))
    }

    #[test]
    fn a_file_under_an_analysed_root_is_analysed() {
        let classified = roles().classify(Path::new("/w/src/a/b.php"));
        assert_eq!(classified.role, Role::Analysed);
        assert!(!classified.is_stub);
    }

    #[test]
    fn a_root_is_normalized() {
        assert_eq!(
            roles().classify(Path::new("/w/tests/unit/a.php")).role,
            Role::Analysed
        );
    }

    #[test]
    fn a_file_under_no_analysed_root_is_context() {
        for path in ["/w/vendor/x/a.php", "/w/srcs/a.php", "/elsewhere/a.php"] {
            assert_eq!(roles().classify(Path::new(path)).role, Role::Context);
        }
    }

    #[test]
    fn a_stub_is_context_unless_an_analysed_root_holds_it() {
        let roles = roles();
        let stub = roles.classify(Path::new("/w/stubs/a.php"));
        assert_eq!((stub.role, stub.is_stub), (Role::Context, true));
        let both = FileRoles::default()
            .with_analysed(Path::new("/w"))
            .with_stubs(Path::new("/w/stubs"))
            .classify(Path::new("/w/stubs/a.php"));
        assert_eq!((both.role, both.is_stub), (Role::Analysed, true));
    }

    #[test]
    fn a_root_that_is_a_file_holds_only_that_file() {
        let roles = FileRoles::default().with_analysed(Path::new("/w/a.php"));
        assert_eq!(roles.classify(Path::new("/w/a.php")).role, Role::Analysed);
        assert_eq!(roles.classify(Path::new("/w/b.php")).role, Role::Context);
    }

    #[test]
    fn a_pruned_name_below_a_root_makes_a_file_context() {
        let roles = FileRoles::default().with_pruned_analysed(Path::new("/w"), &["vendor"]);
        assert_eq!(
            roles.classify(Path::new("/w/src/a.php")).role,
            Role::Analysed
        );
        assert_eq!(
            roles.classify(Path::new("/w/vendor/a.php")).role,
            Role::Context
        );
        assert_eq!(
            roles.classify(Path::new("/w/src/vendor/a.php")).role,
            Role::Context
        );
    }

    #[test]
    fn a_name_above_a_root_prunes_nothing() {
        let roles = FileRoles::default().with_pruned_analysed(Path::new("/vendor/w"), &["vendor"]);
        assert_eq!(
            roles.classify(Path::new("/vendor/w/a.php")).role,
            Role::Analysed
        );
    }

    #[test]
    fn without_a_root_a_file_is_analysed_unless_under_an_excluded_name() {
        let roles = FileRoles::default();
        assert_eq!(roles.classify(Path::new("a.php")).role, Role::Analysed);
        assert_eq!(
            roles.classify(Path::new("/w/src/a.php")).role,
            Role::Analysed
        );
        assert_eq!(
            roles.classify(Path::new("/w/vendor/x/a.php")).role,
            Role::Context
        );
        assert_eq!(
            roles.classify(Path::new("/w/node_modules/a.php")).role,
            Role::Context
        );
    }

    #[test]
    fn a_walk_may_hold_an_analysed_root() {
        let roles = roles();
        assert!(roles.holds_analysed(Path::new("/w")));
        assert!(roles.holds_analysed(Path::new("/w/src")));
        assert!(!roles.holds_analysed(Path::new("/w/vendor")));
        assert!(!roles.holds_analysed(Path::new("/w/src/a")));
    }

    #[test]
    fn a_directory_inside_another_or_equal_to_an_earlier_one_is_not_outermost() {
        let dirs: Vec<PathBuf> = ["src", "src/a", "./src", "lib", "src2"]
            .into_iter()
            .map(PathBuf::from)
            .collect();
        let kept = outermost(&dirs, |dir| {
            lexically_normalized(&Path::new("/w").join(dir))
        });
        assert_eq!(kept, [&dirs[0], &dirs[3], &dirs[4]]);
    }
}
