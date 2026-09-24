//! PHP 8.4 property hooks - virtual-property detection,
//! `propertyGetHook.noRead`/`propertySetHook.noAssign`,
//! `propertySetHook.nativeParameterType` contravariance,
//! `class.notFound` existence-checking for a set hook's own parameter
//! type, function-like body checking (undefined variables,
//! `return.missing`, generator-return-type compatibility), and
//! `PropertyVarianceRule` for a property's own single type.

use super::helpers::*;

#[test]
fn set_hook_body_can_legally_access_a_private_sibling_property_on_the_same_class() {
    // `check_get_hook`/`check_set_hook` must save/set/restore
    // `Checker::current_class`, or an entirely legal same-class private
    // property access from inside a hook body is wrongly flagged
    // `property.private` (visibility checks read `current_class` to
    // allow it).
    let d = diagnostics_for(
        r#"<?php
            class SameClassPrivateAccess {
                private float $celsius = 0.0;
                public float $fahrenheit {
                    get => $this->celsius;
                    set(float $value) {
                        $this->celsius = $value;
                    }
                }
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "property.private"),
        "unexpected diagnostics: {d:?}"
    );
}
