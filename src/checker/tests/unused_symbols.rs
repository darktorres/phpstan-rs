//! Unused constructor parameters, unused closure `use(...)` captures,
//! and unused private consts/methods/properties.

use super::helpers::*;

#[test]
fn unused_constructor_parameter_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Point {
                public function __construct(int $x, int $y) {
                    $this->store($x);
                }
                private function store(int $x): void {}
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.message.contains("unused parameter $y")),
        "got: {d:?}"
    );
    assert!(!d.iter().any(|x| x.message.contains("$x")), "got: {d:?}");
}

#[test]
fn used_constructor_parameter_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Point {
                public function __construct(int $x, int $y) {
                    echo $x + $y;
                }
            }
        "#,
    );
    assert!(!d.iter().any(|x| x.message.contains("unused parameter")));
}

#[test]
fn promoted_constructor_parameter_is_never_flagged_as_unused() {
    // The whole point of promotion is that the parameter never needs to
    // be read again in the constructor's own body - the promotion itself
    // (`private readonly Foo $foo`) is the real use.
    let d = diagnostics_for(
        r#"<?php
            class Wrapper {
                public function __construct(private readonly int $value) {
                    // Body deliberately does nothing else with $value.
                }
            }
        "#,
    );
    assert!(!d.iter().any(|x| x.message.contains("unused parameter")));
}

#[test]
fn only_constructors_are_checked_for_unused_parameters_not_ordinary_methods() {
    // A general "unused parameter" rule on ordinary methods would flag
    // interface implementations and overrides constantly - real PHPStan's
    // own UnusedConstructorParametersRule is constructor-only, and this
    // project follows that scope exactly (see unused.rs's own doc comment).
    let d = diagnostics_for(
        r#"<?php
            class Handler {
                public function handle(int $x, int $y): int {
                    return $x;
                }
            }
        "#,
    );
    assert!(d.is_empty(), "got: {d:?}");
}

#[test]
fn func_get_args_suppresses_unused_constructor_parameter() {
    let d = diagnostics_for(
        r#"<?php
            class Bag {
                public function __construct(int $x, int $y) {
                    $args = func_get_args();
                    echo count($args);
                }
            }
        "#,
    );
    assert!(!d.iter().any(|x| x.message.contains("unused parameter")));
}

#[test]
fn compact_with_literal_names_only_marks_those_names_used() {
    let d = diagnostics_for(
        r#"<?php
            class Bag {
                public array $data = [];
                public function __construct(int $x, int $y) {
                    $this->data = compact('x');
                }
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.message.contains("unused parameter $y")),
        "got: {d:?}"
    );
    assert!(!d.iter().any(|x| x.message.contains("$x")), "got: {d:?}");
}

#[test]
fn compact_with_a_dynamic_name_suppresses_the_whole_constructor() {
    let d = diagnostics_for(
        r#"<?php
            class Bag {
                public function __construct(int $x, int $y, string $which) {
                    $this->store(compact($which));
                }
                private function store(array $a): void {}
            }
        "#,
    );
    assert!(!d.iter().any(|x| x.message.contains("unused parameter")));
}

#[test]
fn get_defined_vars_suppresses_unused_constructor_parameters() {
    let d = diagnostics_for(
        r#"<?php
            class Bag {
                public function __construct(int $x, int $y) {
                    $this->store(get_defined_vars());
                }
                private function store(array $a): void {}
            }
        "#,
    );
    assert!(!d.iter().any(|x| x.message.contains("unused parameter")));
}

#[test]
fn unused_closure_use_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function makeAdder(int $base) {
                return function (int $x) use ($base) {
                    return $x;
                };
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.message.contains("unused use ($base)")),
        "got: {d:?}"
    );
}

#[test]
fn used_closure_use_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function makeAdder(int $base) {
                return function (int $x) use ($base) {
                    return $x + $base;
                };
            }
        "#,
    );
    assert!(!d.iter().any(|x| x.message.contains("unused use")));
}

#[test]
fn closure_use_referenced_only_via_nested_closure_capture_is_not_flagged() {
    // The outer closure's own `use ($base)` capture is "used" by the fact
    // that the inner closure captures it too - the inner closure's own
    // body doesn't need to read it for the outer capture to be considered
    // referenced.
    let d = diagnostics_for(
        r#"<?php
            function make(int $base) {
                return function () use ($base) {
                    return function () use ($base) {
                        return $base;
                    };
                };
            }
        "#,
    );
    assert!(!d.iter().any(|x| x.message.contains("unused use")));
}

