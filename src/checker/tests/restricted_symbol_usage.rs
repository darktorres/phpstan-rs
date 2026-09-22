//! The declarative, config-driven `restricted_symbols` list - one
//! behavior-per-test, mirroring `internal_usage.rs`'s own test style.

use super::helpers::*;
use crate::restricted_symbols::{RestrictedSymbolEntry, RestrictedSymbolKind, RestrictedSymbols};

fn with_entries(src: &str, entries: Vec<RestrictedSymbolEntry>) -> Vec<crate::checker::Diagnostic> {
    diagnostics_for_with(src, move |r| {
        r.restricted_symbols = RestrictedSymbols::compile(&entries);
    })
}

fn entry(
    kind: RestrictedSymbolKind,
    class: Option<&str>,
    name: &str,
    message: &str,
) -> RestrictedSymbolEntry {
    RestrictedSymbolEntry {
        kind,
        class: class.map(str::to_string),
        name: name.to_string(),
        message: message.to_string(),
        allow_in: Vec::new(),
        allow_except_in: Vec::new(),
        allow_in_class_with_attributes: Vec::new(),
        allow_except_in_class_with_attributes: Vec::new(),
        allow_in_methods_with_attributes: Vec::new(),
        allow_except_in_methods_with_attributes: Vec::new(),
        allow_in_class_with_method_attributes: Vec::new(),
        allow_except_in_class_with_method_attributes: Vec::new(),
        allow_in_use: false,
    }
}

fn entry_with(
    kind: RestrictedSymbolKind,
    class: Option<&str>,
    name: &str,
    message: &str,
    f: impl FnOnce(&mut RestrictedSymbolEntry),
) -> RestrictedSymbolEntry {
    let mut e = entry(kind, class, name, message);
    f(&mut e);
    e
}

#[test]
fn absent_from_config_never_fires() {
    let d = diagnostics_for(r#"<?php var_dump(1);"#);
    assert!(
        !d.iter()
            .any(|x| x.identifier.starts_with("restrictedSymbol")),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_restricted_function_call_is_flagged() {
    let d = with_entries(
        r#"<?php var_dump(1);"#,
        vec![entry(
            RestrictedSymbolKind::Function,
            None,
            "var_dump",
            "no debug dumps",
        )],
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "restrictedSymbol.function"
                && x.message.contains("no debug dumps")),
        "got: {d:?}"
    );
}

#[test]
fn function_lookup_is_case_insensitive() {
    let d = with_entries(
        r#"<?php VAR_DUMP(1);"#,
        vec![entry(
            RestrictedSymbolKind::Function,
            None,
            "var_dump",
            "no debug dumps",
        )],
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "restrictedSymbol.function"),
        "got: {d:?}"
    );
}

#[test]
fn a_restricted_method_call_is_flagged() {
    let d = with_entries(
        r#"<?php
            class Db { public function query(string $sql) {} }
            function f(Db $db): void { $db->query('select 1'); }
        "#,
        vec![entry(
            RestrictedSymbolKind::Method,
            Some("Db"),
            "query",
            "use the connection service",
        )],
    );
    assert!(
        d.iter().any(|x| x.identifier == "restrictedSymbol.method"),
        "got: {d:?}"
    );
}

#[test]
fn a_restricted_static_method_call_is_flagged() {
    let d = with_entries(
        r#"<?php
            class Db { public static function query(string $sql) {} }
            function f(): void { Db::query('select 1'); }
        "#,
        vec![entry(
            RestrictedSymbolKind::StaticMethod,
            Some("Db"),
            "query",
            "use the connection service",
        )],
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "restrictedSymbol.staticMethod"),
        "got: {d:?}"
    );
}

#[test]
fn a_restricted_property_access_is_flagged() {
    let d = with_entries(
        r#"<?php
            class Legacy { public string $password = ""; }
            function f(Legacy $l): string { return $l->password; }
        "#,
        vec![entry(
            RestrictedSymbolKind::Property,
            Some("Legacy"),
            "password",
            "do not read this directly",
        )],
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "restrictedSymbol.property"),
        "got: {d:?}"
    );
}

#[test]
fn a_restricted_static_property_access_is_flagged() {
    let d = with_entries(
        r#"<?php
            class Legacy { public static string $password = ""; }
            function f(): string { return Legacy::$password; }
        "#,
        vec![entry(
            RestrictedSymbolKind::StaticProperty,
            Some("Legacy"),
            "password",
            "do not read this directly",
        )],
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "restrictedSymbol.staticProperty"),
        "got: {d:?}"
    );
}

