//! Relating two recorded type dumps binding by binding: each binding
//! whose type text changed is parsed back and related both ways, so a
//! drift report lists what widened, what can't be compared, and what
//! stopped or started being bound, apart from what only narrowed.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use rustc_hash::FxHashMap;
use types::Relation;

use super::TemplateScope;

/// How a changed type relates to the one before it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Verdict {
    Equal,
    Narrower,
    Wider,
    Incomparable,
    Unparsed,
}

impl Verdict {
    fn name(self) -> &'static str {
        match self {
            Verdict::Equal => "equal",
            Verdict::Narrower => "narrower",
            Verdict::Wider => "wider",
            Verdict::Incomparable => "incomparable",
            Verdict::Unparsed => "unparsed",
        }
    }

    /// Whether the change needs no reading: the new type is the old one
    /// or within it.
    fn is_gain(self) -> bool {
        matches!(self, Verdict::Equal | Verdict::Narrower)
    }
}

/// Which of a binding's two types changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Facet {
    /// The PHPDoc (inferred) type.
    Doc,
    /// The native type, which inference changes shouldn't move.
    Native,
}

/// The drift between two dumps: counts per facet and verdict, and each
/// change that isn't a gain grouped by its masked `old -> new` pattern.
#[derive(Debug, Default)]
pub struct Drift {
    pub unchanged: usize,
    pub counts: BTreeMap<(Facet, Verdict), usize>,
    pub groups: BTreeMap<(Facet, Verdict, String), Vec<String>>,
    pub added: BTreeMap<String, Vec<String>>,
    pub removed: BTreeMap<String, Vec<String>>,
    pub moved: usize,
}

/// A binding's place: `path:line:col [host] var`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
struct Location {
    path: String,
    line: u32,
    col: u32,
    host: String,
    var: String,
}

impl Location {
    fn text(&self) -> String {
        format!(
            "{}:{}:{} [{}] {}",
            self.path, self.line, self.col, self.host, self.var
        )
    }
}

/// A binding's two types, as displayed.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Binding {
    doc: String,
    native: Option<String>,
}

/// Relates every binding of `new` to the same one in `old`. `scope` gives
/// the templates in scope at a dump point, so a template name reads as
/// the template rather than a class.
pub fn type_drift(
    old: &str,
    new: &str,
    relation: &Relation,
    scope: &mut dyn FnMut(&str, u32, u32) -> TemplateScope,
) -> Drift {
    let mut old_bindings: FxHashMap<Location, Binding> = FxHashMap::default();
    for line in old.lines() {
        old_bindings.extend(parse_line(line));
    }
    let mut drift = Drift::default();
    let mut added: Vec<(Location, Binding)> = Vec::new();
    for line in new.lines() {
        for (location, binding) in parse_line(line) {
            let Some(before) = old_bindings.remove(&location) else {
                added.push((location, binding));
                continue;
            };
            if before == binding {
                drift.unchanged += 1;
                continue;
            }
            let templates = scope(&location.path, location.line, location.col);
            let facets = [
                (Facet::Doc, Some(&before.doc), Some(&binding.doc)),
                (
                    Facet::Native,
                    before.native.as_ref(),
                    binding.native.as_ref(),
                ),
            ];
            for (facet, was, is) in facets {
                if was == is {
                    continue;
                }
                let verdict = match (was, is) {
                    (Some(was), Some(is)) => relate(relation, &templates, was, is),
                    _ => Verdict::Incomparable,
                };
                *drift.counts.entry((facet, verdict)).or_default() += 1;
                if !verdict.is_gain() {
                    let pattern = format!(
                        "{} -> {}",
                        mask(was.map_or("(none)", String::as_str)),
                        mask(is.map_or("(none)", String::as_str))
                    );
                    drift
                        .groups
                        .entry((facet, verdict, pattern))
                        .or_default()
                        .push(location.text());
                }
            }
        }
    }
    // A binding whose line moved is one removed and one added with the
    // same path, host and variable, and the same types.
    let mut removed: Vec<(Location, Binding)> = old_bindings.into_iter().collect();
    removed.sort_by(|a, b| a.0.cmp(&b.0));
    added.sort_by(|a, b| a.0.cmp(&b.0));
    let mut by_identity: FxHashMap<(String, String, String), Vec<(Location, Binding)>> =
        FxHashMap::default();
    for (location, binding) in removed {
        by_identity
            .entry((
                location.path.clone(),
                location.host.clone(),
                location.var.clone(),
            ))
            .or_default()
            .push((location, binding));
    }
    for (location, binding) in added {
        let identity = (
            location.path.clone(),
            location.host.clone(),
            location.var.clone(),
        );
        let candidates = by_identity.entry(identity).or_default();
        if let Some(at) = candidates.iter().position(|(_, b)| *b == binding) {
            candidates.remove(at);
            drift.moved += 1;
        } else {
            drift
                .added
                .entry(mask(&binding.doc))
                .or_default()
                .push(location.text());
        }
    }
    for (location, binding) in by_identity.into_values().flatten() {
        drift
            .removed
            .entry(mask(&binding.doc))
            .or_default()
            .push(location.text());
    }
    drift
}

