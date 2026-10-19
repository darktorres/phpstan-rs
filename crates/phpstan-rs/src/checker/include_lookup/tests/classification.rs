//! What the text of an include names, in each world: a pure function of the
//! world and the text.

use super::super::*;

fn names(names: &[&str]) -> Names {
    Names(names.iter().map(|n| n.to_string()).collect())
}

#[test]
fn names_drop_dots_and_empty_names_and_keep_parents_where_they_are_written() {
    for world in World::ALL {
        assert_eq!(
            Names::parse(world, "a/./b//c/../d"),
            names(&["a", "b", "c", "..", "d"])
        );
        assert_eq!(Names::parse(world, "../../x"), names(&["..", "..", "x"]));
        assert_eq!(
            Names::parse(world, "a/../../x"),
            names(&["a", "..", "..", "x"])
        );
        assert_eq!(Names::parse(world, ""), names(&[]));
    }
}

#[test]
fn folding_takes_a_parent_out_with_the_name_before_it_and_stops_at_the_root() {
    for (text, folded_text) in [
        ("/a/./b/../c", "/a/c"),
        ("/a/../../b", "/b"),
        ("/../a", "/a"),
        ("/a/b/..", "/a"),
    ] {
        assert_eq!(
            folded(Path::new(text)),
            PathBuf::from(folded_text),
            "{text}"
        );
    }
}

#[test]
fn a_backslash_separates_names_in_the_windows_world_only() {
    assert_eq!(Names::parse(World::Unix, "a\\b/c"), names(&["a\\b", "c"]));
    assert_eq!(
        Names::parse(World::Windows, "a\\b/c"),
        names(&["a", "b", "c"])
    );
}

#[test]
fn a_stream_wrapper_is_elsewhere_in_both_worlds() {
    for world in World::ALL {
        for text in [
            "phar:///a.phar/x.php",
            "data://text/plain,x",
            "http://h/x.php",
        ] {
            assert_eq!(classify(world, text), Target::Elsewhere, "{text}");
        }
    }
}

#[test]
fn a_leading_slash_is_absolute_in_both_worlds() {
    for world in World::ALL {
        assert_eq!(
            classify(world, "/var/www/a.php"),
            Target::Absolute(Anchor::Root, names(&["var", "www", "a.php"]))
        );
    }
}

#[test]
fn a_dot_slash_name_is_relative_and_any_other_is_plain() {
    for world in World::ALL {
        assert_eq!(
            classify(world, "./lib/a.php"),
            Target::Relative(names(&["lib", "a.php"]))
        );
        assert_eq!(
            classify(world, "../lib/a.php"),
            Target::Relative(names(&["..", "lib", "a.php"]))
        );
        assert_eq!(
            classify(world, "lib/a.php"),
            Target::Plain(names(&["lib", "a.php"]))
        );
        assert_eq!(classify(world, "a.php"), Target::Plain(names(&["a.php"])));
        assert_eq!(
            classify(world, ".hidden"),
            Target::Plain(names(&[".hidden"]))
        );
        assert_eq!(
            classify(world, "..x/a"),
            Target::Plain(names(&["..x", "a"]))
        );
    }
}

#[test]
fn a_dot_backslash_name_is_relative_in_the_windows_world_only() {
    assert_eq!(
        classify(World::Windows, ".\\lib\\a.php"),
        Target::Relative(names(&["lib", "a.php"]))
    );
    assert_eq!(
        classify(World::Windows, "..\\a.php"),
        Target::Relative(names(&["..", "a.php"]))
    );
    assert_eq!(
        classify(World::Unix, ".\\lib\\a.php"),
        Target::Plain(names(&[".\\lib\\a.php"]))
    );
}

#[test]
fn a_drive_and_a_share_are_absolute_in_the_windows_world_only() {
    assert_eq!(
        classify(World::Windows, "C:\\proj\\a.php"),
        Target::Absolute(Anchor::Drive('C'), names(&["proj", "a.php"]))
    );
    assert_eq!(
        classify(World::Windows, "c:/proj/a.php"),
        Target::Absolute(Anchor::Drive('C'), names(&["proj", "a.php"]))
    );
    assert_eq!(
        classify(World::Windows, "\\\\server\\share\\dir\\a.php"),
        Target::Absolute(
            Anchor::Share("server\\share".to_string()),
            names(&["dir", "a.php"])
        )
    );
    assert_eq!(
        classify(World::Windows, "//server/share/a.php"),
        Target::Absolute(
            Anchor::Share("server\\share".to_string()),
            names(&["a.php"])
        )
    );
    assert_eq!(
        classify(World::Unix, "C:\\proj\\a.php"),
        Target::Plain(names(&["C:\\proj\\a.php"]))
    );
}

