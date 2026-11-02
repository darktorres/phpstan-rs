//! The level oracle: the PHPStan phar on each of PHPStan's rule data files,
//! each its own project, at every level, against `phpstan-rs` on the same
//! projects.

use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

use serde_json::Value;

use super::jsonl::{by_id, by_id_values, parse_lines, unported};
use super::oracle::{Oracle, piped};
use super::pins;
use super::score::{Case, Verdict};
use crate::files;

pub(crate) const LEVELS: &str = "levels";

/// One `<id>\t<file>` line per `.php` file under a `data` directory of
/// `tests/PHPStan/Rules`, in byte order of the id, its path in phpstan-src.
fn inputs(oracle: &Oracle) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    for id in files::php_files(&oracle.src, &oracle.src.join("tests/PHPStan/Rules"))? {
        if !id.split('/').any(|part| part == "data") {
            continue;
        }
        let file = oracle.src.join(&id);
        let file = file
            .to_str()
            .ok_or_else(|| format!("{} is not UTF-8", file.display()))?;
        out.extend_from_slice(format!("{id}\t{file}\n").as_bytes());
    }
    Ok(out)
}

pub(crate) fn run(oracle: &Oracle, binary: &Path) -> Result<Vec<Case>, String> {
    let inputs = inputs(oracle)?;
    let phar = pins::phar(oracle)?;
    let phar = phar
        .to_str()
        .ok_or_else(|| format!("{} is not UTF-8", phar.display()))?;
    let expected = by_id(&oracle.extract(LEVELS, &["levels.php"], &[phar], &inputs)?)?;
    let output = piped(Command::new(binary).args(["debug", LEVELS]), &inputs)?;
    let actual = parse_lines(&output)?;
    if let Some(reason) = unported(LEVELS, &actual) {
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
                Some(got) => compare(&want, &got),
            };
            Case {
                group: group(&id),
                id,
                verdict,
            }
        })
        .collect())
}

/// The rule family: the directory under `tests/PHPStan/Rules`.
fn group(id: &str) -> String {
    id.split('/').nth(3).unwrap_or_default().to_owned()
}

/// Equal exit codes, findings and other errors pass. Otherwise the reason
/// names the first field that differs, and for the findings, how many of
/// each identifier only one side reported.
fn compare(want: &Value, got: &Value) -> Verdict {
    if want["exit"] != got["exit"] {
        return Verdict::Fail(format!(
            "exit: expected {}, got {}",
            want["exit"], got["exit"]
        ));
    }
    if want["errors"] != got["errors"] {
        return Verdict::Fail(unmatched(&want["errors"], &got["errors"]));
    }
    if want["other"] != got["other"] {
        return Verdict::Fail(format!(
            "other errors: expected {}, got {}",
            want["other"], got["other"]
        ));
    }
    Verdict::Pass
}

/// `<identifier>: <n> PHPStan-only, <m> ours-only` for each identifier
/// whose findings differ, matching findings equal in every field once each.
fn unmatched(want: &Value, got: &Value) -> String {
    let empty = Vec::new();
    let mut ours: Vec<&Value> = got.as_array().unwrap_or(&empty).iter().collect();
    let mut counts: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    let identifier = |finding: &Value| finding["identifier"].as_str().unwrap_or("-").to_owned();
    for finding in want.as_array().unwrap_or(&empty) {
        match ours.iter().position(|candidate| *candidate == finding) {
            Some(index) => {
                ours.remove(index);
            }
            None => counts.entry(identifier(finding)).or_default().0 += 1,
        }
    }
    for finding in ours {
        counts.entry(identifier(finding)).or_default().1 += 1;
    }
    let parts: Vec<String> = counts
        .into_iter()
        .map(|(id, (phpstan, ours))| format!("{id}: {phpstan} PHPStan-only, {ours} ours-only"))
        .collect();
    if parts.is_empty() {
        "findings: the same, in another order".to_owned()
    } else {
        format!("findings: {}", parts.join("; "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_group_is_the_rule_family() {
        assert_eq!(
            group("tests/PHPStan/Rules/Classes/data/bug-1.php@3"),
            "Classes"
        );
    }

    #[test]
    fn unmatched_findings_are_counted_per_identifier() {
        let a = json!({"line": 1, "identifier": "a", "message": "m"});
        let b = json!({"line": 2, "identifier": "b", "message": "m"});
        let c = json!({"line": 3, "identifier": "b", "message": "n"});
        let want = json!({"exit": 1, "errors": [a, b], "other": []});
        let got = json!({"exit": 1, "errors": [a, c, c], "other": []});
        let Verdict::Fail(reason) = compare(&want, &got) else {
            panic!("the findings differ");
        };
        assert_eq!(reason, "findings: b: 1 PHPStan-only, 2 ours-only");
        assert!(matches!(compare(&want, &want), Verdict::Pass));
    }
}