/// `was` against `is`, both parsed with `templates` in scope.
fn relate(relation: &Relation, templates: &TemplateScope, was: &str, is: &str) -> Verdict {
    let resolve = |name: &str| templates.resolve(name);
    let (Some(was), Some(is)) = (
        types::try_parse_displayed_with(was, &resolve),
        types::try_parse_displayed_with(is, &resolve),
    ) else {
        return Verdict::Unparsed;
    };
    let holds_new = relation.is_super_type_of(&was, &is).yes();
    let holds_old = relation.is_super_type_of(&is, &was).yes();
    match (holds_new, holds_old) {
        (true, true) => Verdict::Equal,
        (true, false) => Verdict::Narrower,
        (false, true) => Verdict::Wider,
        (false, false) => Verdict::Incomparable,
    }
}

/// The bindings one dump line records: `path:line:col [host] var: type
/// (native type); ...`.
fn parse_line(line: &str) -> Vec<(Location, Binding)> {
    let Some((head, rest)) = line.split_once(" [") else {
        return Vec::new();
    };
    let mut parts = head.rsplitn(3, ':');
    let (Some(col), Some(line_no), Some(path)) = (parts.next(), parts.next(), parts.next()) else {
        return Vec::new();
    };
    let (Ok(line_no), Ok(col)) = (line_no.parse::<u32>(), col.parse::<u32>()) else {
        return Vec::new();
    };
    let Some((host, vars)) = rest
        .split_once("] ")
        .or_else(|| rest.strip_suffix(']').map(|host| (host, "")))
    else {
        return Vec::new();
    };
    split_top_level(vars, "; ")
        .into_iter()
        .filter(|entry| !entry.is_empty())
        .filter_map(|entry| {
            let (var, types) = split_first_top_level(entry, ": ")?;
            let (doc, native) = match types.rfind(" (native ") {
                Some(at) if types.ends_with(')') && balanced(&types[..at]) => (
                    &types[..at],
                    Some(types[at + " (native ".len()..types.len() - 1].to_string()),
                ),
                _ => (types, None),
            };
            Some((
                Location {
                    path: path.to_string(),
                    line: line_no,
                    col,
                    host: host.to_string(),
                    var: var.to_string(),
                },
                Binding {
                    doc: doc.to_string(),
                    native,
                },
            ))
        })
        .collect()
}

/// Whether every bracket in `s` outside quotes closes.
fn balanced(s: &str) -> bool {
    let mut depth = 0i32;
    let mut quote: Option<char> = None;
    let mut escaped = false;
    for c in s.chars() {
        if let Some(q) = quote {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == q {
                quote = None;
            }
            continue;
        }
        match c {
            '\'' | '"' => quote = Some(c),
            '(' | '[' | '{' | '<' => depth += 1,
            ')' | ']' | '}' | '>' => depth -= 1,
            _ => {}
        }
    }
    depth == 0 && quote.is_none()
}

/// `s` split at each `sep` outside quotes and brackets.
fn split_top_level<'s>(s: &'s str, sep: &str) -> Vec<&'s str> {
    let mut parts = Vec::new();
    let mut rest = s;
    while let Some((part, after)) = split_first_top_level(rest, sep) {
        parts.push(part);
        rest = after;
    }
    parts.push(rest);
    parts
}