#[test]
fn a_restricted_class_constant_access_is_flagged() {
    let d = with_entries(
        r#"<?php
            class Legacy { const VERSION = 1; }
            function f(): int { return Legacy::VERSION; }
        "#,
        vec![entry(
            RestrictedSymbolKind::ClassConstant,
            Some("Legacy"),
            "VERSION",
            "use Config::VERSION instead",
        )],
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "restrictedSymbol.classConstant"),
        "got: {d:?}"
    );
}

#[test]
fn class_constant_lookup_is_case_sensitive() {
    let d = with_entries(
        r#"<?php
            class Legacy { const VERSION = 1; }
            function f(): int { return Legacy::VERSION; }
        "#,
        vec![entry(
            RestrictedSymbolKind::ClassConstant,
            Some("Legacy"),
            "version",
            "wrong case, should not match",
        )],
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "restrictedSymbol.classConstant"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_restricted_class_instantiation_is_flagged() {
    let d = with_entries(
        r#"<?php
            class Banned {}
            function f(): void { new Banned(); }
        "#,
        vec![entry(
            RestrictedSymbolKind::Class,
            None,
            "Banned",
            "do not use Banned",
        )],
    );
    assert!(
        d.iter().any(|x| x.identifier == "restrictedSymbol.class"),
        "got: {d:?}"
    );
}

#[test]
fn a_restricted_class_instanceof_is_flagged() {
    let d = with_entries(
        r#"<?php
            class Banned {}
            function f(object $x): bool { return $x instanceof Banned; }
        "#,
        vec![entry(
            RestrictedSymbolKind::Class,
            None,
            "Banned",
            "do not use Banned",
        )],
    );
    assert!(
        d.iter().any(|x| x.identifier == "restrictedSymbol.class"),
        "got: {d:?}"
    );
}

// The `Class` kind's own remaining detection surface - `extends`/
// `implements`/`catch`/param/return native type hints, plus the
// first-class-callable bypass for a restricted function - closed
// alongside `instanceof`/`new`/`catch` above.

#[test]
fn extending_a_restricted_class_is_flagged() {
    let d = with_entries(
        r#"<?php
            class Banned {}
            class Sub extends Banned {}
        "#,
        vec![entry(
            RestrictedSymbolKind::Class,
            None,
            "Banned",
            "do not use Banned",
        )],
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "restrictedSymbol.class" && x.message.contains("extends")),
        "got: {d:?}"
    );
}

#[test]
fn implementing_a_restricted_interface_is_flagged() {
    let d = with_entries(
        r#"<?php
            interface Banned {}
            class Sub implements Banned {}
        "#,
        vec![entry(
            RestrictedSymbolKind::Class,
            None,
            "Banned",
            "do not use Banned",
        )],
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "restrictedSymbol.class" && x.message.contains("implements")),
        "got: {d:?}"
    );
}

#[test]
fn catching_a_restricted_class_is_flagged() {
    let d = with_entries(
        r#"<?php
            class Banned extends \Exception {}
            function f(): void {
                try {
                } catch (Banned $e) {
                }
            }
        "#,
        vec![entry(
            RestrictedSymbolKind::Class,
            None,
            "Banned",
            "do not use Banned",
        )],
    );
    assert!(
        d.iter().any(|x| x.identifier == "restrictedSymbol.class"),
        "got: {d:?}"
    );
}

#[test]
fn a_restricted_class_parameter_type_hint_is_flagged() {
    let d = with_entries(
        r#"<?php
            class Banned {}
            function f(Banned $x): void {}
        "#,
        vec![entry(
            RestrictedSymbolKind::Class,
            None,
            "Banned",
            "do not use Banned",
        )],
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "restrictedSymbol.class" && x.message.contains("Parameter")),
        "got: {d:?}"
    );
}

#[test]
fn a_restricted_class_return_type_hint_is_flagged() {
    let d = with_entries(
        r#"<?php
            class Banned {}
            function f(): Banned {}
        "#,
        vec![entry(
            RestrictedSymbolKind::Class,
            None,
            "Banned",
            "do not use Banned",
        )],
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "restrictedSymbol.class" && x.message.contains("Return type")),
        "got: {d:?}"
    );
}

