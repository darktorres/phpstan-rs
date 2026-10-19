//! Where a Composer project's manifest and vendor directories are, as Composer
//! finds them. The manifest is the file the `COMPOSER` variable names, else
//! `composer.json`. The vendor directory is the first candidate that resolves,
//! in Composer's order: `COMPOSER_VENDOR_DIR`, then the manifest's
//! `config.vendor-dir`, then `vendor`. A candidate resolves as Composer's
//! `Config` resolves a `-dir` key: its `{$name}` references are expanded, its
//! trailing slashes go, a leading `~/` and a leading `$VAR` or `%VAR%` expand,
//! and a relative result joins the project's directory.
//!
//! A directory is Composer's when Composer has written into it, or when its
//! parent's manifest names it as the vendor directory. Both are read from
//! listings, so a directory that is not built yet is recognized from its
//! manifest alone.

use rustc_hash::FxHashMap;
use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use crate::canonical;
use crate::dir_listings::DirListings;
use crate::hashed_path::HashedPath;
use crate::manifest;

/// The value of an environment variable, `None` when it is not set.
pub type Variables<'a> = &'a (dyn Fn(&OsStr) -> Option<OsString> + Sync);

/// The environment that sets no variable, for a load whose answer must not
/// depend on the process's environment.
pub fn no_variables(_: &OsStr) -> Option<OsString> {
    None
}

/// The keys Composer derives from the user's own directories. A reference to
/// one of them is not resolved, since those directories are not modelled, and
/// the candidate that holds it is left out.
const UNMODELLED_KEYS: [&str; 7] = [
    "home",
    "cache-dir",
    "data-dir",
    "bin-dir",
    "cache-files-dir",
    "cache-repo-dir",
    "cache-vcs-dir",
];

/// The vendor directories of one project root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VendorLocation {
    /// The directory Composer builds: the first candidate that resolves. It
    /// may not exist yet.
    pub resolved: PathBuf,
    /// The first candidate that Composer has written into, if any. A later
    /// candidate is this when the first is not built, so a stray variable
    /// loses no context.
    pub installed: Option<PathBuf>,
    /// Every candidate that resolves, in Composer's order, without repeats.
    candidates: Vec<PathBuf>,
}

/// What is known of one project root: whether its manifest is a file, and
/// where its vendor directories are.
#[derive(Debug, Clone)]
struct Project {
    manifest: bool,
    location: Arc<VendorLocation>,
}

/// The layout of the projects of one load. Its reads go through `listings`
/// and the environment `variables`, and each project's answer is kept once.
pub struct Layout<'a> {
    listings: &'a DirListings,
    variables: Variables<'a>,
    projects: Mutex<FxHashMap<PathBuf, Project>>,
}

impl<'a> Layout<'a> {
    pub fn new(listings: &'a DirListings, variables: Variables<'a>) -> Self {
        Layout {
            listings,
            variables,
            projects: Mutex::new(FxHashMap::default()),
        }
    }

    /// The file that holds `root`'s manifest: the file `COMPOSER` names,
    /// relative to `root`, else `root/composer.json`.
    pub fn manifest_path(&self, root: &Path) -> PathBuf {
        match self.variable("COMPOSER").as_deref().map(trimmed) {
            Some(named) if !named.is_empty() => root.join(named),
            _ => root.join("composer.json"),
        }
    }

    /// Whether `root` has a manifest, which is a file.
    pub fn has_manifest(&self, root: &Path) -> bool {
        self.project(root).manifest
    }

    /// Where `root`'s vendor directories are. Without a manifest there is no
    /// Composer vendor directory: `installed` is `None`, and `resolved` is
    /// `vendor`, which is what Composer would build there.
    pub fn vendor_location(&self, root: &Path) -> Arc<VendorLocation> {
        self.project(root).location
    }

    /// Whether `dir`, a resolved directory, is Composer's: Composer has written
    /// into it, or its parent's manifest resolves to it as the vendor directory
    /// (whether or not it is built yet).
    pub fn is_composer_dir(&self, dir: &Path) -> bool {
        if self.written_into(dir) {
            return true;
        }
        let Some(parent) = dir.parent() else {
            return false;
        };
        self.has_manifest(parent) && self.project(parent).location.resolved == dir
    }

