//! Context files loaded by name, as PHP's autoloader loads them. A class the
//! code names is found in the classmap, or else under a PSR-4 prefix, and its
//! file is read then. The eager files (the `files` entries and the loader
//! class) are read whatever is asked for; a file reached by neither is not
//! read.

use std::collections::hash_map::Entry;
use std::path::{Path, PathBuf};

use composer::vendor::VendorContext;
use rustc_hash::{FxHashMap, FxHashSet};

/// Where a context file sits in the order its declarations take effect in: the
/// eager files first, then the files the lookup reaches in the order their
/// entries are listed (a classmap entry before a PSR-4 one), then the rest in
/// path order. A name that two files declare resolves to the file with the
/// lower rank, as the autoloader that lists the file first resolves it.
pub(crate) type Rank = (u8, usize, u8, usize);

pub(crate) struct LazyContext {
    /// Each classmap name, lowercased, with the file it maps to. The first
    /// package listed for a name wins, as Composer's classmap does.
    classes: FxHashMap<String, PathBuf>,
    /// The files of the classmap, with the position of their first entry.
    class_files: FxHashMap<PathBuf, (usize, usize)>,
    /// Each PSR-4 prefix, lowercased and ending in `\` (the empty prefix
    /// matches every name), with its base directories in the order listed.
    prefixes: FxHashMap<String, Vec<PathBuf>>,
    /// Each PSR-4 base directory, with the position of each entry that lists it.
    dirs: FxHashMap<PathBuf, Vec<(usize, usize)>>,
    /// The files Composer's bootstrap loads whatever is asked for.
    eager: FxHashSet<PathBuf>,
}

impl LazyContext {
    /// The lookup of the contexts of `vendors`, in the order they are listed.
    pub(crate) fn new<'a>(vendors: impl IntoIterator<Item = &'a VendorContext>) -> Self {
        let mut classes = FxHashMap::default();
        let mut class_files = FxHashMap::default();
        let mut prefixes: FxHashMap<String, Vec<PathBuf>> = FxHashMap::default();
        let mut dirs: FxHashMap<PathBuf, Vec<(usize, usize)>> = FxHashMap::default();
        let mut eager = FxHashSet::default();
        for (vendor_at, vendor) in vendors.into_iter().enumerate() {
            for (at, (name, path)) in vendor.classes.iter().enumerate() {
                let path = path.as_path().to_path_buf();
                class_files.entry(path.clone()).or_insert((vendor_at, at));
                classes.entry(name.clone()).or_insert(path);
            }
            for (at, (prefix, dir)) in vendor.prefixes.iter().enumerate() {
                let dir = dir.as_path().to_path_buf();
                prefixes
                    .entry(prefix.clone())
                    .or_default()
                    .push(dir.clone());
                dirs.entry(dir).or_default().push((vendor_at, at));
            }
            for path in &vendor.eager {
                eager.insert(path.as_path().to_path_buf());
            }
        }
        LazyContext {
            classes,
            class_files,
            prefixes,
            dirs,
            eager,
        }
    }

    /// Whether the file at `path` is read only when a name asks for it: it is
    /// a classmap file or lies under a PSR-4 base directory, and it is not
    /// eager.
    pub(crate) fn is_lazy(&self, path: &Path) -> bool {
        !self.eager.contains(path)
            && (self.class_files.contains_key(path)
                || path.ancestors().any(|dir| self.dirs.contains_key(dir)))
    }

    /// The rank of the context file at `path`: see `Rank`.
    pub(crate) fn rank(&self, path: &Path) -> Rank {
        if self.eager.contains(path) {
            return (0, 0, 0, 0);
        }
        if let Some(&(vendor_at, at)) = self.class_files.get(path) {
            return (1, vendor_at, 0, at);
        }
        let by_prefix = path
            .ancestors()
            .filter_map(|dir| self.dirs.get(dir))
            .flatten()
            .min();
        match by_prefix {
            Some(&(vendor_at, at)) => (1, vendor_at, 1, at),
            None => (2, 0, 0, 0),
        }
    }

    /// The files a class name may be found in, in the order Composer tries
    /// them: the classmap's file for the name, or else each PSR-4 candidate,
    /// longest prefix first. A candidate may not exist.
    pub(crate) fn candidates(&self, name: &str) -> Vec<PathBuf> {
        let name = name.trim_start_matches('\\');
        let lower = name.to_ascii_lowercase();
        if let Some(path) = self.classes.get(&lower) {
            return vec![path.clone()];
        }
        let mut out = Vec::new();
        // Each namespace prefix of the name, the longest first, then the empty one.
        for end in lower.match_indices('\\').map(|(at, _)| at + 1).rev() {
            self.push_candidates(&mut out, &lower[..end], &name[end..]);
        }
        self.push_candidates(&mut out, "", name);
        out
    }

    fn push_candidates(&self, out: &mut Vec<PathBuf>, prefix: &str, rest: &str) {
        if rest.is_empty() {
            return;
        }
        let Some(dirs) = self.prefixes.get(prefix) else {
            return;
        };
        let file = format!("{}.php", rest.replace('\\', "/"));
        out.extend(dirs.iter().map(|dir| dir.join(&file)));
    }
}

