//! The root package's `composer.json` and the installed packages listed
//! in `vendor/composer/installed.json`, read for the dependency-hygiene
//! checks: declared requirements, autoload roots, `provide`, `scripts`,
//! `config.platform.php`, path repositories, and which installed package
//! owns a given file.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::fmt;
use std::marker::PhantomData;
use std::ops::Deref;
use std::path::{Path, PathBuf};

use serde::de::{self, IgnoredAny, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer};
use serde_json::value::RawValue;
use syntax::span::LineIndex;

use crate::canonical;

/// One `composer.json` entry and the 1-based line it's written on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Located<T> {
    pub value: T,
    pub line: u32,
}

/// An `autoload`/`autoload-dev` section, with every directory absolute.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Autoload {
    /// `(namespace prefix with trailing \, directories)`.
    pub psr4: Vec<(String, Vec<PathBuf>)>,
    pub classmap: Vec<PathBuf>,
    pub files: Vec<PathBuf>,
    /// `exclude-from-classmap` paths (also excluded from PSR-4 scanning).
    pub excluded: Vec<PathBuf>,
}

impl Autoload {
    fn read(section: Option<Members<'_>>, root: &Path) -> Autoload {
        let Some(section) = section else {
            return Autoload::default();
        };
        let paths = |raw: &RawValue| -> Vec<PathBuf> {
            strings(raw).iter().map(|s| root.join(s)).collect()
        };
        let psr4 = section
            .read::<Members>("psr-4")
            .map(|map| {
                map.iter()
                    .map(|(prefix, dirs)| (prefix.to_string(), paths(dirs)))
                    .collect()
            })
            .unwrap_or_default();
        let excluded = section
            .raw("exclude-from-classmap")
            .map(strings)
            .unwrap_or_default()
            .into_iter()
            .map(|p| root.join(p.trim_start_matches('/').trim_end_matches(['/', '*'])))
            .collect();
        Autoload {
            psr4,
            classmap: section.raw("classmap").map(paths).unwrap_or_default(),
            files: section.raw("files").map(paths).unwrap_or_default(),
            excluded,
        }
    }
}

