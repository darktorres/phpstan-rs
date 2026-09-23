//! Removed and deprecated ini directives.

use super::helpers::*;

fn ini(src: &str, version: Option<(u8, u8)>, deprecations: bool) -> Vec<String> {
    let mut v: Vec<String> = diagnostics_for_with(src, |r| {
        r.php_version = version;
        r.check_deprecated_usage = deprecations;
    })
    .into_iter()
    .filter(|d| d.identifier.starts_with("ini."))
    .map(|d| format!("{} {}", d.span.start.line, d.identifier))
    .collect();
    v.sort();
    v
}

const SRC: &str = r#"<?php
function f(bool $b): void {
    ini_get('track_errors');
    ini_set('assert.active', '1');
    ini_get('assert.active');
    ini_restore(option: 'mysqli.reconnect');
    ini_set($b ? 'allow_url_include' : 'memory_limit', '1');
    ini_get('TRACK_ERRORS');
    ini_get('memory_limit');
}
"#;

#[test]
fn removed_directives_follow_the_configured_version() {
    assert_eq!(
        ini(SRC, Some((8, 3)), false),
        vec!["3 ini.removed", "6 ini.removed"]
    );
    assert_eq!(ini(SRC, Some((7, 4)), false), Vec::<String>::new());
    assert_eq!(ini(SRC, None, true), Vec::<String>::new());
}

#[test]
fn deprecated_directives_need_the_deprecation_gate() {
    assert_eq!(
        ini(SRC, Some((8, 3)), true),
        vec![
            "3 ini.removed",
            "4 ini.deprecated",
            "6 ini.removed",
            "7 ini.deprecated"
        ]
    );
    assert_eq!(
        ini(SRC, Some((7, 4)), true),
        vec!["3 ini.deprecated", "7 ini.deprecated"]
    );
}

#[test]
fn messages_name_the_version() {
    let d = diagnostics_for_with(
        "<?php\nini_restore('mysqli.reconnect');\nini_set('session.sid_length', '48');\n",
        |r| {
            r.php_version = Some((8, 4));
            r.check_deprecated_usage = true;
        },
    );
    let messages: Vec<&str> = d
        .iter()
        .filter(|d| d.identifier.starts_with("ini."))
        .map(|d| d.message.as_str())
        .collect();
    assert_eq!(
        messages,
        vec![
            "Call to ini_restore() with ini directive mysqli.reconnect, removed in PHP 8.2; it does nothing.",
            "Call to ini_set(): Changing ini directive session.sid_length is deprecated since PHP 8.4.",
        ]
    );
}
