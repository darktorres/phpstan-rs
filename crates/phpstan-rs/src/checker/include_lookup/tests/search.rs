//! Whether an include is found: missing only when no combination of the
//! working directory, the include path and the world finds a file in the
//! project, and found when a combination leaves the project.

use super::super::*;

/// A project on a machine, with the files that exist in it.
struct Site {
    machine: World,
    root: PathBuf,
    includer: PathBuf,
    trees: Vec<PathBuf>,
    unbuilt: Vec<PathBuf>,
    working_dirs: Vec<PathBuf>,
    working_dirs_open: bool,
    include_path: IncludePath,
    files: Vec<PathBuf>,
}

#[cfg(unix)]
fn base() -> PathBuf {
    PathBuf::from("/proj")
}

#[cfg(windows)]
fn base() -> PathBuf {
    PathBuf::from("C:\\proj")
}

impl Site {
    /// The root, with `src` and `admin` holding scripts, and the includer in
    /// `src`.
    fn new() -> Site {
        let root = base();
        Site {
            machine: World::machine(),
            includer: root.join("src"),
            trees: vec![root.clone()],
            unbuilt: Vec::new(),
            working_dirs: vec![root.clone(), root.join("src"), root.join("admin")],
            working_dirs_open: false,
            include_path: IncludePath::default(),
            files: Vec::new(),
            root,
        }
    }

    fn with_file(mut self, relative: &str) -> Site {
        self.files.push(self.root.join(relative));
        self
    }

    fn declaring(mut self, declared: &str) -> Site {
        self.include_path.declared = Some(declared.to_string());
        self
    }

    fn setting(mut self, unix: Option<&[&str]>, windows: Option<&[&str]>) -> Site {
        let texts = |values: Option<&[&str]>| {
            values.map(|values| values.iter().map(|v| v.to_string()).collect::<Vec<_>>())
        };
        self.include_path.set.push(WorldValues {
            by_world: [texts(unix), texts(windows)],
        });
        self
    }

    fn place(&self) -> Place<'_> {
        Place {
            machine: self.machine,
            root: Some(&self.root),
            includer_dir: Some(&self.includer),
            trees: &self.trees,
            unbuilt: &self.unbuilt,
            working_dirs: &self.working_dirs,
            working_dirs_open: self.working_dirs_open,
            include_path: &self.include_path,
        }
    }

    fn found(&self, world: World, text: &str) -> bool {
        self.place().is_found(world, text, &|path| {
            let path = folded(path);
            self.files.contains(&path)
        })
    }

    /// Whether `text` is found, and the paths the file system was asked
    /// about, in order.
    fn found_asking(&self, world: World, text: &str) -> (bool, Vec<PathBuf>) {
        let asked = std::cell::RefCell::new(Vec::new());
        let found = self.place().is_found(world, text, &|path| {
            asked.borrow_mut().push(path.to_path_buf());
            let path = folded(path);
            self.files.contains(&path)
        });
        (found, asked.into_inner())
    }

    /// The text of a path under the root, in the machine's own spelling.
    fn text(&self, relative: &str) -> String {
        self.root.join(relative).to_string_lossy().into_owned()
    }
}

#[test]
fn a_stream_wrapper_is_never_missing() {
    let project = Site::new();
    for world in World::ALL {
        assert!(project.found(world, "phar:///x.phar/a.php"));
    }
}

#[test]
fn an_absolute_path_in_the_project_is_found_when_the_file_exists_and_missing_otherwise() {
    let missing = Site::new();
    let present = Site::new().with_file("lib/a.php");
    let text = missing.text("lib/a.php");
    assert!(!missing.found(World::machine(), &text));
    assert!(present.found(World::machine(), &text));
}

#[test]
fn an_absolute_path_outside_the_project_is_not_judged() {
    let project = Site::new();
    for text in ["/var/www/other/a.php", "/etc/passwd"] {
        for world in World::ALL {
            assert!(project.found(world, text), "{world:?} {text}");
        }
    }
}

#[test]
fn a_dot_slash_name_is_looked_up_in_each_working_directory() {
    let missing = Site::new();
    assert!(!missing.found(World::Unix, "./config.php"));
    for dir in ["", "src/", "admin/"] {
        let project = Site::new().with_file(&format!("{dir}config.php"));
        assert!(project.found(World::Unix, "./config.php"), "{dir}");
    }
}