/// An installed package from `installed.json`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledPackage {
    pub name: String,
    pub kind: String,
    /// Canonical install directory.
    pub install_dir: PathBuf,
    /// Canonical `vendor/<name>` directory: a symlink to `install_dir` for
    /// a path repository is followed.
    pub vendor_dir: PathBuf,
    pub provide: Vec<String>,
    pub replace: Vec<String>,
    /// Whether it ships command-line binaries (`bin`), used by running
    /// them rather than from code.
    pub has_bin: bool,
    /// Whether its `extra` registers it with a framework's auto-discovery
    /// (a Pest/PHPStan/Psalm plugin, a Laravel package), so the framework
    /// loads it without code naming it.
    pub auto_discovered: bool,
    /// Its `include-path` directories, below `install_dir`.
    pub include_path: Vec<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Repository {
    pub kind: String,
    pub url: String,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ComposerManifest {
    /// The root as analysed paths spell it; the manifest's own paths
    /// (`path`, autoload directories, `vendor_dir`s) are spelled from it.
    pub root: PathBuf,
    /// The same directory, canonical: what `absolute` resolves the
    /// manifest's paths against.
    pub absolute_root: PathBuf,
    pub path: PathBuf,
    pub name: Option<String>,
    pub require: Vec<Located<String>>,
    pub require_dev: Vec<Located<String>>,
    /// Version constraints by package name, both sections.
    pub constraints: BTreeMap<String, String>,
    pub autoload: Autoload,
    pub autoload_dev: Autoload,
    pub provide: Vec<Located<String>>,
    /// `suggest` keys: declared optional dependencies.
    pub suggest: Vec<String>,
    /// `(script name, command)`, one entry per command of a script.
    pub scripts: Vec<Located<(String, String)>>,
    pub platform_php: Option<Located<String>>,
    pub repositories: Vec<Repository>,
    pub installed: Vec<InstalledPackage>,
    /// The root package's `include-path` directories, below `absolute_root`.
    pub include_path: Vec<PathBuf>,
}

impl ComposerManifest {
    /// Reads the manifest at `path`, which is `root`'s (the file its
    /// `COMPOSER` setting names, or `composer.json`), and the packages that
    /// `installed`'s `composer/installed.json` lists when `installed` is the
    /// vendor directory Composer built; `None` without a readable, parseable
    /// manifest. `absolute_root` is `root` canonicalized.
    pub fn load(
        path: &Path,
        root: &Path,
        absolute_root: &Path,
        installed: Option<&Path>,
    ) -> Option<ComposerManifest> {
        let text = std::fs::read_to_string(path).ok()?;
        let json: Members = parse_document(&text)?;
        let lines = LineFinder::new(&text);
        let mut manifest = ComposerManifest {
            root: root.to_path_buf(),
            absolute_root: absolute_root.to_path_buf(),
            path: path.to_path_buf(),
            name: json.read("name"),
            ..ComposerManifest::default()
        };
        for (section, out) in [
            ("require", &mut manifest.require),
            ("require-dev", &mut manifest.require_dev),
        ] {
            if let Some(members) = json.read::<Members>(section) {
                for (package, constraint) in members.iter() {
                    out.push(Located {
                        value: package.to_string(),
                        line: lines.key_in_section(section, package),
                    });
                    if let Some(c) = read_raw::<String>(constraint) {
                        manifest.constraints.insert(package.to_ascii_lowercase(), c);
                    }
                }
            }
        }
        manifest.autoload = Autoload::read(json.read("autoload"), root);
        manifest.autoload_dev = Autoload::read(json.read("autoload-dev"), root);
        if let Some(members) = json.read::<Members>("provide") {
            for package in members.keys() {
                manifest.provide.push(Located {
                    value: package.to_string(),
                    line: lines.key_in_section("provide", package),
                });
            }
        }
        manifest.suggest = keys(json.read("suggest"));
        if let Some(members) = json.read::<Members>("scripts") {
            for (name, commands) in members.iter() {
                let line = lines.key_in_section("scripts", name);
                for command in strings(commands) {
                    let command_line = lines.line_of_string_after(&command, line);
                    manifest.scripts.push(Located {
                        value: (name.to_string(), command),
                        line: command_line,
                    });
                }
            }
        }
        if let Some(php) = json
            .read::<Members>("config")
            .and_then(|config| config.read::<Members>("platform"))
            .and_then(|platform| platform.read::<String>("php"))
        {
            manifest.platform_php = Some(Located {
                value: php,
                line: lines.key_in_section("platform", "php"),
            });
        }
        let repositories: Vec<Members> = json
            .read::<Vec<Members>>("repositories")
            .or_else(|| {
                json.read::<BTreeMap<Cow<str>, Members>>("repositories")
                    .map(|map| map.into_values().collect())
            })
            .unwrap_or_default();
        for repo in &repositories {
            if let (Some(kind), Some(url)) = (repo.read("type"), repo.read("url")) {
                manifest.repositories.push(Repository { kind, url });
            }
        }
        manifest.include_path = include_directories(absolute_root, json.raw("include-path"));
        manifest.installed = read_installed(installed);
        Some(manifest)
    }

    /// The directories that Composer's autoloader puts in front of PHP's
    /// include path: the root package's, then each installed package's.
    pub fn include_paths(&self) -> impl Iterator<Item = &Path> {
        self.include_path
            .iter()
            .chain(self.installed.iter().flat_map(|p| &p.include_path))
            .map(PathBuf::as_path)
    }

    pub fn requires(&self, package: &str) -> bool {
        self.require
            .iter()
            .any(|r| r.value.eq_ignore_ascii_case(package))
    }

    pub fn suggests(&self, package: &str) -> bool {
        self.suggest.iter().any(|s| s.eq_ignore_ascii_case(package))
    }

    pub fn requires_dev(&self, package: &str) -> bool {
        self.require_dev
            .iter()
            .any(|r| r.value.eq_ignore_ascii_case(package))
    }

    /// `path`, one of this manifest's paths or an analysed path spelled
    /// from the same root, as an absolute path: `root` swapped for
    /// `absolute_root`, then `.` and `..` resolved lexically.
    pub fn absolute(&self, path: &Path) -> PathBuf {
        let joined = match path.strip_prefix(&self.root) {
            Ok(rest) => self.absolute_root.join(rest),
            Err(_) => self.absolute_root.join(path),
        };
        lexically_normalized(&joined)
    }

    /// Whether the absolute `file` is under one of `section`'s
    /// `exclude-from-classmap` paths.
    pub fn excludes(&self, section: &Autoload, file: &Path) -> bool {
        section
            .excluded
            .iter()
            .any(|dir| path_starts_with(file, &self.absolute(dir)))
    }

    /// Whether the absolute `file` lies under any of `section`'s roots.
    pub fn covers(&self, section: &Autoload, file: &Path) -> bool {
        if self.excludes(section, file) {
            return false;
        }
        section
            .psr4
            .iter()
            .flat_map(|(_, dirs)| dirs)
            .chain(&section.classmap)
            .any(|dir| path_starts_with(file, &self.absolute(dir)))
            || section
                .files
                .iter()
                .any(|f| lexically_normalized(file) == self.absolute(f))
    }

    /// Installed packages that come from a `type: path` repository: local
    /// sibling packages whose `vendor/<name>` is a link into the
    /// repository's directory (a `*` in the URL matches any directory).
    pub fn path_repository_packages(&self) -> Vec<&InstalledPackage> {
        let roots: Vec<PathBuf> = self
            .repositories
            .iter()
            .filter(|r| r.kind == "path")
            .map(|r| {
                let fixed = r.url.split('*').next().unwrap_or(&r.url);
                let joined = self.absolute_root.join(fixed.trim_end_matches('/'));
                canonical::canonicalize(&joined).unwrap_or(joined)
            })
            .collect();
        self.installed
            .iter()
            .filter(|p| roots.iter().any(|root| p.install_dir.starts_with(root)))
            .collect()
    }

    pub fn installed_package(&self, name: &str) -> Option<&InstalledPackage> {
        self.installed
            .iter()
            .find(|p| p.name.eq_ignore_ascii_case(name))
    }
}

fn read_installed(installed: Option<&Path>) -> Vec<InstalledPackage> {
    let Some(installed_dir) = installed else {
        return Vec::new();
    };
    let composer_dir = installed_dir.join("composer");
    let Ok(text) = std::fs::read_to_string(composer_dir.join("installed.json")) else {
        return Vec::new();
    };
    let Some(Installed(packages)) = parse_document::<Installed>(&text) else {
        return Vec::new();
    };
    packages
        .iter()
        .filter_map(|package| {
            let name: String = package.read("name")?;
            let vendor_dir = resolve(&installed_dir.join(&name));
            let install_dir = package
                .read::<String>("install-path")
                .map(|rel| composer_dir.join(rel))
                .unwrap_or_else(|| vendor_dir.clone());
            let install_dir = resolve(&install_dir);
            Some(InstalledPackage {
                kind: package
                    .read("type")
                    .unwrap_or_else(|| "library".to_string()),
                auto_discovered: package.read::<Members>("extra").is_some_and(|extra| {
                    ["pest", "laravel", "phpstan", "psalm"]
                        .iter()
                        .any(|key| extra.contains_key(*key))
                }),
                has_bin: package
                    .read::<Vec<&RawValue>>("bin")
                    .is_some_and(|bin| !bin.is_empty()),
                provide: keys(package.read("provide")),
                replace: keys(package.read("replace")),
                include_path: include_directories(&install_dir, package.raw("include-path")),
                name,
                install_dir,
                vendor_dir,
            })
        })
        .collect()
}

/// The directories an `include-path` lists, below `base`. Composer trims the
/// `/` around an entry, so an entry never leaves `base` by being absolute.
fn include_directories(base: &Path, entries: Option<&RawValue>) -> Vec<PathBuf> {
    entries
        .map(strings)
        .unwrap_or_default()
        .iter()
        .map(|entry| match entry.trim_matches('/') {
            "" => base.to_path_buf(),
            relative => base.join(relative),
        })
        .collect()
}

/// `path` with `.` components dropped and each `..` removing the
/// component before it, without touching the filesystem.
pub fn lexically_normalized(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

/// The resolved path of `path`: its links followed and its `.` and `..`
/// taken out. A path that does not exist resolves through its nearest
/// ancestor that does, so a file that is gone keeps the path it had while
/// its directory stands. A path with no existing ancestor is returned as
/// given.
pub fn resolve(path: &Path) -> PathBuf {
    if let Ok(resolved) = canonical::canonicalize(path) {
        return resolved;
    }
    for ancestor in path.ancestors().skip(1) {
        if let Ok(resolved) = canonical::canonicalize(ancestor) {
            let rest = path.strip_prefix(ancestor).unwrap_or(Path::new(""));
            return canonical::form(lexically_normalized(&resolved.join(rest)));
        }
    }
    path.to_path_buf()
}

/// Component-wise prefix test of two paths spelled the same way (both
/// absolute, or both relative to one directory), after resolving `.`
/// and `..` lexically.
pub fn path_starts_with(path: &Path, prefix: &Path) -> bool {
    path.starts_with(prefix) || lexically_normalized(path).starts_with(lexically_normalized(prefix))
}

/// The members of a JSON object, each as the raw text of its value, in key
/// order, with a repeated key keeping its last value. A member is read as a
/// type only where it is used, and is otherwise only scanned.
#[derive(Default)]
struct Members<'de>(BTreeMap<Cow<'de, str>, &'de RawValue>);

impl<'de> Members<'de> {
    /// The raw text of member `key`, if the object has it.
    fn raw(&self, key: &str) -> Option<&'de RawValue> {
        self.0.get(key).copied()
    }

    /// Member `key` read as `T`; `None` when it is absent or of another shape.
    fn read<T: Deserialize<'de>>(&self, key: &str) -> Option<T> {
        self.raw(key).and_then(read_raw)
    }
}