#[test]
fn a_single_leading_backslash_is_rooted_on_the_current_drive() {
    assert_eq!(
        classify(World::Windows, "\\lib\\a.php"),
        Target::Absolute(Anchor::Root, names(&["lib", "a.php"]))
    );
}

#[test]
fn a_path_relative_to_a_drive_names_nothing_this_analysis_can_see() {
    assert_eq!(classify(World::Windows, "C:lib\\a.php"), Target::Elsewhere);
    assert_eq!(classify(World::Windows, "C:"), Target::Elsewhere);
}

#[test]
fn an_empty_include_path_entry_is_the_root_of_the_file_system() {
    for world in World::ALL {
        assert_eq!(
            entry_target(world, ""),
            Target::Absolute(Anchor::Root, names(&[]))
        );
        assert_eq!(entry_target(world, "."), Target::Plain(names(&[])));
    }
}

#[test]
fn the_separators_are_the_worlds() {
    assert_eq!(World::Unix.path_separator(), ':');
    assert_eq!(World::Windows.path_separator(), ';');
    assert_eq!(World::Unix.directory_separator(), '/');
    assert_eq!(World::Windows.directory_separator(), '\\');
    assert_eq!(World::ALL[World::Windows.index()], World::Windows);
}

#[test]
fn a_unix_machines_places_are_rooted_paths_in_either_world() {
    for world in World::ALL {
        assert_eq!(
            spell(world, World::Unix, Some("/proj"), "/proj/src/a.php").as_deref(),
            Some("/proj/src/a.php")
        );
    }
}

#[test]
fn a_windows_machines_places_keep_their_spelling_in_the_windows_world() {
    assert_eq!(
        spell(
            World::Windows,
            World::Windows,
            Some("C:\\proj"),
            "C:\\proj\\src"
        )
        .as_deref(),
        Some("C:\\proj\\src")
    );
}

#[test]
fn a_windows_machines_places_are_below_the_root_of_the_projects_drive_in_the_unix_world() {
    let spelled = |root, text| spell(World::Unix, World::Windows, root, text);
    for (text, expected) in [
        ("C:\\proj\\src\\a.php", Some("/proj/src/a.php")),
        ("c:\\proj\\src", Some("/proj/src")),
        ("C:\\", Some("/")),
        ("D:\\other\\a.php", None),
        ("\\\\server\\share\\a.php", None),
        ("C:a.php", None),
        ("src\\a.php", None),
    ] {
        assert_eq!(
            spelled(Some("C:\\proj"), text).as_deref(),
            expected,
            "{text}"
        );
    }
    // Without a root there is no drive to compare with.
    assert_eq!(
        spelled(None, "D:\\other\\a.php").as_deref(),
        Some("/other/a.php")
    );
}

#[test]
fn a_share_is_the_drive_of_the_places_below_it() {
    let root = Some("\\\\server\\share\\proj");
    let spelled = |text| spell(World::Unix, World::Windows, root, text);
    assert_eq!(
        spelled("\\\\server\\share\\proj\\a.php").as_deref(),
        Some("/proj/a.php")
    );
    assert_eq!(spelled("\\\\server\\other\\proj\\a.php"), None);
    assert_eq!(spelled("C:\\proj\\a.php"), None);
}

#[test]
fn a_place_spelled_for_the_unix_world_reads_back_as_the_place() {
    let text = spell(
        World::Unix,
        World::Windows,
        Some("C:\\proj"),
        "C:\\proj\\src\\a.php",
    )
    .unwrap();
    assert_eq!(
        classify(World::Unix, &text),
        Target::Absolute(Anchor::Root, names(&["proj", "src", "a.php"]))
    );
    assert_eq!(World::Unix.dirname(&text, 1), "/proj/src");
}
