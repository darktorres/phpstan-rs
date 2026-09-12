//! Phase 67: the declarative, config-driven `restricted_symbols` list -
//! one behavior-per-test, mirroring `internal_usage.rs`'s own test style.

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
    }
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