/// `s` split at its first `sep` outside quotes and brackets.
fn split_first_top_level<'s>(s: &'s str, sep: &str) -> Option<(&'s str, &'s str)> {
    let bytes = s.as_bytes();
    let mut depth = 0i32;
    let mut quote: Option<u8> = None;
    let mut escaped = false;
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if let Some(q) = quote {
            if escaped {
                escaped = false;
            } else if b == b'\\' {
                escaped = true;
            } else if b == q {
                quote = None;
            }
            i += 1;
            continue;
        }
        match b {
            b'\'' | b'"' => quote = Some(b),
            b'(' | b'[' | b'{' | b'<' => depth += 1,
            b')' | b']' | b'}' | b'>' => depth -= 1,
            _ if depth == 0 && s[i..].starts_with(sep) => {
                return Some((&s[..i], &s[i + sep.len()..]));
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// A type's text with literals and class names masked, so changes of one
/// kind group together: a quoted string reads `'S'`, a number `N`, a name
/// starting upper-case or holding a `\` `C`.
fn mask(s: &str) -> String {
    let mut out = String::new();
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '\'' || c == '"' {
            let mut j = i + 1;
            while j < chars.len() && chars[j] != c {
                if chars[j] == '\\' {
                    j += 1;
                }
                j += 1;
            }
            out.push_str("'S'");
            i = j + 1;
        } else if c.is_ascii_digit()
            || (c == '-' && chars.get(i + 1).is_some_and(char::is_ascii_digit))
        {
            let mut j = i + 1;
            while j < chars.len()
                && (chars[j].is_ascii_digit()
                    || chars[j] == '.'
                    || chars[j] == 'E'
                    || chars[j] == '+')
            {
                j += 1;
            }
            out.push('N');
            i = j;
        } else if c.is_alphabetic() || c == '_' || c == '\\' {
            let mut j = i;
            while j < chars.len()
                && (chars[j].is_alphanumeric() || matches!(chars[j], '_' | '\\' | '-'))
            {
                j += 1;
            }
            let word: String = chars[i..j].iter().collect();
            if word.starts_with(|c: char| c.is_uppercase()) || word.contains('\\') {
                out.push('C');
            } else {
                out.push_str(&word);
            }
            i = j;
        } else {
            out.push(c);
            i += 1;
        }
    }
    out
}

impl Drift {
    /// This drift's locations prefixed with `fixture: `.
    pub fn tagged(mut self, fixture: &str) -> Drift {
        let tag = |locations: &mut Vec<String>| {
            for location in locations {
                *location = format!("{fixture}: {location}");
            }
        };
        self.groups.values_mut().for_each(tag);
        self.added.values_mut().for_each(tag);
        self.removed.values_mut().for_each(tag);
        self
    }

    /// This drift's added and removed bindings marked as coming from a
    /// fixture only one dump has.
    pub fn in_one_dump(mut self) -> Drift {
        let mark = |map: BTreeMap<String, Vec<String>>| -> BTreeMap<String, Vec<String>> {
            map.into_iter()
                .map(|(pattern, l)| (format!("(renamed or deleted fixture) {pattern}"), l))
                .collect()
        };
        self.added = mark(std::mem::take(&mut self.added));
        self.removed = mark(std::mem::take(&mut self.removed));
        self
    }

    /// Adds `other`'s counts and groups to this drift's.
    pub fn merge(&mut self, other: Drift) {
        self.unchanged += other.unchanged;
        self.moved += other.moved;
        for (key, count) in other.counts {
            *self.counts.entry(key).or_default() += count;
        }
        for (key, locations) in other.groups {
            self.groups.entry(key).or_default().extend(locations);
        }
        for (key, locations) in other.added {
            self.added.entry(key).or_default().extend(locations);
        }
        for (key, locations) in other.removed {
            self.removed.entry(key).or_default().extend(locations);
        }
    }

    /// The report: counts, then each group by size with up to 20 of its
    /// locations. Relations use today's hierarchy for both sides.
    pub fn report(&self) -> String {
        let mut out = String::new();
        let _ = writeln!(
            out,
            "# Type drift (both dumps related with the current class hierarchy)"
        );
        let _ = writeln!(out);
        let _ = writeln!(out, "unchanged: {}", self.unchanged);
        let _ = writeln!(out, "moved: {}", self.moved);
        for ((facet, verdict), count) in &self.counts {
            let _ = writeln!(out, "{facet:?} {}: {count}", verdict.name());
        }
        let _ = writeln!(
            out,
            "added: {}",
            self.added.values().map(Vec::len).sum::<usize>()
        );
        let _ = writeln!(
            out,
            "removed: {}",
            self.removed.values().map(Vec::len).sum::<usize>()
        );
        let mut groups: Vec<(String, &Vec<String>)> = self
            .groups
            .iter()
            .map(|((facet, verdict, pattern), locations)| {
                (
                    format!("{facet:?} {}: {pattern}", verdict.name()),
                    locations,
                )
            })
            .chain(
                self.removed
                    .iter()
                    .map(|(pattern, l)| (format!("removed: {pattern}"), l)),
            )
            .chain(
                self.added
                    .iter()
                    .map(|(pattern, l)| (format!("added: {pattern}"), l)),
            )
            .collect();
        groups.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then_with(|| a.0.cmp(&b.0)));
        for (title, locations) in groups {
            let _ = writeln!(out);
            let _ = writeln!(out, "## {} ({})", title, locations.len());
            for location in locations.iter().take(20) {
                let _ = writeln!(out, "  {location}");
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use types::Type;

    fn drift(old: &str, new: &str) -> Drift {
        type_drift(old, new, &Relation::structural(), &mut |_, _, _| {
            TemplateScope::default()
        })
    }

    #[test]
    fn a_narrower_type_is_a_gain() {
        let d = drift("a.php:1:1 [] x: int", "a.php:1:1 [] x: int<0, max>");
        assert_eq!(d.counts.get(&(Facet::Doc, Verdict::Narrower)), Some(&1));
        assert!(d.groups.is_empty());
    }

    #[test]
    fn a_wider_type_is_grouped() {
        let d = drift("a.php:1:1 [] x: 1", "a.php:1:1 [] x: int");
        assert_eq!(d.counts.get(&(Facet::Doc, Verdict::Wider)), Some(&1));
        assert!(
            d.groups
                .contains_key(&(Facet::Doc, Verdict::Wider, "N -> int".to_string()))
        );
    }

    #[test]
    fn equal_texts_of_one_type_are_equal() {
        let d = drift("a.php:1:1 [] x: true|false", "a.php:1:1 [] x: bool");
        assert_eq!(d.counts.get(&(Facet::Doc, Verdict::Equal)), Some(&1));
    }

    #[test]
    fn unrelated_types_are_incomparable() {
        let d = drift("a.php:1:1 [] x: int", "a.php:1:1 [] x: string");
        assert_eq!(d.counts.get(&(Facet::Doc, Verdict::Incomparable)), Some(&1));
    }

    #[test]
    fn a_type_that_does_not_parse_is_unparsed() {
        let d = drift("a.php:1:1 [] x: int", "a.php:1:1 [] x: int<<");
        assert_eq!(d.counts.get(&(Facet::Doc, Verdict::Unparsed)), Some(&1));
    }

    #[test]
    fn bindings_on_one_side_only_are_added_or_removed() {
        let d = drift(
            "a.php:1:1 [] x: int; y: int",
            "a.php:1:1 [] x: int; z: string",
        );
        assert_eq!(d.unchanged, 1);
        assert_eq!(d.removed.values().map(Vec::len).sum::<usize>(), 1);
        assert_eq!(d.added.values().map(Vec::len).sum::<usize>(), 1);
    }

    #[test]
    fn a_binding_whose_line_moved_is_moved() {
        let d = drift("a.php:1:1 [] x: int", "a.php:3:1 [] x: int");
        assert_eq!(d.moved, 1);
        assert!(d.added.is_empty() && d.removed.is_empty());
    }

    #[test]
    fn a_native_change_is_its_own_facet() {
        let d = drift(
            "a.php:1:1 [] x: int (native int)",
            "a.php:1:1 [] x: int (native mixed)",
        );
        assert_eq!(d.counts.get(&(Facet::Native, Verdict::Wider)), Some(&1));
        assert!(!d.counts.contains_key(&(Facet::Doc, Verdict::Equal)));
    }

    #[test]
    fn a_template_name_resolves_through_the_scope() {
        let mut scope = |_: &str, _: u32, _: u32| {
            let mut param = types::TemplateParam::unbounded("T".to_string());
            param.bound = Box::new(Type::int());
            TemplateScope::from_iter([("T".to_string(), Type::template_param(param))])
        };
        let d = type_drift(
            "a.php:1:1 [] x: T",
            "a.php:1:1 [] x: int",
            &Relation::structural(),
            &mut scope,
        );
        assert_eq!(d.counts.get(&(Facet::Doc, Verdict::Wider)), Some(&1));
        let unscoped = drift("a.php:1:1 [] x: T", "a.php:1:1 [] x: int");
        assert_eq!(
            unscoped.counts.get(&(Facet::Doc, Verdict::Incomparable)),
            Some(&1)
        );
    }

    #[test]
    fn separators_inside_literals_and_brackets_do_not_split() {
        let d = drift(
            "a.php:1:1 [] x: 'a; b: c'|array{k: int}",
            "a.php:1:1 [] x: 'a; b: c'|array{k: int}",
        );
        assert_eq!(d.unchanged, 1);
    }
}
