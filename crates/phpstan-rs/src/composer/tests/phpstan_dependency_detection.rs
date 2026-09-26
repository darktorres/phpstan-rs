//! Detecting a `phpstan/phpstan`/`phpstan/extension-installer`
//! Composer dependency, the gate `checker::phpstan_api` runs behind.

use super::super::*;
use super::helpers::*;

#[test]
fn no_composer_json_means_not_detected() {
    let dir = TempDir::new("api_no_composer_json");
    assert!(!PackageContext::load(dir.path(), dir.path()).requires_phpstan());
}

#[test]
fn a_composer_json_with_unrelated_dependencies_is_not_detected() {
    let dir = TempDir::new("api_unrelated_deps");
    dir.write(
        "composer.json",
        r#"{
            "require": {
                "php": ">=8.1",
                "symfony/console": "^6.0"
            }
        }"#,
    );
    assert!(!PackageContext::load(dir.path(), dir.path()).requires_phpstan());
}

#[test]
fn phpstan_phpstan_in_require_is_detected() {
    let dir = TempDir::new("api_require_phpstan");
    dir.write(
        "composer.json",
        r#"{
            "require": {
                "php": ">=8.1",
                "phpstan/phpstan": "^2.0"
            }
        }"#,
    );
    assert!(PackageContext::load(dir.path(), dir.path()).requires_phpstan());
}

#[test]
fn phpstan_extension_installer_in_require_dev_is_detected() {
    let dir = TempDir::new("api_require_dev_installer");
    dir.write(
        "composer.json",
        r#"{
            "require-dev": {
                "phpstan/extension-installer": "^1.0"
            }
        }"#,
    );
    assert!(PackageContext::load(dir.path(), dir.path()).requires_phpstan());
}

#[test]
fn malformed_json_degrades_to_not_detected_rather_than_panicking() {
    let dir = TempDir::new("api_malformed_json");
    dir.write("composer.json", "{ this is not valid json");
    assert!(!PackageContext::load(dir.path(), dir.path()).requires_phpstan());
}
