//! The end-to-end suite: the PHPStan phar on whole projects at every level,
//! against `phpstan-rs` on the same projects and configs.

use std::path::Path;
use std::process::Command;

use serde_json::Value;

use super::jsonl::{by_id, by_id_values, parse_lines, unported};
use super::levels;
use super::oracle::{Oracle, piped};
use super::pins;
use super::score::{Case, Verdict};

pub(crate) const E2E: &str = "e2e";

/// The projects, by their `e2e-<name>` pins.
const PROJECTS: [&str; 2] = ["console", "monolog"];

/// One `<name>\t<dir>\t<paths>\t<exclude>` line per project, the project
/// fetched and installed by its pin.
fn inputs(oracle: &Oracle) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    for name in PROJECTS {
        let dir = pins::project(oracle, name)?;
        let dir = dir
            .to_str()
            .ok_or_else(|| format!("{} is not UTF-8", dir.display()))?;
        let section = format!("e2e-{name}");
        let paths = oracle.pin(&section, "paths")?;
        let exclude = oracle.pin(&section, "exclude")?;
        out.extend_from_slice(format!("{name}\t{dir}\t{paths}\t{exclude}\n").as_bytes());
    }
    Ok(out)
}

pub(crate) fn run(oracle: &Oracle, binary: &Path) -> Result<Vec<Case>, String> {
    let inputs = inputs(oracle)?;
    let phar = pins::phar(oracle)?;
    let phar = phar
        .to_str()
        .ok_or_else(|| format!("{} is not UTF-8", phar.display()))?;
    let expected = by_id(&oracle.extract(E2E, &["e2e.php"], &[phar], &inputs)?)?;
    let output = piped(Command::new(binary).args(["debug", E2E]), &inputs)?;
    let actual = parse_lines(&output)?;
    if let Some(reason) = unported(E2E, &actual) {
        return Ok(expected
            .into_keys()
            .map(|id| Case {
                group: group(&id),
                id,
                verdict: Verdict::Fail(reason.clone()),
            })
            .collect());
    }
    let mut actual = by_id_values(actual)?;
    Ok(expected
        .into_iter()
        .map(|(id, want)| {
            let verdict = match actual.remove(&id) {
                None => Verdict::Error("phpstan-rs printed nothing for this case".to_owned()),
                Some(got) if got == want => Verdict::Pass,
                Some(got) => Verdict::Fail(difference(&want, &got)),
            };
            Case {
                group: group(&id),
                id,
                verdict,
            }
        })
        .collect())
}

/// The project: the id up to its first `/` or `@`.
fn group(id: &str) -> String {
    id.split(['/', '@']).next().unwrap_or_default().to_owned()
}

/// The first field whose values differ: for the findings, how many of each
/// identifier only one side reported; otherwise both values.
fn difference(want: &Value, got: &Value) -> String {
    for field in ["exit", "other"] {
        if want[field] != got[field] {
            return format!("{field}: expected {}, got {}", want[field], got[field]);
        }
    }
    if want["errors"] != got["errors"] {
        return levels::unmatched(&want["errors"], &got["errors"]);
    }
    "the records differ".to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_group_is_the_project() {
        assert_eq!(group("monolog/src/Monolog/Logger.php@3"), "monolog");
        assert_eq!(group("console@10"), "console");
    }

    #[test]
    fn a_difference_names_the_first_field_that_differs() {
        let want = json!({"exit": 1, "other": []});
        let got = json!({"exit": 0, "other": []});
        assert_eq!(difference(&want, &got), "exit: expected 1, got 0");
    }
}