#[test]
fn a_restricted_class_closure_parameter_type_hint_is_flagged() {
    let d = with_entries(
        r#"<?php
            class Banned {}
            $f = function (Banned $x): void {};
        "#,
        vec![entry(
            RestrictedSymbolKind::Class,
            None,
            "Banned",
            "do not use Banned",
        )],
    );
    assert!(
        d.iter().any(|x| x.identifier == "restrictedSymbol.class"),
        "got: {d:?}"
    );
}

#[test]
fn importing_a_restricted_class_via_use_is_flagged() {
    // A plain, class-shaped `use Foo;` is not itself a type hint - real
    // PHPStan's own `ClassNameCheck` skips existence-checking there, but
    // a `Class`-kind ban still reports it as its own, separate
    // occurrence (confirmed against the real `spaze/phpstan-disallowed-
    // calls` docs).
    let d = with_entries(
        r#"<?php
            namespace App;
            use Vendor\Banned;
        "#,
        vec![entry(
            RestrictedSymbolKind::Class,
            None,
            "Vendor\\Banned",
            "do not use Banned",
        )],
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "restrictedSymbol.class" && x.message.contains("Importing")),
        "got: {d:?}"
    );
}

#[test]
fn allow_in_use_suppresses_only_the_use_import_occurrence() {
    let d = with_entries(
        r#"<?php
            namespace App;
            use Vendor\Banned;
            function f(): void { new Banned(); }
        "#,
        vec![entry_with(
            RestrictedSymbolKind::Class,
            None,
            "Vendor\\Banned",
            "do not use Banned",
            |e| e.allow_in_use = true,
        )],
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "restrictedSymbol.class" && x.message.contains("Importing")),
        "the use import itself should be allowed: {d:?}"
    );
    assert!(
        d.iter().any(
            |x| x.identifier == "restrictedSymbol.class" && x.message.contains("Instantiation")
        ),
        "instantiation must still be flagged: {d:?}"
    );
}

#[test]
fn a_restricted_function_s_first_class_callable_is_flagged() {
    let d = with_entries(
        r#"<?php
            $f = var_dump(...);
        "#,
        vec![entry(
            RestrictedSymbolKind::Function,
            None,
            "var_dump",
            "no debug dumps",
        )],
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "restrictedSymbol.function"),
        "got: {d:?}"
    );
}

// Phase 67's own filter-DSL extension: attribute-context matching
// (checked inline, unlike path-scoping - see `restricted_symbols.rs`'s
// own unit tests for that half).