    /// The directory that holds `root`'s vendor files when Composer built it
    /// there: `installed` of `vendor_location`.
    pub fn installed_dir(&self, root: &Path) -> Option<PathBuf> {
        self.vendor_location(root).installed.clone()
    }

    fn variable(&self, name: &str) -> Option<OsString> {
        (self.variables)(OsStr::new(name))
    }

    fn project(&self, root: &Path) -> Project {
        if let Some(found) = self
            .projects
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(root)
        {
            return found.clone();
        }
        let project = self.read_project(root);
        self.projects
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(root.to_path_buf(), project.clone());
        project
    }

    fn read_project(&self, root: &Path) -> Project {
        // Discovery reads a vendor directory only beside a manifest, so a root
        // without one has no Composer vendor directory, whatever its name.
        let Some(text) = self.read_manifest(root) else {
            return Project {
                manifest: false,
                location: Arc::new(VendorLocation {
                    resolved: self.resolve(root, "vendor"),
                    installed: None,
                    candidates: Vec::new(),
                }),
            };
        };
        let candidates = self.candidates(root, &manifest::declared_config(&text));
        let installed = candidates
            .iter()
            .find(|candidate| self.written_into(candidate))
            .cloned();
        let resolved = candidates
            .first()
            .cloned()
            .unwrap_or_else(|| self.resolve(root, "vendor"));
        Project {
            manifest: true,
            location: Arc::new(VendorLocation {
                resolved,
                installed,
                candidates,
            }),
        }
    }

    /// The text of `root`'s manifest, when the manifest is a file that reads.
    fn read_manifest(&self, root: &Path) -> Option<String> {
        let manifest = self.manifest_path(root);
        if !self.manifest_exists(root, &manifest) {
            return None;
        }
        std::fs::read_to_string(&manifest).ok()
    }

    /// Whether the manifest file exists. A manifest in `root` itself is looked
    /// up in its listing, which the walk has usually read already.
    fn manifest_exists(&self, root: &Path, manifest: &Path) -> bool {
        match (manifest.parent(), manifest.file_name()) {
            (Some(dir), Some(name)) if dir == root => self
                .listings
                .listing(&HashedPath::new(root.to_path_buf()))
                .is_some_and(|listing| {
                    listing
                        .entry(name)
                        .is_some_and(|entry| entry.kind.is_none_or(|kind| !kind.is_dir()))
                }),
            _ => manifest.is_file(),
        }
    }

    fn candidates(&self, root: &Path, config: &BTreeMap<String, String>) -> Vec<PathBuf> {
        let mut raws: Vec<OsString> = Vec::new();
        raws.extend(self.variable("COMPOSER_VENDOR_DIR"));
        raws.extend(config.get("vendor-dir").map(OsString::from));
        raws.push(OsString::from("vendor"));
        let mut candidates: Vec<PathBuf> = Vec::new();
        for raw in raws {
            let Some(text) = raw
                .to_str()
                .and_then(|text| self.expand_dir(text, config, 0))
            else {
                continue;
            };
            let path = self.resolve(root, &text);
            if !candidates.contains(&path) {
                candidates.push(path);
            }
        }
        candidates
    }

    /// A `-dir` value as Composer's `Config` reads it, before its path is
    /// joined: references expanded, trailing slashes trimmed, the home and
    /// variables expanded. `None` when it holds a reference that is not
    /// modelled.
    fn expand_dir(
        &self,
        text: &str,
        config: &BTreeMap<String, String>,
        depth: u8,
    ) -> Option<String> {
        let referenced = self.references(text, config, depth)?;
        let trimmed = referenced.trim_end_matches(['/', '\\']);
        Some(self.expand_path(trimmed))
    }