impl<'de> Deref for Members<'de> {
    type Target = BTreeMap<Cow<'de, str>, &'de RawValue>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<'de> Shape<'de> for Members<'de> {
    fn seq<A: SeqAccess<'de>>(mut seq: A) -> Result<Self, A::Error> {
        while seq.next_element::<IgnoredAny>()?.is_some() {}
        Ok(Members::default())
    }

    fn map<A: MapAccess<'de>>(mut map: A) -> Result<Self, A::Error> {
        let mut members = BTreeMap::new();
        while let Some((key, raw)) = map.next_entry::<Cow<'de, str>, &'de RawValue>()? {
            members.insert(key, raw);
        }
        Ok(Members(members))
    }
}

impl<'de> Deserialize<'de> for Members<'de> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserialize_shape(deserializer)
    }
}

/// `raw` read as `T`; `None` when it is not a `T`.
fn read_raw<'de, T: Deserialize<'de>>(raw: &'de RawValue) -> Option<T> {
    serde_json::from_str(raw.get()).ok()
}

/// The string values of the manifest's `config` section, by key. Composer
/// reads a directory setting and its `{$name}` references from here; a value
/// that is not a string is left out, so a reference to it reads as absent.
pub(crate) fn declared_config(text: &str) -> BTreeMap<String, String> {
    let Some(json) = parse_document::<Members>(text) else {
        return BTreeMap::new();
    };
    let Some(config) = json.read::<Members>("config") else {
        return BTreeMap::new();
    };
    config
        .iter()
        .filter_map(|(key, raw)| Some((key.to_string(), read_raw::<String>(raw)?)))
        .collect()
}

