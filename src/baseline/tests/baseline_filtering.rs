//! Generating and loading baseline files, and filtering diagnostics against them.

use super::super::*;
use super::helpers::*;

#[test]
fn generate_then_load_filters_out_every_recorded_diagnostic() {
    let mut results = HashMap::new();
    results.insert(
        PathBuf::from("a.php"),
        vec![
            diag("Foo::bar", "undefined variable $x"),
            diag("Foo::baz", "undefined variable $y"),
        ],
    );
    results.insert(
        PathBuf::from("b.php"),
        vec![diag("<main>", "undefined variable $z")],
    );

    let path = temp_path("roundtrip");
    let count = generate(&path, &results).unwrap();
    assert_eq!(count, 3);

    let baseline = Baseline::load(&path).unwrap();
    filter(&mut results, &baseline);
    assert!(results[&PathBuf::from("a.php")].is_empty());
    assert!(results[&PathBuf::from("b.php")].is_empty());

    let _ = fs::remove_file(&path);
}

#[test]
fn filter_keeps_a_new_diagnostic_not_in_the_baseline() {
    let mut baselined = HashMap::new();
    baselined.insert(
        PathBuf::from("a.php"),
        vec![diag("Foo::bar", "undefined variable $x")],
    );
    let path = temp_path("keep_new");
    generate(&path, &baselined).unwrap();
    let baseline = Baseline::load(&path).unwrap();

    let mut results = HashMap::new();
    results.insert(
        PathBuf::from("a.php"),
        vec![
            diag("Foo::bar", "undefined variable $x"),
            diag("Foo::bar", "undefined variable $newOne"),
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
fn the_same_location_and_message_in_a_different_file_is_not_baselined() {
    // `<main>`'s own generic label is shared across every file's
    // top-level script diagnostics - the file path has to be part of
    // the key, or baselining one file's finding would silently
    // suppress an unrelated file's identical-looking one too.
    let mut baselined = HashMap::new();
    baselined.insert(
        PathBuf::from("a.php"),
        vec![diag("<main>", "undefined variable $x")],
    );
    let path = temp_path("cross_file");
    generate(&path, &baselined).unwrap();
    let baseline = Baseline::load(&path).unwrap();

    let mut results = HashMap::new();
    results.insert(
        PathBuf::from("b.php"),
        vec![diag("<main>", "undefined variable $x")],
    );
    filter(&mut results, &baseline);
    assert_eq!(results[&PathBuf::from("b.php")].len(), 1);

    let _ = fs::remove_file(&path);
}

#[test]
fn generating_twice_over_the_same_results_produces_a_byte_identical_file() {
    let mut results = HashMap::new();
    results.insert(
        PathBuf::from("z.php"),
        vec![diag("Z::m", "one"), diag("Z::m", "two")],
    );
    results.insert(PathBuf::from("a.php"), vec![diag("A::m", "three")]);

    let path = temp_path("deterministic");
    generate(&path, &results).unwrap();
    let first = fs::read_to_string(&path).unwrap();
    generate(&path, &results).unwrap();
    let second = fs::read_to_string(&path).unwrap();
    assert_eq!(first, second);

    let _ = fs::remove_file(&path);
}

#[test]
fn a_differently_spelled_but_identical_path_is_still_matched() {
    let mut baselined = HashMap::new();
    baselined.insert(
        PathBuf::from("./t.php"),
        vec![diag("f", "undefined variable $x")],
    );
    let path = temp_path("path_spelling");
    generate(&path, &baselined).unwrap();
    let baseline = Baseline::load(&path).unwrap();

    let mut results = HashMap::new();
    results.insert(
        PathBuf::from("././t.php"),
        vec![diag("f", "undefined variable $x")],
    );
    filter(&mut results, &baseline);
    assert!(results[&PathBuf::from("././t.php")].is_empty());

    let _ = fs::remove_file(&path);
}

#[test]
fn filter_reports_a_baseline_entry_no_current_diagnostic_matches() {
    let mut baselined = HashMap::new();
    baselined.insert(
        PathBuf::from("a.php"),
        vec![diag("Foo::bar", "undefined variable $x")],
    );
    let path = temp_path("stale_entry");
    generate(&path, &baselined).unwrap();
    let baseline = Baseline::load(&path).unwrap();

    let mut results = HashMap::new();
    results.insert(PathBuf::from("a.php"), vec![]);
    let stale = filter(&mut results, &baseline);
    assert_eq!(stale.len(), 1);
    assert!(stale[0].contains("a.php"));
    assert!(stale[0].contains("undefined variable $x"));

    let _ = fs::remove_file(&path);
}

#[test]
fn filter_reports_nothing_stale_when_every_entry_still_matches() {
    let mut baselined = HashMap::new();
    baselined.insert(
        PathBuf::from("a.php"),
        vec![diag("Foo::bar", "undefined variable $x")],
    );
    let path = temp_path("no_stale_entries");
    generate(&path, &baselined).unwrap();
    let baseline = Baseline::load(&path).unwrap();

    let mut results = baselined;
    let stale = filter(&mut results, &baseline);
    assert!(stale.is_empty());

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

fn results_of(file: &str, diagnostics: Vec<Diagnostic>) -> HashMap<PathBuf, Vec<Diagnostic>> {
    HashMap::from([(PathBuf::from(file), diagnostics)])
}

#[test]
fn a_baseline_entry_absorbs_only_its_recorded_count() {
    let path = temp_path("counts");
    let recorded = results_of(
        "a.php",
        vec![diag("f", "same problem"), diag("f", "same problem")],
    );
    assert_eq!(generate(&path, &recorded).unwrap(), 2);
    let written = fs::read_to_string(&path).unwrap();
    assert!(written.contains("\"count\": 2"), "{written}");

    let mut now = results_of(
        "a.php",
        vec![
            diag("f", "same problem"),
            diag("f", "same problem"),
            diag("f", "same problem"),
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
    generate(
        &path,
        &results_of(
            "a.php",
            vec![diag("f", "p"), diag("f", "p"), diag("f", "p")],
        ),
    )
    .unwrap();
    let mut now = results_of("a.php", vec![diag("f", "p")]);
    let stale = filter(&mut now, &Baseline::load(&path).unwrap());
    assert_eq!(
        stale,
        vec!["a.php:f: p (expected 3 occurrence(s), found 1)"]
    );
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
    let mut now = results_of(
        "a.php",
        vec![diag("f", "p"), diag("f", "p"), diag("f", "p")],
    );
    filter(&mut now, &Baseline::load(&path).unwrap());
    assert_eq!(now[&PathBuf::from("a.php")].len(), 1);
    let _ = fs::remove_file(&path);
}

#[test]
fn identifier_mode_survives_a_reworded_message() {
    let path = temp_path("identifier");
    generate(
        &path,
        &results_of(
            "a.php",
            vec![diag_with("f", "Old wording.", "argument.type")],
        ),
    )
    .unwrap();
    let reworded = || {
        results_of(
            "a.php",
            vec![diag_with("f", "New wording.", "argument.type")],
        )
    };

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

    let mut other_identifier =
        results_of("a.php", vec![diag_with("f", "New wording.", "return.type")]);
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
            diag_with("f", "one", "argument.type"),
            diag_with("f", "two", "argument.type"),
            diag_with("g", "three", "return.type"),
        ],
    );
    fs::create_dir_all(dir.join("baseline")).unwrap();
    fs::write(dir.join("baseline/stale.identifier.json"), "[]").unwrap();
    assert_eq!(generate_per_identifier(&index, &recorded).unwrap(), 3);
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
