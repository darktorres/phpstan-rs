//! Tests for `@method` (resolved at `from_decl` time, inserted into
//! `ClassInfo::methods` by `Registry::resolve_method_tags`, after trait
//! flattening) and `@mixin` (`Registry::resolve_deferred_mixins`).
use super::helpers::*;

#[test]
fn a_method_tag_never_shadows_a_same_named_real_trait_method() {
    // `pestphp/pest-plugin-arch`'s own `Blueprint` tags `@method void
    // assertDoesNotDependOn(Layer $target, Layer $dependency)` while also
    // `use`-ing a trait that really implements it, under different
    // parameter names. `@method` synthesis has to run *after* trait
    // flattening (`Registry::resolve_method_tags`'s own doc comment) -
    // otherwise the tag's own synthetic signature (with no real body)
    // permanently wins the vacant-only merge before the real trait
    // method ever gets a chance to fill the slot, and the real trait
    // body then gets checked against the wrong parameter names.
    let d = diagnostics_for(
        r#"<?php
            trait Helper {
                public function realMethod($layerA, $layerB): int {
                    return $layerA + $layerB;
                }
            }
            /** @method int realMethod($target, $dependency) */
            class Host {
                use Helper;
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "variable.undefined"),
        "unexpected diagnostics: {d:?}"
    );
}
