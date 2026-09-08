//! @template binding, variance, bounds, and class-level generics.

use super::helpers::*;

#[test]
fn template_binds_from_argument_and_flags_downstream_misuse() {
    let d = diagnostics_for(
        r#"<?php
            /**
             * @template T
             * @param T $value
             * @return T
             */
            function identity($value) { return $value; }
            function needsInt(int $x): int { return $x; }
            needsInt(identity("wrong"));
        "#,
    );
    assert_eq!(d.len(), 1);
    assert!(d[0].message.contains("needsInt"));
}

#[test]
fn template_binds_from_argument_in_a_namespaced_file() {
    // The non-namespaced twin above already covers the substitution
    // logic itself - what's specifically under test here is that
    // resolving `identity` as a call target (through
    // resolve_function_candidates, since Phase 9.5) doesn't disturb
    // the separate template-binding step that runs afterward on
    // whatever signature that resolution finds.
    let d = diagnostics_for(
        r#"<?php
            namespace App;
            /**
             * @template T
             * @param T $value
             * @return T
             */
            function identity($value) { return $value; }
            function needsInt(int $x): int { return $x; }
            needsInt(identity("wrong"));
        "#,
    );
    assert_eq!(d.len(), 1, "unexpected diagnostics: {d:?}");
    assert!(d[0].message.contains("needsInt"));
}

#[test]
fn template_binding_accepts_correctly_typed_usage() {
    let d = diagnostics_for(
        r#"<?php
            /**
             * @template T
             * @param T $value
             * @return T
             */
            function identity($value) { return $value; }
            function needsInt(int $x): int { return $x; }
            needsInt(identity(5));
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn array_template_binds_element_type_from_a_precisely_typed_argument() {
    let d = diagnostics_for(
        r#"<?php
            /**
             * @template T
             * @param T[] $items
             * @return T
             */
            function firstOf(array $items) { return $items[0]; }
            function needsInt(int $x): int { return $x; }
            /** @param string[] $words */
            function process(array $words) {
                return needsInt(firstOf($words));
            }
        "#,
    );
    assert_eq!(d.len(), 1);
    assert!(d[0].message.contains("needsInt"));
}

#[test]
fn class_level_template_substitutes_a_method_return_type() {
    // Collection<User>::first(): T resolves to User, not an unbound T
    // (which would otherwise read as Mixed and never mismatch
    // anything) - proven here by returning it from a function
    // declared to return `int`, which only fails if the inferred type
    // really is User. $c's own generic type comes from a `@param`
    // annotation on an actual property/param/return declaration - a
    // separate family of tests (see
    // `an_inline_var_annotation_...` below) covers the inline
    // `@var` form on an arbitrary statement.
    let d = diagnostics_for(
        r#"<?php
            class User {}
            /**
             * @template T
             */
            class Collection {
                /** @return T */
                public function first() { return null; }
            }
            /**
             * @param Collection<User> $c
             * @return int
             */
            function f($c): int {
                return $c->first();
            }
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.message.contains("return type mismatch") && x.message.contains("User")),
        "expected a return-type mismatch naming User, got: {d:?}"
    );
}

#[test]
fn class_string_template_binds_the_concrete_subclass_from_a_static_factory_call() {
    // Phase 35 Finding E: `TypedRepository::narrow`-shaped repro -
    // `class-string<R>` needs to survive from the argument
    // (`Derived::class`) all the way through `@template R of Base`
    // binding to the `@return R` result, not collapse to plain
    // `string` (which would carry no `R` to bind at all).
    let d = diagnostics_for(
        r#"<?php
            class Base {}
            class Derived extends Base {}
            class TypedRepository {
                /**
                 * @template R of Base
                 * @param class-string<R> $repositoryClass
                 * @return R
                 */
                public static function narrow(string $repositoryClass) {
                    return new $repositoryClass();
                }
            }
            function f(): int {
                return TypedRepository::narrow(Derived::class);
            }
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.message.contains("return type mismatch") && x.message.contains("Derived")),
        "expected a return-type mismatch naming Derived, got: {d:?}"
    );
}

