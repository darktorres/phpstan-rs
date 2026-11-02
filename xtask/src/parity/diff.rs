//! `--diff <commit>`: this run's per-case results against those of an
//! earlier run of `<commit>`, case by case (`docs/design/scoreboard.md`,
//! "Output"). A types case whose answer changed is also classified by how
//! the new type relates to the old one, as PHPStan's types judge it.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

use super::oracle::{Oracle, text};
use super::recorded;
use crate::files::{self, glob};

/// One case of a results file.
#[derive(Clone, Debug, PartialEq)]
struct Line {
    outcome: String,
    detail: String,
}

/// A results file, by suite and case id.
type Results = BTreeMap<(String, String), Line>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Change {
    /// Passes now, did not before.
    Fixed,
    /// Passed before, does not now.
    Broken,
    /// Did not pass either time, with another outcome or detail.
    Changed,
    /// A case only this run has.
    Added,
    /// A case only the earlier run has.
    Removed,
}

impl Change {
    const ALL: [Self; 5] = [
        Self::Fixed,
        Self::Broken,
        Self::Changed,
        Self::Added,
        Self::Removed,
    ];

    fn name(self) -> &'static str {
        match self {
            Self::Fixed => "fixed",
            Self::Broken => "broken",
            Self::Changed => "changed",
            Self::Added => "added",
            Self::Removed => "removed",
        }
    }
}

/// A case that differs between the two runs.
#[derive(Debug, PartialEq)]
struct Entry {
    change: Change,
    suite: String,
    id: String,
    before: Option<Line>,
    after: Option<Line>,
}

/// How many cases a class lists on the terminal; the report file has all.
const SHOWN: usize = 10;

/// Compares the results at `head` with `<commit>`'s, over the `suites` this
/// run covered and its `filter`, prints a summary and writes the whole
/// report beside the results.
pub(crate) fn report(
    oracle: &Oracle,
    root: &Path,
    head: &Path,
    commit: &str,
    suites: &[String],
    filter: Option<&str>,
) -> Result<(), String> {
    let base_path = base(oracle, root, commit)?;
    let base = read(&base_path)?;
    let head_results = read(head)?;
    let base_suites: BTreeSet<&str> = base.keys().map(|(suite, _)| suite.as_str()).collect();
    let mut out = format!(
        "diff of {} against {}\n",
        file_stem(head),
        file_stem(&base_path)
    );
    let mut compared = Vec::new();
    for suite in suites {
        if base_suites.contains(suite.as_str()) {
            compared.push(suite.clone());
        } else {
            let _ = writeln!(out, "{suite}: not in the earlier run");
        }
    }
    let entries = compare(&base, &head_results, &compared, filter);
    let relations = relations(oracle, &entries)?;
    for suite in &compared {
        let mine: Vec<&Entry> = entries.iter().filter(|e| &e.suite == suite).collect();
        let counts: Vec<String> = Change::ALL
            .iter()
            .map(|change| {
                let n = mine.iter().filter(|e| e.change == *change).count();
                format!("{n} {}", change.name())
            })
            .collect();
        let _ = write!(out, "{suite}: {}", counts.join(", "));
        let mut by_relation: BTreeMap<&str, usize> = BTreeMap::new();
        for entry in &mine {
            if let Some(relation) = relations.get(&entry.id) {
                *by_relation.entry(relation.as_str()).or_default() += 1;
            }
        }
        if !by_relation.is_empty() {
            let parts: Vec<String> = by_relation
                .iter()
                .map(|(relation, n)| format!("{n} {relation}"))
                .collect();
            let _ = write!(out, " (answers: {})", parts.join(", "));
        }
        out.push('\n');
    }
    let mut full = out.clone();
    for change in Change::ALL {
        let listed: Vec<&Entry> = entries.iter().filter(|e| e.change == change).collect();
        for (n, entry) in listed.iter().enumerate() {
            let line = describe(entry, relations.get(&entry.id).map(String::as_str));
            if n < SHOWN {
                out.push_str(&line);
            }
            full.push_str(&line);
        }
        if listed.len() > SHOWN {
            let _ = writeln!(
                out,
                "  ... and {} more {}",
                listed.len() - SHOWN,
                change.name()
            );
        }
    }
    let path = head.with_file_name(format!(
        "diff-{}-{}.txt",
        file_stem(&base_path),
        file_stem(head)
    ));
    files::write_atomically(&path, full.as_bytes())?;
    print!("{out}");
    println!("diff: {}", path.display());
    Ok(())
}