/// The first character of a lazy name that is an include, not a class name.
/// Followed by the normalized trailing text of the include's path, see
/// `include_name`.
pub(crate) const INCLUDE_MARK: char = '\u{1}';

/// The lazy name of an include whose path no literal ends: it may read any
/// file. It is the mark alone, which `include_name` gives for such a path.
pub(crate) const INCLUDE_ANY: &str = "\u{1}";

/// One part of an include's path, or of a constant's value, as the parse reads
/// it.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) enum PathPart {
    /// Literal text.
    Text(String),
    /// A constant of the project, by its short name, whose value is its text.
    Const(String),
    /// Text the parse does not know: a directory, a call, a variable.
    Unknown,
}

/// What a file's parse hands to the lazy lookup.
#[derive(Clone, Default, serde::Serialize, serde::Deserialize)]
pub(crate) struct LazyRefs {
    /// The class names the file names.
    pub(crate) names: Vec<String>,
    /// The path of each include the file makes.
    pub(crate) includes: Vec<Vec<PathPart>>,
    /// Each constant the file defines, by its short name, with its value.
    pub(crate) constants: Vec<(String, Vec<PathPart>)>,
}

/// The constants the loaded files define, by short name. A name defined twice
/// is unknown, as is a name no loaded file defines: a path that reads one may
/// name any file.
pub(crate) struct Constants(FxHashMap<String, Vec<PathPart>>);

impl Constants {
    pub(crate) fn new<'a>(refs: impl IntoIterator<Item = &'a LazyRefs>) -> Self {
        let mut values: FxHashMap<String, Vec<PathPart>> = FxHashMap::default();
        for (name, value) in refs.into_iter().flat_map(|refs| refs.constants.iter()) {
            match values.entry(name.clone()) {
                Entry::Vacant(entry) => {
                    entry.insert(value.clone());
                }
                Entry::Occupied(mut entry) => {
                    entry.insert(vec![PathPart::Unknown]);
                }
            }
        }
        Constants(values)
    }

    fn get(&self, name: &str) -> Option<&[PathPart]> {
        self.0.get(name).map(Vec::as_slice)
    }
}

/// Prepends to `tail` the text that `parts` ends with, reading a constant
/// through its value. Stops at the first part whose text is not known, which
/// leaves the path's start open: the text before it is not known. `expanding`
/// holds the constants whose values are being read, so that a constant its own
/// value reads is not read again. Returns whether the scan reached the start
/// of `parts`.
fn prepend_tail(
    parts: &[PathPart],
    constants: &Constants,
    expanding: &mut Vec<String>,
    tail: &mut String,
) -> bool {
    for part in parts.iter().rev() {
        match part {
            PathPart::Text(text) => tail.insert_str(0, text),
            PathPart::Unknown => return false,
            PathPart::Const(name) => {
                let Some(value) = constants.get(name) else {
                    return false;
                };
                if expanding.contains(name) {
                    return false;
                }
                expanding.push(name.clone());
                let whole = prepend_tail(value, constants, expanding, tail);
                expanding.pop();
                if !whole {
                    return false;
                }
            }
        }
    }
    true
}

/// The lazy name of an include of `parts`: `INCLUDE_MARK`, then the normalized
/// trailing text of the path. The text is empty when no literal ends the path,
/// and then the include may read any file.
pub(crate) fn include_name(parts: &[PathPart], constants: &Constants) -> String {
    let mut tail = String::new();
    prepend_tail(parts, constants, &mut Vec::new(), &mut tail);
    format!("{INCLUDE_MARK}{}", normalize_tail(&tail))
}