#[test]
fn closure_param_with_the_same_name_as_an_unrelated_outer_use_is_not_a_false_use() {
    // Real shape this guards against: a nested closure's own *parameter*
    // (a completely independent binding) happens to share a name with the
    // outer closure's own `use(...)` capture - the outer capture must
    // still be reported unused, not "saved" by the inner shadowing param.
    let d = diagnostics_for(
        r#"<?php
            function make(int $unused) {
                return function () use ($unused) {
                    return function ($unused) {
                        return $unused;
                    };
                };
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.message.contains("unused use ($unused)")),
        "got: {d:?}"
    );
}

#[test]
fn unused_private_constant_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Config {
                private const UNUSED = 1;
                private const USED = 2;
                public function get(): int {
                    return self::USED;
                }
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x
            .message
            .contains("private constant Config::UNUSED is unused")),
        "got: {d:?}"
    );
    assert!(
        !d.iter()
            .any(|x| x.message.contains("USED is unused") && !x.message.contains("UN"))
    );
}

#[test]
fn public_and_protected_constants_are_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Config {
                public const A = 1;
                protected const B = 2;
            }
        "#,
    );
    assert!(d.is_empty(), "got: {d:?}");
}

#[test]
fn private_constant_referenced_from_another_private_constant_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Config {
                private const BASE = 1;
                private const DERIVED = self::BASE + 1;
                public function get(): int {
                    return self::DERIVED;
                }
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.message.contains("is unused")),
        "got: {d:?}"
    );
}

#[test]
fn unused_private_method_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Service {
                public function run(): int {
                    return $this->helper();
                }
                private function helper(): int { return 1; }
                private function deadCode(): int { return 2; }
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x
            .message
            .contains("private method Service::deadCode() is unused")),
        "got: {d:?}"
    );
    assert!(!d.iter().any(|x| x.message.contains("helper() is unused")));
}

#[test]
fn private_static_method_gets_the_static_wording() {
    let d = diagnostics_for(
        r#"<?php
            class Service {
                private static function deadCode(): int { return 2; }
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x
            .message
            .contains("private static method Service::deadCode() is unused")),
        "got: {d:?}"
    );
}

#[test]
fn private_method_called_via_self_or_static_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Service {
                public function run(): int {
                    return self::helper() + static::other();
                }
                private static function helper(): int { return 1; }
                private static function other(): int { return 2; }
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.message.contains("is unused")),
        "got: {d:?}"
    );
}

#[test]
fn private_method_used_only_as_an_array_callable_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Sorter {
                public function sort(array $items): array {
                    usort($items, [$this, 'compare']);
                    return $items;
                }
                private function compare(int $a, int $b): int { return $a <=> $b; }
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.message.contains("is unused")),
        "got: {d:?}"
    );
}

#[test]
fn private_method_referenced_only_inside_a_closure_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Mapper {
                public function mapAll(array $xs): array {
                    return array_map(function ($x) {
                        return self::helper($x);
                    }, $xs);
                }
                private static function helper(int $x): int { return $x * 2; }
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.message.contains("is unused")),
        "got: {d:?}"
    );
}

#[test]
fn private_method_referenced_only_inside_an_arrow_fn_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Mapper {
                public function mapAll(array $xs): array {
                    return array_map(fn ($x) => self::helper($x), $xs);
                }
                private static function helper(int $x): int { return $x * 2; }
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.message.contains("is unused")),
        "got: {d:?}"
    );
}

#[test]
fn private_property_read_only_inside_a_closure_is_not_write_only() {
    let d = diagnostics_for(
        r#"<?php
            class Widget {
                private int $count;
                public function __construct(int $count) {
                    $this->count = $count;
                }
                public function describe(): \Closure {
                    return function (): int {
                        return $this->count;
                    };
                }
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.message.contains("Widget::$count")),
        "got: {d:?}"
    );
}

#[test]
fn private_constant_referenced_only_inside_a_closure_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Config {
                private const LIMIT = 10;
                public function makeChecker(): \Closure {
                    return function (int $x): bool {
                        return $x < self::LIMIT;
                    };
                }
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.message.contains("is unused")),
        "got: {d:?}"
    );
}

#[test]
fn private_const_referenced_only_in_an_anonymous_class_constructor_arg_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Config {
                private const LIMIT = 10;
                public function make(): object {
                    return new class(self::LIMIT) {
                        public function __construct(public int $limit) {}
                    };
                }
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.message.contains("is unused")),
        "got: {d:?}"
    );
}

#[test]
fn private_method_reached_through_a_new_self_assigned_local_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Node {
                public function withSibling(): self {
                    $other = new self();
                    $other->helper();
                    return $other;
                }
                private function helper(): void {}
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.message.contains("is unused")),
        "got: {d:?}"
    );
}

