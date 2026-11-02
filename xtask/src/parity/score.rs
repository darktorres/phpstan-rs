//! Turning compared cases into results: exclusions, the `--filter` glob,
//! per-suite tallies, `parity/score.json` and the per-case results file.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

use super::SUITES;
use super::oracle::{Oracle, hex};
use crate::files::{self, glob};

/// A suite's verdict on one case, before exclusions apply.
pub(crate) enum Verdict {
    Pass,
    /// The two sides differ; the text says how.
    Fail(String),
    /// phpstan-rs crashed or printed nothing for the case.
    Error(String),
}

/// One case as a suite compared it.
pub(crate) struct Case {
    pub(crate) id: String,
    /// The group the case is counted under in `score.json`.
    pub(crate) group: String,
    pub(crate) verdict: Verdict,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Outcome {
    Pass,
    Fail,
    Excluded,
    Error,
}

impl Outcome {
    fn name(self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::Fail => "fail",
            Self::Excluded => "excluded",
            Self::Error => "error",
        }
    }
}

pub(crate) struct CaseResult {
    suite: String,
    id: String,
    group: String,
    outcome: Outcome,
    detail: String,
}

/// The rows of `parity/exclusions.md`: a suite and a case-id glob each.
pub(crate) struct Exclusions {
    rows: Vec<(String, String)>,
}

impl Exclusions {
    pub(crate) fn read(path: &Path) -> Result<Self, String> {
        let text =
            fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
        Self::parse(&text).map_err(|error| format!("{}: {error}", path.display()))
    }

    /// Reads the table rows; the header and its separator are skipped.
    fn parse(text: &str) -> Result<Self, String> {
        let mut rows = Vec::new();
        for (number, line) in text.lines().enumerate() {
            let Some(inner) = line.trim().strip_prefix('|') else {
                continue;
            };
            let cells: Vec<&str> = inner
                .trim_end_matches('|')
                .split('|')
                .map(str::trim)
                .collect();
            if cells.first() == Some(&"suite")
                || cells.iter().all(|c| c.chars().all(|ch| ch == '-'))
            {
                continue;
            }
            let [suite, test, kind, reason] = cells.as_slice() else {
                return Err(format!("line {}: an exclusion has four cells", number + 1));
            };
            if !SUITES.contains(suite) {
                return Err(format!("line {}: unknown suite {suite}", number + 1));
            }
            if kind.is_empty() || reason.is_empty() {
                return Err(format!(
                    "line {}: an exclusion needs a kind and a reason",
                    number + 1
                ));
            }
            rows.push(((*suite).to_owned(), test.trim_matches('`').to_owned()));
        }
        Ok(Self { rows })
    }

    fn excludes(&self, suite: &str, id: &str) -> bool {
        self.rows
            .iter()
            .any(|(s, pattern)| s == suite && glob(pattern, id))
    }
}

/// Applies the filter and the exclusions to a suite's cases.
pub(crate) fn judge(
    suite: &str,
    cases: Vec<Case>,
    exclusions: &Exclusions,
    filter: Option<&str>,
) -> Vec<CaseResult> {
    cases
        .into_iter()
        .filter(|case| filter.is_none_or(|pattern| glob(pattern, &case.id)))
        .map(|case| {
            let (outcome, detail) = if exclusions.excludes(suite, &case.id) {
                (Outcome::Excluded, String::new())
            } else {
                match case.verdict {
                    Verdict::Pass => (Outcome::Pass, String::new()),
                    Verdict::Fail(detail) => (Outcome::Fail, detail),
                    Verdict::Error(detail) => (Outcome::Error, detail),
                }
            };
            CaseResult {
                suite: suite.to_owned(),
                id: case.id,
                group: case.group,
                outcome,
                detail,
            }
        })
        .collect()
}

#[derive(Default)]
pub(crate) struct Tally {
    counts: BTreeMap<&'static str, u64>,
    groups: BTreeMap<String, (u64, u64)>,
    /// The ids of the passing cases, so equal totals with a different set
    /// of passing cases still read as a change.
    passing: Vec<String>,
}

impl Tally {
    /// SHA-256 of the passing ids, sorted, one per line.
    fn passing_sha256(&self) -> String {
        let mut ids: Vec<&str> = self.passing.iter().map(String::as_str).collect();
        ids.sort_unstable();
        let mut hasher = Sha256::new();
        for id in ids {
            hasher.update(id.as_bytes());
            hasher.update(b"\n");
        }
        hex(&hasher.finalize())
    }
}

/// Per-suite tallies, keyed by suite name.
pub(crate) fn tally(results: &[CaseResult]) -> BTreeMap<String, Tally> {
    let mut tallies: BTreeMap<String, Tally> = BTreeMap::new();
    for result in results {
        let tally = tallies.entry(result.suite.clone()).or_default();
        *tally.counts.entry(result.outcome.name()).or_default() += 1;
        if result.outcome == Outcome::Pass {
            tally.passing.push(result.id.clone());
        }
        if result.outcome != Outcome::Excluded {
            let group = tally.groups.entry(result.group.clone()).or_default();
            group.0 += u64::from(result.outcome == Outcome::Pass);
            group.1 += 1;
        }
    }
    tallies
}

