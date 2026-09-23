//! The root package's `composer.json` and the installed packages listed
//! in `vendor/composer/installed.json`, read for the dependency-hygiene
//! checks: declared requirements, autoload roots, `provide`, `scripts`,
//! `config.platform.php`, path repositories, and which installed package
//! owns a given file.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::Value;

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
    /// Whether `file` is under an `exclude-from-classmap` path.
    pub fn excludes(&self, file: &Path) -> bool {
        self.excluded.iter().any(|dir| path_starts_with(file, dir))
    }

    fn read(section: Option<&Value>, root: &Path) -> Autoload {
        let Some(section) = section else {
            return Autoload::default();
        };
        let paths = |v: &Value| -> Vec<PathBuf> {
            match v {
                Value::String(s) => vec![root.join(s)],
                Value::Array(items) => items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(|s| root.join(s))
                    .collect(),
                _ => Vec::new(),
            }
        };
        let psr4 = section
            .get("psr-4")
            .and_then(Value::as_object)
            .map(|map| {
                map.iter()
                    .map(|(prefix, dirs)| (prefix.clone(), paths(dirs)))
                    .collect()
            })
            .unwrap_or_default();
        let excluded = match section.get("exclude-from-classmap") {
            Some(Value::String(s)) => vec![s.as_str()],
            Some(Value::Array(items)) => items.iter().filter_map(Value::as_str).collect(),
            _ => Vec::new(),
        }
        .into_iter()
        .map(|p| root.join(p.trim_start_matches('/').trim_end_matches(['/', '*'])))
        .collect();
        Autoload {
            psr4,
            classmap: section.get("classmap").map(paths).unwrap_or_default(),
            files: section.get("files").map(paths).unwrap_or_default(),
            excluded,
        }
    }

    /// Whether `file` lies under any of this section's roots.
    pub fn covers(&self, file: &Path) -> bool {
        if self.excludes(file) {
            return false;
        }
        self.psr4
            .iter()
            .flat_map(|(_, dirs)| dirs)
            .chain(&self.classmap)
            .any(|dir| path_starts_with(file, dir))
            || self.files.iter().any(|f| same_path(f, file))
    }
}

/// An installed package from `installed.json`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledPackage {
    pub name: String,
    pub kind: String,
    /// Canonical install directory.
    pub install_dir: PathBuf,
    /// The `vendor/<name>` directory as written, which can be a symlink
    /// to `install_dir` for a path repository.
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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Repository {
    pub kind: String,
    pub url: String,
}

#[derive(Debug, Default, Clone)]
pub struct ComposerManifest {
    pub root: PathBuf,
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
}

