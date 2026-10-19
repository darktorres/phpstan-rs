//! Generating and loading baseline files, and filtering diagnostics against them.

use super::super::*;
use super::helpers::*;

#[test]
fn generate_then_load_filters_out_every_recorded_diagnostic() {
    let mut results = FxHashMap::default();
    results.insert(
        PathBuf::from("a.php"),
        vec![diag("undefined variable $x"), diag("undefined variable $y")],
    );
    results.insert(PathBuf::from("b.php"), vec![diag("undefined variable $z")]);

    let path = temp_path("roundtrip");
    let count = generate_all(&path, &results).unwrap();
    assert_eq!(count, 3);

    let baseline = Baseline::load(&path).unwrap();
    filter(&mut results, &baseline);
    assert!(results[&PathBuf::from("a.php")].is_empty());
    assert!(results[&PathBuf::from("b.php")].is_empty());

    let _ = fs::remove_file(&path);
}

#[test]
fn filter_keeps_a_new_diagnostic_not_in_the_baseline() {
    let mut baselined = FxHashMap::default();
    baselined.insert(PathBuf::from("a.php"), vec![diag("undefined variable $x")]);
    let path = temp_path("keep_new");
    generate_all(&path, &baselined).unwrap();
    let baseline = Baseline::load(&path).unwrap();

    let mut results = FxHashMap::default();
    results.insert(
        PathBuf::from("a.php"),
        vec![
            diag("undefined variable $x"),
            diag("undefined variable $newOne"),
        ],
    );
    filter(&mut results, &baseline);
    assert_eq!(results[&PathBuf::from("a.php")].len(), 1);
    assert!(
        results[&PathBuf::from("a.php")][0]
            .message
            .contains("$newOne")
    );

    let _ = fs::remove_file(&path);
}

#[test]
fn the_same_message_in_a_different_file_is_not_baselined() {
    // `<main>`'s own generic label is shared across every file's
    // top-level script diagnostics - the file path has to be part of
    // the key, or baselining one file's finding would silently
    // suppress an unrelated file's identical-looking one too.
    let mut baselined = FxHashMap::default();
    baselined.insert(PathBuf::from("a.php"), vec![diag("undefined variable $x")]);
    let path = temp_path("cross_file");
    generate_all(&path, &baselined).unwrap();
    let baseline = Baseline::load(&path).unwrap();

    let mut results = FxHashMap::default();
    results.insert(PathBuf::from("b.php"), vec![diag("undefined variable $x")]);
    filter(&mut results, &baseline);
    assert_eq!(results[&PathBuf::from("b.php")].len(), 1);

    let _ = fs::remove_file(&path);
}

#[test]
fn generating_twice_over_the_same_results_produces_a_byte_identical_file() {
    let mut results = FxHashMap::default();
    results.insert(PathBuf::from("z.php"), vec![diag("one"), diag("two")]);
    results.insert(PathBuf::from("a.php"), vec![diag("three")]);

    let path = temp_path("deterministic");
    generate_all(&path, &results).unwrap();
    let first = fs::read_to_string(&path).unwrap();
    generate_all(&path, &results).unwrap();
    let second = fs::read_to_string(&path).unwrap();
    assert_eq!(first, second);

    let _ = fs::remove_file(&path);
}

#[test]
fn a_differently_spelled_but_identical_path_is_still_matched() {
    let mut baselined = FxHashMap::default();
    baselined.insert(
        PathBuf::from("./t.php"),
        vec![diag("undefined variable $x")],
    );
    let path = temp_path("path_spelling");
    generate_all(&path, &baselined).unwrap();
    let baseline = Baseline::load(&path).unwrap();

    let mut results = FxHashMap::default();
    results.insert(
        PathBuf::from("././t.php"),
        vec![diag("undefined variable $x")],
    );
    filter(&mut results, &baseline);
    assert!(results[&PathBuf::from("././t.php")].is_empty());

    let _ = fs::remove_file(&path);
}