#[test]
fn a_dot_slash_name_is_looked_up_in_the_working_directory_and_not_beside_the_includer() {
    let mut project = Site::new().declaring(".").with_file("src/config.php");
    project.working_dirs = vec![project.root.clone()];
    assert!(!project.found(World::Unix, "./config.php"));
    assert!(project.found(World::Unix, "config.php"));
}

#[test]
fn a_parent_name_is_looked_up_from_each_working_directory() {
    let from_the_scripts = |project: &mut Site| {
        project.working_dirs = vec![project.root.join("src"), project.root.join("admin")];
    };
    let mut present = Site::new().with_file("config.php");
    from_the_scripts(&mut present);
    assert!(present.found(World::Unix, "../config.php"));
    let mut missing = Site::new();
    from_the_scripts(&mut missing);
    assert!(!missing.found(World::Unix, "../config.php"));
}

#[test]
fn a_parent_name_that_leaves_the_project_from_any_working_directory_is_not_judged() {
    let project = Site::new();
    assert!(project.found(World::Unix, "../config.php"));
    assert!(project.found(World::Unix, "../../other/config.php"));
}

#[test]
fn a_dot_slash_name_that_folds_to_a_parent_is_read_after_folding() {
    let project = Site::new().with_file("lib/a.php");
    assert!(project.found(World::Unix, "./x/../lib/a.php"));
    assert!(!project.found(World::Unix, "./x/../lib/b.php"));
}

#[test]
fn working_directories_that_are_open_settle_every_name_relative_to_them() {
    let mut site = Site::new().declaring(".");
    assert!(!site.found(World::Unix, "./x.php"));
    assert!(!site.found(World::Unix, "x.php"));
    site.working_dirs_open = true;
    assert!(site.found(World::Unix, "./x.php"));
    assert!(site.found(World::Unix, "x.php"));
    let mut relative_entry = Site::new().declaring("lib");
    assert!(!relative_entry.found(World::Unix, "x.php"));
    relative_entry.working_dirs_open = true;
    assert!(relative_entry.found(World::Unix, "x.php"));
}

#[test]
fn an_absolute_name_is_judged_whatever_the_working_directories() {
    let mut site = Site::new().declaring(".");
    site.working_dirs_open = true;
    assert!(!site.found(World::machine(), &site.text("lib/a.php")));
}

#[test]
fn a_path_below_a_directory_this_checkout_lacks_is_not_judged() {
    let mut site = Site::new().declaring(".");
    let vendor = site.root.join("vendor");
    let text = site.text("vendor/autoload.php");
    assert!(!site.found(World::machine(), &text));
    site.unbuilt = vec![vendor];
    assert!(site.found(World::machine(), &text));
    // Only below it: its sibling is still judged.
    assert!(!site.found(World::machine(), &site.text("vendors/autoload.php")));
    assert!(!site.found(World::machine(), &site.text("lib/autoload.php")));
}

#[test]
fn a_parent_after_a_name_is_left_to_the_file_system() {
    let mut project = Site::new();
    project.working_dirs = vec![project.root.clone()];
    let (found, asked) = project.found_asking(World::Unix, "./link/../a.php");
    assert!(!found);
    assert_eq!(asked, vec![project.root.join("link/../a.php")]);
}

#[test]
fn the_including_files_directory_is_asked_before_the_other_working_directories() {
    let project = Site::new().with_file("src/x.php");
    let (found, asked) = project.found_asking(World::Unix, "./x.php");
    assert!(found);
    assert_eq!(asked, vec![project.root.join("src/x.php")]);
}

#[test]
fn an_entry_outside_the_project_settles_a_name_before_the_file_system_is_asked() {
    let project = Site::new().declaring(".:/usr/share/php");
    let (found, asked) = project.found_asking(World::Unix, "PEAR.php");
    assert!(found);
    assert!(asked.is_empty(), "{asked:?}");
}

#[test]
fn a_name_that_is_nowhere_is_asked_about_in_every_place_it_can_be() {
    let project = Site::new().declaring(".");
    let (found, asked) = project.found_asking(World::Unix, "x.php");
    assert!(!found);
    for dir in ["", "src/", "admin/"] {
        assert!(
            asked.contains(&project.root.join(format!("{dir}x.php"))),
            "{dir}"
        );
    }
}

#[test]
fn a_parent_that_climbs_out_of_the_project_by_its_text_is_not_judged() {
    let project = Site::new();
    assert!(project.found(World::Unix, "./lib/../../a.php"));
    assert!(!project.found(World::Unix, "./lib/../a.php"));
}

