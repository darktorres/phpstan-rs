//! Composer version constraints over a single concrete version: `||`
//! (or `|`) alternatives of space- or comma-separated conditions, each
//! `^x.y`, `~x.y`, a comparison (`>=`, `>`, `<=`, `<`, `=`, `==`, `!=`,
//! `<>`), a wildcard (`8.*`, `*`), a hyphen range (`8.1 - 8.3`), or an
//! exact version. Stability flags (`@dev`) and `v` prefixes are ignored.

use std::cmp::Ordering;

/// Up to four numeric parts; missing parts are zero.
type Version = [u64; 4];

fn parse_version(text: &str) -> Option<(Version, usize)> {
    let text = text.trim().trim_start_matches(['v', 'V']);
    let text = text.split(['@', '-', '+']).next()?;
    let mut version = [0; 4];
    let mut parts = 0;
    for (i, part) in text.split('.').enumerate() {
        if i >= 4 {
            return None;
        }
        version[i] = part.parse().ok()?;
        parts += 1;
    }
    (parts > 0).then_some((version, parts))
}

/// `version` bumped at part `index` (the rest zeroed).
fn bump(version: Version, index: usize) -> Version {
    let mut out = [0; 4];
    out[..index].copy_from_slice(&version[..index]);
    out[index] = version[index] + 1;
    out
}

enum Condition {
    Range {
        min: Option<(Version, bool)>,
        max: Option<(Version, bool)>,
    },
    NotEqual(Version),
}

impl Condition {
    fn matches(&self, v: &Version) -> bool {
        match self {
            Condition::NotEqual(x) => v != x,
            Condition::Range { min, max } => {
                min.is_none_or(|(m, inclusive)| match v.cmp(&m) {
                    Ordering::Greater => true,
                    Ordering::Equal => inclusive,
                    Ordering::Less => false,
                }) && max.is_none_or(|(m, inclusive)| match v.cmp(&m) {
                    Ordering::Less => true,
                    Ordering::Equal => inclusive,
                    Ordering::Greater => false,
                })
            }
        }
    }
}

fn range(min: Option<(Version, bool)>, max: Option<(Version, bool)>) -> Condition {
    Condition::Range { min, max }
}

fn parse_condition(atom: &str) -> Option<Condition> {
    let atom = atom.trim();
    if atom == "*" || atom.is_empty() {
        return Some(range(None, None));
    }
    for (op, make) in [
        (">=", 0u8),
        ("<=", 1),
        ("<>", 2),
        ("!=", 2),
        ("==", 3),
        (">", 4),
        ("<", 5),
        ("=", 3),
    ] {
        if let Some(rest) = atom.strip_prefix(op) {
            let (version, _) = parse_version(rest)?;
            return Some(match make {
                0 => range(Some((version, true)), None),
                1 => range(None, Some((version, true))),
                2 => Condition::NotEqual(version),
                3 => range(Some((version, true)), Some((version, true))),
                4 => range(Some((version, false)), None),
                _ => range(None, Some((version, false))),
            });
        }
    }
    if let Some(rest) = atom.strip_prefix('^') {
        let (version, parts) = parse_version(rest)?;
        let significant = version[..parts.min(3)]
            .iter()
            .position(|p| *p != 0)
            .unwrap_or(parts.saturating_sub(1).min(2));
        return Some(range(
            Some((version, true)),
            Some((bump(version, significant), false)),
        ));
    }
    if let Some(rest) = atom.strip_prefix('~') {
        let (version, parts) = parse_version(rest)?;
        let index = if parts <= 1 { 0 } else { parts - 2 };
        return Some(range(
            Some((version, true)),
            Some((bump(version, index), false)),
        ));
    }
    if let Some(prefix) = atom.strip_suffix(".*").or_else(|| atom.strip_suffix(".x")) {
        let (version, parts) = parse_version(prefix)?;
        return Some(range(
            Some((version, true)),
            Some((bump(version, parts - 1), false)),
        ));
    }
    // A bare version is exact, as Composer reads it: `8.1` is `8.1.0`.
    let (version, _) = parse_version(atom)?;
    Some(range(Some((version, true)), Some((version, true))))
}

