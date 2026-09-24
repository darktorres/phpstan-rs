//! `@phpstan-assert`/`-if-true`/`-if-false` tags - PHPStan's
//! custom-assertion-function convention: an accessor pair like
//! `hasTestFilesFile(): bool` (tagged `@phpstan-assert-if-true !null
//! $this->testFilesFile`) then `testFilesFile(): string`'s own body
//! guarding on it.

use super::helpers::*;

// `AssertTarget::ThisProp`'s "not already narrowed" fallback
// (`assert_target_key_and_current`, `checker/narrowing.rs`) must not
// match the receiver's type against `Type::Object` directly, or it
// silently falls to `Type::Mixed(false)` (discarding the property's real
// declared type entirely) for a `Union`/`Intersection` receiver -
// reachable even through the already-correct `ThisSelf`/
// branch-conditional call site above, since this fallback re-infers the
// receiver's type independently.
#[test]
fn assert_if_true_narrows_a_this_property_through_a_union_typed_receiver() {
    // Asserts the narrowed *type* directly (`assertType`), not merely the
    // absence of diagnostics - `Type::Mixed(false)` (the bug's own fallback) is
    // itself permissive enough to pass a plain `string $s` argument
    // without complaint, so a bare "no diagnostics" check would pass
    // vacuously whether or not the fix is in place.
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            class Base {
                public ?string $flag = null;
                /** @phpstan-assert-if-true !null $this->flag */
                public function isReady(): bool { return $this->flag !== null; }
            }
            class A extends Base {}
            class B extends Base {}
            function run(A|B $v): void {
                if ($v->isReady()) {
                    assertType('string', $v->flag);
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}
