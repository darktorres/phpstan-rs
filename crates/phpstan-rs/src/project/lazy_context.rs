//! Context files loaded by name, as PHP's autoloader loads them. A class the
//! code names is found in the classmap, or else under a PSR-4 prefix, and its
//! file is read then. The eager files (the `files` entries and the loader
//! class) are read whatever is asked for; a file reached by neither is not
//! read.

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