/// The results file of `commit`: its clean full run if there is one, then a
/// run from a dirty tree at that commit, then a partial run.
fn base(oracle: &Oracle, root: &Path, commit: &str) -> Result<PathBuf, String> {
    let spec = format!("{commit}^{{commit}}");
    let sha = text(Command::new("git").arg("-C").arg(root).args([
        "rev-parse",
        "--verify",
        "--quiet",
        &spec,
    ]))
    .map_err(|_| format!("{commit} names no commit"))?;
    let dir = oracle.cache.join("results");
    let candidates = [
        format!("{sha}.jsonl"),
        format!("{sha}-dirty.jsonl"),
        format!("{sha}.partial.jsonl"),
        format!("{sha}-dirty.partial.jsonl"),
    ];
    candidates
        .iter()
        .map(|name| dir.join(name))
        .find(|path| path.is_file())
        .ok_or_else(|| {
            format!(
                "no results for {sha} in {}; check that commit out and run the scoreboard there first",
                dir.display()
            )
        })
}

fn file_stem(path: &Path) -> String {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    name.strip_suffix(".jsonl").unwrap_or(&name).to_owned()
}

fn read(path: &Path) -> Result<Results, String> {
    let text = fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
    parse(&text).map_err(|error| format!("{}: {error}", path.display()))
}

fn parse(text: &str) -> Result<Results, String> {
    let mut results = Results::new();
    for line in text.lines().filter(|line| !line.is_empty()) {
        let value: Value = serde_json::from_str(line).map_err(|error| error.to_string())?;
        let field = |name: &str| value[name].as_str().unwrap_or_default().to_owned();
        results.insert(
            (field("suite"), field("id")),
            Line {
                outcome: field("outcome"),
                detail: field("detail"),
            },
        );
    }
    Ok(results)
}

/// The cases that differ, in suite and id order. Cases of other suites, and
/// under a filter the cases it does not match, are left out.
fn compare(base: &Results, head: &Results, suites: &[String], filter: Option<&str>) -> Vec<Entry> {
    let wanted = |(suite, id): &(String, String)| {
        suites.contains(suite) && filter.is_none_or(|pattern| glob(pattern, id))
    };
    let mut entries = Vec::new();
    for (key, after) in head.iter().filter(|(key, _)| wanted(key)) {
        let before = base.get(key);
        let change = match before {
            None => Some(Change::Added),
            Some(before) if before == after => None,
            Some(before) => Some(match (before.outcome == "pass", after.outcome == "pass") {
                (false, true) => Change::Fixed,
                (true, false) => Change::Broken,
                _ => Change::Changed,
            }),
        };
        if let Some(change) = change {
            entries.push(Entry {
                change,
                suite: key.0.clone(),
                id: key.1.clone(),
                before: before.cloned(),
                after: Some(after.clone()),
            });
        }
    }
    for (key, before) in base.iter().filter(|(key, _)| wanted(key)) {
        if !head.contains_key(key) {
            entries.push(Entry {
                change: Change::Removed,
                suite: key.0.clone(),
                id: key.1.clone(),
                before: Some(before.clone()),
                after: None,
            });
        }
    }
    entries.sort_by(|a, b| (&a.suite, &a.id).cmp(&(&b.suite, &b.id)));
    entries
}

/// The old and new answers of a types case whose answer changed, when both
/// are types: a passing case answered what it expected.
fn answers(entry: &Entry) -> Option<(String, String)> {
    if entry.suite != recorded::TYPES || entry.id.ends_with(":variableCertainty") {
        return None;
    }
    let before = entry.before.as_ref()?;
    let after = entry.after.as_ref()?;
    let (want_before, got_before) = recorded::expected_and_actual(&before.detail);
    let (want_after, got_after) = recorded::expected_and_actual(&after.detail);
    let want = want_after.or(want_before);
    let answer = |line: &Line, got: Option<Value>| {
        if line.outcome == "pass" {
            want.clone()
        } else {
            got
        }
    };
    let old = answer(before, got_before)?;
    let new = answer(after, got_after)?;
    match (old, new) {
        (Value::String(old), Value::String(new)) if old != new => Some((old, new)),
        _ => None,
    }
}

/// The relation of each changed types answer to the old one, by case id.
fn relations(oracle: &Oracle, entries: &[Entry]) -> Result<BTreeMap<String, String>, String> {
    let pairs: Vec<(&str, (String, String))> = entries
        .iter()
        .filter_map(|entry| Some((entry.id.as_str(), answers(entry)?)))
        .collect();
    if pairs.is_empty() {
        return Ok(BTreeMap::new());
    }
    let mut input = String::new();
    for (_, pair) in &pairs {
        input.push_str(&serde_json::to_string(&[&pair.0, &pair.1]).map_err(|e| e.to_string())?);
        input.push('\n');
    }
    let work = oracle.cache.join("relation");
    let output = oracle.php("relation.php", &[], input.as_bytes(), &work)?;
    let output = String::from_utf8_lossy(&output);
    let lines: Vec<&str> = output.lines().collect();
    if lines.len() != pairs.len() {
        return Err(format!(
            "relation.php answered {} of {} pairs",
            lines.len(),
            pairs.len()
        ));
    }
    Ok(pairs
        .into_iter()
        .zip(lines)
        .map(|((id, _), relation)| (id.to_owned(), relation.to_owned()))
        .collect())
}

