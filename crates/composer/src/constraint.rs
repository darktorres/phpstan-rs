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
    let (version, parts) = parse_version(atom)?;
    // `8.1` as an exact constraint means any 8.1.x release.
    if parts < 3 {
        return Some(range(
            Some((version, true)),
            Some((bump(version, parts - 1), false)),
        ));
    }
    Some(range(Some((version, true)), Some((version, true))))
}

/// Whether `version` satisfies `constraint`; `None` when either can't
/// be parsed.
pub fn satisfies(constraint: &str, version: &str) -> Option<bool> {
    let (version, _) = parse_version(version)?;
    let mut any = false;
    for alternative in constraint.split("||").flat_map(|a| a.split('|')) {
        let alternative = alternative.trim();
        let conditions: Vec<Condition> = if let Some((low, high)) = alternative.split_once(" - ") {
            let (low, _) = parse_version(low)?;
            let (high_version, high_parts) = parse_version(high)?;
            let max = if high_parts < 3 {
                (bump(high_version, high_parts - 1), false)
            } else {
                (high_version, true)
            };
            vec![range(Some((low, true)), Some(max))]
        } else {
            alternative
                .split([' ', ','])
                .filter(|a| !a.trim().is_empty())
                .map(parse_condition)
                .collect::<Option<_>>()?
        };
        if conditions.iter().all(|c| c.matches(&version)) {
            any = true;
        }
    }
    Some(any)
}

#[cfg(test)]
mod tests {
    use super::satisfies;

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
            ("8.1", "8.1.5", true),
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
