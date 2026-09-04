//! Phase 60: `isset()`/`??`/`??=`/`?->` firing on an expression whose
//! statically-known type already proves the null-probe is pointless -
//! mirrors real PHPStan's `IssetRule`/`NullCoalesceRule`/
//! `NullsafeMethodCallRule`/`NullsafePropertyFetchRule`. `empty()` is
//! deliberately out of scope - see `is_isset_probe`'s own doc comment in
//! `checker/expr.rs` for why.

use super::super::Diagnostic;
use super::helpers::*;

fn null_probe_diagnostics(src: &str) -> Vec<Diagnostic> {
    diagnostics_for(src)
        .into_iter()
        .filter(|d| {
            d.identifier.starts_with("isset.")
                || d.identifier.starts_with("nullCoalesce.")
                || d.identifier == "nullsafe.neverNull"
        })
        .collect()
}

#[test]
fn isset_on_a_never_nullable_local_is_flagged() {
    let d = null_probe_diagnostics(
        r#"<?php
            function test(int $x): void {
                if (isset($x)) {
                }
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert_eq!(d[0].identifier, "isset.variable");
    assert!(d[0].message.contains("is not nullable"), "{}", d[0].message);
}

#[test]
fn isset_on_a_maybe_undefined_variable_is_never_flagged() {
    // `$x` isn't guaranteed-defined here (only one `if` branch assigns
    // it) - `isset()` is exactly the legitimate way to probe that.
    let d = null_probe_diagnostics(
        r#"<?php
            function test(bool $cond): void {
                if ($cond) {
                    $x = 1;
                }
                if (isset($x)) {
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn isset_on_a_nullable_local_is_never_flagged() {
    let d = null_probe_diagnostics(
        r#"<?php
            function test(?int $x): void {
                if (isset($x)) {
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn isset_on_an_always_null_local_is_flagged_as_always_null() {
    let d = null_probe_diagnostics(
        r#"<?php
            function test(): void {
                $x = null;
                if (isset($x)) {
                }
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert_eq!(d[0].identifier, "isset.variable");
    assert!(d[0].message.contains("is always null"), "{}", d[0].message);
}

#[test]
fn empty_on_a_never_nullable_local_is_never_flagged() {
    // `empty()` tests falsiness, not just null-ness - real PHPStan's own
    // `EmptyRule` stays silent here too, since a plain `int` could still
    // be `0` (falsy) or non-zero (truthy) - out of this phase's scope.
    let d = null_probe_diagnostics(
        r#"<?php
            function test(int $x): void {
                if (empty($x)) {
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn coalesce_on_a_never_nullable_local_is_flagged() {
    let d = null_probe_diagnostics(
        r#"<?php
            function test(int $x): int {
                return $x ?? 0;
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert_eq!(d[0].identifier, "nullCoalesce.variable");
    assert!(d[0].message.contains("??"), "{}", d[0].message);
}

#[test]
fn coalesce_on_a_nullable_local_is_never_flagged() {
    let d = null_probe_diagnostics(
        r#"<?php
            function test(?int $x): int {
                return $x ?? 0;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn compound_coalesce_assign_on_a_never_nullable_local_is_flagged() {
    let d = null_probe_diagnostics(
        r#"<?php
            function test(int $x): void {
                $x ??= 5;
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert_eq!(d[0].identifier, "nullCoalesce.variable");
    assert!(d[0].message.contains("??="), "{}", d[0].message);
}

// Real corpus find (Symfony's own AbstractObjectNormalizer::
// validateAndDenormalize): `$err`'s type is only ever widened by this
// exact statement, run once (unmodeled) per loop iteration - a second
// or later real iteration would see the widened type from the first,
// but this project's own single-pass loop-body checking can't. Locking
// this in against `Checker::loop_depth`'s own guard regressing.
#[test]
fn a_self_referential_coalesce_assign_inside_a_loop_is_never_flagged() {
    let d = null_probe_diagnostics(
        r#"<?php
            function test(array $items): void {
                $err = null;
                foreach ($items as $item) {
                    if ($item) {
                        $err ??= $item;
                    }
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// Real corpus find (Symfony's own HttpClient::create()): a `static`
// local can carry a value left over from an earlier call - the same
// "this single pass can't see a wider type a different execution would
// produce" unsoundness the loop case above has, just across invocations
// instead of iterations. Confirmed genuinely silent in real PHPStan too
// (verified directly against `vendor/bin/phpstan` on this exact file).
// Locking this in against `Checker::static_var_names`'s own guard
// regressing.
#[test]
fn a_coalesce_assign_on_a_static_local_is_never_flagged() {
    let d = null_probe_diagnostics(
        r#"<?php
            function curl_version(): array { return []; }
            function create(): void {
                static $curlVersion = null;
                $curlVersion ??= curl_version();
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn isset_on_an_index_expression_is_never_flagged() {
    // Deliberately out of this phase's scope - offset existence isn't
    // modeled, so only a bare variable root is ever flagged.
    let d = null_probe_diagnostics(
        r#"<?php
            function test(array $x): void {
                if (isset($x['key'])) {
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn nullsafe_property_access_on_a_never_nullable_receiver_is_flagged() {
    let d = null_probe_diagnostics(
        r#"<?php
            class Foo {
                public int $x = 1;
            }
            function test(Foo $f): ?int {
                return $f?->x;
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert_eq!(d[0].identifier, "nullsafe.neverNull");
    assert!(d[0].message.contains("property access"), "{}", d[0].message);
}

#[test]
fn nullsafe_method_call_on_a_never_nullable_receiver_is_flagged() {
    let d = null_probe_diagnostics(
        r#"<?php
            class Foo {
                public function bar(): int { return 1; }
            }
            function test(Foo $f): ?int {
                return $f?->bar();
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert_eq!(d[0].identifier, "nullsafe.neverNull");
    assert!(d[0].message.contains("method call"), "{}", d[0].message);
}

#[test]
fn nullsafe_property_access_on_a_nullable_receiver_is_never_flagged() {
    let d = null_probe_diagnostics(
        r#"<?php
            class Foo {
                public int $x = 1;
            }
            function test(?Foo $f): ?int {
                return $f?->x;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}