#[test]
fn filter_reports_a_baseline_entry_no_current_diagnostic_matches() {
    let mut baselined = FxHashMap::default();
    baselined.insert(PathBuf::from("a.php"), vec![diag("undefined variable $x")]);
    let path = temp_path("stale_entry");
    generate_all(&path, &baselined).unwrap();
    let baseline = Baseline::load(&path).unwrap();

    let mut results = FxHashMap::default();
    results.insert(PathBuf::from("a.php"), vec![]);
    let stale = filter(&mut results, &baseline);
    assert_eq!(stale.len(), 1);
    assert!(stale[0].contains("a.php"));
    assert!(stale[0].contains("undefined variable $x"));

    let _ = fs::remove_file(&path);
}

#[test]
fn filter_reports_nothing_stale_when_every_entry_still_matches() {
    let mut baselined = FxHashMap::default();
    baselined.insert(PathBuf::from("a.php"), vec![diag("undefined variable $x")]);
    let path = temp_path("no_stale_entries");
    generate_all(&path, &baselined).unwrap();
    let baseline = Baseline::load(&path).unwrap();

    let mut results = baselined;
    let stale = filter(&mut results, &baseline);
    assert!(stale.is_empty());

    let _ = fs::remove_file(&path);
}

#[test]
fn a_baseline_entry_written_with_backslashes_names_the_file() {
    let path = temp_path("backslash_separator");
    fs::write(
        &path,
        r#"{"version": 2, "entries": [
            {"file": "src\\a.php", "message": "p"},
            {"file": "src/a.php", "message": "p"}
        ]}"#,
    )
    .unwrap();
    let baseline = Baseline::load(&path).unwrap();
    let mut now = results_of("src/a.php", vec![diag("p"), diag("p"), diag("p")]);
    let stale = filter(&mut now, &baseline);
    assert!(stale.is_empty(), "{stale:?}");
    assert_eq!(now[&PathBuf::from("src/a.php")].len(), 1);
    let _ = fs::remove_file(&path);
}

#[cfg(windows)]
#[test]
fn a_baseline_spells_a_file_with_slashes_on_windows() {
    let path = temp_path("windows_spelling");
    let recorded = results_of("src\\a.php", vec![diag("p")]);
    generate_all(&path, &recorded).unwrap();
    let written = fs::read_to_string(&path).unwrap();
    assert!(written.contains("\"file\": \"src/a.php\""), "{written}");
    let baseline = Baseline::load(&path).unwrap();
    let mut now = results_of("src\\a.php", vec![diag("p")]);
    assert!(filter(&mut now, &baseline).is_empty());
    assert!(now[&PathBuf::from("src\\a.php")].is_empty());
    let _ = fs::remove_file(&path);
}

#[cfg(unix)]
#[test]
fn a_backslash_in_a_baseline_file_also_names_one_in_a_name_outside_windows() {
    let path = temp_path("backslash_name");
    fs::write(
        &path,
        r#"{"version": 2, "entries": [{"file": "dir\\a.php", "message": "p", "count": 2}]}"#,
    )
    .unwrap();
    let baseline = Baseline::load(&path).unwrap();
    let mut now = results_of("dir\\a.php", vec![diag("p")]);
    now.insert(PathBuf::from("dir/a.php"), vec![diag("p")]);
    let stale = filter(&mut now, &baseline);
    assert!(stale.is_empty(), "{stale:?}");
    assert!(now[&PathBuf::from("dir\\a.php")].is_empty());
    assert!(now[&PathBuf::from("dir/a.php")].is_empty());

    fs::write(
        &path,
        r#"{"version": 2, "entries": [{"file": "dir/a.php", "message": "p"}]}"#,
    )
    .unwrap();
    let baseline = Baseline::load(&path).unwrap();
    let mut now = results_of("dir\\a.php", vec![diag("p")]);
    filter(&mut now, &baseline);
    assert_eq!(now[&PathBuf::from("dir\\a.php")].len(), 1);
    let _ = fs::remove_file(&path);
}

#[test]
fn normalize_path_resolves_dot_and_dotdot_segments() {
    assert_eq!(normalize_path(Path::new("./t.php")), PathBuf::from("t.php"));
    assert_eq!(
        normalize_path(Path::new("././t.php")),
        PathBuf::from("t.php")
    );
    assert_eq!(
        normalize_path(Path::new("src/../t.php")),
        PathBuf::from("t.php")
    );
    assert_eq!(normalize_path(Path::new(".")), PathBuf::from("."));
    assert_eq!(normalize_path(Path::new("./.")), PathBuf::from("."));
}