/// The operators that may be separated from their version by a space
/// (`>= 8.1`); the others are written together, as Composer requires.
const COMPARATORS: [&str; 8] = [">=", "<=", "<>", "!=", "==", ">", "<", "="];

/// The conditions of one `||` alternative. Its terms are separated by spaces
/// or commas, except that a comparison operator takes the next term whole.
fn conditions(alternative: &str) -> Option<Vec<Condition>> {
    let mut terms: Vec<String> = Vec::new();
    let mut joins_next = false;
    for term in alternative.split([' ', ',']).filter(|t| !t.is_empty()) {
        if joins_next {
            terms.last_mut()?.push_str(term);
        } else {
            terms.push(term.to_string());
        }
        joins_next = COMPARATORS.contains(&term);
    }
    terms.iter().map(|term| parse_condition(term)).collect()
}

/// Each `||` alternative of `constraint`, as the conditions that must all hold.
fn alternatives(constraint: &str) -> Option<Vec<Vec<Condition>>> {
    constraint
        .split("||")
        .flat_map(|a| a.split('|'))
        .map(|alternative| {
            let alternative = alternative.trim();
            if let Some((low, high)) = alternative.split_once(" - ") {
                let (low, _) = parse_version(low)?;
                let (high_version, high_parts) = parse_version(high)?;
                let max = if high_parts < 3 {
                    (bump(high_version, high_parts - 1), false)
                } else {
                    (high_version, true)
                };
                Some(vec![range(Some((low, true)), Some(max))])
            } else {
                conditions(alternative)
            }
        })
        .collect()
}

/// Whether `version` satisfies `constraint`; `None` when either can't
/// be parsed.
pub fn satisfies(constraint: &str, version: &str) -> Option<bool> {
    let (version, _) = parse_version(version)?;
    Some(
        alternatives(constraint)?
            .iter()
            .any(|conditions| conditions.iter().all(|c| c.matches(&version))),
    )
}

/// Whether some release of the series `major.minor` (any patch) satisfies
/// `constraint`; `None` when it can't be parsed. The series is the closed
/// interval from `major.minor.0` to its last version, and each condition's
/// bounds narrow it.
pub fn allows_series(constraint: &str, major: u64, minor: u64) -> Option<bool> {
    let series = (
        ([major, minor, 0, 0], true),
        ([major, minor, u64::MAX, u64::MAX], true),
    );
    Some(alternatives(constraint)?.iter().any(|conditions| {
        let mut low = series.0;
        let mut high = series.1;
        let mut excluded = Vec::new();
        for condition in conditions {
            match condition {
                Condition::Range { min, max } => {
                    if let Some(min) = *min {
                        low = tighter_low(low, min);
                    }
                    if let Some(max) = *max {
                        high = tighter_high(high, max);
                    }
                }
                Condition::NotEqual(version) => excluded.push(*version),
            }
        }
        admits(low, high, &excluded)
    }))
}

/// The higher of two lower bounds; at a shared version the exclusive one.
fn tighter_low(a: (Version, bool), b: (Version, bool)) -> (Version, bool) {
    match a.0.cmp(&b.0) {
        Ordering::Less => b,
        Ordering::Greater => a,
        Ordering::Equal => (a.0, a.1 && b.1),
    }
}

/// The lower of two upper bounds; at a shared version the exclusive one.
fn tighter_high(a: (Version, bool), b: (Version, bool)) -> (Version, bool) {
    match a.0.cmp(&b.0) {
        Ordering::Less => a,
        Ordering::Greater => b,
        Ordering::Equal => (a.0, a.1 && b.1),
    }
}

/// The version after `version` in lexicographic order over its four parts.
fn next(version: Version) -> Option<Version> {
    let mut out = version;
    out[3] = version[3].checked_add(1)?;
    Some(out)
}