#[test]
fn a_plain_name_is_not_judged_while_the_include_path_is_undeclared() {
    let project = Site::new();
    assert!(project.found(World::Unix, "PEAR.php"));
    assert!(project.found(World::Windows, "PEAR.php"));
}

#[test]
fn a_plain_name_is_judged_once_the_include_path_is_declared_inside_the_project() {
    let project = Site::new().declaring(".");
    assert!(!project.found(World::Unix, "lib/a.php"));
    let present = Site::new().declaring(".").with_file("lib/a.php");
    assert!(present.found(World::Unix, "lib/a.php"));
}

#[test]
fn a_plain_name_is_found_in_the_including_files_directory_and_in_any_working_directory() {
    for dir in ["src/", "admin/", ""] {
        let project = Site::new().declaring(".").with_file(&format!("{dir}a.php"));
        assert!(project.found(World::Unix, "a.php"), "{dir}");
    }
}

#[test]
fn an_entry_that_leaves_the_project_opens_the_path() {
    let project = Site::new().declaring(".:/usr/share/php");
    assert!(project.found(World::Unix, "PEAR.php"));
}

#[test]
fn a_declared_path_that_is_empty_is_not_known() {
    // PHP does not take an empty include path, and keeps its default.
    let site = Site::new().declaring("");
    assert!(site.found(World::Unix, "PEAR.php"));
}

#[test]
fn an_empty_entry_at_the_end_names_nothing() {
    let site = Site::new().declaring(".:");
    assert!(!site.found(World::Unix, "PEAR.php"));
    assert!(!Site::new().declaring("lib:").found(World::Unix, "PEAR.php"));
}

#[test]
fn an_empty_value_the_code_sets_adds_no_entry() {
    let site = Site::new().declaring(".").setting(Some(&[""]), Some(&[""]));
    assert!(!site.found(World::Unix, "PEAR.php"));
}

#[test]
fn an_empty_entry_before_another_is_the_root_and_opens_the_path() {
    for declared in [".::lib", ":lib", ":"] {
        let site = Site::new().declaring(declared);
        assert!(site.found(World::Unix, "PEAR.php"), "{declared}");
    }
}

#[test]
fn a_relative_entry_is_relative_to_each_working_directory() {
    let project = Site::new().declaring("lib").with_file("admin/lib/a.php");
    assert!(project.found(World::Unix, "a.php"));
    let missing = Site::new().declaring("lib");
    assert!(!missing.found(World::Unix, "a.php"));
}

#[test]
fn an_absolute_entry_inside_the_project_is_looked_in() {
    let project = Site::new();
    let entry = project.text("vendor/pear");
    let present = Site::new()
        .declaring(&entry)
        .with_file("vendor/pear/PEAR.php");
    assert!(present.found(World::machine(), "PEAR.php"));
    let missing = Site::new().declaring(&entry);
    assert!(!missing.found(World::machine(), "PEAR.php"));
}

#[test]
fn a_value_the_code_sets_adds_its_entries() {
    let project = Site::new()
        .declaring(".")
        .setting(Some(&["lib"]), Some(&["lib"]))
        .with_file("lib/a.php");
    assert!(project.found(World::Unix, "a.php"));
    let without = Site::new().declaring(".");
    assert!(!without.found(World::Unix, "a.php"));
}

#[test]
fn a_value_that_cannot_be_computed_opens_the_path() {
    let project = Site::new().declaring(".").setting(None, None);
    assert!(project.found(World::Unix, "a.php"));
    assert!(project.found(World::Windows, "a.php"));
}

#[test]
fn a_value_that_one_world_cannot_compute_opens_that_world_only() {
    let project = Site::new().declaring(".").setting(Some(&["lib"]), None);
    assert!(!project.found(World::Unix, "a.php"));
    assert!(project.found(World::Windows, "a.php"));
}

#[test]
fn get_include_path_in_a_set_value_is_the_declared_path() {
    let value = format!("lib{}{GET_INCLUDE_PATH}", World::Unix.path_separator());
    let project = Site::new()
        .declaring(".")
        .setting(Some(&[&value]), Some(&[&value.replace(':', ";")]))
        .with_file("lib/a.php");
    assert!(project.found(World::Unix, "a.php"));
    assert!(!project.found(World::Unix, "b.php"));
}

#[test]
fn get_include_path_that_is_part_of_an_entry_cannot_be_computed() {
    let value = format!("{GET_INCLUDE_PATH}/lib");
    let project = Site::new()
        .declaring(".")
        .setting(Some(&[&value]), Some(&[&value]));
    assert!(project.found(World::Unix, "a.php"));
}

