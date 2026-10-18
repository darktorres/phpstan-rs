//! An edit that moves what the include rule is given re-checks the other
//! files, and the server publishes what changed in them.

use super::super::*;
use super::helpers::*;

const SETS_THE_INCLUDE_PATH: &str = "<?php\nset_include_path(__DIR__ . '/lib');\n";

/// Whether the diagnostics `publish` holds include a missing file.
fn reports_a_missing_file(publish: &serde_json::Value) -> bool {
    diagnostics_of(publish).iter().any(|d| {
        d["code"]
            .as_str()
            .is_some_and(|c| c.ends_with(".fileNotFound"))
    })
}

#[test]
fn an_edit_that_sets_the_include_path_clears_the_report_in_another_file_and_taking_it_out_restores_it()
 {
    let dir = TempDir::new("lsp_include_environment");
    dir.write(
        "phpstan-rs.toml",
        "include_path = \".\"\nworking_dirs = [\".\"]\n",
    );
    let main = dir.write("main.php", "<?php\nrequire 'present.php';\n");
    let bootstrap = dir.write("bootstrap.php", "<?php\n");
    dir.write("lib/present.php", "<?php\n");
    let main_uri = path_to_uri(&main).to_string();
    let (mut server, initial) = loaded_server(&dir);
    let published = publishes_for(&initial, &main_uri);
    assert!(
        published.iter().any(|p| reports_a_missing_file(p)),
        "present.php is on no include path yet, got: {initial:?}"
    );

    let set = as_values(server.handle_debounced_change(
        &bootstrap,
        &path_to_uri(&bootstrap),
        SETS_THE_INCLUDE_PATH,
        2,
        &mut || false,
    ));
    let published = publishes_for(&set, &main_uri);
    assert_eq!(published.len(), 1, "main.php is published, got: {set:?}");
    assert!(
        !reports_a_missing_file(published[0]),
        "bootstrap.php puts lib on the include path, got: {set:?}"
    );

    let unset = as_values(server.handle_debounced_change(
        &bootstrap,
        &path_to_uri(&bootstrap),
        "<?php\n",
        3,
        &mut || false,
    ));
    let published = publishes_for(&unset, &main_uri);
    assert_eq!(published.len(), 1, "main.php is published, got: {unset:?}");
    assert!(
        reports_a_missing_file(published[0]),
        "lib is off the include path again, got: {unset:?}"
    );
}