/// The deepest nesting of arrays and objects that `json_decode` reads at its
/// default depth of 512.
const MAX_NESTING: usize = 511;

/// Parses a whole JSON document. Invalid JSON, and nesting deeper than
/// `MAX_NESTING`, give `None`.
fn parse_document<'de, T: Deserialize<'de>>(text: &'de str) -> Option<T> {
    if !nests_within(text, MAX_NESTING) {
        return None;
    }
    serde_json::from_str(text).ok()
}

/// Whether no array or object in `text` nests deeper than `limit`. Brackets
/// inside strings do not count.
fn nests_within(text: &str, limit: usize) -> bool {
    let mut depth: usize = 0;
    let mut in_string = false;
    let mut escaped = false;
    for byte in text.bytes() {
        if in_string {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            }
            continue;
        }
        match byte {
            b'"' => in_string = true,
            b'[' | b'{' => {
                depth += 1;
                if depth > limit {
                    return false;
                }
            }
            b']' | b'}' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    true
}

/// The strings of a `string | string[]` value: the string itself, or the
/// strings of an array, with other items skipped.
fn strings(raw: &RawValue) -> Vec<String> {
    if let Some(one) = read_raw::<String>(raw) {
        return vec![one];
    }
    read_raw::<Vec<&RawValue>>(raw)
        .unwrap_or_default()
        .into_iter()
        .filter_map(read_raw::<String>)
        .collect()
}

/// The keys of an object in key order; a value that is not an object has none.
fn keys(members: Option<Members<'_>>) -> Vec<String> {
    members
        .map(|members| members.keys().map(|key| key.to_string()).collect())
        .unwrap_or_default()
}

/// A type read from any JSON value. `seq` and `map` read an array or an
/// object and consume it whole; any other value reads as the default. A
/// value read in the middle of a stream goes through this, so a value of the
/// wrong shape is skipped rather than failing the whole file.
trait Shape<'de>: Default + Sized {
    fn seq<A: SeqAccess<'de>>(seq: A) -> Result<Self, A::Error>;
    fn map<A: MapAccess<'de>>(map: A) -> Result<Self, A::Error>;
}

