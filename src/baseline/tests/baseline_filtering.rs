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