fn results_of(file: &str, diagnostics: Vec<Diagnostic>) -> FxHashMap<PathBuf, Vec<Diagnostic>> {
    FxHashMap::from_iter([(PathBuf::from(file), diagnostics)])
}

#[test]
fn a_baseline_entry_absorbs_only_its_recorded_count() {
    let path = temp_path("counts");
    let recorded = results_of("a.php", vec![diag("same problem"), diag("same problem")]);
    assert_eq!(generate_all(&path, &recorded).unwrap(), 2);
    let written = fs::read_to_string(&path).unwrap();
    assert!(written.contains("\"count\": 2"), "{written}");

    let mut now = results_of(
        "a.php",
        vec![
            diag("same problem"),
            diag("same problem"),
            diag("same problem"),
        ],
    );
    let stale = filter(&mut now, &Baseline::load(&path).unwrap());
    assert!(stale.is_empty(), "{stale:?}");
    assert_eq!(now[&PathBuf::from("a.php")].len(), 1);
    let _ = fs::remove_file(&path);
}

#[test]
fn fewer_occurrences_than_recorded_are_reported_with_both_counts() {
    let path = temp_path("fewer");
    generate_all(
        &path,
        &results_of("a.php", vec![diag("p"), diag("p"), diag("p")]),
    )
    .unwrap();
    let mut now = results_of("a.php", vec![diag("p")]);
    let stale = filter(&mut now, &Baseline::load(&path).unwrap());
    assert_eq!(stale, vec!["a.php: p (expected 3 occurrence(s), found 1)"]);
    let _ = fs::remove_file(&path);
}

#[test]
fn a_legacy_baseline_with_repeated_entries_adds_them_up() {
    let path = temp_path("legacy");
    fs::write(
        &path,
        r#"[
            {"file": "a.php", "location": "f", "message": "p"},
            {"file": "a.php", "location": "f", "message": "p"}
        ]"#,
    )
    .unwrap();
    let mut now = results_of("a.php", vec![diag("p"), diag("p"), diag("p")]);
    filter(&mut now, &Baseline::load(&path).unwrap());
    assert_eq!(now[&PathBuf::from("a.php")].len(), 1);
    let _ = fs::remove_file(&path);
}

#[test]
fn identifier_mode_survives_a_reworded_message() {
    let path = temp_path("identifier");
    generate_all(
        &path,
        &results_of("a.php", vec![diag_with("Old wording.", "argument.type")]),
    )
    .unwrap();
    let reworded = || results_of("a.php", vec![diag_with("New wording.", "argument.type")]);

    let mut by_message = reworded();
    let stale = filter(&mut by_message, &Baseline::load(&path).unwrap());
    assert_eq!(by_message[&PathBuf::from("a.php")].len(), 1);
    assert_eq!(stale.len(), 1);

    let mut by_identifier = reworded();
    let baseline = Baseline::load(&path)
        .unwrap()
        .with_match_mode(MatchMode::Identifier);
    let stale = filter(&mut by_identifier, &baseline);
    assert!(by_identifier[&PathBuf::from("a.php")].is_empty());
    assert!(stale.is_empty(), "{stale:?}");

    let mut other_identifier = results_of("a.php", vec![diag_with("New wording.", "return.type")]);
    filter(&mut other_identifier, &baseline);
    assert_eq!(other_identifier[&PathBuf::from("a.php")].len(), 1);
    let _ = fs::remove_file(&path);
}

