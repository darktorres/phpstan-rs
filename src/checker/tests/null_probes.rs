//! `isset()`/`??`/`??=`/`?->` firing on an expression whose
//! statically-known type already proves the null-probe is pointless -
//! mirrors PHPStan's `IssetRule`/`NullCoalesceRule`/
//! `NullsafeMethodCallRule`/`NullsafePropertyFetchRule`. `empty()` is
//! deliberately out of scope - see `is_isset_probe`'s own doc comment in
//! `checker/expr.rs` for why. Covers both the bare-variable/nullsafe-
//! receiver half (`Checker::check_pointless_null_probe`) and the
//! offset/property-chain half (`Checker::resolve_always_exists`/
//! `check_pointless_offset_or_property_probe` - mirrors PHPStan's
//! `checkAdvancedIsset`-gated `IssetabilityDescriptor`/`IssetCheck`).

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

// A variable assigned only inside a loop body (never before it) must
// not be treated as unconditionally defined afterward -
// `Checker::converge_loop_body` (`checker/control_flow.rs`) must not
// silently drop the loop's "ran zero times" case, which PHP always
// allows, from the returned exit state. `isset()` right after such a
// loop (checking exactly this - "did the loop actually find/set
// anything") is a common, legitimate idiom that must not be wrongly
// flagged "always exists".
#[test]
fn isset_after_a_foreach_only_ever_setting_the_variable_inside_the_body_is_not_flagged() {
    let d = null_probe_diagnostics(
        r#"<?php
            /** @param string[] $items */
            function test(array $items): void {
                foreach ($items as $item) {
                    $found = $item;
                }
                if (! isset($found)) {
                    return;
                }
                echo $found;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// When the variable is pre-declared `null` before the conditional/loop,
// not introduced fresh inside it, the fix above must not special-case
// this as "already defined" (keeping a pre-existing name exactly as the
// loop left it, only dropping brand-new names) - that would silently
// lose the real `null` possibility the pre-loop declaration carries.
#[test]
fn isset_after_a_pre_declared_null_reassigned_only_inside_a_conditional_foreach_is_not_flagged() {
    let d = null_probe_diagnostics(
        r#"<?php
            /** @param string[] $items */
            function test(array $items, bool $cond): void {
                $file_path = null;
                if ($cond) {
                    foreach ($items as $item) {
                        $file_path = $item;
                    }
                }
                if (! isset($file_path)) {
                    return;
                }
                echo $file_path;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
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

// A variable's type only ever widened by a statement run once
// (unmodeled) per loop iteration - a second or later real iteration
// would see the widened type from the first, but this project's
// single-pass loop-body checking can't. Locking this in against
// `Checker::loop_depth`'s guard regressing.
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

// A `static` local can carry a value left over from an earlier call -
// the same "this single pass can't see a wider type a different
// execution would produce" unsoundness the loop case above has, just
// across invocations instead of iterations. Genuinely silent in PHPStan
// too. Locking this in against `Checker::static_var_names`'s guard
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
    // A plain, unshaped array specifically - stays silent for a real
    // reason (a general `array<K, V>` makes no claim about any specific
    // key), not because offset existence goes unmodeled - see the tests
    // further down for the `array{...}`-shaped cases this correctly
    // flags/exempts.
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

// The offset/property-chain half of isset()/??/??= - mirrors PHPStan's
// recursive `IssetabilityDescriptor`/`IssetCheck` (`checkAdvancedIsset`),
// adapted to this project's type system in
// `Checker::resolve_always_exists`/`check_pointless_offset_or_property_probe`.

#[test]
fn isset_on_a_required_non_nullable_shape_key_is_flagged() {
    let d = null_probe_diagnostics(
        r#"<?php
            /** @param array{key: int} $x */
            function test(array $x): void {
                if (isset($x['key'])) {
                }
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert_eq!(d[0].identifier, "isset.offset");
    assert!(d[0].message.contains("is not nullable"), "{}", d[0].message);
}

#[test]
fn isset_on_a_required_but_nullable_shape_key_is_never_flagged() {
    let d = null_probe_diagnostics(
        r#"<?php
            /** @param array{key: ?int} $x */
            function test(array $x): void {
                if (isset($x['key'])) {
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn isset_on_a_shape_key_never_declared_at_all_is_flagged_as_absent() {
    let d = null_probe_diagnostics(
        r#"<?php
            /** @param array{other: int} $x */
            function test(array $x): void {
                if (isset($x['key'])) {
                }
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert_eq!(d[0].identifier, "isset.offset");
    assert!(d[0].message.contains("does not exist"), "{}", d[0].message);
}

#[test]
fn isset_on_a_plain_array_key_is_never_flagged() {
    // The ordinary, overwhelmingly common case - a general `array<string,
    // int>` makes no claim about any specific key, so isset() here is
    // always a legitimate probe.
    let d = null_probe_diagnostics(
        r#"<?php
            /** @param array<string, int> $x */
            function test(array $x): void {
                if (isset($x['key'])) {
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn isset_on_a_non_nullable_property_with_a_default_is_flagged() {
    let d = null_probe_diagnostics(
        r#"<?php
            class Foo {
                public int $x = 1;
            }
            function test(Foo $f): void {
                if (isset($f->x)) {
                }
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert_eq!(d[0].identifier, "isset.property");
    assert!(d[0].message.contains("is not nullable"), "{}", d[0].message);
}

#[test]
fn isset_on_a_non_nullable_property_with_no_default_is_never_flagged() {
    // A real Phase-62-shaped candidate: genuinely might still be
    // uninitialized (no `PropertyInitializationExpr`-equivalent dataflow
    // built here - see `PropertySig::has_default`'s own doc comment for
    // the disclosed, safe-direction simplification), so isset() stays a
    // legitimate check.
    let d = null_probe_diagnostics(
        r#"<?php
            class Foo {
                public int $x;
            }
            function test(Foo $f): void {
                if (isset($f->x)) {
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn isset_on_a_nullable_property_is_never_flagged_regardless_of_default() {
    let d = null_probe_diagnostics(
        r#"<?php
            class Foo {
                public ?int $x = null;
            }
            function test(Foo $f): void {
                if (isset($f->x)) {
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn isset_on_a_chain_broken_by_a_method_call_is_never_flagged() {
    // Real isset()/?? semantics: never evaluated past a call.
    let d = null_probe_diagnostics(
        r#"<?php
            class Foo {
                /** @return array{key: int} */
                public function getArr(): array { return ['key' => 1]; }
            }
            function test(Foo $f): void {
                if (isset($f->getArr()['key'])) {
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn null_coalesce_on_a_required_non_nullable_shape_key_is_flagged() {
    let d = null_probe_diagnostics(
        r#"<?php
            /** @param array{key: int} $x */
            function test(array $x): int {
                return $x['key'] ?? 0;
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert_eq!(d[0].identifier, "nullCoalesce.offset");
}

#[test]
fn null_coalesce_assign_on_a_required_non_nullable_shape_key_is_flagged() {
    let d = null_probe_diagnostics(
        r#"<?php
            /** @param array{key: int} $x */
            function test(array $x): void {
                $x['key'] ??= 0;
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert_eq!(d[0].identifier, "nullCoalesce.offset");
}