#[test]
fn composer_directories_are_looked_in_without_opening_the_path() {
    let mut project = Site::new().declaring(".").with_file("vendor/pear/PEAR.php");
    project.include_path.composer = vec![project.root.join("vendor/pear")];
    assert!(project.found(World::Unix, "PEAR.php"));
    assert!(!project.found(World::Unix, "Other.php"));
}

#[test]
fn a_backslash_separates_names_in_the_windows_world_and_is_part_of_a_name_in_the_unix_world() {
    let project = Site::new().declaring(".").with_file("lib/a.php");
    for text in [".\\lib\\a.php", "lib\\a.php"] {
        assert!(project.found(World::Windows, text), "{text}");
        assert!(!project.found(World::Unix, text), "{text}");
    }
}

#[cfg(unix)]
#[test]
fn a_name_with_a_backslash_is_one_name_in_the_unix_world() {
    let mut project = Site::new().declaring(".");
    project.files.push(project.root.join("lib\\a.php"));
    assert!(project.found(World::Unix, "lib\\a.php"));
    assert!(!project.found(World::Windows, "lib\\a.php"));
}

#[cfg(unix)]
mod on_unix {
    use super::*;

    #[test]
    fn a_drive_path_names_a_machine_this_one_is_not() {
        let project = Site::new();
        assert!(project.found(World::Windows, "C:\\proj\\lib\\a.php"));
        assert!(project.found(World::Windows, "\\\\server\\share\\a.php"));
    }

    #[test]
    fn a_rooted_windows_path_is_the_root_of_this_machine() {
        let present = Site::new().with_file("lib/a.php");
        // `\proj\lib\a.php` is rooted on the current drive.
        assert!(present.found(World::Windows, "\\proj\\lib\\a.php"));
        let missing = Site::new();
        assert!(!missing.found(World::Windows, "\\proj\\lib\\a.php"));
    }

    #[test]
    fn native_absolute_paths_follow_the_table() {
        let root = PathBuf::from("/proj");
        let names = Names::parse(World::Unix, "/a/b");
        let unix = |world| native_absolute(world, World::Unix, Some(&root), &Anchor::Root, &names);
        assert_eq!(unix(World::Unix), Some(PathBuf::from("/a/b")));
        assert_eq!(unix(World::Windows), Some(PathBuf::from("/a/b")));
        assert_eq!(
            native_absolute(
                World::Windows,
                World::Unix,
                Some(&root),
                &Anchor::Drive('C'),
                &names
            ),
            None
        );
    }
}

#[cfg(windows)]
mod on_windows {
    use super::*;

    #[test]
    fn a_drive_path_in_the_project_is_found_when_the_file_exists() {
        let present = Site::new().with_file("lib\\a.php");
        assert!(present.found(World::Windows, "C:\\proj\\lib\\a.php"));
        let missing = Site::new();
        assert!(!missing.found(World::Windows, "C:\\proj\\lib\\a.php"));
        assert!(!missing.found(World::Windows, "c:/proj/lib/a.php"));
    }

    #[test]
    fn a_drive_path_elsewhere_is_not_judged() {
        let project = Site::new();
        assert!(project.found(World::Windows, "D:\\other\\a.php"));
        assert!(project.found(World::Windows, "\\\\server\\share\\a.php"));
    }

    #[test]
    fn a_unix_path_names_a_machine_this_one_is_not() {
        let project = Site::new();
        assert!(project.found(World::Unix, "/proj/lib/a.php"));
    }

    #[test]
    fn a_rooted_path_is_the_root_of_the_projects_drive() {
        let project = Site::new();
        assert!(project.found(World::Windows, "\\other\\a.php"));
    }

    #[test]
    fn native_absolute_paths_follow_the_table() {
        let root = PathBuf::from("C:\\proj");
        let names = Names::parse(World::Windows, "\\a\\b");
        let native =
            |world, anchor| native_absolute(world, World::Windows, Some(&root), &anchor, &names);
        assert_eq!(
            native(World::Windows, Anchor::Root),
            Some(PathBuf::from("C:\\a\\b"))
        );
        assert_eq!(
            native(World::Windows, Anchor::Drive('D')),
            Some(PathBuf::from("D:\\a\\b"))
        );
        assert_eq!(native(World::Unix, Anchor::Root), None);
    }
}