#[test]
fn private_property_reached_through_an_instanceof_self_narrowed_local_is_not_flagged() {
    // `$other` is deliberately typed `object`, not `Backend` (unlike the
    // sibling `private_method_reached_through_a_self_typed_parameter_is_
    // not_flagged` test) - a same-class parameter type would already
    // establish the alias via `seed_self_typed_param_aliases` (rule 3),
    // which would defeat the point of this test: isolating the
    // `instanceof self` narrowing rule (rule 2) on its own (an unknown/
    // interface-typed value only narrowed by the `instanceof` check).
    let d = diagnostics_for(
        r#"<?php
            class Backend {
                private bool $flag = false;
                public function compose(object $other): void {
                    if (! $other instanceof self) {
                        throw new \LogicException("nope");
                    }
                    if (! $other->flag) {
                        $other->flag = true;
                    }
                }
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.message.contains("Backend::$flag")),
        "got: {d:?}"
    );
}

#[test]
fn private_method_reached_through_a_self_typed_parameter_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Money {
                public function add(self $other): void {
                    $other->helper();
                }
                private function helper(): void {}
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.message.contains("is unused")),
        "got: {d:?}"
    );
}

#[test]
fn a_local_variable_assigned_an_unrelated_class_is_not_treated_as_self() {
    let d = diagnostics_for(
        r#"<?php
            class Other {
                public function helper(): void {}
            }
            class Foo {
                public function run(): void {
                    $x = new Other();
                    $x->helper();
                }
                private function helper(): void {}
            }
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.message.contains("private method Foo::helper() is unused")),
        "got: {d:?}"
    );
}

#[test]
fn constructor_and_clone_are_never_flagged_as_unused_private_methods() {
    let d = diagnostics_for(
        r#"<?php
            class Value {
                private function __construct(private int $x) {}
                private function __clone() {}
                public static function of(int $x): self {
                    return new self($x);
                }
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.message.contains("is unused")),
        "got: {d:?}"
    );
}

#[test]
fn completely_unused_private_property_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Widget {
                private int $deadWeight;
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x
            .message
            .contains("private property Widget::$deadWeight is unused")),
        "got: {d:?}"
    );
}

#[test]
fn write_only_private_property_is_flagged_as_never_read() {
    let d = diagnostics_for(
        r#"<?php
            class Widget {
                private int $count;
                public function bump(): void {
                    $this->count = 1;
                }
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x
            .message
            .contains("private property Widget::$count is never read, only written")),
        "got: {d:?}"
    );
}

#[test]
fn read_only_private_property_with_no_writer_is_flagged_as_never_written() {
    let d = diagnostics_for(
        r#"<?php
            class Widget {
                private int $count;
                public function get(): int {
                    return $this->count;
                }
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x
            .message
            .contains("private property Widget::$count is never written, only read")),
        "got: {d:?}"
    );
}

#[test]
fn private_property_with_default_and_no_write_is_never_read_only_written_not_unused() {
    // A declared default counts as an implicit write - matches real
    // PHPStan (`$written = $alwaysWritten || $property->getDefault() !==
    // null`), so this is "never read, only written", not fully unused.
    let d = diagnostics_for(
        r#"<?php
            class Widget {
                private int $count = 0;
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x
            .message
            .contains("private property Widget::$count is never read, only written")),
        "got: {d:?}"
    );
    assert!(
        !d.iter().any(|x| x.message.contains("is unused")),
        "got: {d:?}"
    );
}

#[test]
fn private_property_read_and_written_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Widget {
                private int $count = 0;
                public function bump(): void {
                    $this->count = $this->count + 1;
                }
                public function get(): int {
                    return $this->count;
                }
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.message.contains("Widget::$count")),
        "got: {d:?}"
    );
}

#[test]
fn promoted_private_property_actually_read_elsewhere_is_not_flagged() {
    // The extremely common `private readonly` promotion idiom - promotion
    // itself counts as the write, and this test's own body supplies a
    // real read too, so nothing should be reported at all.
    let d = diagnostics_for(
        r#"<?php
            class Money {
                public function __construct(private readonly int $cents) {}
                public function inDollars(): float {
                    return $this->cents / 100.0;
                }
            }
        "#,
    );
    assert!(d.is_empty(), "got: {d:?}");
}

#[test]
fn promoted_private_property_never_read_anywhere_is_flagged_as_write_only() {
    let d = diagnostics_for(
        r#"<?php
            class Money {
                public function __construct(private readonly int $cents) {}
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x
            .message
            .contains("private property Money::$cents is never read, only written")),
        "got: {d:?}"
    );
}

