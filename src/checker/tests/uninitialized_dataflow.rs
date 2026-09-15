//! The premature-access half - `checker/uninitialized_dataflow.rs`'s
//! per-constructor "was this property definitely assigned by this exact
//! point" dataflow. Purely additive to
//! `checker/tests/uninitialized.rs`'s existing "never initialized"
//! suite: every scenario here uses a property that genuinely does get
//! assigned somewhere, so the only question is whether a read reaches
//! it too early on some path.

use super::helpers::*;

fn diagnostics_for(src: &str) -> Vec<super::super::Diagnostic> {
    diagnostics_for_with(src, |r| r.check_uninitialized_properties = true)
}

#[test]
fn a_read_on_the_branch_that_has_not_yet_assigned_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {
                public int $x;
                public function __construct(bool $c) {
                    if ($c) {
                        $this->x = 1;
                    } else {
                        echo $this->x;
                        $this->x = 2;
                    }
                }
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "property.uninitialized"),
        "got: {d:?}"
    );
}

#[test]
fn a_read_after_both_branches_have_converged_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {
                public int $x;
                public function __construct(bool $c) {
                    if ($c) {
                        $this->x = 1;
                    } else {
                        $this->x = 2;
                    }
                    echo $this->x;
                }
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "property.uninitialized"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_read_after_an_unconditional_assignment_in_the_same_constructor_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {
                public int $x;
                public function __construct() {
                    $this->x = 1;
                    echo $this->x;
                }
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "property.uninitialized"),
        "unexpected diagnostics: {d:?}"
    );
}

// The real, new piece: a property assigned only inside a same-class
// method the constructor calls - absorbed, not flagged, proving the
// walker actually follows the call into `init()`'s own body.
#[test]
fn a_read_after_calling_a_same_class_method_that_assigns_it_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {
                public int $x;
                public function __construct() {
                    $this->init();
                    echo $this->x;
                }
                private function init(): void {
                    $this->x = 1;
                }
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "property.uninitialized"),
        "unexpected diagnostics: {d:?}"
    );
}

// The mirror image: a read *inside* the called method, before that same
// method's own assignment - proves the recursive walk's entry-to-exit
// threading actually reaches into the called method's body, not just
// its end result.
#[test]
fn a_premature_read_inside_a_called_method_before_its_own_assignment_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {
                public int $x;
                public function __construct() {
                    $this->init();
                }
                private function init(): void {
                    echo $this->x;
                    $this->x = 1;
                }
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "property.uninitialized"),
        "got: {d:?}"
    );
}

// A loop might run zero times - a property assigned only inside one is
// never guaranteed by the time execution reaches just past it.
#[test]
fn a_read_immediately_after_a_loop_that_only_maybe_assigns_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {
                public int $x;
                public function __construct(array $items) {
                    foreach ($items as $item) {
                        $this->x = $item;
                    }
                    echo $this->x;
                }
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "property.uninitialized"),
        "got: {d:?}"
    );
}

// A regression test for the disclosed cycle-handling simplification
// (`PropertyInitCtx::visiting`) - a same-class call cycle reachable from
// the constructor must not hang or panic.
#[test]
fn a_cyclic_same_class_call_graph_does_not_hang_or_panic() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {
                public int $x;
                public function __construct() {
                    $this->a();
                }
                private function a(): void {
                    $this->b();
                }
                private function b(): void {
                    $this->a();
                    $this->x = 1;
                }
            }
        "#,
    );
    // The point of this test is that the line above returns at all -
    // whatever it reports (or doesn't) about this deliberately
    // convoluted shape is secondary to that.
    let _ = d;
}

// Design ##3's own motivating shape: a property re-declared across an
// inheritance chain, initialized only by the *parent's* own constructor -
// `Child`'s own body never assigns `$x` directly at all, so without
// `parent::__construct()` absorption this would be indistinguishable
// from a genuinely premature read.
#[test]
fn a_property_initialized_only_by_parent_construct_is_not_flagged_in_the_subclass() {
    let d = diagnostics_for(
        r#"<?php
            class Base {
                protected int $x;
                public function __construct() {
                    $this->x = 1;
                }
            }
            class Child extends Base {
                protected int $x;
                public function __construct() {
                    parent::__construct();
                    echo $this->x;
                }
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "property.uninitialized"),
        "unexpected diagnostics: {d:?}"
    );
}

// The virtual-dispatch subtlety: `Base::__construct()` calls
// `$this->setup()`, which `Child` overrides to perform the real
// initialization. When the walker recurses into `parent::__construct()`
// (an inherited method body), `$this->setup()` inside *that* body must
// still resolve against `Child` (the class actually being checked), not
// `Base` (`setup`'s own lexical declaring class) - otherwise the walker
// would resolve to `Base::setup()` (which does nothing) and wrongly
// flag a genuinely safe read as premature.
//
// `Base::$x` deliberately has a default value here, so `Base` checked in
// isolation is never itself a "never initialized" candidate at all - a
// concrete class whose own constructor relies entirely on a *subclass's*
// override to finish initialization is a real, separate, harder gap
// this project's existing heuristic doesn't (and can't, in general)
// close (see `property_assigned_anywhere_in_hierarchy`'s own doc
// comment: it only ever walks *up* an ancestor chain, never down into a
// subclass's own overrides). Isolating that from this test keeps this
// one scenario testing only the virtual-dispatch resolution it's meant
// to.
#[test]
fn this_method_calls_inside_an_absorbed_parent_body_dispatch_virtually() {
    let d = diagnostics_for(
        r#"<?php
            class Base {
                protected int $x = 0;
                public function __construct() {
                    $this->setup();
                }
                protected function setup(): void {
                }
            }
            class Child extends Base {
                protected int $x;
                public function __construct() {
                    parent::__construct();
                    echo $this->x;
                }
                protected function setup(): void {
                    $this->x = 1;
                }
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "property.uninitialized"),
        "unexpected diagnostics: {d:?}"
    );
}

// The existing "never initialized" heuristic's own suite
// (`checker/tests/uninitialized.rs`) is untouched by this module - this
// is purely additive. A quick, direct spot-check here rather than trust
// alone: a property genuinely never assigned anywhere still gets exactly
// one diagnostic (from the existing check), not a second one from this
// module double-reporting the same property.
#[test]
fn a_never_initialized_property_is_reported_exactly_once_not_twice() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {
                public int $x;
                public function __construct() {
                    echo "hi";
                }
            }
        "#,
    );
    let count = d
        .iter()
        .filter(|x| x.identifier == "property.uninitialized")
        .count();
    assert_eq!(count, 1, "got: {d:?}");
}