#[test]
fn a_class_literal_still_satisfies_an_ordinary_string_parameter() {
    // Backward compatibility: `Foo::class` used to infer plain
    // `String`; now it's `ClassString(Object("Foo"))`, which must stay
    // freely assignable to an ordinary `string`-typed parameter - the
    // extremely common logging/error-message idiom.
    let d = diagnostics_for(
        r#"<?php
            class Foo {}
            function needsString(string $s): void {}
            function f(): void {
                needsString(Foo::class);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_class_string_template_parameter_still_accepts_a_plain_string_argument() {
    // The other, deliberately lenient direction: this project can't
    // verify a plain string's actual runtime content names a real
    // class, so a `class-string<R>`-typed parameter must still accept
    // an ordinary, non-literal `string` argument rather than
    // false-positive on it.
    let d = diagnostics_for(
        r#"<?php
            class Base {}
            class TypedRepository {
                /**
                 * @template R of Base
                 * @param class-string<R> $repositoryClass
                 * @return R
                 */
                public static function narrow(string $repositoryClass) {
                    return new $repositoryClass();
                }
            }
            function f(string $repositoryClass): void {
                TypedRepository::narrow($repositoryClass);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn get_class_of_a_known_object_infers_a_class_string_binding_the_same_template() {
    let d = diagnostics_for(
        r#"<?php
            class Base {}
            class Derived extends Base {}
            class TypedRepository {
                /**
                 * @template R of Base
                 * @param class-string<R> $repositoryClass
                 * @return R
                 */
                public static function narrow(string $repositoryClass) {
                    return new $repositoryClass();
                }
            }
            function f(Derived $d): int {
                return TypedRepository::narrow(get_class($d));
            }
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.message.contains("return type mismatch") && x.message.contains("Derived")),
        "expected a return-type mismatch naming Derived, got: {d:?}"
    );
}

#[test]
fn get_class_of_an_unknown_typed_value_stays_plain_string() {
    let d = diagnostics_for(
        r#"<?php
            function needsString(string $s): void {}
            function f($x): void {
                needsString(get_class($x));
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn class_level_template_substitutes_a_property_type() {
    let d = diagnostics_for(
        r#"<?php
            class User {}
            /**
             * @template T
             */
            class Box {
                /** @var T */
                public $value;
            }
            /**
             * @param Box<User> $b
             * @return int
             */
            function f($b): int {
                return $b->value;
            }
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.message.contains("return type mismatch") && x.message.contains("User")),
        "expected a return-type mismatch naming User, got: {d:?}"
    );
}

#[test]
fn class_level_template_substitutes_a_method_argument_type() {
    let d = diagnostics_for(
        r#"<?php
            class User {}
            class Post {}
            /**
             * @template T
             */
            class Collection {
                /** @param T $item */
                public function add($item): void {}
            }
            /** @param Collection<User> $c */
            function f($c) {
                $c->add(new Post());
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.message.contains("expects User")),
        "expected an argument-type mismatch naming User, got: {d:?}"
    );
}

#[test]
fn class_level_template_argument_matching_the_bound_type_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class User {}
            /**
             * @template T
             */
            class Collection {
                /** @param T $item */
                public function add($item): void {}
            }
            /** @param Collection<User> $c */
            function f($c) {
                $c->add(new User());
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn bare_reference_to_a_generic_class_treats_template_as_mixed() {
    // No `<...>` at all - an "erased"/unspecified generic - so T stays
    // unbound and reads as Mixed, matching this checker's general
    // "don't know, don't flag" policy rather than guessing.
    let d = diagnostics_for(
        r#"<?php
            /**
             * @template T
             */
            class Collection {
                /** @return T */
                public function first() { return null; }
            }
            /** @return int */
            function f(): int {
                $c = new Collection();
                return $c->first();
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn extends_tag_fixes_a_non_generic_subclass_s_inherited_template_param() {
    // IntCollection isn't generic itself, but its own `@extends
    // Collection<int>` permanently binds Collection's own T to int
    // (Phase 10.5) - proven the same way the class-level tests above
    // do, via a return-type mismatch that only fires if T really did
    // resolve to int rather than staying unbound (Mixed).
    let d = diagnostics_for(
        r#"<?php
            /**
             * @template T
             */
            class Collection {
                /** @return T */
                public function first() { return null; }
            }
            /** @extends Collection<int> */
            class IntCollection extends Collection {}
            /**
             * @param IntCollection $c
             * @return string
             */
            function f($c): string {
                return $c->first();
            }
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.message.contains("return type mismatch") && x.message.contains("int")),
        "expected a return-type mismatch naming int, got: {d:?}"
    );
}

#[test]
fn extends_tag_argument_matching_the_fixed_type_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            /**
             * @template T
             */
            class Collection {
                /** @param T $item */
                public function add($item): void {}
            }
            /** @extends Collection<int> */
            class IntCollection extends Collection {}
            function f(IntCollection $c) {
                $c->add(1);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn extends_tag_argument_of_the_wrong_type_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            /**
             * @template T
             */
            class Collection {
                /** @param T $item */
                public function add($item): void {}
            }
            /** @extends Collection<int> */
            class IntCollection extends Collection {}
            function f(IntCollection $c) {
                $c->add("not an int");
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.message.contains("expects int")),
        "expected an argument-type mismatch naming int, got: {d:?}"
    );
}

#[test]
fn extends_tag_passes_a_generic_subclass_s_own_template_through() {
    // Box<T> extends Collection<T> - Box doesn't fix T to a concrete
    // type, it just forwards its own T - so Box<User>'s inherited
    // first() should resolve to User, not stay unbound.
    let d = diagnostics_for(
        r#"<?php
            class User {}
            /**
             * @template T
             */
            class Collection {
                /** @return T */
                public function first() { return null; }
            }
            /**
             * @template T
             * @extends Collection<T>
             */
            class Box extends Collection {}
            /**
             * @param Box<User> $b
             * @return int
             */
            function f($b): int {
                return $b->first();
            }
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.message.contains("return type mismatch") && x.message.contains("User")),
        "expected a return-type mismatch naming User, got: {d:?}"
    );
}

#[test]
fn implements_tag_fixes_an_interface_s_own_template_param() {
    let d = diagnostics_for(
        r#"<?php
            class User {}
            class Post {}
            /**
             * @template T
             */
            interface Comparator {
                /** @param T $other */
                public function compareTo($other): int;
            }
            // Deliberately doesn't redeclare compareTo - this checker
            // doesn't enforce interface completeness (a separate,
            // untracked gap), and the point here is exercising
            // lookup_method's fall-through to the interface's own
            // declaration, not a concrete override's.
            /** @implements Comparator<User> */
            class UserComparator implements Comparator {}
            function f(UserComparator $c) {
                $c->compareTo(new Post());
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.message.contains("expects User")),
        "expected an argument-type mismatch naming User, got: {d:?}"
    );
}

#[test]
fn a_subclass_with_no_extends_tag_leaves_the_inherited_template_unbound() {
    // Plain native `extends` with no `@extends` generic-fixing tag at
    // all - T should stay unbound (Mixed), the same conservative
    // fallback as a bare `new Collection()` reference, not an
    // accidental false positive from treating "extends something
    // generic" alone as enough to bind anything.
    let d = diagnostics_for(
        r#"<?php
            /**
             * @template T
             */
            class Collection {
                /** @return T */
                public function first() { return null; }
            }
            class PlainCollection extends Collection {}
            /** @return string */
            function f(PlainCollection $c): string {
                return $c->first();
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_subclass_s_own_template_name_is_not_shadowed_by_an_unrelated_ancestor_s_same_name() {
    // Real bug this reproduces (found verifying Phase 10.5 against
    // symfony/type-info's real NullableType<T of Type> extends
    // UnionType<T|BuiltinType<...>>, where UnionType has its own,
    // unrelated T): binding a whole flat map across the entire
    // extends chain lets an ancestor's own template name silently
    // overwrite an unrelated same-named one further down the chain,
    // since `T` is by far the most common single-letter generic name
    // in real PHPStan code and nothing stops two unrelated classes
    // from both using it. Child's own `ownValue()` must resolve
    // Child's own `T` (bound to Money at the call site) - not
    // ParentGeneric's unrelated `T` (fixed to Whatever via Child's
    // own `@extends` tag).
    let d = diagnostics_for(
        r#"<?php
            class Money {}
            class Whatever {}
            /**
             * @template T
             */
            class ParentGeneric {
                /** @return T */
                public function parentValue() { return null; }
            }
            /**
             * @template T
             * @extends ParentGeneric<Whatever>
             */
            class Child extends ParentGeneric {
                /** @param T $value */
                public function __construct(private $value) {}
                /** @return T */
                public function ownValue() { return $this->value; }
            }
            /**
             * @param Child<Money> $c
             * @return string
             */
            function f($c): string {
                return $c->ownValue();
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.message.contains("return type mismatch")
            && x.message.contains("Money")
            && !x.message.contains("Whatever")),
        "expected a return-type mismatch naming Money (not Whatever), got: {d:?}"
    );
}

#[test]
fn a_bounded_template_flags_a_param_type_that_violates_the_bound() {
    let d = diagnostics_for(
        r#"<?php
            interface Shape {}
            class NotAShape {}
            /**
             * @template T of Shape
             */
            class Collection {}
            /** @param Collection<NotAShape> $c */
            function f($c) {}
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.message.contains("@template T requires Shape")
                && x.message.contains("NotAShape")),
        "expected a bound-violation diagnostic, got: {d:?}"
    );
}

#[test]
fn a_bounded_template_argument_satisfying_the_bound_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            interface Shape {}
            class Circle implements Shape {}
            /**
             * @template T of Shape
             */
            class Collection {}
            /** @param Collection<Circle> $c */
            function f($c) {}
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// Phase 80 follow-up: a `@template T of value-of<Enum>` bound - exercises
// `Registry::resolve_deferred_value_of` (`registry.rs`) resolving a
// class-level `template_bounds` entry, the one walked location the rest
// of this session's own `checker/tests/key_value_of.rs` tests don't cover
// (those only exercise ordinary param/return/property positions).

#[test]
fn a_template_bound_by_value_of_an_enum_flags_a_non_matching_literal() {
    let d = diagnostics_for(
        r#"<?php
            enum Suit: string {
                case Hearts = 'H';
                case Spades = 'S';
            }
            /**
             * @template T of value-of<Suit>
             */
            class Collection {}
            /** @param Collection<'nope'> $c */
            function f($c) {}
        "#,
    );
    assert!(
        d.iter().any(|x| x.message.contains("@template T requires")
            && x.message.contains("'H'|'S'")
            && x.message.contains("nope")),
        "expected a bound-violation diagnostic, got: {d:?}"
    );
}

#[test]
fn a_template_bound_by_value_of_an_enum_accepts_a_matching_literal() {
    let d = diagnostics_for(
        r#"<?php
            enum Suit: string {
                case Hearts = 'H';
                case Spades = 'S';
            }
            /**
             * @template T of value-of<Suit>
             */
            class Collection {}
            /** @param Collection<'H'> $c */
            function f($c) {}
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_bare_unspecified_generic_never_triggers_bound_checking() {
    // No `<...>` at all - nothing to check, same "don't know, don't
    // guess" fallback as everywhere else an erased generic shows up.
    let d = diagnostics_for(
        r#"<?php
            interface Shape {}
            /**
             * @template T of Shape
             */
            class Collection {}
            function f(Collection $c) {}
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_bound_violation_in_a_return_type_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            interface Shape {}
            class NotAShape {}
            /**
             * @template T of Shape
             */
            class Collection {}
            /** @return Collection<NotAShape> */
            function f() {}
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.message.contains("@template T requires Shape")),
        "expected a bound-violation diagnostic, got: {d:?}"
    );
}

// Real corpus false positive: doctrine/collections' own `Collection<TKey,
// T>` self-reference inside its own `map()`/`filter()`/`partition()`
// methods' `@phpstan-return` tags, where `TKey` (this same class's own
// template, itself bound to `array-key`, i.e. `int|string`) is passed
// right back as the argument satisfying that identical bound.
// `is_subtype`'s/`Registry::is_assignable`'s own generic `(_, Union(
// members)) => any(is_subtype(sub, m))` arm ran *before* the dedicated
// `TemplateParam` arm, splitting the target union member-by-member
// instead of comparing `TKey`'s own union bound against it as a whole -
// each split member failed alone even though the whole bound matched.
#[test]
fn a_template_whose_own_bound_is_a_union_satisfies_that_same_union_bound_via_self_reference() {
    let d = diagnostics_for(
        r#"<?php
            /**
             * @template TKey of array-key
             * @template T
             */
            class Collection {
                /**
                 * @phpstan-return Collection<TKey, T>
                 */
                public function filter() {}
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "generics.templateBound"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_bound_violation_in_a_property_type_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            interface Shape {}
            class NotAShape {}
            /**
             * @template T of Shape
             */
            class Collection {}
            class Holder {
                /** @var Collection<NotAShape> */
                public $items;
            }
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.message.contains("@template T requires Shape")),
        "expected a bound-violation diagnostic, got: {d:?}"
    );
}

#[test]
fn a_bound_violation_via_an_extends_tag_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            interface Shape {}
            class NotAShape {}
            /**
             * @template T of Shape
             */
            class Collection {}
            /** @extends Collection<NotAShape> */
            class Bad extends Collection {}
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.message.contains("@template T requires Shape")),
        "expected a bound-violation diagnostic, got: {d:?}"
    );
}

#[test]
fn a_bound_referencing_the_same_class_s_own_earlier_template_resolves_positionally() {
    // @template B of A - checking B's argument against "A" only makes
    // sense once A's own concrete argument (from the same <...> list)
    // is known, not treated as a literal unrelated class named "A".
    let d = diagnostics_for(
        r#"<?php
            interface Shape {}
            class Circle implements Shape {}
            class NotAShape {}
            /**
             * @template A of Shape
             * @template B of A
             */
            class Pair {}
            /** @param Pair<Circle, NotAShape> $p */
            function f($p) {}
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.message.contains("@template B requires Circle")
                && x.message.contains("NotAShape")),
        "expected B's bound to resolve to Circle (A's own argument), got: {d:?}"
    );
}

#[test]
fn a_bound_of_object_is_satisfied_by_any_class_the_common_doctrine_repository_shape() {
    // Real bug found verifying bound-enforcement against the real
    // Piwigo corpus: `is_assignable` had never treated the native
    // `object` type hint as a universal supertype of any class
    // instance before this (nothing had compared against it this way
    // until bound-checking started) - `Doctrine\ORM\EntityRepository
    // <T of object>` is an extremely common real-world shape every
    // one of Piwigo's own repository classes uses.
    let d = diagnostics_for(
        r#"<?php
            class ActivityEntity {}
            /**
             * @template T of object
             */
            class EntityRepository {}
            /** @extends EntityRepository<ActivityEntity> */
            class ActivityRepository extends EntityRepository {}
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_bound_argument_that_s_an_unregistered_class_is_never_flagged() {
    // Real bug found on the real Piwigo corpus: real phpstan/phpstan
    // ships as a compiled PHAR (no loose source files at all), so
    // `PHPStan\Type\ArrayType` (a real class implementing the real
    // `PHPStan\Type\Type` interface) is never actually registered -
    // this project has no way to verify that relationship, so it
    // shouldn't guess a violation. Uses `Bound` itself as the
    // stand-in for a class this project simply never saw declared.
    let d = diagnostics_for(
        r#"<?php
            /**
             * @template T of Bound
             */
            class Collection {}
            /** @param Collection<UnregisteredArg> $c */
            function f($c) {}
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_covariant_template_used_only_in_return_position_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            /**
             * @template-covariant T
             */
            class Producer {
                /** @return T */
                public function produce() { return null; }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_covariant_template_used_in_a_parameter_position_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            /**
             * @template-covariant T
             */
            class Bad {
                /** @param T $x */
                public function consume($x): void {}
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.message.contains("@template-covariant T")
            && x.message.contains("input (parameter) position")),
        "expected a covariance-violation diagnostic, got: {d:?}"
    );
}

#[test]
fn a_contravariant_template_used_only_in_parameter_position_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            /**
             * @template-contravariant T
             */
            class Consumer {
                /** @param T $x */
                public function consume($x): void {}
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_contravariant_template_used_in_return_position_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            /**
             * @template-contravariant T
             */
            class Bad {
                /** @return T */
                public function produce() { return null; }
            }
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.message.contains("@template-contravariant T")
                && x.message.contains("output (return) position")),
        "expected a contravariance-violation diagnostic, got: {d:?}"
    );
}

#[test]
fn a_plain_invariant_template_is_unrestricted_in_either_position() {
    // No `-covariant`/`-contravariant` at all - Phase 10's original,
    // unrestricted behavior must still hold for a plain @template.
    let d = diagnostics_for(
        r#"<?php
            /**
             * @template T
             */
            class Box {
                /** @param T $x */
                public function set($x): void {}
                /** @return T */
                public function get() { return null; }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_covariant_template_nested_in_an_array_parameter_type_is_still_flagged() {
    let d = diagnostics_for(
        r#"<?php
            /**
             * @template-covariant T
             */
            class Bad {
                /** @param T[] $items */
                public function consumeAll($items): void {}
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.message.contains("@template-covariant T")
            && x.message.contains("input (parameter) position")),
        "expected a covariance-violation diagnostic for the nested T, got: {d:?}"
    );
}

#[test]
fn a_top_level_function_has_no_enclosing_class_variance_to_violate() {
    // Sanity check that check_variance's class_name lookup doesn't
    // panic or misfire for a plain function (no class at all).
    let d = diagnostics_for(
        r#"<?php
            /** @param mixed $x */
            function f($x): void {}
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_covariant_template_in_a_constructor_parameter_is_exempt() {
    // Real bug found on the real Piwigo corpus: every single one of
    // 27 real-corpus diagnostics this check produced before this
    // exemption was on a constructor - the extremely common readonly
    // value-holder pattern real PHPStan's own variance rule
    // explicitly exempts, since a constructor is only ever called to
    // build one specific concrete instance, never invoked
    // polymorphically through a supertype-typed variable.
    let d = diagnostics_for(
        r#"<?php
            /**
             * @template-covariant T
             */
            final class Success {
                /** @param T $value */
                public function __construct(private $value) {}
                /** @return T */
                public function get() {
                    return $this->value;
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn an_erased_generic_with_a_declared_default_resolves_to_it_instead_of_mixed() {
    // Phase 54: `@template T = Default` - a bare (type-argument-free)
    // reference used to always leave T reading as Mixed (see
    // `bare_reference_to_a_generic_class_treats_template_as_mixed` above);
    // with a declared default it now resolves to that default instead,
    // proven the same way every other template-substitution test here is:
    // a return-type mismatch that only fires if T really did become int.
    let d = diagnostics_for(
        r#"<?php
            /**
             * @template T = int
             */
            class Box {
                /** @return T */
                public function get() { return null; }
            }
            function f(): string {
                $b = new Box();
                return $b->get();
            }
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.message.contains("return type mismatch") && x.message.contains("int")),
        "expected a return-type mismatch naming int (T's own default), got: {d:?}"
    );
}

#[test]
fn an_erased_generic_s_default_matching_the_declared_type_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            /**
             * @template T = int
             */
            class Box {
                /** @return T */
                public function get() { return null; }
            }
            function f(): int {
                $b = new Box();
                return $b->get();
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_covariant_template_nested_inside_another_covariant_class_s_argument_is_still_flagged_in_input_position()
 {
    // Phase 54's own real algorithmic addition: the old flat check never
    // recursed into another class's own generic argument at all (see this
    // test file's own history), so `T` used as `Producer<T>`'s own
    // argument in a parameter position was silently missed entirely, no
    // matter how it composed. `Producer`'s own template is (implicitly)
    // covariant-compatible with pass-through - composing a parameter
    // (input) position with a covariant slot leaves it an input position,
    // so `Bad`'s own covariant `T` is exactly as unsound to find there as
    // a direct, unwrapped `T` parameter would be.
    let d = diagnostics_for(
        r#"<?php
            /**
             * @template-covariant V
             */
            class Producer {}
            /**
             * @template-covariant T
             */
            class Bad {
                /** @param Producer<T> $x */
                public function consume($x): void {}
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.message.contains("@template-covariant T")
            && x.message.contains("input (parameter) position")),
        "expected a covariance-violation diagnostic for T nested in Producer<T>, got: {d:?}"
    );
}

#[test]
fn a_covariant_template_nested_inside_another_covariant_class_s_return_position_is_not_flagged() {
    // The output-position mirror of the test just above: T nested inside
    // Producer<T> found in a *return* type composes to the same output
    // position (a covariant slot never flips), which is exactly where a
    // covariant T is allowed to be.
    let d = diagnostics_for(
        r#"<?php
            /**
             * @template-covariant V
             */
            class Producer {}
            /**
             * @template-covariant T
             */
            class Good {
                /** @return Producer<T> */
                public function produce() { return new Producer(); }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_covariant_template_nested_inside_a_contravariant_class_s_return_position_is_flagged_via_flipping()
 {
    // The genuinely new case a flat, non-composing check could never
    // catch even in principle: `T` doesn't appear directly in a return
    // position at all here - it's nested inside `Sink<T>`, and `Sink`'s
    // own template is contravariant, which *flips* the surrounding
    // output position into an input one. A covariant `T` found there is
    // exactly as unsound as if it had been written as a bare parameter.
    let d = diagnostics_for(
        r#"<?php
            /**
             * @template-contravariant V
             */
            class Sink {}
            /**
             * @template-covariant T
             */
            class Bad {
                /** @return Sink<T> */
                public function makeSink() { return null; }
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.message.contains("@template-covariant T")
            && x.message.contains("input (parameter) position")),
        "expected a covariance-violation diagnostic for T flipped through Sink<T>, got: {d:?}"
    );
}

#[test]
fn a_contravariant_template_nested_inside_a_contravariant_class_s_return_position_is_not_flagged() {
    // The exact mirror of the flip test above, for a contravariant T:
    // flipping an output position through Sink's own contravariant slot
    // lands on an input position, which is precisely where a
    // contravariant T is allowed to be - so this is sound, not a
    // violation.
    let d = diagnostics_for(
        r#"<?php
            /**
             * @template-contravariant V
             */
            class Sink {}
            /**
             * @template-contravariant T
             */
            class Good {
                /** @return Sink<T> */
                public function makeSink() { return new Sink(); }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_template_nested_inside_an_unregistered_class_s_argument_is_never_flagged() {
    // "Don't know, don't guess" - same leniency `check_generic_bounds`'s
    // own `is_type_fully_known` already applies elsewhere: this project
    // has no real data on `Unregistered`'s own declared variance (an
    // uninstalled dependency, or a PHAR-bundled one), so it must not
    // guess `Invariant` (or anything else) for its own template slot and
    // risk a false positive on real, unverifiable third-party generics.
    //
    // Phase 48 (symbol-existence checking) landed after this test was
    // first written and correctly flags `Unregistered` itself as a
    // missing class - genuinely real, separate behavior, not a
    // regression: an unresolvable class name in a typehint is always
    // worth reporting on its own. What this test actually guards is
    // narrower now that both checks coexist: `check_generic_bounds`
    // must not ALSO pile a second, generics-specific diagnostic on top
    // once Phase 48 has already reported the class as missing - exactly
    // one diagnostic, and it's the class-existence one.
    let d = diagnostics_for(
        r#"<?php
            /**
             * @template-covariant T
             */
            class Bad {
                /** @param Unregistered<T> $x */
                public function consume($x): void {}
            }
        "#,
    );
    assert_eq!(d.len(), 1, "unexpected diagnostics: {d:?}");
    assert_eq!(d[0].identifier, "class.notFound");
}

#[test]
fn a_method_s_own_template_still_binds_on_a_non_generic_class() {
    // Real Piwigo corpus bug (Piwigo\PluginConfig\EventDispatcher, a
    // plain PSR-14-style class with zero class-level templates of its
    // own): substitute_class_templates ran first and always defaulted
    // an unbound TemplateParam to Mixed, wiping out the method's own
    // separate @template before infer_call_return_type's later,
    // correct per-call-argument pass ever got to bind it - regardless
    // of whether the enclosing class was generic at all.
    let d = diagnostics_for(
        r#"<?php
            class EventDispatcher {
                /**
                 * @template T of object
                 * @param T $event
                 * @return T
                 */
                public function dispatch(object $event): object {
                    return $event;
                }
            }
            final class RenderTagName {
                public function __construct(public string $tagName) {}
            }
            function needsString(string $s): void {}
            function f(EventDispatcher $d): void {
                $result = $d->dispatch(new RenderTagName('x'));
                needsString($result->tagName);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}