/// One report line: the change, the suite, the case, the relation of a
/// types answer, and the outcome and detail before and after.
fn describe(entry: &Entry, relation: Option<&str>) -> String {
    let side = |line: &Option<Line>| match line {
        None => "-".to_owned(),
        Some(line) if line.detail.is_empty() => line.outcome.clone(),
        Some(line) => format!("{}: {}", line.outcome, shorten(&line.detail)),
    };
    let relation = relation.map(|r| format!(" ({r})")).unwrap_or_default();
    format!(
        "{} {} {}{relation}\n  before: {}\n  after:  {}\n",
        entry.change.name(),
        entry.suite,
        entry.id,
        side(&entry.before),
        side(&entry.after),
    )
}

/// `detail` cut to 300 characters.
fn shorten(detail: &str) -> String {
    match detail.char_indices().nth(300) {
        Some((at, _)) => format!("{}...", &detail[..at]),
        None => detail.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(outcome: &str, detail: &str) -> Line {
        Line {
            outcome: outcome.to_owned(),
            detail: detail.to_owned(),
        }
    }

    fn results(rows: &[(&str, &str, &str, &str)]) -> Results {
        rows.iter()
            .map(|(suite, id, outcome, detail)| {
                (
                    ((*suite).to_owned(), (*id).to_owned()),
                    line(outcome, detail),
                )
            })
            .collect()
    }

    #[test]
    fn each_kind_of_change_is_classified() {
        let base = results(&[
            ("lexer", "a", "fail", "x"),
            ("lexer", "b", "pass", ""),
            ("lexer", "c", "fail", "x"),
            ("lexer", "d", "fail", "x"),
            ("lexer", "gone", "fail", "x"),
            ("parser", "p", "fail", "x"),
        ]);
        let head = results(&[
            ("lexer", "a", "pass", ""),
            ("lexer", "b", "fail", "y"),
            ("lexer", "c", "fail", "y"),
            ("lexer", "d", "fail", "x"),
            ("lexer", "new", "fail", "x"),
            ("parser", "p", "pass", ""),
        ]);
        let found: Vec<(Change, String)> = compare(&base, &head, &["lexer".to_owned()], None)
            .into_iter()
            .map(|entry| (entry.change, entry.id))
            .collect();
        let want = [
            (Change::Fixed, "a"),
            (Change::Broken, "b"),
            (Change::Changed, "c"),
            (Change::Removed, "gone"),
            (Change::Added, "new"),
        ]
        .map(|(change, id)| (change, id.to_owned()));
        assert_eq!(found, want);
    }

    #[test]
    fn a_filter_limits_both_sides() {
        let base = results(&[
            ("lexer", "x/gone", "fail", ""),
            ("lexer", "y/gone", "fail", ""),
        ]);
        let head = results(&[]);
        let found = compare(&base, &head, &["lexer".to_owned()], Some("x/*"));
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].id, "x/gone");
    }

    #[test]
    fn a_types_answer_is_read_from_the_detail_or_the_expectation() {
        let entry = Entry {
            change: Change::Fixed,
            suite: "types".to_owned(),
            id: "f.php:3:1:type".to_owned(),
            before: Some(line("fail", r#"expected "int", got "mixed""#)),
            after: Some(line("pass", "")),
        };
        assert_eq!(
            answers(&entry),
            Some(("mixed".to_owned(), "int".to_owned())),
            "a passing case answered what it expected"
        );
        let entry = Entry {
            change: Change::Changed,
            before: Some(line("fail", r#"expected "int", got "mixed""#)),
            after: Some(line("fail", r#"expected "int", got "int|string""#)),
            ..entry
        };
        assert_eq!(
            answers(&entry),
            Some(("mixed".to_owned(), "int|string".to_owned()))
        );
        let certainty = Entry {
            id: "f.php:3:1:variableCertainty".to_owned(),
            ..entry
        };
        assert_eq!(answers(&certainty), None);
    }

    #[test]
    fn a_line_is_read_back() {
        let text = r#"{"detail":"d","id":"i","outcome":"fail","suite":"lexer"}"#;
        assert_eq!(
            parse(text).unwrap(),
            results(&[("lexer", "i", "fail", "d")])
        );
    }
}