/// `lexer 0/N parser 0/N ...`, where N leaves out excluded cases.
pub(crate) fn summary(tallies: &BTreeMap<String, Tally>) -> String {
    SUITES
        .iter()
        .filter_map(|suite| {
            let tally = tallies.get(*suite)?;
            let count = |name| tally.counts.get(name).copied().unwrap_or(0);
            let counted = count("pass") + count("fail") + count("error");
            Some(format!("{suite} {}/{counted}", count("pass")))
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Rewrites the suites in `tallies` inside `parity/score.json`, keeping the
/// others. Keys are written in sorted order.
pub(crate) fn write_score(path: &Path, tallies: &BTreeMap<String, Tally>) -> Result<(), String> {
    let mut score: Map<String, Value> = match fs::read_to_string(path) {
        Ok(text) => {
            serde_json::from_str(&text).map_err(|error| format!("{}: {error}", path.display()))?
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Map::new(),
        Err(error) => return Err(format!("{}: {error}", path.display())),
    };
    for (suite, tally) in tallies {
        let mut counts = Map::new();
        for name in ["pass", "fail", "excluded", "error"] {
            counts.insert(
                name.to_owned(),
                json!(tally.counts.get(name).copied().unwrap_or(0)),
            );
        }
        let groups: Map<String, Value> = tally
            .groups
            .iter()
            .map(|(group, (pass, total))| (group.clone(), json!({ "pass": pass, "total": total })))
            .collect();
        counts.insert("groups".to_owned(), Value::Object(groups));
        counts.insert("passing_sha256".to_owned(), json!(tally.passing_sha256()));
        score.insert(suite.clone(), Value::Object(counts));
    }
    let mut text = serde_json::to_string_pretty(&score).map_err(|error| error.to_string())?;
    text.push('\n');
    files::write_atomically(path, text.as_bytes())
}

/// Writes every case's result to `<cache>/results/<commit>.jsonl`, sorted by
/// suite and id, and returns the path. A run of only some suites or cases
/// writes `<commit>.partial.jsonl` instead, so it never replaces a full run.
pub(crate) fn write_results(
    oracle: &Oracle,
    root: &Path,
    results: &[CaseResult],
    partial: bool,
) -> Result<PathBuf, String> {
    let head = Command::new("git")
        .arg("-C")
        .arg(root)
        .args([
            "describe",
            "--always",
            "--dirty",
            "--abbrev=40",
            "--exclude=*",
        ])
        .output()
        .map_err(|error| format!("git describe: {error}"))?;
    let head = String::from_utf8_lossy(&head.stdout).trim().to_owned();
    let mut order: Vec<&CaseResult> = results.iter().collect();
    order.sort_by(|a, b| (&a.suite, &a.id).cmp(&(&b.suite, &b.id)));
    let mut out = String::new();
    for result in order {
        let line = json!({
            "suite": result.suite,
            "id": result.id,
            "outcome": result.outcome.name(),
            "detail": result.detail,
        });
        out.push_str(&line.to_string());
        out.push('\n');
    }
    let dir = oracle.cache.join("results");
    fs::create_dir_all(&dir).map_err(|error| format!("{}: {error}", dir.display()))?;
    let suffix = if partial { ".partial" } else { "" };
    let path = dir.join(format!("{head}{suffix}.jsonl"));
    files::write_atomically(&path, out.as_bytes())?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exclusions_skip_the_header_and_need_a_reason() {
        let table = "| suite | test | kind | reason |\n|---|---|---|---|\n| lexer | `tests/x.php*` | won't-port | because |\n";
        let exclusions = Exclusions::parse(table).unwrap();
        assert!(exclusions.excludes("lexer", "tests/x.php#cr"));
        assert!(!exclusions.excludes("lexer", "tests/y.php"));
        assert!(Exclusions::parse("| lexer | x | won't-port | |\n").is_err());
        assert!(Exclusions::parse("| nosuch | x | k | r |\n").is_err());
    }

    #[test]
    fn the_passing_digest_follows_the_set_of_passing_cases() {
        let digest = |passing: &[(&str, bool)]| {
            let cases = passing
                .iter()
                .map(|&(id, pass)| Case {
                    id: id.to_owned(),
                    group: "g".to_owned(),
                    verdict: if pass {
                        Verdict::Pass
                    } else {
                        Verdict::Fail(String::new())
                    },
                })
                .collect();
            let results = judge("lexer", cases, &Exclusions { rows: Vec::new() }, None);
            tally(&results)["lexer"].passing_sha256()
        };
        assert_eq!(
            digest(&[("a", true), ("b", false)]),
            digest(&[("b", false), ("a", true)])
        );
        assert_ne!(
            digest(&[("a", true), ("b", false)]),
            digest(&[("a", false), ("b", true)])
        );
    }

    #[test]
    fn excluded_cases_are_not_counted() {
        let cases = vec![
            Case {
                id: "a".to_owned(),
                group: "g".to_owned(),
                verdict: Verdict::Pass,
            },
            Case {
                id: "b".to_owned(),
                group: "g".to_owned(),
                verdict: Verdict::Fail("x".to_owned()),
            },
            Case {
                id: "c".to_owned(),
                group: "g".to_owned(),
                verdict: Verdict::Fail("x".to_owned()),
            },
        ];
        let exclusions = Exclusions::parse("| lexer | c | won't-port | r |\n").unwrap();
        let results = judge("lexer", cases, &exclusions, None);
        assert_eq!(summary(&tally(&results)), "lexer 1/2");
    }
}
