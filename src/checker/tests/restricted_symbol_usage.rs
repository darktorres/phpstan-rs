//! The declarative, config-driven `restricted_symbols` list - one
//! behavior-per-test, mirroring `internal_usage.rs`'s own test style.

use super::helpers::*;
use crate::restricted_symbols::{
    ParamCondition, ParamFlagsCondition, ParamRef, ParamScalarValue, RestrictedSymbolEntry,
    RestrictedSymbolKind, RestrictedSymbols,
};

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
        allow_in_param_types: false,
        allow_except_in_param_types: false,
        allow_in_return_type: false,
        allow_except_in_return_type: false,
        allow_in_methods: Vec::new(),
        allow_except_in_methods: Vec::new(),
        allow_in_instance_of: Vec::new(),
        allow_except_in_instance_of: Vec::new(),
        allow_params_anywhere: Vec::new(),
        allow_params_anywhere_any_value: Vec::new(),
        allow_except_params_anywhere: Vec::new(),
        allow_except_params_any_value: Vec::new(),
        allow_except_case_insensitive_params: Vec::new(),
        allow_params_in_allowed: Vec::new(),
        allow_params_in_allowed_any_value: Vec::new(),
        allow_except_params_in_allowed: Vec::new(),
        allow_param_flags_in_allowed: Vec::new(),
        allow_param_flags_anywhere: Vec::new(),
        allow_except_param_flags_in_allowed: Vec::new(),
        allow_except_param_flags: Vec::new(),
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

// `allowInParamTypes`/`allowInReturnType` and their `allowExceptIn*`
// inverses - `Class`-kind only. Mirrors the real docs' own worked
// examples (`OldService`/`LegacyValue`) exactly.

#[test]
fn allow_in_param_types_allows_only_the_parameter_position() {
    let d = with_entries(
        r#"<?php
            class OldService {}
            class Foo {
                public function __construct(OldService $service) {}
                public function handle(OldService $svc): void {
                    $old = new OldService();
                }
                public function get(): OldService {}
            }
        "#,
        vec![entry_with(
            RestrictedSymbolKind::Class,
            None,
            "OldService",
            "use NewService instead",
            |e| e.allow_in_param_types = true,
        )],
    );
    let flagged: Vec<_> = d
        .iter()
        .filter(|x| x.identifier == "restrictedSymbol.class")
        .collect();
    // `new` and the return type are still disallowed - only the two
    // parameter type hints (constructor + `handle`) are allowed.
    assert!(
        flagged.iter().any(|x| x.message.contains("Instantiation")),
        "got: {d:?}"
    );
    assert!(
        flagged
            .iter()
            .any(|x| x.message.contains("Return type references")),
        "got: {d:?}"
    );
    assert!(
        !flagged
            .iter()
            .any(|x| x.message.contains("Parameter") && x.message.contains("references")),
        "got: {d:?}"
    );
}

#[test]
fn allow_except_in_param_types_disallows_only_the_parameter_position() {
    let d = with_entries(
        r#"<?php
            class LegacyValue {}
            class Bar {
                public function process(LegacyValue $v): void {}
                public function get(): LegacyValue {}
                public function create(): void {
                    $v = new LegacyValue();
                }
            }
        "#,
        vec![entry_with(
            RestrictedSymbolKind::Class,
            None,
            "LegacyValue",
            "do not accept LegacyValue as a parameter type",
            |e| e.allow_except_in_param_types = true,
        )],
    );
    let flagged: Vec<_> = d
        .iter()
        .filter(|x| x.identifier == "restrictedSymbol.class")
        .collect();
    // Only the parameter type hint is disallowed - the return type and
    // `new` are both allowed now that the entry's own default is
    // inverted (`allow_except_in_return_type_combined_with_allow_in_
    // use_also_allows_the_use_import` below covers the `use`-import
    // side of the same inversion).
    assert_eq!(flagged.len(), 1, "got: {d:?}");
    assert!(
        flagged[0].message.contains("Parameter") && flagged[0].message.contains("references"),
        "got: {:?}",
        flagged[0]
    );
}