#[test]
fn a_per_identifier_split_round_trips_through_its_index() {
    let dir = temp_path("split");
    fs::create_dir_all(&dir).unwrap();
    let index = dir.join("baseline.json");
    let recorded = results_of(
        "a.php",
        vec![
            diag_with("one", "argument.type"),
            diag_with("two", "argument.type"),
            diag_with("three", "return.type"),
        ],
    );
    fs::create_dir_all(dir.join("baseline")).unwrap();
    fs::write(dir.join("baseline/stale.identifier.json"), "[]").unwrap();
    let checked = recorded.keys().cloned().collect();
    assert_eq!(
        generate_per_identifier(&index, &recorded, &checked).unwrap(),
        3
    );
    let mut files: Vec<String> = fs::read_dir(dir.join("baseline"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    files.sort();
    assert_eq!(files, vec!["argument.type.json", "return.type.json"]);
    let index_text = fs::read_to_string(&index).unwrap();
    assert!(
        index_text.contains("baseline/argument.type.json"),
        "{index_text}"
    );

    let mut now = recorded.clone();
    let stale = filter(&mut now, &Baseline::load(&index).unwrap());
    assert!(now[&PathBuf::from("a.php")].is_empty());
    assert!(stale.is_empty(), "{stale:?}");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn an_unchecked_file_keeps_the_entries_recorded_for_it() {
    let path = temp_path("unchecked_kept");
    let mut before = results_of("a.php", vec![diag("old a")]);
    before.insert(PathBuf::from("b.php"), vec![diag("b finding")]);
    generate_all(&path, &before).unwrap();

    // The run checked a.php again and did not check b.php.
    let after = results_of("a.php", vec![diag("new a")]);
    let checked = [PathBuf::from("a.php")].into_iter().collect();
    generate(&path, &after, &checked).unwrap();

    let mut now = before.clone();
    filter(&mut now, &Baseline::load(&path).unwrap());
    assert!(
        now[&PathBuf::from("b.php")].is_empty(),
        "b.php was not checked, so its entry must stay"
    );
    assert_eq!(
        now[&PathBuf::from("a.php")].len(),
        1,
        "a.php was checked, so its old entry must go"
    );
    let _ = fs::remove_file(&path);
}

#[test]
fn a_checked_file_with_no_diagnostics_loses_the_entries_recorded_for_it() {
    let path = temp_path("checked_clean");
    generate_all(&path, &results_of("a.php", vec![diag("old a")])).unwrap();

    let checked = [PathBuf::from("a.php")].into_iter().collect();
    generate(&path, &FxHashMap::default(), &checked).unwrap();

    let mut now = results_of("a.php", vec![diag("old a")]);
    filter(&mut now, &Baseline::load(&path).unwrap());
    assert_eq!(
        now[&PathBuf::from("a.php")].len(),
        1,
        "a.php is clean now, so its old entry must not absorb"
    );
    let _ = fs::remove_file(&path);
}

#[test]
fn an_unparsed_file_keeps_its_entries_and_records_no_new_diagnostic() {
    let path = temp_path("unparsed");
    generate_all(&path, &results_of("a.php", vec![diag("old a")])).unwrap();

    // a.php no longer parses, so the run reports its parse error and does not check it.
    let results = results_of("a.php", vec![diag_with("syntax error", "parse.error")]);
    generate(&path, &results, &FxHashSet::default()).unwrap();

    let mut now = results_of(
        "a.php",
        vec![diag("old a"), diag_with("syntax error", "parse.error")],
    );
    filter(&mut now, &Baseline::load(&path).unwrap());
    let remaining = &now[&PathBuf::from("a.php")];
    assert_eq!(
        remaining.len(),
        1,
        "the old entry should absorb, and only the parse error remain"
    );
    assert_eq!(remaining[0].message, "syntax error");
    let _ = fs::remove_file(&path);
}

#[test]
fn regenerating_an_index_keeps_its_unchecked_entries_in_one_file() {
    let dir = temp_path("flatten");
    fs::create_dir_all(&dir).unwrap();
    let index = dir.join("baseline.json");
    let mut before = results_of("a.php", vec![diag_with("one", "argument.type")]);
    before.insert(
        PathBuf::from("b.php"),
        vec![diag_with("two", "return.type")],
    );
    let checked_all = before.keys().cloned().collect();
    generate_per_identifier(&index, &before, &checked_all).unwrap();
    assert!(!read_file(&index).unwrap().includes.is_empty());

    // A later run checked only b.php, which is clean now; a.php was not checked.
    let checked = [PathBuf::from("b.php")].into_iter().collect();
    generate(&index, &FxHashMap::default(), &checked).unwrap();

    assert!(
        read_file(&index).unwrap().includes.is_empty(),
        "the index still names included files"
    );
    let mut now = before.clone();
    filter(&mut now, &Baseline::load(&index).unwrap());
    assert!(
        now[&PathBuf::from("a.php")].is_empty(),
        "a.php's entry, kept from an include, must still absorb"
    );
    assert_eq!(
        now[&PathBuf::from("b.php")].len(),
        1,
        "b.php was checked and is clean, so its entry is gone"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn an_existing_baseline_that_does_not_load_is_an_error_and_is_not_overwritten() {
    let path = temp_path("unreadable");
    fs::write(&path, "{ not a baseline").unwrap();
    let checked = [PathBuf::from("a.php")].into_iter().collect();
    let result = generate(&path, &results_of("a.php", vec![diag("new")]), &checked);
    assert!(result.is_err());
    assert_eq!(fs::read_to_string(&path).unwrap(), "{ not a baseline");
    let _ = fs::remove_file(&path);
}

#[test]
fn a_per_identifier_regeneration_over_an_unreadable_index_removes_nothing() {
    let dir = temp_path("unreadable_split");
    fs::create_dir_all(dir.join("baseline")).unwrap();
    let index = dir.join("baseline.json");
    fs::write(&index, "{ not a baseline").unwrap();
    fs::write(dir.join("baseline/kept.identifier.json"), "[]").unwrap();
    let checked = [PathBuf::from("a.php")].into_iter().collect();
    let result = generate_per_identifier(
        &index,
        &results_of("a.php", vec![diag_with("new", "argument.type")]),
        &checked,
    );
    assert!(result.is_err());
    assert!(dir.join("baseline/kept.identifier.json").exists());
    assert_eq!(fs::read_to_string(&index).unwrap(), "{ not a baseline");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_version_1_baseline_merges_entries_that_differed_only_by_location() {
    let path = temp_path("v1_locations");
    fs::write(
        &path,
        r#"[
            {"file": "a.php", "location": "f", "message": "p"},
            {"file": "a.php", "location": "g", "message": "p", "count": 2}
        ]"#,
    )
    .unwrap();
    let mut now = results_of("a.php", vec![diag("p"), diag("p"), diag("p"), diag("p")]);
    let stale = filter(&mut now, &Baseline::load(&path).unwrap());
    assert_eq!(now[&PathBuf::from("a.php")].len(), 1);
    assert!(stale.is_empty());
    let _ = fs::remove_file(&path);
}

#[test]
fn generate_writes_the_current_version() {
    let path = temp_path("version");
    generate_all(&path, &results_of("a.php", vec![diag("p")])).unwrap();
    let written: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(written["version"], 2);
    assert_eq!(written["entries"][0]["message"], "p");
    assert!(written["entries"][0].get("location").is_none());
    let _ = fs::remove_file(&path);
}

#[test]
fn convert_rewrites_a_version_1_index_and_its_includes() {
    let dir = temp_path("convert_dir");
    fs::create_dir_all(dir.join("parts")).unwrap();
    let index = dir.join("baseline.json");
    fs::write(&index, r#"{"includes": ["parts/a.json"]}"#).unwrap();
    fs::write(
        dir.join("parts/a.json"),
        r#"[{"file": "a.php", "location": "f", "message": "p", "identifier": "x.y"}]"#,
    )
    .unwrap();
    assert_eq!(convert(&index).unwrap(), 2);
    let top: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&index).unwrap()).unwrap();
    assert_eq!(top["version"], 2);
    assert_eq!(top["includes"][0], "parts/a.json");
    let part: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(dir.join("parts/a.json")).unwrap()).unwrap();
    assert_eq!(part["version"], 2);
    assert_eq!(part["entries"][0]["identifier"], "x.y");
    assert!(part["entries"][0].get("location").is_none());
    let mut now = results_of("a.php", vec![diag("p")]);
    filter(&mut now, &Baseline::load(&index).unwrap());
    assert!(now[&PathBuf::from("a.php")].is_empty());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn an_unknown_version_is_an_error() {
    let path = temp_path("unknown_version");
    fs::write(&path, r#"{"version": 99, "entries": []}"#).unwrap();
    assert!(Baseline::load(&path).is_err());
    let _ = fs::remove_file(&path);
}