impl ComposerManifest {
    /// Reads `root/composer.json` (and `root/vendor/composer/installed.json`
    /// when present); `None` without a readable, parseable composer.json.
    pub fn load(root: &Path) -> Option<ComposerManifest> {
        let path = root.join("composer.json");
        let text = std::fs::read_to_string(&path).ok()?;
        let json: Value = serde_json::from_str(&text).ok()?;
        let lines = LineFinder::new(&text);
        let mut manifest = ComposerManifest {
            root: root.to_path_buf(),
            path,
            name: json.get("name").and_then(Value::as_str).map(str::to_string),
            ..ComposerManifest::default()
        };
        for (section, out) in [
            ("require", &mut manifest.require),
            ("require-dev", &mut manifest.require_dev),
        ] {
            if let Some(map) = json.get(section).and_then(Value::as_object) {
                for (package, constraint) in map {
                    out.push(Located {
                        value: package.clone(),
                        line: lines.key_in_section(section, package),
                    });
                    if let Some(c) = constraint.as_str() {
                        manifest
                            .constraints
                            .insert(package.to_ascii_lowercase(), c.to_string());
                    }
                }
            }
        }
        manifest.autoload = Autoload::read(json.get("autoload"), root);
        manifest.autoload_dev = Autoload::read(json.get("autoload-dev"), root);
        if let Some(map) = json.get("provide").and_then(Value::as_object) {
            for package in map.keys() {
                manifest.provide.push(Located {
                    value: package.clone(),
                    line: lines.key_in_section("provide", package),
                });
            }
        }
        if let Some(map) = json.get("suggest").and_then(Value::as_object) {
            manifest.suggest = map.keys().cloned().collect();
        }
        if let Some(map) = json.get("scripts").and_then(Value::as_object) {
            for (name, commands) in map {
                let line = lines.key_in_section("scripts", name);
                let commands: Vec<&str> = match commands {
                    Value::String(s) => vec![s.as_str()],
                    Value::Array(items) => items.iter().filter_map(Value::as_str).collect(),
                    _ => Vec::new(),
                };
                for command in commands {
                    manifest.scripts.push(Located {
                        value: (name.clone(), command.to_string()),
                        line: lines.line_of_string_after(command, line),
                    });
                }
            }
        }
        if let Some(php) = json
            .get("config")
            .and_then(|c| c.get("platform"))
            .and_then(|p| p.get("php"))
            .and_then(Value::as_str)
        {
            manifest.platform_php = Some(Located {
                value: php.to_string(),
                line: lines.key_in_section("platform", "php"),
            });
        }
        if let Some(repos) = json.get("repositories") {
            let entries: Vec<&Value> = match repos {
                Value::Array(items) => items.iter().collect(),
                Value::Object(map) => map.values().collect(),
                _ => Vec::new(),
            };
            for repo in entries {
                if let (Some(kind), Some(url)) = (
                    repo.get("type").and_then(Value::as_str),
                    repo.get("url").and_then(Value::as_str),
                ) {
                    manifest.repositories.push(Repository {
                        kind: kind.to_string(),
                        url: url.to_string(),
                    });
                }
            }
        }
        manifest.installed = read_installed(root);
        Some(manifest)
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

    /// The installed package whose directory contains `file`.
    pub fn owning_package(&self, file: &Path) -> Option<&InstalledPackage> {
        let canonical = std::fs::canonicalize(file).ok();
        self.installed
            .iter()
            .filter(|p| {
                path_starts_with(file, &p.vendor_dir)
                    || canonical
                        .as_deref()
                        .is_some_and(|c| path_starts_with(c, &p.install_dir))
            })
            .max_by_key(|p| p.install_dir.components().count())
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
                let joined = self.root.join(fixed.trim_end_matches('/'));
                std::fs::canonicalize(&joined).unwrap_or(joined)
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

fn read_installed(root: &Path) -> Vec<InstalledPackage> {
    let composer_dir = root.join("vendor").join("composer");
    let Ok(text) = std::fs::read_to_string(composer_dir.join("installed.json")) else {
        return Vec::new();
    };
    let Ok(json) = serde_json::from_str::<Value>(&text) else {
        return Vec::new();
    };
    let packages: Vec<&Value> = match &json {
        Value::Array(items) => items.iter().collect(),
        Value::Object(map) => map
            .get("packages")
            .and_then(Value::as_array)
            .map(|items| items.iter().collect())
            .unwrap_or_default(),
        _ => Vec::new(),
    };
    let names = |v: Option<&Value>| -> Vec<String> {
        v.and_then(Value::as_object)
            .map(|m| m.keys().cloned().collect())
            .unwrap_or_default()
    };
    packages
        .into_iter()
        .filter_map(|p| {
            let name = p.get("name")?.as_str()?.to_string();
            let vendor_dir = root.join("vendor").join(&name);
            let install_dir = p
                .get("install-path")
                .and_then(Value::as_str)
                .map(|rel| composer_dir.join(rel))
                .unwrap_or_else(|| vendor_dir.clone());
            let install_dir = std::fs::canonicalize(&install_dir).unwrap_or(install_dir);
            Some(InstalledPackage {
                kind: p
                    .get("type")
                    .and_then(Value::as_str)
                    .unwrap_or("library")
                    .to_string(),
                auto_discovered: p
                    .get("extra")
                    .and_then(Value::as_object)
                    .is_some_and(|extra| {
                        ["pest", "laravel", "phpstan", "psalm"]
                            .iter()
                            .any(|key| extra.contains_key(*key))
                    }),
                has_bin: p
                    .get("bin")
                    .and_then(Value::as_array)
                    .is_some_and(|b| !b.is_empty()),
                provide: names(p.get("provide")),
                replace: names(p.get("replace")),
                name,
                install_dir,
                vendor_dir,
            })
        })
        .collect()
}

/// Component-wise prefix test that tolerates `./` and relative vs
/// absolute spellings by comparing canonical forms when both exist.
pub fn path_starts_with(path: &Path, prefix: &Path) -> bool {
    if path.starts_with(prefix) {
        return true;
    }
    let normalize = |p: &Path| -> PathBuf {
        let absolute = if p.is_absolute() {
            p.to_path_buf()
        } else {
            std::env::current_dir()
                .map(|cwd| cwd.join(p))
                .unwrap_or_else(|_| p.to_path_buf())
        };
        let mut out = PathBuf::new();
        for component in absolute.components() {
            match component {
                std::path::Component::CurDir => {}
                std::path::Component::ParentDir => {
                    out.pop();
                }
                other => out.push(other),
            }
        }
        out
    };
    normalize(path).starts_with(normalize(prefix))
}

fn same_path(a: &Path, b: &Path) -> bool {
    path_starts_with(a, b) && path_starts_with(b, a)
}

/// Finds the line a key is written on in the raw JSON text.
struct LineFinder<'t> {
    text: &'t str,
}

impl<'t> LineFinder<'t> {
    fn new(text: &'t str) -> Self {
        LineFinder { text }
    }

    fn line_at(&self, offset: usize) -> u32 {
        self.text[..offset].matches('\n').count() as u32 + 1
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
            .text
            .match_indices('\n')
            .nth(line.saturating_sub(2) as usize)
            .map(|(i, _)| i)
            .filter(|_| line > 1)
            .unwrap_or(0);
        self.find_quoted_from(value, start)
            .map(|at| self.line_at(at))
            .unwrap_or(line)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn path_repository_packages_are_recognized_through_their_symlink() {
        let root = std::env::temp_dir().join(format!("phpstan-rs-manifest-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("packages/b/src")).unwrap();
        std::fs::create_dir_all(root.join("vendor/acme")).unwrap();
        std::fs::create_dir_all(root.join("vendor/composer")).unwrap();
        std::fs::create_dir_all(root.join("vendor/other/lib")).unwrap();
        std::os::unix::fs::symlink(root.join("packages/b"), root.join("vendor/acme/b")).unwrap();
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

        let manifest = ComposerManifest::load(&root).unwrap();
        let names: Vec<&str> = manifest
            .path_repository_packages()
            .iter()
            .map(|p| p.name.as_str())
            .collect();
        assert_eq!(names, vec!["acme/b"]);
        let owner = manifest.owning_package(&root.join("vendor/acme/b/src/B.php"));
        assert_eq!(owner.map(|p| p.name.as_str()), Some("acme/b"));
        let owner = manifest.owning_package(&root.join("packages/b/src/B.php"));
        assert_eq!(owner.map(|p| p.name.as_str()), Some("acme/b"));
        let _ = std::fs::remove_dir_all(&root);
    }
}