fn deserialize_shape<'de, D: Deserializer<'de>, T: Shape<'de>>(
    deserializer: D,
) -> Result<T, D::Error> {
    deserializer.deserialize_any(ShapeVisitor(PhantomData))
}

struct ShapeVisitor<T>(PhantomData<T>);

impl<'de, T: Shape<'de>> Visitor<'de> for ShapeVisitor<T> {
    type Value = T;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("any JSON value")
    }

    fn visit_seq<A: SeqAccess<'de>>(self, seq: A) -> Result<T, A::Error> {
        T::seq(seq)
    }

    fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<T, A::Error> {
        T::map(map)
    }

    fn visit_bool<E: de::Error>(self, _: bool) -> Result<T, E> {
        Ok(T::default())
    }

    fn visit_i64<E: de::Error>(self, _: i64) -> Result<T, E> {
        Ok(T::default())
    }

    fn visit_u64<E: de::Error>(self, _: u64) -> Result<T, E> {
        Ok(T::default())
    }

    fn visit_f64<E: de::Error>(self, _: f64) -> Result<T, E> {
        Ok(T::default())
    }

    fn visit_str<E: de::Error>(self, _: &str) -> Result<T, E> {
        Ok(T::default())
    }

    fn visit_unit<E: de::Error>(self) -> Result<T, E> {
        Ok(T::default())
    }
}

/// The entries of a `packages` list; any other value has none.
#[derive(Default)]
struct Packages<'de>(Vec<Members<'de>>);

impl<'de> Shape<'de> for Packages<'de> {
    fn seq<A: SeqAccess<'de>>(mut seq: A) -> Result<Self, A::Error> {
        let mut packages = Vec::new();
        while let Some(package) = seq.next_element::<Members<'de>>()? {
            packages.push(package);
        }
        Ok(Packages(packages))
    }

    fn map<A: MapAccess<'de>>(mut map: A) -> Result<Self, A::Error> {
        while map.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {}
        Ok(Packages::default())
    }
}

impl<'de> Deserialize<'de> for Packages<'de> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserialize_shape(deserializer)
    }
}

/// `vendor/composer/installed.json`: a list of packages, or an object whose
/// `packages` is one. Any other value has none.
#[derive(Default)]
struct Installed<'de>(Vec<Members<'de>>);

impl<'de> Shape<'de> for Installed<'de> {
    fn seq<A: SeqAccess<'de>>(seq: A) -> Result<Self, A::Error> {
        Ok(Installed(Packages::seq(seq)?.0))
    }

    fn map<A: MapAccess<'de>>(mut map: A) -> Result<Self, A::Error> {
        let mut packages = Vec::new();
        while let Some(key) = map.next_key::<Cow<'de, str>>()? {
            if key == "packages" {
                packages = map.next_value::<Packages<'de>>()?.0;
            } else {
                map.next_value::<IgnoredAny>()?;
            }
        }
        Ok(Installed(packages))
    }
}

