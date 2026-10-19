//! The vendor directory of each case of the probed table. The table,
//! `data/vendor_dir_cases.json`, is what `tools/composer-vendor-dir/probe.php`
//! records from the vendored Composer's `Config`: the manifest's `vendor-dir`,
//! `COMPOSER_VENDOR_DIR`, references to other config keys, `$VAR` and `%VAR%`,
//! and `~`. The project is `/ROOT` and the user's home directory `/HOME` there.
//!
//! Composer does not normalise a path. A resolved directory here is canonical
//! when it exists and lexically normalised otherwise, so each expectation is
//! normalised the same way.

use super::helpers::*;
use crate::canonical::canonicalize;
use crate::dir_listings::DirListings;
use crate::layout::Layout;
use crate::manifest::lexically_normalized;
use serde_json::{Map, Value, json};
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

/// The table, as the probe wrote it.
const TABLE: &str = include_str!("data/vendor_dir_cases.json");

/// An environment whose only variables are `pairs`.
fn environment(pairs: Vec<(&'static str, String)>) -> impl Fn(&OsStr) -> Option<OsString> + Sync {
    move |asked| {
        pairs
            .iter()
            .find(|(name, _)| OsStr::new(name) == asked)
            .map(|(_, value)| OsString::from(value))
    }
}

/// One row of the table.
struct Case {
    name: String,
    /// The manifest's `config` object: the case's config keys and `vendor-dir`.
    config: Map<String, Value>,
    /// The value of `COMPOSER_VENDOR_DIR`, when the case sets it.
    env: Option<String>,
    /// The directory Composer's `Config` answered, with `/ROOT` and `/HOME`.
    resolved: String,
}

fn cases() -> Vec<Case> {
    let rows: Vec<Value> = serde_json::from_str(TABLE).unwrap();
    rows.iter()
        .map(|row| {
            let text = |key: &str| row[key].as_str().map(str::to_owned);
            let mut config = match &row["config"] {
                Value::Object(map) => map.clone(),
                _ => Map::new(),
            };
            if let Some(dir) = text("vendor-dir") {
                config.insert("vendor-dir".to_owned(), Value::String(dir));
            }
            Case {
                name: text("name").unwrap(),
                config,
                env: text("env"),
                resolved: text("resolved").unwrap(),
            }
        })
        .collect()
}

/// The directory `resolved` names for a project at `root`, normalised as the
/// layout normalises a directory that does not exist.
fn expected(resolved: &str, root: &Path) -> PathBuf {
    let path = if let Some(rest) = resolved.strip_prefix("/ROOT") {
        root.join(rest.trim_start_matches('/'))
    } else if let Some(rest) = resolved.strip_prefix("/HOME") {
        root.join("home").join(rest.trim_start_matches('/'))
    } else {
        PathBuf::from(resolved)
    };
    lexically_normalized(&path)
}

#[test]
fn each_probed_case_resolves_its_vendor_directory_as_composer_does() {
    for case in cases() {
        let dir = TempDir::new(&case.name);
        let root = canonicalize(dir.path()).unwrap();
        let mut pairs = vec![
            ("HOME", root.join("home").to_string_lossy().into_owned()),
            ("PROBE_LIB", root.join("lib").to_string_lossy().into_owned()),
        ];
        if let Some(env) = &case.env {
            pairs.push(("COMPOSER_VENDOR_DIR", env.clone()));
        }
        // The manifest is written even when the case has no `vendor-dir`: an
        // environment-only case means that the manifest names no directory,
        // and Composer's answer does not depend on the manifest's presence.
        dir.write(
            "composer.json",
            &json!({ "config": Value::Object(case.config.clone()) }).to_string(),
        );
        let variables = environment(pairs);
        let listings = DirListings::default();
        let location = Layout::new(&listings, &variables).vendor_location(&root);
        assert_eq!(
            location.resolved,
            expected(&case.resolved, &root),
            "case `{}`",
            case.name
        );
    }
}
