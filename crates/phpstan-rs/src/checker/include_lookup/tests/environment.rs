//! The environment the include rule is built with: the settings as given,
//! and what the code adds to them.

use super::super::*;

fn settings() -> IncludeSettings {
    IncludeSettings {
        include_path: Some(".".to_string()),
        composer: vec![PathBuf::from("/proj/vendor/a/lib")],
        trees: vec![PathBuf::from("/shared")],
        unbuilt: vec![PathBuf::from("/proj/vendor")],
        ..IncludeSettings::default()
    }
}

fn values(unix: Option<&[&str]>, windows: Option<&[&str]>) -> WorldValues {
    let texts = |values: Option<&[&str]>| {
        values.map(|values| values.iter().map(|v| v.to_string()).collect::<Vec<_>>())
    };
    WorldValues {
        by_world: [texts(unix), texts(windows)],
    }
}

fn moved_to(chdir: Vec<WorldValues>) -> IncludeEnv {
    let root = PathBuf::from("/proj");
    IncludeEnv::build(
        &settings(),
        Some(&root),
        || vec![root.clone(), root.join("src")],
        LookupChanges {
            chdir,
            ..LookupChanges::default()
        },
    )
}

#[test]
fn the_settings_are_kept_and_the_root_is_a_tree() {
    let env = moved_to(Vec::new());
    assert_eq!(env.root.as_deref(), Some(Path::new("/proj")));
    assert_eq!(
        env.trees,
        vec![PathBuf::from("/shared"), PathBuf::from("/proj")]
    );
    assert_eq!(env.unbuilt, vec![PathBuf::from("/proj/vendor")]);
    assert_eq!(env.include_path.declared.as_deref(), Some("."));
    assert_eq!(
        env.include_path.composer,
        vec![PathBuf::from("/proj/vendor/a/lib")]
    );
    assert!(!env.working_dirs_open);
}

#[test]
fn working_directories_are_the_declared_ones_or_else_the_script_directories() {
    let root = PathBuf::from("/proj");
    let declared = IncludeSettings {
        working_dirs: Some(vec![PathBuf::from("/proj/web")]),
        ..IncludeSettings::default()
    };
    let env = IncludeEnv::build(
        &declared,
        Some(&root),
        || panic!("the script directories are not asked for"),
        LookupChanges::default(),
    );
    assert_eq!(env.working_dirs, vec![PathBuf::from("/proj/web")]);
    assert_eq!(
        moved_to(Vec::new()).working_dirs,
        vec![PathBuf::from("/proj"), PathBuf::from("/proj/src")]
    );
}

#[test]
fn the_values_the_code_sets_the_include_path_to_are_kept() {
    let root = PathBuf::from("/proj");
    let set = vec![values(Some(&["lib"]), Some(&["lib"]))];
    let env = IncludeEnv::build(
        &settings(),
        Some(&root),
        Vec::new,
        LookupChanges {
            include_path: set.clone(),
            ..LookupChanges::default()
        },
    );
    assert_eq!(env.include_path.set, set);
}

#[cfg(unix)]
mod on_unix {
    use super::*;

    #[test]
    fn a_chdir_to_a_directory_of_this_machine_adds_it_once_for_both_worlds() {
        let env = moved_to(vec![values(Some(&["/proj/lib"]), Some(&["/proj/lib"]))]);
        assert_eq!(
            env.working_dirs,
            vec![
                PathBuf::from("/proj"),
                PathBuf::from("/proj/src"),
                PathBuf::from("/proj/lib")
            ]
        );
        assert!(!env.working_dirs_open);
    }

    #[test]
    fn a_chdir_to_a_directory_outside_the_project_is_a_directory_a_run_can_be_in() {
        let env = moved_to(vec![values(Some(&["/elsewhere"]), Some(&["/elsewhere"]))]);
        assert!(env.working_dirs.contains(&PathBuf::from("/elsewhere")));
        assert!(!env.working_dirs_open);
    }

    #[test]
    fn a_chdir_to_a_relative_directory_opens_the_working_directories() {
        for text in ["lib", "..", "./lib", ""] {
            let env = moved_to(vec![values(Some(&[text]), Some(&[text]))]);
            assert!(env.working_dirs_open, "{text:?}");
        }
    }

    #[test]
    fn a_chdir_that_is_not_computed_opens_the_working_directories() {
        for chdir in [
            values(None, None),
            values(Some(&["/proj/lib"]), None),
            values(None, Some(&["/proj/lib"])),
        ] {
            assert!(moved_to(vec![chdir]).working_dirs_open);
        }
    }

    #[test]
    fn a_drive_path_the_windows_world_names_adds_no_directory_to_this_machine() {
        // The machine's own world names the place; the other world's reading
        // is a spelling of it.
        let env = moved_to(vec![values(Some(&["/proj/lib"]), Some(&["D:\\data"]))]);
        assert!(!env.working_dirs_open);
        assert!(env.working_dirs.contains(&PathBuf::from("/proj/lib")));
    }

    #[test]
    fn a_drive_path_the_machines_own_world_cannot_name_opens_the_working_directories() {
        let env = moved_to(vec![values(Some(&["D:\\data"]), Some(&["D:\\data"]))]);
        assert!(env.working_dirs_open);
    }
}

#[cfg(windows)]
mod on_windows {
    use super::*;

    #[test]
    fn a_chdir_to_a_directory_of_this_machine_adds_it_and_the_unix_reading_opens_nothing() {
        let root = PathBuf::from("C:\\proj");
        let env = IncludeEnv::build(
            &IncludeSettings::default(),
            Some(&root),
            || vec![root.clone()],
            LookupChanges {
                chdir: vec![values(Some(&["C:\\proj\\lib"]), Some(&["C:\\proj\\lib"]))],
                ..LookupChanges::default()
            },
        );
        assert!(env.working_dirs.contains(&PathBuf::from("C:\\proj\\lib")));
        assert!(!env.working_dirs_open);
    }
}