impl<'de> Deserialize<'de> for Installed<'de> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserialize_shape(deserializer)
    }
}

/// Finds the line a key is written on in the raw JSON text.
struct LineFinder<'t> {
    text: &'t str,
    lines: LineIndex,
}

impl<'t> LineFinder<'t> {
    fn new(text: &'t str) -> Self {
        LineFinder {
            text,
            lines: LineIndex::new(text.as_bytes()),
        }
    }

    fn line_at(&self, offset: usize) -> u32 {
        self.lines.line(offset as u32)
    }

    fn find_quoted_from(&self, needle: &str, from: usize) -> Option<usize> {
        let quoted = serde_json::to_string(needle).ok()?;
        self.text[from..].find(&quoted).map(|i| from + i)
    }

    /// The line of `"key"` inside the object keyed `"section"`, else the
    /// section's own line, else 1.
    fn key_in_section(&self, section: &str, key: &str) -> u32 {
        let Some(section_at) = self.find_quoted_from(section, 0) else {
            return 1;
        };
        match self.find_quoted_from(key, section_at + section.len() + 2) {
            Some(at) => self.line_at(at),
            None => self.line_at(section_at),
        }
    }

    /// The line of the string `value` at or after line `line`.
    fn line_of_string_after(&self, value: &str, line: u32) -> u32 {
        let start = self
            .lines
            .line_start(line)
            .filter(|_| line > 1)
            .unwrap_or(0) as usize;
        self.find_quoted_from(value, start)
            .map(|at| self.line_at(at))
            .unwrap_or(line)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_repository_packages_are_recognized_through_their_symlink() {
        let root = std::env::temp_dir().join(format!("phpstan-rs-manifest-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("packages/b/src")).unwrap();
        std::fs::create_dir_all(root.join("vendor/acme")).unwrap();
        std::fs::create_dir_all(root.join("vendor/composer")).unwrap();
        std::fs::create_dir_all(root.join("vendor/other/lib")).unwrap();
        crate::test_links::link_dir(root.join("packages/b"), root.join("vendor/acme/b"));
        std::fs::write(
            root.join("composer.json"),
            r#"{"name": "acme/app", "repositories": [{"type": "path", "url": "packages/*"}],
                "require": {"acme/b": "*", "other/lib": "*"}}"#,
        )
        .unwrap();
        std::fs::write(
            root.join("vendor/composer/installed.json"),
            r#"{"packages": [
                {"name": "acme/b", "type": "library", "install-path": "../acme/b"},
                {"name": "other/lib", "type": "library", "install-path": "../other/lib"}
            ]}"#,
        )
        .unwrap();
        std::fs::write(root.join("packages/b/src/B.php"), "<?php").unwrap();

        let root = canonical::canonicalize(&root).unwrap();
        let manifest = ComposerManifest::load(
            &root.join("composer.json"),
            &root,
            &root,
            Some(&root.join("vendor")),
        )
        .unwrap();
        let names: Vec<&str> = manifest
            .path_repository_packages()
            .iter()
            .map(|p| p.name.as_str())
            .collect();
        assert_eq!(names, vec!["acme/b"]);
        let index = crate::PackageIndex::new(vec![manifest]);
        for file in ["vendor/acme/b/src/B.php", "packages/b/src/B.php"] {
            let resolved = resolve(&root.join(file));
            let owner = index.owner_of_dir(resolved.parent().unwrap()).unwrap();
            assert_eq!(index.name(owner), Some("acme/b"), "{file}");
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn nesting_counts_only_brackets_outside_strings() {
        assert!(nests_within("[[]]", 2));
        assert!(!nests_within("[[]]", 1));
        assert!(nests_within(r#"{"a": "[[[[", "b": "\"[\""}"#, 1));
        assert!(nests_within(r#"{"a": "\\", "b": [1]}"#, 2));
        assert!(!nests_within(r#"{"a": "\\", "b": [1]}"#, 1));
        assert!(nests_within("]]]", 0));
    }
}
