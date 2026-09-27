//! Extension activation.

use config::{ConfigFile, Extension, Schema};

use super::strict_rules::StrictRulesSettings;
use super::*;

fn schema() -> Schema {
    Schema::new()
        .section::<extras::ExtrasSettings>()
        .section::<strict_rules::StrictRulesSettings>()
        .section::<deprecation_rules::DeprecationRulesSettings>()
}

fn nothing_installed(_: &str) -> bool {
    false
}

fn strict_rules_installed(package: &str) -> bool {
    package == "phpstan/phpstan-strict-rules"
}

#[test]
fn an_extension_is_active_when_one_of_its_packages_is_installed() {
    let settings = StrictRulesSettings::default();
    assert!(settings.is_active(&strict_rules_installed));
    assert!(!settings.is_active(&nothing_installed));
}

#[test]
fn enabled_overrides_detection_either_way() {
    let on = StrictRulesSettings {
        enabled: Some(true),
        ..StrictRulesSettings::default()
    };
    let off = StrictRulesSettings {
        enabled: Some(false),
        ..StrictRulesSettings::default()
    };
    assert!(on.is_active(&nothing_installed));
    assert!(!off.is_active(&strict_rules_installed));
}

#[test]
fn the_extras_have_no_package_and_are_off_unless_enabled() {
    let file = ConfigFile::parse("", &schema()).unwrap();
    let active = ActiveExtensions::resolve(&file, &|_| true).unwrap();
    assert!(!active.extras);
    let file = ConfigFile::parse("[ext.extras]\nenabled = true\n", &schema()).unwrap();
    assert!(
        ActiveExtensions::resolve(&file, &nothing_installed)
            .unwrap()
            .extras
    );
}

#[test]
fn resolve_reads_each_table_and_detects_the_rest() {
    let file = ConfigFile::parse("[ext.deprecation-rules]\nenabled = false\n", &schema()).unwrap();
    let active = ActiveExtensions::resolve(&file, &|p| {
        p == "phpstan/phpstan-strict-rules" || p == "phpstan/phpstan-deprecation-rules"
    })
    .unwrap();
    assert!(active.strict_rules);
    assert!(!active.deprecation_rules);
    assert!(!active.shipmonk);
}
