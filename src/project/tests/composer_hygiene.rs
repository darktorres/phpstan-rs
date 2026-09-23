//! `Project::check_composer_hygiene` against a fixture Composer project.

use rustc_hash::FxHashMap;
use std::path::{Path, PathBuf};

use crate::composer::manifest::ComposerManifest;

use super::super::*;

struct Fixture {
    root: PathBuf,
    files: Vec<(PathBuf, bool)>,
}

impl Fixture {
    fn new(name: &str, composer_json: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("phpstan-rs-composer-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("composer.json"), composer_json).unwrap();
        Fixture {
            root,
            files: Vec::new(),
        }
    }

    fn installed(self, packages: &[(&str, &str)]) -> Self {
        let entries: Vec<String> = packages
            .iter()
            .map(|(name, kind)| {
                format!(r#"{{"name": "{name}", "type": "{kind}", "install-path": "../{name}"}}"#)
            })
            .collect();
        let dir = self.root.join("vendor/composer");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("installed.json"),
            format!(r#"{{"packages": [{}]}}"#, entries.join(",")),
        )
        .unwrap();
        for (name, _) in packages {
            std::fs::create_dir_all(self.root.join("vendor").join(name)).unwrap();
        }
        self
    }

    fn file(mut self, relative: &str, source: &str, analyzed: bool) -> Self {
        let path = self.root.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, source).unwrap();
        self.files.push((path, analyzed));
        self
    }

    fn check(&self) -> Vec<String> {
        let mut project = Project::new();
        for (path, analyzed) in &self.files {
            let source = std::fs::read_to_string(path).unwrap();
            if *analyzed {
                project.load_analyzed_file(path.clone(), &source).unwrap();
            } else {
                project.load_file(path.clone(), &source).unwrap();
            }
        }
        project.finish_loading();
        let manifest = ComposerManifest::load(&self.root).expect("composer.json loads");
        let results: FxHashMap<PathBuf, Vec<Diagnostic>> =
            project.check_composer_hygiene(&manifest);
        let mut out: Vec<String> = results
            .into_iter()
            .flat_map(|(path, diagnostics)| {
                let relative = path
                    .strip_prefix(&self.root)
                    .map(Path::to_path_buf)
                    .unwrap_or(path);
                diagnostics.into_iter().map(move |d| {
                    format!(
                        "{}:{} {}: {}",
                        relative.display(),
                        d.span.start.line,
                        d.identifier,
                        d.message.replace(&*self.root.to_string_lossy(), "<root>")
                    )
                })
            })
            .collect();
        out.sort();
        out
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

const COMPOSER_JSON: &str = r#"{
    "name": "acme/app",
    "require": {
        "php": "^8.1",
        "acme/used": "^1.0",
        "acme/unused": "^1.0",
        "acme/plugin": "^1.0"
    },
    "require-dev": {
        "acme/dev-tool": "^1.0"
    },
    "autoload": {"psr-4": {"App\\": "src/"}},
    "autoload-dev": {"psr-4": {"App\\Tests\\": "tests/"}}
}"#;

fn base(name: &str) -> Fixture {
    Fixture::new(name, COMPOSER_JSON)
        .installed(&[
            ("acme/used", "library"),
            ("acme/unused", "library"),
            ("acme/plugin", "composer-plugin"),
            ("acme/dev-tool", "library"),
            ("acme/transitive", "library"),
        ])
        .file(
            "vendor/acme/used/src/Used.php",
            "<?php namespace Acme\\Used; class Used {}",
            false,
        )
        .file(
            "vendor/acme/unused/src/Unused.php",
            "<?php namespace Acme\\Unused; class Unused {}",
            false,
        )
        .file(
            "vendor/acme/dev-tool/src/Tool.php",
            "<?php namespace Acme\\DevTool; class Tool {}",
            false,
        )
        .file(
            "vendor/acme/transitive/src/Helper.php",
            "<?php namespace Acme\\Transitive; class Helper {} function helper(): void {}",
            false,
        )
}

#[test]
fn dependency_declarations_are_checked_against_usage() {
    let found = base("deps")
        .file(
            "src/Service.php",
            "<?php namespace App;\nuse Acme\\Used\\Used;\nuse Acme\\Transitive\\Helper;\nuse Acme\\DevTool\\Tool;\nfinal class Service {\n    public function run(): void {\n        new Used();\n        new Helper();\n        \\Acme\\Transitive\\helper();\n        new Tool();\n    }\n}\n",
            true,
        )
        .file(
            "tests/ServiceTest.php",
            "<?php namespace App\\Tests;\nuse Acme\\DevTool\\Tool;\nfinal class ServiceTest {\n    public function t(): void { new Tool(); }\n}\n",
            true,
        )
        .check();
    assert_eq!(
        found,
        vec![
            "composer.json:6 unused.composerDependency: Package acme/unused is in require, but the analysed code uses none of its symbols.",
            "src/Service.php:10 misplaced.composerDependency: Class Acme\\DevTool\\Tool is declared in package acme/dev-tool, which composer.json only requires in require-dev, but this file is autoloaded in production.",
            "src/Service.php:8 undeclared.composerDependency: Class Acme\\Transitive\\Helper is declared in package acme/transitive, which composer.json doesn't require; it's only installed as another package's dependency.",
        ]
    );
}

#[test]
fn a_class_name_string_counts_as_using_its_package() {
    let found = base("strings")
        .file(
            "src/Wiring.php",
            "<?php namespace App;\nfinal class Wiring {\n    public const SERVICES = ['Acme\\\\Used\\\\Used', 'Acme\\\\Unused\\\\Unused'];\n}\n",
            true,
        )
        .check();
    assert!(
        !found
            .iter()
            .any(|f| f.contains("acme/used ") || f.contains("acme/unused ")),
        "{found:?}"
    );
}

#[test]
fn production_code_using_an_autoload_dev_class_is_flagged() {
    let found = base("autoload-dev")
        .file(
            "tests/Support/Fake.php",
            "<?php namespace App\\Tests\\Support; final class Fake {}",
            true,
        )
        .file(
            "src/Uses.php",
            "<?php namespace App;\nfinal class Uses {\n    public function f(): object { return new \\App\\Tests\\Support\\Fake(); }\n}\n",
            true,
        )
        .check();
    assert!(
        found.contains(&"src/Uses.php:3 misplaced.autoloadDevSymbol: Class App\\Tests\\Support\\Fake is only autoloaded through autoload-dev, but this file is autoloaded in production; `composer install --no-dev` can't load it.".to_string()),
        "{found:?}"
    );
}

#[test]
fn psr4_locations_are_checked() {
    let found = base("psr4")
        .file(
            "src/Http/Controller.php",
            "<?php namespace App\\Http; final class Controller {}",
            true,
        )
        .file(
            "src/http/Router.php",
            "<?php namespace App\\Http; final class Router {}",
            true,
        )
        .file(
            "src/Misplaced.php",
            "<?php namespace App\\Other; final class Elsewhere {}",
            true,
        )
        .file(
            "src/Outside.php",
            "<?php namespace Unrelated; final class Outside {}",
            true,
        )
        .check();
    let psr4: Vec<&String> = found.iter().filter(|f| f.contains("psr4.")).collect();
    assert_eq!(psr4.len(), 3, "{found:?}");
    assert!(
        psr4.iter()
            .any(|f| f.starts_with("src/http/Router.php:1 psr4.caseMismatch")),
        "{psr4:?}"
    );
    assert!(
        psr4.iter()
            .any(|f| f.starts_with("src/Misplaced.php:1 psr4.pathMismatch")
                && f.contains("src/Other/Elsewhere.php")),
        "{psr4:?}"
    );
    assert!(
        psr4.iter()
            .any(|f| f.starts_with("src/Outside.php:1 psr4.pathMismatch")
                && f.contains("doesn't start with that prefix")),
        "{psr4:?}"
    );
}

#[test]
fn extensions_must_be_required_unless_probed_or_suggested() {
    let found = Fixture::new(
        "ext",
        r#"{"name": "acme/app", "require": {"ext-json": "*"}, "suggest": {"ext-imagick": "optional"},
            "autoload": {"psr-4": {"App\\": "src/"}}}"#,
    )
    .file(
        "src/Ext.php",
        "<?php namespace App;\nfinal class Ext {\n    public function f(): void {\n        mb_strlen('x');\n        mb_substr('x', 0);\n        json_encode([]);\n        strlen('x');\n        new \\Imagick();\n        if (extension_loaded('gd')) { imagecreate(1, 1); }\n    }\n}\n",
        true,
    )
    .check();
    assert_eq!(
        found,
        vec![
            "src/Ext.php:4 ext.notRequired: Function mb_strlen() belongs to the mbstring extension, but composer.json doesn't require ext-mbstring."
        ]
    );
}

#[test]
fn provide_scripts_and_platform_are_checked() {
    let found = Fixture::new(
        "manifest",
        r#"{
    "name": "acme/lib",
    "require": {"php": "^8.3"},
    "provide": {
        "psr/log-implementation": "1.0",
        "psr/container-implementation": "1.0"
    },
    "scripts": {
        "post-install-cmd": "App\\Installer::install",
        "check": ["App\\Installer::check", "App\\Missing::run", "App\\Installer::missing", "phpunit --colors"]
    },
    "config": {"platform": {"php": "8.1.0"}},
    "autoload": {"psr-4": {"App\\": "src/"}}
}"#,
    )
    .file(
        "src/Installer.php",
        "<?php namespace App;\nfinal class Installer {\n    public static function install(): void {}\n    public function check(): void {}\n}\n",
        true,
    )
    .file(
        "src/Logger.php",
        "<?php namespace Psr\\Container; interface ContainerInterface {}\nnamespace App; final class Container implements \\Psr\\Container\\ContainerInterface {}\n",
        true,
    )
    .check();
    let manifest_level: Vec<String> = found
        .into_iter()
        .filter(|f| f.starts_with("composer.json"))
        .collect();
    let mut expected: Vec<String> = [
        "composer.json:10 composerScript.notPublicStatic: Composer script check calls App\\Installer::check, but App\\Installer::check() is not static; Composer calls it statically from outside the class.",
        "composer.json:10 composerScript.notFound: Composer script check calls App\\Missing::run, but class App\\Missing doesn't exist.",
        "composer.json:10 composerScript.notFound: Composer script check calls App\\Installer::missing, but method App\\Installer::missing() doesn't exist.",
        "composer.json:12 composer.platformPhpInconsistentWithRequire: config.platform.php 8.1.0 doesn't satisfy require.php ^8.3; Composer resolves dependencies for a PHP version the package doesn't support.",
        "composer.json:5 provide.unimplemented: composer.json provides psr/log-implementation, but no class implements Psr\\Log\\LoggerInterface.",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    expected.sort();
    assert_eq!(manifest_level, expected);
}

#[test]
fn dev_only_usage_and_unused_dev_dependencies_are_reported() {
    let fixture = Fixture::new(
        "dev-only",
        r#"{
    "name": "acme/app",
    "require": {"acme/assert": "^1.0"},
    "require-dev": {"acme/faker": "^1.0", "acme/tool": "^1.0", "acme/cli": "^1.0"},
    "autoload": {"psr-4": {"App\\": "src/"}},
    "autoload-dev": {"psr-4": {"App\\Tests\\": "tests/"}}
}"#,
    );
    let dir = fixture.root.join("vendor/composer");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("installed.json"),
        r#"{"packages": [
            {"name": "acme/assert", "type": "library", "install-path": "../acme/assert"},
            {"name": "acme/faker", "type": "library", "install-path": "../acme/faker"},
            {"name": "acme/tool", "type": "library", "install-path": "../acme/tool"},
            {"name": "acme/cli", "type": "library", "install-path": "../acme/cli", "bin": ["bin/cli"]}
        ]}"#,
    )
    .unwrap();
    let found = fixture
        .file(
            "vendor/acme/assert/src/Assert.php",
            "<?php namespace Acme\\Assert; final class Assert {}",
            false,
        )
        .file(
            "vendor/acme/faker/src/Faker.php",
            "<?php namespace Acme\\Faker; final class Faker {}",
            false,
        )
        .file(
            "vendor/acme/tool/src/Tool.php",
            "<?php namespace Acme\\Tool; final class Tool {}",
            false,
        )
        .file(
            "tests/SomeTest.php",
            "<?php namespace App\\Tests;\nfinal class SomeTest {\n    public function t(): void { new \\Acme\\Assert\\Assert(); new \\Acme\\Faker\\Faker(); }\n}\n",
            true,
        )
        .check();
    assert_eq!(
        found,
        vec![
            "composer.json:3 devOnly.composerDependency: Package acme/assert is in require, but only code autoloaded through autoload-dev uses it; it belongs in require-dev.",
            "composer.json:4 unused.composerDependency: Package acme/tool is in require-dev, but the analysed code uses none of its symbols.",
        ]
    );
}
