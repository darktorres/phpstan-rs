//! Classes, interfaces, traits, overrides, self/static/parent resolution.

use super::helpers::*;

#[test]
fn parent_pseudo_type_resolves_to_the_real_parent_class() {
    // Coverage measurement (cargo llvm-cov) found resolve_pseudo's
    // Type::ParentRef branch had zero test coverage - the exact same
    // pseudo-type-resolution machinery that turned out buggy for
    // self/static elsewhere in this audit. Manually verified this
    // particular path is currently correct; this test locks that in
    // rather than leaving it to keep passing by luck.
    let d = diagnostics_for(
        r#"<?php
            class Animal {}
            class Unrelated {}
            class Dog extends Animal {
                /** @return parent */
                public function asAnimal() {
                    return new Unrelated();
                }
            }
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.message.contains("expected Animal, got Unrelated")),
        "expected 'parent' to resolve to the real parent class Animal, got: {d:?}"
    );
}

#[test]
fn anonymous_class_method_resolves_a_static_return_type() {
    // The sibling gap found alongside the use-import one: the
    // `Some(sig)` branch resolves self/static/parent pseudo-types via
    // `resolve_pseudo` whenever `class_name` is known; the anonymous-
    // class fallback branch did this for neither params nor the
    // return type. Before the fix, `: static` stayed the literal,
    // unresolved pseudo-type `Type::Object("static")`, which could
    // never match `$this`'s real (synthetic anonymous-class) type -
    // a return-type-mismatch false positive on the most ordinary
    // possible body (`return $this;`).
    let d = diagnostics_for(
        r#"<?php
            function make() {
                return new class {
                    public function self(): static {
                        return $this;
                    }
                };
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn an_override_with_no_declared_type_is_not_flagged_as_narrowing() {
    // Type::Mixed(false) here means "nothing declared, native or docblock",
    // not a genuinely explicit `mixed` - real PHP doesn't enforce
    // override compatibility for an omitted type at all.
    let d = diagnostics_for(
        r#"<?php
            class Base11 { public function make(): int { return 1; } }
            class Sub11 extends Base11 { public function make() { return 1; } }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}