    /// `text` with each `{$name}` replaced by the value of `name`. Composer
    /// matches from the first `{$` to the last `}`, so one reference may span
    /// others; that is kept as it is.
    fn references(
        &self,
        text: &str,
        config: &BTreeMap<String, String>,
        depth: u8,
    ) -> Option<String> {
        let Some(start) = text.find("{$") else {
            return Some(text.to_string());
        };
        let Some(end) = text.rfind('}').filter(|&end| end >= start + 3) else {
            return Some(text.to_string());
        };
        let value = self.reference(&text[start + 2..end], config, depth)?;
        Some(format!("{}{}{}", &text[..start], value, &text[end + 1..]))
    }

    /// The value a `{$name}` reference gives, or `None` when it is not modelled
    /// or refers back to itself too deeply.
    fn reference(
        &self,
        name: &str,
        config: &BTreeMap<String, String>,
        depth: u8,
    ) -> Option<String> {
        if depth > 8 {
            return None;
        }
        if let Some(value) = config.get(name) {
            return self.references(value, config, depth + 1);
        }
        if name == "vendor-dir" {
            return Some("vendor".to_string());
        }
        if UNMODELLED_KEYS.contains(&name) {
            return None;
        }
        Some(String::new())
    }

    /// Composer's `Platform::expandPath`: a leading `~/` or `~\` takes the home
    /// directory, and a leading `$VAR`, or `%VAR%`, takes the variable's value.
    /// A variable that is not set expands to nothing, so `$NOPE/libs` is
    /// `/libs`, as it is for Composer.
    fn expand_path(&self, text: &str) -> String {
        if text.starts_with("~/") || text.starts_with("~\\") {
            return format!("{}{}", self.home(), &text[1..]);
        }
        if let Some(after) = text.strip_prefix('$') {
            let len = name_len(after);
            if len > 0 {
                return format!("{}{}", self.variable_text(&after[..len]), &after[len..]);
            }
        }
        if let Some(after) = text.strip_prefix('%') {
            let len = name_len(after);
            if len > 0 && after[len..].starts_with('%') {
                return format!("{}{}", self.variable_text(&after[..len]), &after[len + 1..]);
            }
        }
        text.to_string()
    }

    fn home(&self) -> String {
        let home = self.variable("HOME").filter(|home| !home.is_empty());
        let home = home.or_else(|| self.variable("USERPROFILE"));
        home.map_or_else(String::new, |home| home.to_string_lossy().into_owned())
    }

    /// A variable's value as text. On Windows `HOME` is read as `USERPROFILE`
    /// when it is not set, as Composer reads it.
    fn variable_text(&self, name: &str) -> String {
        let value = match name {
            "HOME" if cfg!(windows) => self
                .variable("HOME")
                .filter(|home| !home.is_empty())
                .or_else(|| self.variable("USERPROFILE")),
            _ => self.variable(name),
        };
        value.map_or_else(String::new, |value| value.to_string_lossy().into_owned())
    }

    /// The directory a resolved candidate names: an absolute one as it is, a
    /// relative one joined to `root`, and canonical when it exists.
    fn resolve(&self, root: &Path, text: &str) -> PathBuf {
        let joined = if is_absolute(text) {
            PathBuf::from(text)
        } else {
            root.join(text)
        };
        canonical::canonicalize(&joined).unwrap_or_else(|_| manifest::lexically_normalized(&joined))
    }

    /// Whether `dir` holds Composer's marks: a `composer` directory with an
    /// `installed.json` or an `autoload_real.php`, which every install and
    /// dump writes.
    fn written_into(&self, dir: &Path) -> bool {
        let composer = HashedPath::new(dir.join("composer"));
        self.listings.listing(&composer).is_some_and(|listing| {
            ["installed.json", "autoload_real.php"].iter().any(|name| {
                listing
                    .entry(OsStr::new(name))
                    .is_some_and(|entry| entry.kind.is_none_or(|kind| !kind.is_dir()))
            })
        })
    }
}

/// `text` without the whitespace that surrounds it, as Composer trims the
/// `COMPOSER` value.
fn trimmed(text: &OsStr) -> &OsStr {
    text.to_str().map_or(text, |text| OsStr::new(text.trim()))
}

/// The length of the leading run of ASCII letters, digits and underscores:
/// the name a `$VAR` or `%VAR%` reference names (PCRE's `\w`).
fn name_len(text: &str) -> usize {
    text.bytes()
        .take_while(|b| b.is_ascii_alphanumeric() || *b == b'_')
        .count()
}

