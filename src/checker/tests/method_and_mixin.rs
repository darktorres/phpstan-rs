//! Tests for `@method` (resolved at `from_decl` time, inserted into
//! `ClassInfo::methods` by `Registry::resolve_method_tags`, after trait
//! flattening) and `@mixin` (`Registry::resolve_deferred_mixins`).
use super::helpers::*;

#[test]
fn a_method_tag_synthesizes_a_callable_virtual_method() {
    let d = diagnostics_for(
        r#"<?php
            /** @method int getX() */
            class Point {}
            function test(Point $p): int {
                return $p->getX();
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_method_tag_enforces_its_own_argument_types() {
    let d = diagnostics_for(
        r#"<?php
            /** @method void setX(int $x) */
            class Point {}
            function test(Point $p): void {
                $p->setX("not an int");
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert_eq!(d[0].identifier, "argument.type");
}

#[test]
fn a_method_tag_enforces_its_own_return_type() {
    let d = diagnostics_for(
        r#"<?php
            /** @method int getX() */
            class Point {}
            function needsString(string $s): void {}
            function test(Point $p): void {
                needsString($p->getX());
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert_eq!(d[0].identifier, "argument.type");
}

#[test]
fn a_real_method_declaration_wins_over_a_conflicting_method_tag() {
    let d = diagnostics_for(
        r#"<?php
            /** @method string getX() */
            class Point {
                public function getX(): int { return 1; }
            }
            function needsInt(int $x): void {}
            function test(Point $p): void {
                needsInt($p->getX());
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_method_with_no_return_type_defaults_to_mixed() {
    let d = diagnostics_for(
        r#"<?php
            /** @method getX() */
            class Point {}
            function needsInt(int $x): void {}
            function test(Point $p): void {
                needsInt($p->getX());
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_mixin_makes_the_target_s_public_methods_callable() {
    let d = diagnostics_for(
        r#"<?php
            class Collection {
                public function count(): int { return 0; }
            }
            /** @mixin Collection */
            class Query {}
            function needsInt(int $x): void {}
            function test(Query $q): void {
                needsInt($q->count());
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_mixin_never_pulls_in_a_private_method() {
    let d = diagnostics_for(
        r#"<?php
            class Collection {
                private function internalHelper(): int { return 0; }
            }
            /** @mixin Collection */
            class Query {}
            function test(Query $q): void {
                $q->internalHelper();
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "method.notFound"),
        "expected the private mixin method to stay inaccessible, got: {d:?}"
    );
}

#[test]
fn a_mixin_never_overrides_a_real_own_declaration() {
    let d = diagnostics_for(
        r#"<?php
            class Collection {
                public function count(): string { return "many"; }
            }
            /** @mixin Collection */
            class Query {
                public function count(): int { return 0; }
            }
            function needsInt(int $x): void {}
            function test(Query $q): void {
                needsInt($q->count());
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

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

#[test]
fn a_purely_magic_method_tag_on_an_interface_is_never_required_of_implementors() {
    // Tracy's own `IBarPanel`, `@method ?string getAgentInfo()` - a
    // magic-`__call` capability hint, never a real requirement (real PHP
    // has no way to even express "this interface method is optional").
    let d = diagnostics_for(
        r#"<?php
            /** @method ?string getAgentInfo() */
            interface IBarPanel {
                function getTab();
            }
            class ConcretePanel implements IBarPanel {
                function getTab() { return "tab"; }
            }
        "#,
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "class.abstractMethodNotImplemented"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_mixin_also_makes_the_target_s_public_properties_available() {
    let d = diagnostics_for(
        r#"<?php
            class Collection {
                public int $total = 0;
            }
            /** @mixin Collection */
            class Query {}
            function needsInt(int $x): void {}
            function test(Query $q): void {
                needsInt($q->total);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}