#[test]
fn always_read_tag_suppresses_a_false_never_read_report() {
    let d = diagnostics_for(
        r#"<?php
            class Widget {
                /** @phpstan-always-read */
                private int $count;
                public function bump(): void {
                    $this->count = 1;
                }
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.message.contains("Widget::$count")),
        "got: {d:?}"
    );
}

#[test]
fn dynamic_property_access_suppresses_the_whole_class_private_member_check() {
    let d = diagnostics_for(
        r#"<?php
            class Widget {
                private int $one;
                private int $two;
                private function helper(): int { return 1; }
                public function magic(string $name): int {
                    return $this->{$name};
                }
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.message.contains("Widget")),
        "got: {d:?}"
    );
}

#[test]
fn unused_private_enum_constant_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            enum Suit {
                case Hearts;
                case Spades;
                private const UNUSED = 'x';
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x
            .message
            .contains("private constant Suit::UNUSED is unused")),
        "got: {d:?}"
    );
}

#[test]
fn private_methods_declared_in_a_trait_are_not_flagged_by_the_using_class() {
    // Trait-provided members aren't candidates on the using class at all -
    // this project's own trait-flattening (`registry::flatten_traits`)
    // never feeds back into `ClassDecl::methods` itself, so a class that
    // merely `use`s a trait shouldn't get spurious "unused" findings for
    // methods it never literally declares.
    let d = diagnostics_for(
        r#"<?php
            trait Greets {
                private function greeting(): string { return 'hi'; }
                public function greet(): string { return $this->greeting(); }
            }
            class Greeter {
                use Greets;
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.message.contains("is unused")),
        "got: {d:?}"
    );
}

#[test]
fn unused_constructor_parameter_uses_its_own_identifier() {
    let d = diagnostics_for(
        r#"<?php
            class Point {
                public function __construct(int $x) {
                    echo 1;
                }
            }
        "#,
    );
    let found = d
        .iter()
        .find(|x| x.message.contains("unused parameter $x"))
        .unwrap_or_else(|| panic!("got: {d:?}"));
    assert_eq!(found.identifier, "constructor.unusedParameter");
    assert!(
        !d.iter().any(|x| x.identifier == "parameter.unused"),
        "got: {d:?}"
    );
}

#[test]
fn readonly_or_final_only_promoted_parameter_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Point {
                public function __construct(readonly int $x, final int $y) {
                    echo 1;
                }
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.message.contains("unused parameter")),
        "got: {d:?}"
    );
}

#[test]
fn attribute_class_constructor_parameters_are_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            #[\Attribute]
            class Route {
                public function __construct(string $path) {
                    echo 1;
                }
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.message.contains("unused parameter")),
        "got: {d:?}"
    );
}

#[test]
fn constructor_dictated_by_an_interface_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            interface Makeable {
                public function __construct(int $seed);
            }
            abstract class Base implements Makeable {}
            class Direct implements Makeable {
                public function __construct(int $seed) {
                    echo 1;
                }
            }
            class Inherited extends Base {
                public function __construct(int $seed) {
                    echo 1;
                }
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.message.contains("unused parameter")),
        "got: {d:?}"
    );
}

#[test]
fn interface_without_a_constructor_does_not_exempt_the_class() {
    let d = diagnostics_for(
        r#"<?php
            interface Shape {
                public function area(): int;
            }
            class Square implements Shape {
                public function __construct(int $side) {
                    echo 1;
                }
                public function area(): int { return 1; }
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x
            .message
            .contains("Constructor of class Square has an unused parameter $side.")),
        "got: {d:?}"
    );
}

#[test]
fn anonymous_class_unused_constructor_parameter_names_the_anonymous_class() {
    let d = diagnostics_for(
        r#"<?php
            function make(): object {
                return new class(1) {
                    public function __construct(int $seed) {
                        echo 1;
                    }
                };
            }
        "#,
    );
    assert!(
        d.iter().any(
            |x| x.message == "Constructor of an anonymous class has an unused parameter $seed."
        ),
        "got: {d:?}"
    );
}

#[test]
fn empty_bodied_constructor_parameters_are_still_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Empty1 {
                public function __construct(int $x) {}
            }
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "constructor.unusedParameter"),
        "got: {d:?}"
    );
}

#[test]
fn an_eval_in_a_constructor_body_counts_as_using_every_parameter() {
    let d = diagnostics_for(
        r#"<?php
            class Scripted {
                public function __construct(string $code, int $seed) {
                    eval($code);
                }
            }
        "#,
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "constructor.unusedParameter"),
        "{d:?}"
    );
}