/// Whether some version between `low` and `high`, with their inclusions, is
/// not in `excluded`. The versions in that interval are taken in order from
/// its first: among `excluded.len() + 1` of them one is not excluded.
fn admits(low: (Version, bool), high: (Version, bool), excluded: &[Version]) -> bool {
    let mut candidate = if low.1 { Some(low.0) } else { next(low.0) };
    for _ in 0..=excluded.len() {
        let Some(version) = candidate else {
            return false;
        };
        let below_high = match version.cmp(&high.0) {
            Ordering::Less => true,
            Ordering::Equal => high.1,
            Ordering::Greater => false,
        };
        if !below_high {
            return false;
        }
        if !excluded.contains(&version) {
            return true;
        }
        candidate = next(version);
    }
    false
}

#[cfg(test)]
mod tests {
    use super::{allows_series, satisfies};

    #[test]
    fn series_admitted_by_a_window_or_an_exact_bound() {
        let cases = [
            (">=8.0.2, <8.0.15", 8, 0, true),
            (">=8.0.2, <8.0.15", 8, 1, false),
            (">= 8.0.2 <8.0.15", 8, 0, true),
            ("8.1", 8, 1, true),
            ("8.1", 8, 2, false),
            ("<8.0", 8, 0, false),
            ("<8.0", 7, 4, true),
            ("<=8.1", 8, 1, true),
            (">8.1", 8, 1, true),
            (">8.1 <8.1.1", 8, 1, true),
            ("=8.1.0, !=8.1.0", 8, 1, false),
            ("!=8.1.0", 8, 1, true),
            ("^8.1", 8, 4, true),
            ("^8.1", 9, 0, false),
            ("~8.1", 8, 9, true),
            ("~8.1", 9, 0, false),
            ("8.1 - 8.3", 8, 3, true),
            ("8.1 - 8.3", 8, 4, false),
            ("*", 5, 6, true),
            (">= 7", 7, 4, true),
        ];
        for (constraint, major, minor, expected) in cases {
            assert_eq!(
                allows_series(constraint, major, minor),
                Some(expected),
                "{constraint} vs {major}.{minor}"
            );
        }
        assert_eq!(allows_series("not a constraint", 8, 1), None);
    }

    #[test]
    fn composer_constraint_forms() {
        let cases = [
            ("^8.3", "8.3.0", true),
            ("^8.3", "8.4.1", true),
            ("^8.3", "8.1.0", false),
            ("^8.3", "9.0.0", false),
            ("^0.3", "0.3.9", true),
            ("^0.3", "0.4.0", false),
            ("~8.1", "8.4", true),
            ("~8.1", "9.0", false),
            ("~8.1.2", "8.1.9", true),
            ("~8.1.2", "8.2.0", false),
            (">=8.1 <8.4", "8.3.10", true),
            (">=8.1,<8.4", "8.4.0", false),
            ("^7.4 || ^8.0", "7.4.33", true),
            ("^7.4 | ^8.0", "8.2", true),
            ("^7.4 || ^8.0", "7.3.0", false),
            ("8.1.*", "8.1.27", true),
            ("8.1.*", "8.2.0", false),
            ("8.1", "8.1.0", true),
            ("8.1", "8.1.5", false),
            (">= 8.1", "8.0.9", false),
            (">= 8.1 <8.4", "8.3.10", true),
            ("8.1.5", "8.1.5", true),
            ("8.1.5", "8.1.6", false),
            ("8.1 - 8.3", "8.3.9", true),
            ("8.1 - 8.3", "8.4.0", false),
            ("*", "5.6", true),
            ("!=8.2.0", "8.2.0", false),
            (">8.1", "8.1.0", false),
            ("^8.1@dev", "8.2", true),
        ];
        for (constraint, version, expected) in cases {
            assert_eq!(
                satisfies(constraint, version),
                Some(expected),
                "{constraint} vs {version}"
            );
        }
        assert_eq!(satisfies("not a constraint", "8.1"), None);
    }
}