/// `text` with its `.` and `..` segments taken out, so that it names the
/// trailing components of the path it ends. A `..` takes out the segment before
/// it; with none left it takes out nothing, as the path before it is not known.
pub(crate) fn normalize_tail(text: &str) -> String {
    let mut segments: Vec<&str> = Vec::new();
    for segment in text.split(['/', '\\']) {
        match segment {
            "" | "." => {}
            ".." => {
                segments.pop();
            }
            _ => segments.push(segment),
        }
    }
    segments.join("/")
}

/// Whether the file at `path` ends with `tail`, a normalized tail, in either
/// separator and whatever the letter case.
pub(crate) fn path_ends_with(path: &Path, tail: &str) -> bool {
    path.to_string_lossy()
        .replace('\\', "/")
        .to_ascii_lowercase()
        .ends_with(&tail.to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(s: &str) -> PathPart {
        PathPart::Text(s.to_string())
    }

    fn constant(name: &str) -> PathPart {
        PathPart::Const(name.to_string())
    }

    fn named(name: &str, value: Vec<PathPart>) -> LazyRefs {
        LazyRefs {
            constants: vec![(name.to_string(), value)],
            ..LazyRefs::default()
        }
    }

    #[test]
    fn an_include_names_the_literal_text_that_ends_its_path() {
        let none = Constants::new([]);
        assert_eq!(
            include_name(&[PathPart::Unknown, text("/lib/a.php")], &none),
            "\u{1}lib/a.php"
        );
        assert_eq!(
            include_name(&[text("/root/"), PathPart::Unknown, text("a.php")], &none),
            "\u{1}a.php"
        );
        assert_eq!(include_name(&[PathPart::Unknown], &none), "\u{1}");
        assert_eq!(include_name(&[text("../x/./y.php")], &none), "\u{1}x/y.php");
        assert_eq!(include_name(&[text("a/../b.php")], &none), "\u{1}b.php");
    }

    #[test]
    fn a_constant_extends_the_text_only_while_its_value_is_known() {
        let refs = [
            named("ROOT", vec![PathPart::Unknown, text("/inc/")]),
            named("INC", vec![text("inc/")]),
        ];
        let constants = Constants::new(&refs);
        assert_eq!(
            include_name(&[constant("INC"), text("a.php")], &constants),
            "\u{1}inc/a.php"
        );
        assert_eq!(
            include_name(&[text("lib/"), constant("INC"), text("a.php")], &constants),
            "\u{1}lib/inc/a.php"
        );
        assert_eq!(
            include_name(&[constant("ROOT"), text("a.php")], &constants),
            "\u{1}inc/a.php"
        );
        assert_eq!(
            include_name(&[text("lib/"), constant("ROOT"), text("a.php")], &constants),
            "\u{1}inc/a.php"
        );
        assert_eq!(
            include_name(&[constant("NOPE"), text("a.php")], &constants),
            "\u{1}a.php"
        );
    }

    #[test]
    fn a_name_defined_twice_is_unknown() {
        let refs = [
            named("X", vec![text("one/")]),
            named("X", vec![text("two/")]),
        ];
        let constants = Constants::new(&refs);
        assert_eq!(
            include_name(&[constant("X"), text("a.php")], &constants),
            "\u{1}a.php"
        );
    }

    #[test]
    fn a_constant_that_reads_itself_stops_the_text() {
        let refs = [named("X", vec![constant("X"), text("a/")])];
        let constants = Constants::new(&refs);
        assert_eq!(
            include_name(&[constant("X"), text("b.php")], &constants),
            "\u{1}a/b.php"
        );
    }

    #[test]
    fn a_path_ends_with_its_tail_in_either_separator_and_case() {
        let path = Path::new("/root/vendor/Acme/Lib/Thing.php");
        assert!(path_ends_with(path, "lib/thing.php"));
        assert!(path_ends_with(path, "thing.php"));
        assert!(!path_ends_with(path, "vendor/thing.php/x"));
        assert!(path_ends_with(
            Path::new("C:\\app\\Lib\\T.php"),
            "lib/t.php"
        ));
    }
}