/// Whether Composer's `realpath` takes `text` as it is: a leading slash, a
/// drive letter, a scheme, or two backslashes.
fn is_absolute(text: &str) -> bool {
    let bytes = text.as_bytes();
    let has_drive = bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':';
    let scheme = bytes
        .iter()
        .take_while(|b| b.is_ascii_alphanumeric() || **b == b'.')
        .count();
    text.starts_with('/')
        || text.starts_with(r"\\")
        || has_drive
        || (scheme > 0 && text[scheme..].starts_with("://"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canonical;
    use rustc_hash::FxHashMap;

    /// The environment as a lookup over `pairs`.
    fn variables(pairs: &[(&str, &str)]) -> impl Fn(&OsStr) -> Option<OsString> + Sync {
        let map: FxHashMap<String, String> = pairs
            .iter()
            .map(|(name, value)| (name.to_string(), value.to_string()))
            .collect();
        move |name: &OsStr| map.get(name.to_str()?).map(OsString::from)
    }

    /// A fresh directory for one test, removed when the test ends.
    struct Tree(PathBuf);

    impl Tree {
        fn new(name: &str) -> Tree {
            let path = std::env::temp_dir()
                .join(format!("phpstan-rs-layout-{}-{name}", std::process::id()));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).unwrap();
            Tree(canonical::canonicalize(&path).unwrap())
        }

        fn write(&self, path: &str, text: &str) {
            let file = self.0.join(path);
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(file, text).unwrap();
        }
    }

    impl Drop for Tree {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// Asserts the text a `-dir` setting expands to before its path is joined,
    /// as Composer's `Config::get` expands it (`php` 8.5.4; its values, joined
    /// to `/proj`, are in the commit's test data).
    fn check(text: &str, config: &[(&str, &str)], env: &[(&str, &str)], expected: Option<&str>) {
        let listings = DirListings::default();
        let vars = variables(env);
        let layout = Layout::new(&listings, &vars);
        let config: BTreeMap<String, String> = config
            .iter()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect();
        assert_eq!(
            layout.expand_dir(text, &config, 0).as_deref(),
            expected,
            "{text:?}"
        );
    }

    #[test]
    fn a_directory_setting_expands_as_composer_expands_it() {
        check("libs", &[], &[], Some("libs"));
        check("libs/vendor", &[], &[], Some("libs/vendor"));
        check("./libs/", &[], &[], Some("./libs"));
        check("../vendor", &[], &[], Some("../vendor"));
        check("/abs/v", &[], &[], Some("/abs/v"));
        check("libs\\", &[], &[], Some("libs"));
        check("C:\\v", &[], &[], Some("C:\\v"));
        check("~/v", &[], &[("HOME", "/home/u")], Some("/home/u/v"));
        check(
            "{$base}/vendor",
            &[("base", "deps")],
            &[],
            Some("deps/vendor"),
        );
        check("{$base}/vendor", &[("base", "")], &[], Some("/vendor"));
        check(
            "{$base}/vendor",
            &[("base", "deps/")],
            &[],
            Some("deps//vendor"),
        );
        check("{$nope}/vendor", &[], &[], Some("/vendor"));
        check("{$home}/x", &[], &[], None);
        check(
            "$PRS_PROBE/v",
            &[],
            &[("PRS_PROBE", "/probe")],
            Some("/probe/v"),
        );
        check("$NOPE_PRS/libs", &[], &[], Some("/libs"));
        check(
            "%PRS_PROBE%/v",
            &[],
            &[("PRS_PROBE", "/probe")],
            Some("/probe/v"),
        );
        check(
            "%PRS_PROBE/v",
            &[],
            &[("PRS_PROBE", "/probe")],
            Some("%PRS_PROBE/v"),
        );
        check("%NOPE_PRS%/libs", &[], &[], Some("/libs"));
    }

    #[test]
    fn the_manifest_and_the_environment_name_the_vendor_directory() {
        let tree = Tree::new("location");
        tree.write("composer.json", r#"{"config": {"vendor-dir": "libs"}}"#);
        tree.write("libs/composer/installed.json", "{}");
        let listings = DirListings::default();
        let vars = variables(&[]);
        let layout = Layout::new(&listings, &vars);
        let location = layout.vendor_location(&tree.0);
        assert_eq!(location.resolved, tree.0.join("libs"));
        assert_eq!(location.installed, Some(tree.0.join("libs")));
        assert_eq!(layout.installed_dir(&tree.0), Some(tree.0.join("libs")));
    }

    #[test]
    fn an_unbuilt_candidate_is_resolved_and_a_later_built_one_is_installed() {
        let tree = Tree::new("stray");
        tree.write("composer.json", r#"{"config": {"vendor-dir": "libs"}}"#);
        tree.write("libs/composer/installed.json", "{}");
        let listings = DirListings::default();
        let vars = variables(&[("COMPOSER_VENDOR_DIR", "stray")]);
        let layout = Layout::new(&listings, &vars);
        let location = layout.vendor_location(&tree.0);
        assert_eq!(location.resolved, tree.0.join("stray"));
        assert_eq!(location.installed, Some(tree.0.join("libs")));
    }

    #[test]
    fn the_environment_names_a_built_vendor_directory_before_the_manifest() {
        let tree = Tree::new("env-first");
        tree.write("composer.json", r#"{"config": {"vendor-dir": "libs"}}"#);
        tree.write("env/composer/installed.json", "{}");
        tree.write("libs/composer/installed.json", "{}");
        let listings = DirListings::default();
        let vars = variables(&[("COMPOSER_VENDOR_DIR", "env")]);
        let layout = Layout::new(&listings, &vars);
        assert_eq!(layout.installed_dir(&tree.0), Some(tree.0.join("env")));
    }

    #[test]
    fn the_composer_variable_names_the_manifest() {
        let tree = Tree::new("named");
        tree.write("other.json", r#"{"config": {"vendor-dir": "libs"}}"#);
        tree.write("composer.json", "{}");
        tree.write("libs/composer/installed.json", "{}");
        let listings = DirListings::default();
        let vars = variables(&[("COMPOSER", "  other.json  ")]);
        let layout = Layout::new(&listings, &vars);
        assert_eq!(layout.manifest_path(&tree.0), tree.0.join("other.json"));
        assert_eq!(layout.installed_dir(&tree.0), Some(tree.0.join("libs")));
    }

    #[test]
    fn a_root_without_a_manifest_has_no_composer_vendor_directory() {
        let tree = Tree::new("bare");
        tree.write("vendor/composer/installed.json", "{}");
        let listings = DirListings::default();
        let vars = variables(&[]);
        let layout = Layout::new(&listings, &vars);
        assert!(!layout.has_manifest(&tree.0));
        assert_eq!(layout.installed_dir(&tree.0), None);
    }

    #[test]
    fn a_directory_named_composer_json_is_not_a_manifest() {
        let tree = Tree::new("dir-manifest");
        std::fs::create_dir_all(tree.0.join("composer.json")).unwrap();
        let listings = DirListings::default();
        let vars = variables(&[]);
        let layout = Layout::new(&listings, &vars);
        assert!(!layout.has_manifest(&tree.0));
    }

    #[test]
    fn a_composer_directory_is_the_one_the_manifest_resolves_or_one_that_holds_marks() {
        let tree = Tree::new("composer-dir");
        tree.write("composer.json", r#"{"config": {"vendor-dir": "libs"}}"#);
        tree.write("build/vendor/composer/autoload_real.php", "<?php");
        tree.write("lib/vendor/x.php", "<?php");
        let listings = DirListings::default();
        let vars = variables(&[]);
        let layout = Layout::new(&listings, &vars);
        assert!(layout.is_composer_dir(&tree.0.join("libs")));
        assert!(layout.is_composer_dir(&tree.0.join("build/vendor")));
        assert!(!layout.is_composer_dir(&tree.0.join("vendor")));
        assert!(!layout.is_composer_dir(&tree.0.join("lib/vendor")));
    }
}
