//! `[[assertions]]` and `[[class_argument_returns]]` tables.

use super::helpers::*;
use crate::assertion_tables::{AssertWhen, AssertionEntry, ClassArgumentReturn};

fn types_of(src: &str, configure: impl FnOnce(&mut crate::registry::Registry)) -> Vec<String> {
    let mut v: Vec<String> = diagnostics_for_with(src, configure)
        .into_iter()
        .filter(|d| d.identifier == "phpstanRs.assertType")
        .filter_map(|d| {
            d.message
                .split("actual type \"")
                .nth(1)
                .map(|t| format!("Dumped type: {}", t.trim_end_matches('"')))
        })
        .collect();
    v.sort();
    v
}

#[test]
fn configured_assertions_apply_to_functions_and_conditional_returns() {
    let found = types_of(
        r#"<?php
use function PHPStan\Testing\assertType;
function ensure_int(mixed $value): void {}
function is_user(mixed $value): bool { return true; }
final class User {}
function f(mixed $a, mixed $b): void {
    ensure_int($a);
    assertType('?', $a);
    if (is_user($b)) { assertType('?', $b); }
}
"#,
        |r| {
            r.set_assertion_tables(
                vec![
                    AssertionEntry {
                        class: None,
                        function: Some("ensure_int".to_string()),
                        method: None,
                        parameter: "$value".to_string(),
                        ty: "int".to_string(),
                        when: AssertWhen::Always,
                    },
                    AssertionEntry {
                        class: None,
                        function: Some("is_user".to_string()),
                        method: None,
                        parameter: "value".to_string(),
                        ty: "User".to_string(),
                        when: AssertWhen::True,
                    },
                ],
                vec![],
            );
            r.apply_assertion_table();
        },
    );
    assert_eq!(found, vec!["Dumped type: User", "Dumped type: int"]);
}

#[test]
fn mocks_and_configured_locators_return_the_named_class() {
    let found = types_of(
        r#"<?php
namespace Mockery { interface MockInterface {} class Mockery { public static function mock(mixed ...$args): mixed { return null; } } }
namespace { class Mockery extends \Mockery\Mockery {} }
namespace App {
use function PHPStan\Testing\assertType;
final class Mailer {}
final class Locator { public function get(string $id): mixed { return null; } }
function f(Locator $l): void {
    assertType('?', \Mockery::mock(Mailer::class));
    assertType('?', $l->get(Mailer::class));
    assertType('?', $l->get('nope'));
}
}"#,
        |r| {
            r.set_assertion_tables(
                vec![],
                vec![ClassArgumentReturn {
                    class: "App\\Locator".to_string(),
                    method: "get".to_string(),
                    argument: 1,
                    intersect: vec![],
                }],
            )
        },
    );
    assert_eq!(
        found,
        vec![
            "Dumped type: App\\Mailer",
            "Dumped type: App\\Mailer&Mockery\\MockInterface",
            "Dumped type: mixed",
        ]
    );
}