#[test]
fn allow_except_in_return_type_disallows_only_the_return_position() {
    let d = with_entries(
        r#"<?php
            class RawData {}
            class Processor {
                public function transform(RawData $input): void {}
                public function fetch(): RawData {}
            }
        "#,
        vec![entry_with(
            RestrictedSymbolKind::Class,
            None,
            "RawData",
            "do not expose RawData as a return type",
            |e| e.allow_except_in_return_type = true,
        )],
    );
    let flagged: Vec<_> = d
        .iter()
        .filter(|x| x.identifier == "restrictedSymbol.class")
        .collect();
    assert_eq!(flagged.len(), 1, "got: {d:?}");
    assert!(
        flagged[0].message.contains("Return type references"),
        "got: {:?}",
        flagged[0]
    );
}

#[test]
fn allow_except_in_return_type_combined_with_allow_in_use_also_allows_the_use_import() {
    let d = with_entries(
        r#"<?php
            namespace App;
            use Vendor\RawData;
            class Processor {
                public function fetch(): RawData {}
            }
        "#,
        vec![entry_with(
            RestrictedSymbolKind::Class,
            None,
            "Vendor\\RawData",
            "do not expose RawData as a return type",
            |e| {
                e.allow_except_in_return_type = true;
                e.allow_in_use = true;
            },
        )],
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "restrictedSymbol.class" && x.message.contains("Importing")),
        "the use import should be allowed by allow_in_use: {d:?}"
    );
    assert!(
        d.iter().any(|x| x.identifier == "restrictedSymbol.class"
            && x.message.contains("Return type references")),
        "the return type should still be disallowed: {d:?}"
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

// `allowInMethods`/`allowExceptInMethods` - matches the *calling*
// function/method's own `Class::method()`/`function()`-shaped name,
// unlike `allow_in_methods_with_attributes` above (attribute context, a
// wholly separate mechanism).

#[test]
fn allow_in_methods_allows_only_the_matching_calling_method() {
    let d = with_entries(
        r#"<?php
            class Logger { public function log(string $msg): void {} }
            class Foo {
                public function allowed(Logger $l): void { $l->log('a'); }
                public function other(Logger $l): void { $l->log('b'); }
            }
        "#,
        vec![entry_with(
            RestrictedSymbolKind::Method,
            Some("Logger"),
            "log",
            "use our own logger instead",
            |e| e.allow_in_methods = vec!["Foo::allowed()".to_string()],
        )],
    );
    let flagged: Vec<_> = d
        .iter()
        .filter(|x| x.identifier == "restrictedSymbol.method")
        .collect();
    assert_eq!(flagged.len(), 1, "got: {d:?}");
}

#[test]
fn allow_except_in_methods_disallows_only_the_matching_calling_method() {
    let d = with_entries(
        r#"<?php
            class Controller {
                public function redirect(): void {}
                public function handleRequest(): void { $this->redirect(); }
                public function other(): void { $this->redirect(); }
            }
        "#,
        vec![entry_with(
            RestrictedSymbolKind::Method,
            Some("Controller"),
            "redirect",
            "redirect in startup() instead",
            |e| e.allow_except_in_methods = vec!["Controller::handleRequest()".to_string()],
        )],
    );
    let flagged: Vec<_> = d
        .iter()
        .filter(|x| x.identifier == "restrictedSymbol.method")
        .collect();
    assert_eq!(flagged.len(), 1, "got: {d:?}");
}

#[test]
fn allow_in_methods_matches_a_bare_function_name_with_no_enclosing_class() {
    let d = with_entries(
        r#"<?php
            function var_dump_wrapper() { var_dump(1); }
            var_dump(2);
        "#,
        vec![entry_with(
            RestrictedSymbolKind::Function,
            None,
            "var_dump",
            "no debug dumps",
            |e| e.allow_in_methods = vec!["var_dump_wrapper()".to_string()],
        )],
    );
    let flagged: Vec<_> = d
        .iter()
        .filter(|x| x.identifier == "restrictedSymbol.function")
        .collect();
    assert_eq!(flagged.len(), 1, "got: {d:?}");
}

// `allowInInstanceOf`/`allowExceptInInstanceOf` - matches the current
// class's own name and every transitive `extends`/`implements`
// ancestor's name, not just an exact class-name match.

#[test]
fn allow_in_instance_of_allows_only_inside_the_matching_hierarchy() {
    let d = with_entries(
        r#"<?php
            class Logger { public function log(string $msg): void {} }
            class Wrapper { public function f(Logger $l): void { $l->log('a'); } }
            class Other { public function f(Logger $l): void { $l->log('b'); } }
        "#,
        vec![entry_with(
            RestrictedSymbolKind::Method,
            Some("Logger"),
            "log",
            "use our own logger instead",
            |e| e.allow_in_instance_of = vec!["Wrapper".to_string()],
        )],
    );
    let flagged: Vec<_> = d
        .iter()
        .filter(|x| x.identifier == "restrictedSymbol.method")
        .collect();
    assert_eq!(flagged.len(), 1, "got: {d:?}");
    assert!(flagged[0].location.contains("Other"));
}

#[test]
fn allow_in_instance_of_matches_a_transitive_ancestor_not_just_an_exact_class() {
    let d = with_entries(
        r#"<?php
            class Logger { public function log(string $msg): void {} }
            interface Handler {}
            class BaseHandler implements Handler {
                public function f(Logger $l): void { $l->log('a'); }
            }
            class ConcreteHandler extends BaseHandler {}
        "#,
        vec![entry_with(
            RestrictedSymbolKind::Method,
            Some("Logger"),
            "log",
            "use our own logger instead",
            |e| e.allow_in_instance_of = vec!["Handler".to_string()],
        )],
    );
    assert!(
        !d.iter().any(|x| x.identifier == "restrictedSymbol.method"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn allow_except_in_instance_of_disallows_only_inside_the_matching_hierarchy() {
    let d = with_entries(
        r#"<?php
            interface HandlerInterface {}
            class Dispatcher {
                public function dispatch(): void {}
            }
            class MyHandler implements HandlerInterface {
                public function f(Dispatcher $d): void { $d->dispatch(); }
            }
            class Other {
                public function f(Dispatcher $d): void { $d->dispatch(); }
            }
        "#,
        vec![entry_with(
            RestrictedSymbolKind::Method,
            Some("Dispatcher"),
            "dispatch",
            "module handles its own dispatch",
            |e| e.allow_except_in_instance_of = vec!["HandlerInterface".to_string()],
        )],
    );
    let flagged: Vec<_> = d
        .iter()
        .filter(|x| x.identifier == "restrictedSymbol.method")
        .collect();
    assert_eq!(flagged.len(), 1, "got: {d:?}");
    assert!(flagged[0].location.contains("MyHandler"));
}

// `allowParamsAnywhere`/`allowParamsAnywhereAnyValue` (additive) and
// `allowExceptParamsAnywhere`/`allowExceptParamsAnyValue`/
// `allowExceptCaseInsensitiveParams` (default-inverting) - the
// location-independent half of the real parameter-value-conditions
// family.

#[test]
fn allow_params_anywhere_allows_only_the_call_matching_every_condition() {
    let d = with_entries(
        r#"<?php
            print_r($x);
            print_r($x, true);
        "#,
        vec![entry_with(
            RestrictedSymbolKind::Function,
            None,
            "print_r",
            "use logging instead",
            |e| {
                e.allow_params_anywhere = vec![ParamCondition {
                    position: Some(2),
                    value: Some(ParamScalarValue::Bool(true)),
                    ..Default::default()
                }]
            },
        )],
    );
    let flagged: Vec<_> = d
        .iter()
        .filter(|x| x.identifier == "restrictedSymbol.function")
        .collect();
    assert_eq!(flagged.len(), 1, "got: {d:?}");
}

#[test]
fn allow_params_anywhere_any_value_allows_only_when_the_parameter_is_present() {
    let d = with_entries(
        r#"<?php
            function f($a = null) {}
            f(1);
            f();
        "#,
        vec![entry_with(
            RestrictedSymbolKind::Function,
            None,
            "f",
            "no bare calls",
            |e| {
                e.allow_params_anywhere_any_value = vec![ParamRef {
                    position: Some(1),
                    name: None,
                }]
            },
        )],
    );
    let flagged: Vec<_> = d
        .iter()
        .filter(|x| x.identifier == "restrictedSymbol.function")
        .collect();
    assert_eq!(flagged.len(), 1, "got: {d:?}");
}

#[test]
fn allow_except_params_anywhere_disallows_only_the_exact_matching_value() {
    let d = with_entries(
        r#"<?php
            function foo(string $a, string $b) {}
            foo('bar', 'baz');
            foo('bar', 'BAZ');
        "#,
        vec![entry_with(
            RestrictedSymbolKind::Function,
            None,
            "foo",
            "not with baz",
            |e| {
                e.allow_except_params_anywhere = vec![ParamCondition {
                    position: Some(2),
                    value: Some(ParamScalarValue::Str("baz".to_string())),
                    ..Default::default()
                }]
            },
        )],
    );
    let flagged: Vec<_> = d
        .iter()
        .filter(|x| x.identifier == "restrictedSymbol.function")
        .collect();
    assert_eq!(flagged.len(), 1, "got: {d:?}");
}

#[test]
fn allow_except_case_insensitive_params_matches_regardless_of_case() {
    let d = with_entries(
        r#"<?php
            hash('md5', $s);
            hash('MD5', $s);
            hash('sha256', $s);
        "#,
        vec![entry_with(
            RestrictedSymbolKind::Function,
            None,
            "hash",
            "md5 is not a secure hash",
            |e| {
                e.allow_except_case_insensitive_params = vec![ParamCondition {
                    position: Some(1),
                    name: Some("algo".to_string()),
                    value: Some(ParamScalarValue::Str("md5".to_string())),
                    ..Default::default()
                }]
            },
        )],
    );
    let flagged: Vec<_> = d
        .iter()
        .filter(|x| x.identifier == "restrictedSymbol.function")
        .collect();
    assert_eq!(flagged.len(), 2, "got: {d:?}");
}

#[test]
fn allow_except_params_any_value_disallows_only_when_every_listed_param_is_present() {
    let d = with_entries(
        r#"<?php
            function waldo(string $a = '', string $b = '') {}
            waldo('foo', 'bar');
            waldo('foo');
            waldo();
        "#,
        vec![entry_with(
            RestrictedSymbolKind::Function,
            None,
            "waldo",
            "no waldo",
            |e| {
                e.allow_except_params_any_value = vec![
                    ParamRef {
                        position: Some(1),
                        name: None,
                    },
                    ParamRef {
                        position: Some(2),
                        name: None,
                    },
                ]
            },
        )],
    );
    let flagged: Vec<_> = d
        .iter()
        .filter(|x| x.identifier == "restrictedSymbol.function")
        .collect();
    assert_eq!(flagged.len(), 1, "got: {d:?}");
}

#[test]
fn a_first_class_callable_is_always_flagged_when_allow_params_anywhere_is_configured() {
    let d = with_entries(
        r#"<?php
            $f = print_r(...);
        "#,
        vec![entry_with(
            RestrictedSymbolKind::Function,
            None,
            "print_r",
            "use logging instead",
            |e| {
                e.allow_params_anywhere = vec![ParamCondition {
                    position: Some(2),
                    value: Some(ParamScalarValue::Bool(true)),
                    ..Default::default()
                }]
            },
        )],
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "restrictedSymbol.function"),
        "got: {d:?}"
    );
}

#[test]
fn a_first_class_callable_is_never_flagged_when_allow_except_params_anywhere_is_configured() {
    let d = with_entries(
        r#"<?php
            function foo(string $a, string $b) {}
            $f = foo(...);
        "#,
        vec![entry_with(
            RestrictedSymbolKind::Function,
            None,
            "foo",
            "not with baz",
            |e| {
                e.allow_except_params_anywhere = vec![ParamCondition {
                    position: Some(2),
                    value: Some(ParamScalarValue::Str("baz".to_string())),
                    ..Default::default()
                }]
            },
        )],
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "restrictedSymbol.function"),
        "unexpected diagnostics: {d:?}"
    );
}

// `allowParamsInAllowed`/`allowExceptParamsInAllowed` - narrows
// whichever zone directive (`allow_in_instance_of`/`allow_in_methods`/
// their `allow_except_in_*` siblings) the same entry also configures,
// mirroring the real docs' own `Controller::redirect()`/`dispatch()`
// worked examples directly.

#[test]
fn allow_params_in_allowed_combined_with_allow_except_in_methods() {
    let d = with_entries(
        r#"<?php
            class SafeUrl {}
            class OtherUrl {}
            class Controller {
                public function redirect(SafeUrl|OtherUrl $url): void {}
                public function handleRequest(): void {
                    $this->redirect(new SafeUrl());
                    $this->redirect(new OtherUrl());
                }
                public function other(): void {
                    $this->redirect(new OtherUrl());
                }
            }
        "#,
        vec![entry_with(
            RestrictedSymbolKind::Method,
            Some("Controller"),
            "redirect",
            "use a safe redirect instead",
            |e| {
                e.allow_except_in_methods = vec!["Controller::handleRequest()".to_string()];
                e.allow_params_in_allowed = vec![ParamCondition {
                    position: Some(1),
                    type_string: Some("SafeUrl".to_string()),
                    ..Default::default()
                }];
            },
        )],
    );
    let flagged: Vec<_> = d
        .iter()
        .filter(|x| x.identifier == "restrictedSymbol.method")
        .collect();
    assert_eq!(flagged.len(), 1, "got: {d:?}");
}

#[test]
fn allow_except_params_in_allowed_combined_with_allow_except_in_instance_of() {
    let d = with_entries(
        r#"<?php
            interface HandlerInterface {}
            class SafeEvent {}
            class DangerousEvent {}
            function dispatch(SafeEvent|DangerousEvent $e) {}
            class MyHandler implements HandlerInterface {
                public function f(): void {
                    dispatch(new DangerousEvent());
                    dispatch(new SafeEvent());
                }
            }
            class Other {
                public function f(): void {
                    dispatch(new DangerousEvent());
                }
            }
        "#,
        vec![entry_with(
            RestrictedSymbolKind::Function,
            None,
            "dispatch",
            "use the module-specific handler instead",
            |e| {
                e.allow_except_in_instance_of = vec!["HandlerInterface".to_string()];
                e.allow_except_params_in_allowed = vec![ParamCondition {
                    position: Some(1),
                    type_string: Some("DangerousEvent".to_string()),
                    ..Default::default()
                }];
            },
        )],
    );
    let flagged: Vec<_> = d
        .iter()
        .filter(|x| x.identifier == "restrictedSymbol.function")
        .collect();
    assert_eq!(flagged.len(), 1, "got: {d:?}");
    assert!(flagged[0].location.contains("MyHandler"));
}

// `allowParamFlagsAnywhere`/`allowExceptParamFlags` - the `ParamFlags`
// bitmask sibling of the plain `Params` family: matches when specific
// bits are *set* in the resolved integer, not on exact equality, and
// needs its own literal-preserving constant fold since ordinary type
// inference widens a `BinOp::BitOr` of two literals to plain `Type::Int`.

#[test]
fn allow_param_flags_anywhere_matches_a_bit_set_anywhere_in_a_bitwise_or_expression() {
    let d = with_entries(
        r#"<?php
            define('MY_HEX_TAG', 1);
            define('MY_HEX_APOS', 4);
            function f($flags) {}
            f(MY_HEX_TAG | MY_HEX_APOS);
            f(MY_HEX_TAG);
        "#,
        vec![entry_with(
            RestrictedSymbolKind::Function,
            None,
            "f",
            "no bare calls",
            |e| {
                e.allow_param_flags_anywhere = vec![ParamFlagsCondition {
                    position: Some(1),
                    value: Some(4),
                    ..Default::default()
                }]
            },
        )],
    );
    let flagged: Vec<_> = d
        .iter()
        .filter(|x| x.identifier == "restrictedSymbol.function")
        .collect();
    assert_eq!(flagged.len(), 1, "got: {d:?}");
}

#[test]
fn allow_param_flags_anywhere_type_string_union_matches_either_flag() {
    let d = with_entries(
        r#"<?php
            function f($flags) {}
            f(1);
            f(2);
            f(3);
            f(4);
        "#,
        vec![entry_with(
            RestrictedSymbolKind::Function,
            None,
            "f",
            "no bare calls",
            |e| {
                e.allow_param_flags_anywhere = vec![ParamFlagsCondition {
                    position: Some(1),
                    type_string: Some("1 | 2".to_string()),
                    ..Default::default()
                }]
            },
        )],
    );
    let flagged: Vec<_> = d
        .iter()
        .filter(|x| x.identifier == "restrictedSymbol.function")
        .collect();
    assert_eq!(flagged.len(), 1, "got: {d:?}");
}

#[test]
fn allow_except_param_flags_disallows_only_when_the_configured_bit_is_set() {
    let d = with_entries(
        r#"<?php
            function f($flags) {}
            f(4);
            f(5);
            f(1);
        "#,
        vec![entry_with(
            RestrictedSymbolKind::Function,
            None,
            "f",
            "debug flag is forbidden",
            |e| {
                e.allow_except_param_flags = vec![ParamFlagsCondition {
                    position: Some(1),
                    value: Some(4),
                    ..Default::default()
                }]
            },
        )],
    );
    let flagged: Vec<_> = d
        .iter()
        .filter(|x| x.identifier == "restrictedSymbol.function")
        .collect();
    assert_eq!(flagged.len(), 2, "got: {d:?}");
}

// The `*InAllowed` family's own first-class-callable rule (real docs:
// an "allowed" condition - `allowParamsInAllowed` and siblings - always
// reports an FCC, since there's no argument list to evaluate; an
// "except" condition combined with an `allowIn`-style zone reports an
// FCC exactly when it's *outside* the zone - "the zone rule alone
// determines"; combined with an `allowExceptIn`-style zone, an FCC is
// never reported, inside or outside it).

#[test]
fn a_first_class_callable_is_always_reported_with_allow_params_in_allowed_and_an_allow_style_zone()
{
    let d = with_entries(
        r#"<?php
            class Logger { public function log(string $msg): void {} }
            class Foo {
                public function allowed(Logger $l): void { $f = $l->log(...); }
            }
        "#,
        vec![entry_with(
            RestrictedSymbolKind::Method,
            Some("Logger"),
            "log",
            "use our own logger instead",
            |e| {
                e.allow_in_methods = vec!["Foo::allowed()".to_string()];
                e.allow_params_in_allowed = vec![ParamCondition {
                    position: Some(1),
                    value: Some(ParamScalarValue::Str("safe".to_string())),
                    ..Default::default()
                }];
            },
        )],
    );
    assert!(
        d.iter().any(|x| x.identifier == "restrictedSymbol.method"),
        "got: {d:?}"
    );
}

#[test]
fn a_first_class_callable_is_never_reported_with_allow_except_params_in_allowed_and_an_except_style_zone()
 {
    let d = with_entries(
        r#"<?php
            interface HandlerInterface {}
            class Dispatcher { public function dispatch(string $e): void {} }
            class MyHandler implements HandlerInterface {
                public function f(Dispatcher $d): void { $g = $d->dispatch(...); }
            }
        "#,
        vec![entry_with(
            RestrictedSymbolKind::Method,
            Some("Dispatcher"),
            "dispatch",
            "use the module-specific handler instead",
            |e| {
                e.allow_except_in_instance_of = vec!["HandlerInterface".to_string()];
                e.allow_except_params_in_allowed = vec![ParamCondition {
                    position: Some(1),
                    value: Some(ParamScalarValue::Str("dangerous".to_string())),
                    ..Default::default()
                }];
            },
        )],
    );
    assert!(
        !d.iter().any(|x| x.identifier == "restrictedSymbol.method"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_first_class_callable_is_reported_with_allow_except_params_in_allowed_outside_an_allow_style_zone()
 {
    let d = with_entries(
        r#"<?php
            class Logger { public function log(string $msg): void {} }
            class Foo {
                public function other(Logger $l): void { $f = $l->log(...); }
            }
        "#,
        vec![entry_with(
            RestrictedSymbolKind::Method,
            Some("Logger"),
            "log",
            "use our own logger instead",
            |e| {
                e.allow_in_methods = vec!["Foo::allowed()".to_string()];
                e.allow_except_params_in_allowed = vec![ParamCondition {
                    position: Some(1),
                    value: Some(ParamScalarValue::Str("dangerous".to_string())),
                    ..Default::default()
                }];
            },
        )],
    );
    assert!(
        d.iter().any(|x| x.identifier == "restrictedSymbol.method"),
        "got: {d:?}"
    );
}
