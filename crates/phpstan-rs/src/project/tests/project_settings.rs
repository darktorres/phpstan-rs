//! `Project::apply_settings` stores the PHP version range onto
//! `registry::Registry::php_versions`.
use super::super::*;

fn with_php_versions(php_versions: Option<types::PhpVersions>) -> ProjectSettings {
    ProjectSettings {
        php_versions,
        ..ProjectSettings::default()
    }
}

#[test]
fn php_versions_default_to_unset() {
    let project = Project::new();
    assert_eq!(project.registry().php_versions, None);
}

#[test]
fn php_versions_are_stored_onto_the_registry() {
    let mut project = Project::new();
    let range = types::PhpVersions::exactly(types::PhpVersion::new(8, 1));
    project.apply_settings(with_php_versions(Some(range)));
    assert_eq!(project.registry().php_versions, Some(range));
}

#[test]
fn php_versions_are_cleared_by_settings_without_them() {
    let mut project = Project::new();
    project.apply_settings(with_php_versions(Some(types::PhpVersions::exactly(
        types::PhpVersion::new(8, 1),
    ))));
    project.apply_settings(with_php_versions(None));
    assert_eq!(project.registry().php_versions, None);
}

#[test]
fn a_negative_string_offset_on_versions_either_side_of_7_1_is_either() {
    let mut project = Project::new();
    project.apply_settings(ProjectSettings {
        php_versions: Some(types::PhpVersions::new(
            types::PhpVersion::new(7, 0),
            types::PhpVersion::new(8, 4),
        )),
        ..ProjectSettings::default()
    });
    project
        .load_analyzed_file(
            PathBuf::from("a.php"),
            r#"<?php
use function PHPStan\Testing\assertType;

/** @return void */
function f() {
    $abc = 'abc';
    assertType("'c'|''", $abc[-1]);
    $w = 'ab';
    $w[-1] = 'x';
    assertType("'ax'|'ab'", $w);
    $v = 'ab';
    $v[-5] = 'x';
    assertType("'ab'", $v);
}
"#,
        )
        .unwrap();
    project.finish_loading();
    let reports: Vec<String> = project
        .check_file(Path::new("a.php"))
        .iter()
        .map(|d| format!("{} {} {}", d.span.start.line, d.identifier, d.message))
        .collect();
    assert_eq!(
        reports,
        [
            "7 offsetAccess.mightNotExist Offset -1 might not exist on 'abc'.",
            "12 offsetAssign.valueType Illegal string offset -5.",
        ]
    );
}
