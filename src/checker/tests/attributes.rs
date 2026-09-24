//! Attribute semantic validation: target checking and constructor-argument
//! arity/type/named-argument checking.

use super::helpers::*;

// Constructor-argument validation reuses `resolve_call_args`/`check_args`
// directly for an attribute's own constructor call, the same machinery
// `ExprKind::New` uses.

#[test]
fn an_attribute_class_inheriting_its_constructor_from_an_ancestor_is_not_flagged() {
    // The constructor lookup must walk the ancestor chain
    // (`Registry::lookup_method`), not just this class's own declared
    // methods.
    let d = diagnostics_for(
        r#"<?php
            #[Attribute]
            class BaseAttr {
                public function __construct(public string $name) {}
            }

            #[Attribute]
            class MyAttr extends BaseAttr {}

            #[MyAttr('reset')]
            class Foo {}
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}
