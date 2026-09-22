//! `instanceof.alwaysTrue`/`.alwaysFalse`/`.invalidExprType`.

use super::helpers::*;

fn instanceof_findings(d: &[crate::Diagnostic]) -> Vec<(&str, &str)> {
    d.iter()
        .filter(|x| x.identifier.starts_with("instanceof."))
        .map(|x| (x.identifier.as_str(), x.message.as_str()))
        .collect()
}

#[test]
fn instanceof_of_a_subclass_is_always_true() {
    let d = diagnostics_for(
        r#"<?php
            interface Shape {}
            class Square implements Shape {}
            function f(Square $s): void {
                if ($s instanceof Shape) { echo 1; }
            }
        "#,
    );
    assert_eq!(
        instanceof_findings(&d),
        vec![(
            "instanceof.alwaysTrue",
            "Instanceof between Square and Shape will always evaluate to true."
        )],
        "got: {d:?}"
    );
}

#[test]
fn instanceof_that_can_never_hold_is_always_false() {
    let d = diagnostics_for(
        r#"<?php
            interface Shape {}
            class Circle {}
            class Square {}
            final class Locked {}
            enum Suit { case Hearts; }
            trait Mixin {}
            function f(int $i, Circle $c, Locked $l, Suit $s): void {
                if ($i instanceof Circle) { echo 1; }
                if ($c instanceof Square) { echo 2; }
                if ($l instanceof Shape) { echo 3; }
                if ($s instanceof Shape) { echo 4; }
                if ($c instanceof Mixin) { echo 5; }
            }
        "#,
    );
    let found = instanceof_findings(&d);
    assert_eq!(found.len(), 5, "got: {d:?}");
    assert!(
        found.iter().all(|(id, _)| *id == "instanceof.alwaysFalse"),
        "got: {d:?}"
    );
}

#[test]
fn undecidable_instanceof_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            interface Shape {}
            interface Named {}
            class Base {}
            class Child extends Base {}
            class Open {}
            function f(?Child $n, Base $b, Open $o, Named $named, mixed $m, object $obj): void {
                if ($n instanceof Child) { echo 1; }
                if ($b instanceof Child) { echo 2; }
                if ($o instanceof Shape) { echo 3; }
                if ($named instanceof Shape) { echo 4; }
                if ($m instanceof Shape) { echo 5; }
                if ($obj instanceof Shape) { echo 6; }
            }
        "#,
    );
    assert!(instanceof_findings(&d).is_empty(), "got: {d:?}");
}

#[test]
fn always_true_instanceof_as_the_last_elseif_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            interface Shape {}
            class Square implements Shape {}
            class Circle implements Shape {}
            function f(Square|Circle $s): void {
                if ($s instanceof Square) {
                    echo 1;
                } elseif ($s instanceof Circle) {
                    echo 2;
                }
            }
        "#,
    );
    assert!(instanceof_findings(&d).is_empty(), "got: {d:?}");
}

#[test]
fn dynamic_instanceof_target_that_cannot_name_a_class_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function f(object $o, int $i, string $s): void {
                if ($o instanceof $i) { echo 1; }
                if ($o instanceof $s) { echo 2; }
            }
        "#,
    );
    assert_eq!(
        instanceof_findings(&d),
        vec![(
            "instanceof.invalidExprType",
            "Instanceof between object and int results in an error."
        )],
        "got: {d:?}"
    );
}

#[test]
fn instanceof_in_a_trait_is_flagged_only_when_every_host_agrees() {
    let d = diagnostics_for(
        r#"<?php
            interface Marker {}
            trait Checks {
                public function isMarker(): bool {
                    if ($this instanceof Marker) { return true; }
                    return false;
                }
            }
            class Marked implements Marker { use Checks; }
            class Unmarked { use Checks; }
        "#,
    );
    assert!(instanceof_findings(&d).is_empty(), "got: {d:?}");
}

#[test]
fn dynamic_class_string_targets_are_decided_where_possible() {
    let d = diagnostics_for(
        r#"<?php
            class Base {}
            class Child extends Base {}
            final class Sealed {}
            class Other {}
            /**
             * @param class-string<Base> $base
             * @param class-string<Sealed> $sealed
             */
            function f(Other $o, Child $c, Sealed $s, string $base, string $sealed): void {
                $lit = 'Other';
                if ($o instanceof $lit) { echo 1; }
                if ($o instanceof $base) { echo 2; }
                if ($c instanceof $base) { echo 3; }
                if ($s instanceof $sealed) { echo 4; }
            }
        "#,
    );
    assert_eq!(
        instanceof_findings(&d),
        vec![
            (
                "instanceof.alwaysTrue",
                "Instanceof between Other and Other will always evaluate to true."
            ),
            (
                "instanceof.alwaysFalse",
                "Instanceof between Other and Base will always evaluate to false."
            ),
            (
                "instanceof.alwaysTrue",
                "Instanceof between Sealed and Sealed will always evaluate to true."
            ),
        ],
        "got: {d:?}"
    );
}
