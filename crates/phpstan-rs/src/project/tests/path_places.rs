//! The places of the numbered paths (`FileTable::rank_of`). A load sets them
//! from its listing, so a whole-program merge sorts nothing. A project that
//! was numbered before its load sorts them on first use instead. Either way,
//! a sort by place is a sort by path.

use super::super::*;
use crate::project::session::{OverlayFile, Session, load_test_files};
use crate::registry::stub_set::Stubs;

/// Asserts that `paths`, given in path order, have increasing places, and
/// that a sort by place gives them back in that order.
fn assert_places_follow_paths(project: &Project, paths: &[&str]) {
    let mut by_path: Vec<PathBuf> = paths.iter().map(|path| PathBuf::from(*path)).collect();
    by_path.sort();
    let places: Vec<u32> = by_path.iter().map(|path| project.path_rank(path)).collect();
    assert!(
        places.windows(2).all(|pair| pair[0] < pair[1]),
        "{places:?}"
    );
    let mut by_place = by_path.clone();
    by_place.sort_by_key(|path| project.path_rank(path));
    assert_eq!(by_place, by_path);
}

#[test]
fn a_load_sets_the_places_from_its_listing() {
    // Out of path order, so the listing has to sort them.
    let files = [
        ("/w/app/main.php", true),
        ("/w/lib/a.php", false),
        ("/w/app/Alpha.php", true),
        ("/w/a-b.php", false),
    ];
    let overlay: Vec<OverlayFile> = files
        .iter()
        .enumerate()
        .map(|(index, (path, analysed))| OverlayFile {
            path: PathBuf::from(*path),
            bytes: format!("<?php function placed_{index}() {{}}").into_bytes(),
            analysed: *analysed,
            stub: false,
        })
        .collect();
    let loaded = Session::for_overlay(overlay, Stubs::shared().clone())
        .load()
        .expect("loads");
    assert!(
        loaded.project.files.has_places(),
        "the listing sets the places"
    );
    let paths = files.map(|(path, _)| path);
    assert_places_follow_paths(&loaded.project, &paths);
}

#[test]
fn a_batch_load_into_a_fresh_project_sets_the_places_too() {
    let mut project = Project::new();
    let failures = load_test_files(
        &mut project,
        &[
            (
                PathBuf::from("/w/z.php"),
                "<?php function z_first() {}",
                false,
            ),
            (
                PathBuf::from("/w/a.php"),
                "<?php function a_second() {}",
                true,
            ),
            (
                PathBuf::from("/w/m/b.php"),
                "<?php function m_third() {}",
                false,
            ),
        ],
    );
    assert!(failures.is_empty(), "{failures:?}");
    assert!(project.files.has_places(), "the listing sets the places");
    assert_places_follow_paths(&project, &["/w/a.php", "/w/m/b.php", "/w/z.php"]);
}

#[test]
fn a_load_places_the_composer_manifests_among_its_files() {
    let manifest = |path: &str| composer::manifest::ComposerManifest {
        path: PathBuf::from(path),
        ..Default::default()
    };
    let mut project = Project::new();
    project.apply_settings(ProjectSettings {
        packages: composer::PackageIndex::new(vec![
            manifest("/w/lib/composer.json"),
            manifest("/w/composer.json"),
        ]),
        ..ProjectSettings::default()
    });
    let failures = load_test_files(
        &mut project,
        &[
            (
                PathBuf::from("/w/z.php"),
                "<?php function z_first() {}",
                false,
            ),
            (
                PathBuf::from("/w/lib/x.php"),
                "<?php function x_second() {}",
                false,
            ),
            (
                PathBuf::from("/w/a.php"),
                "<?php function a_third() {}",
                true,
            ),
        ],
    );
    assert!(failures.is_empty(), "{failures:?}");
    assert!(
        project.files.has_places(),
        "the listing and the manifests set the places"
    );
    assert_places_follow_paths(
        &project,
        &[
            "/w/a.php",
            "/w/composer.json",
            "/w/lib/composer.json",
            "/w/lib/x.php",
            "/w/z.php",
        ],
    );
}

#[test]
fn a_project_numbered_before_its_load_sorts_the_places_on_first_use() {
    let mut project = Project::new();
    project.files.id_for(Path::new("/w/stray.php"));
    let failures = load_test_files(
        &mut project,
        &[
            (
                PathBuf::from("/w/z.php"),
                "<?php function z_first() {}",
                false,
            ),
            (
                PathBuf::from("/w/a.php"),
                "<?php function a_second() {}",
                true,
            ),
        ],
    );
    assert!(failures.is_empty(), "{failures:?}");
    // The listing leaves the stray number out, so it sets no places.
    assert!(!project.files.has_places());
    assert_places_follow_paths(&project, &["/w/a.php", "/w/stray.php", "/w/z.php"]);
}
