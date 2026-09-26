//! `value-of<T>`/`key-of<T>`/`T[K]` - PHPStan's own `ValueOfType`/
//! `KeyOfType`/`OffsetAccessType`, `LateResolvableType`s. `T` itself
//! already a real `array`/`array{...}` type is resolved precisely at
//! parse time; `T` a template stays late (`Type::KeyOf`/`OffsetAccess`)
//! until a call binds it, and reads as its bound's answer inside the
//! generic code itself.
//!
//! `T` naming a real, registered enum is also resolved precisely -
//! `value-of<T>` only (matching PHPStan's `KeyOfType::getResult()`,
//! which has no such special case at all) via a one-time
//! `Registry::resolve_deferred_value_of` pass that runs once every
//! class/enum is indexed. Everything else genuinely unresolvable
//! (`key-of<T>` naming a class/enum, `value-of<T>`/`key-of<T>` naming an
//! ordinary, non-enum class) degrades to `Mixed` rather than a bogus,
//! namespace-qualified "class name".

use super::helpers::*;

// `value-of<T>` naming a real, registered enum - resolved by
// `Registry::resolve_deferred_value_of` (`registry.rs`) once every
// class/enum is indexed, reusing the enum's already-built `->value`
// property type rather than recomputing it.

#[test]
fn key_of_a_template_resolves_once_a_call_binds_it() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            /**
             * @template T of array
             * @param T $a
             * @return key-of<T>
             */
            function first_key(array $a): int|string { return 0; }
            /** @param array{id: int, name: string} $row */
            function f(array $row): void {
                assertType("'id'|'name'", first_key($row));
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn an_offset_access_on_a_template_resolves_once_a_call_binds_it() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            /**
             * @template T of array
             * @param T $a
             * @return T['id']
             */
            function id_of(array $a): mixed { return $a['id']; }
            /** @param array{id: int, name: string} $row */
            function f(array $row): void {
                assertType('int', id_of($row));
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn key_of_a_template_reads_as_its_bound_inside_the_generic_code() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            /**
             * @template T of array<string, int>
             * @param T $a
             * @param key-of<T> $k
             */
            function pick(array $a, string $k): int {
                assertType('string', $k);
                return $a[$k];
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}