#[test]
fn a_restricted_call_is_allowed_in_a_method_with_the_configured_attribute() {
    let d = with_entries(
        r#"<?php
            #[Attribute]
            class AllowedLegacy {}
            class Db { public function query(string $sql) {} }
            class Repo {
                #[AllowedLegacy]
                public function f(Db $db): void { $db->query('select 1'); }
            }
        "#,
        vec![entry_with(
            RestrictedSymbolKind::Method,
            Some("Db"),
            "query",
            "use the connection service",
            |e| e.allow_in_methods_with_attributes = vec!["AllowedLegacy".to_string()],
        )],
    );
    assert!(
        !d.iter().any(|x| x.identifier == "restrictedSymbol.method"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_restricted_call_is_still_flagged_in_a_method_without_the_configured_attribute() {
    let d = with_entries(
        r#"<?php
            class Db { public function query(string $sql) {} }
            class Repo {
                public function f(Db $db): void { $db->query('select 1'); }
            }
        "#,
        vec![entry_with(
            RestrictedSymbolKind::Method,
            Some("Db"),
            "query",
            "use the connection service",
            |e| e.allow_in_methods_with_attributes = vec!["AllowedLegacy".to_string()],
        )],
    );
    assert!(
        d.iter().any(|x| x.identifier == "restrictedSymbol.method"),
        "got: {d:?}"
    );
}

#[test]
fn a_restricted_call_is_allowed_in_a_class_with_the_configured_attribute() {
    let d = with_entries(
        r#"<?php
            class Db { public function query(string $sql) {} }
            #[AllowedLegacy]
            class Repo {
                public function f(Db $db): void { $db->query('select 1'); }
            }
        "#,
        vec![entry_with(
            RestrictedSymbolKind::Method,
            Some("Db"),
            "query",
            "use the connection service",
            |e| e.allow_in_class_with_attributes = vec!["AllowedLegacy".to_string()],
        )],
    );
    assert!(
        !d.iter().any(|x| x.identifier == "restrictedSymbol.method"),
        "unexpected diagnostics: {d:?}"
    );
}

// `allowInClassWithMethodAttributes`/`allowExceptInClassWithMethodAttributes` -
// matches when *any* method of the enclosing class carries the
// attribute, not just the currently-executing one (confirmed distinct
// from `allow_in_methods_with_attributes` above against the real docs
// directly).

#[test]
fn a_restricted_call_is_allowed_when_a_sibling_method_carries_the_configured_attribute() {
    let d = with_entries(
        r#"<?php
            class Db { public function query(string $sql) {} }
            class Repo {
                public function f(Db $db): void { $db->query('select 1'); }

                #[AllowedLegacy]
                private function checkThis(): void {}
            }
        "#,
        vec![entry_with(
            RestrictedSymbolKind::Method,
            Some("Db"),
            "query",
            "use the connection service",
            |e| e.allow_in_class_with_method_attributes = vec!["AllowedLegacy".to_string()],
        )],
    );
    assert!(
        !d.iter().any(|x| x.identifier == "restrictedSymbol.method"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_restricted_call_is_still_flagged_when_no_method_in_the_class_carries_the_attribute() {
    let d = with_entries(
        r#"<?php
            class Db { public function query(string $sql) {} }
            class Repo {
                public function f(Db $db): void { $db->query('select 1'); }
                private function checkThis(): void {}
            }
        "#,
        vec![entry_with(
            RestrictedSymbolKind::Method,
            Some("Db"),
            "query",
            "use the connection service",
            |e| e.allow_in_class_with_method_attributes = vec!["AllowedLegacy".to_string()],
        )],
    );
    assert!(
        d.iter().any(|x| x.identifier == "restrictedSymbol.method"),
        "got: {d:?}"
    );
}

#[test]
fn allow_except_in_class_with_method_attributes_only_flags_classes_with_a_matching_method() {
    let d = with_entries(
        r#"<?php
            class Db { public function query(string $sql) {} }
            class Repo {
                public function f(Db $db): void { $db->query('select 1'); }
                #[Special]
                private function checkThis(): void {}
            }
            class OtherRepo {
                public function f(Db $db): void { $db->query('select 1'); }
            }
        "#,
        vec![entry_with(
            RestrictedSymbolKind::Method,
            Some("Db"),
            "query",
            "use the connection service",
            |e| e.allow_except_in_class_with_method_attributes = vec!["Special".to_string()],
        )],
    );
    let flagged: Vec<_> = d
        .iter()
        .filter(|x| x.identifier == "restrictedSymbol.method")
        .collect();
    assert_eq!(flagged.len(), 1, "got: {d:?}");
    assert!(
        flagged[0].location.contains("Repo") && !flagged[0].location.contains("OtherRepo"),
        "got: {:?}",
        flagged[0]
    );
}

#[test]
fn allow_except_in_class_with_attributes_only_flags_inside_the_matching_classes() {
    let d = with_entries(
        r#"<?php
            class Db { public function query(string $sql) {} }
            #[Special]
            class Repo {
                public function f(Db $db): void { $db->query('select 1'); }
            }
            class OtherRepo {
                public function f(Db $db): void { $db->query('select 1'); }
            }
        "#,
        vec![entry_with(
            RestrictedSymbolKind::Method,
            Some("Db"),
            "query",
            "use the connection service",
            |e| e.allow_except_in_class_with_attributes = vec!["Special".to_string()],
        )],
    );
    let flagged: Vec<_> = d
        .iter()
        .filter(|x| x.identifier == "restrictedSymbol.method")
        .collect();
    assert_eq!(flagged.len(), 1, "got: {d:?}");
    assert!(flagged[0].location.contains("Repo") && !flagged[0].location.contains("OtherRepo"));
}

#[test]
fn attribute_name_matching_is_case_insensitive() {
    let d = with_entries(
        r#"<?php
            class Db { public function query(string $sql) {} }
            class Repo {
                #[allowedlegacy]
                public function f(Db $db): void { $db->query('select 1'); }
            }
        "#,
        vec![entry_with(
            RestrictedSymbolKind::Method,
            Some("Db"),
            "query",
            "use the connection service",
            |e| e.allow_in_methods_with_attributes = vec!["AllowedLegacy".to_string()],
        )],
    );
    assert!(
        !d.iter().any(|x| x.identifier == "restrictedSymbol.method"),
        "unexpected diagnostics: {d:?}"
    );
}
