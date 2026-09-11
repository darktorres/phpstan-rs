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
fn class_string_template_binds_through_a_nullable_union_parameter() {
    // Phase 77: `bind_template_param` used to silently no-op on a
    // `Union` param type - `class-string<T>|null` (phpstorm-stubs'
    // pervasive idiom, e.g. `ReflectionClass::getAttributes()`'s own
    // `?string $name` overload once templated) never bound `T` at all,
    // even when the real argument was a non-null `class-string<Derived>`.
    // Otherwise identical to `class_string_template_binds_the_concrete_
    // subclass_from_a_static_factory_call` above, just with the added
    // `|null` on the parameter - confirmed this exact addition is what
    // used to suppress the binding.
    let d = diagnostics_for(
        r#"<?php
            class Base {}
            class Derived extends Base {}
            class TypedRepository {
                /**
                 * @template R of Base
                 * @param class-string<R>|null $repositoryClass
                 * @return R
                 */
                public static function narrow(?string $repositoryClass) {
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

// Phase 54: `Registry::is_assignable` now actually applies variance to a
// generic class's own type arguments, instead of ignoring them entirely
// (`Collection<Cat>` and `Collection<Dog>` used to be mutually
// assignable, always). These exercise it through ordinary argument-type
// checking (`checker/calls.rs`), the same real call site every one of
// these comparisons goes through in practice.

#[test]
fn an_invariant_generic_class_rejects_a_narrower_argument_by_default() {
    // No `@template-covariant`/`-contravariant` at all - the default,
    // by far the most common real shape - so `Collection<Cat>` and
    // `Collection<Animal>` are genuinely different types now, even
    // though `Cat extends Animal`. Previously silent (type arguments
    // were ignored entirely).
    let d = diagnostics_for(
        r#"<?php
            class Animal {}
            class Cat extends Animal {}
            /**
             * @template T
             */
            class Collection {}
            /** @param Collection<Animal> $c */
            function needsAnimalCollection($c) {}
            /** @param Collection<Cat> $c */
            function f($c) {
                needsAnimalCollection($c);
            }
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "argument.type" && x.message.contains("Collection<Animal>")),
        "expected an argument-type mismatch naming Collection<Animal>, got: {d:?}"
    );
}

#[test]
fn an_invariant_generic_class_rejects_a_broader_argument_too() {
    // The reverse direction of the test above - proves this is genuine
    // invariance, not just "narrower rejected, broader accepted"
    // (which would actually be covariance).
    let d = diagnostics_for(
        r#"<?php
            class Animal {}
            class Cat extends Animal {}
            /**
             * @template T
             */
            class Collection {}
            /** @param Collection<Cat> $c */
            function needsCatCollection($c) {}
            /** @param Collection<Animal> $c */
            function f($c) {
                needsCatCollection($c);
            }
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "argument.type" && x.message.contains("Collection<Cat>")),
        "expected an argument-type mismatch naming Collection<Cat>, got: {d:?}"
    );
}

#[test]
fn a_declared_covariant_generic_class_accepts_a_narrower_argument() {
    let d = diagnostics_for(
        r#"<?php
            class Animal {}
            class Cat extends Animal {}
            /**
             * @template-covariant T
             */
            class Box {}
            /** @param Box<Animal> $b */
            function needsAnimalBox($b) {}
            /** @param Box<Cat> $b */
            function f($b) {
                needsAnimalBox($b);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_declared_covariant_generic_class_still_rejects_a_broader_argument() {
    let d = diagnostics_for(
        r#"<?php
            class Animal {}
            class Cat extends Animal {}
            /**
             * @template-covariant T
             */
            class Box {}
            /** @param Box<Cat> $b */
            function needsCatBox($b) {}
            /** @param Box<Animal> $b */
            function f($b) {
                needsCatBox($b);
            }
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "argument.type" && x.message.contains("Box<Cat>")),
        "expected an argument-type mismatch naming Box<Cat>, got: {d:?}"
    );
}

#[test]
fn a_declared_contravariant_generic_class_accepts_a_broader_argument() {
    let d = diagnostics_for(
        r#"<?php
            class Animal {}
            class Cat extends Animal {}
            /**
             * @template-contravariant T
             */
            class Comparator {}
            /** @param Comparator<Cat> $c */
            function needsCatComparator($c) {}
            /** @param Comparator<Animal> $c */
            function f($c) {
                needsCatComparator($c);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_declared_contravariant_generic_class_still_rejects_a_narrower_argument() {
    let d = diagnostics_for(
        r#"<?php
            class Animal {}
            class Cat extends Animal {}
            /**
             * @template-contravariant T
             */
            class Comparator {}
            /** @param Comparator<Animal> $c */
            function needsAnimalComparator($c) {}
            /** @param Comparator<Cat> $c */
            function f($c) {
                needsAnimalComparator($c);
            }
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "argument.type" && x.message.contains("Comparator<Animal>")),
        "expected an argument-type mismatch naming Comparator<Animal>, got: {d:?}"
    );
}

#[test]
fn a_bare_erased_generic_reference_is_never_flagged_either_direction() {
    // The overwhelmingly common real shape: no `<T>` at all. Padding a
    // missing slot with plain `Mixed` (not the class's own declared
    // bound - see `Registry::is_assignable`'s own doc comment for why
    // that distinction matters) keeps this silent, matching this
    // project's established "erased generic = don't know = don't flag"
    // leniency everywhere else - even though `Collection` here declares
    // a real, non-`Mixed` bound on its own template.
    let d = diagnostics_for(
        r#"<?php
            class Shape {}
            class Circle extends Shape {}
            /**
             * @template T of Shape
             */
            class Collection {}
            /** @param Collection<Circle> $c */
            function needsCircleCollection($c) {}
            /** @param Collection $c */
            function f($c) {
                needsCircleCollection($c);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_bare_erased_generic_argument_is_never_flagged_when_passed_where_a_concrete_one_is_declared() {
    // The mirror direction: a concretely-typed value passed where a bare
    // reference is expected is trivially fine too (every slot on the
    // `sup` side pads to `Mixed`, always skipped).
    let d = diagnostics_for(
        r#"<?php
            class Circle {}
            /**
             * @template T
             */
            class Collection {}
            /** @param Collection $c */
            function needsAnyCollection($c) {}
            /** @param Collection<Circle> $c */
            function f($c) {
                needsAnyCollection($c);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_non_generic_subclass_fixing_the_parent_s_argument_is_assignable_to_the_matching_instantiation()
{
    // `IntCollection` isn't generic itself, but its own `@extends
    // Collection<int>` permanently fixes Collection's own `T` to `int` -
    // `is_assignable` needs to map `IntCollection`'s own (empty) type
    // arguments into `Collection`'s frame via `extends_args`, the same
    // ancestor walk `class_template_bindings` already performs
    // elsewhere, to see this correctly.
    let d = diagnostics_for(
        r#"<?php
            /**
             * @template T
             */
            class Collection {}
            /** @extends Collection<int> */
            class IntCollection extends Collection {}
            /** @param Collection<int> $c */
            function needsIntCollection($c) {}
            function f(IntCollection $c) {
                needsIntCollection($c);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_non_generic_subclass_fixing_the_parent_s_argument_is_not_assignable_to_a_different_one() {
    let d = diagnostics_for(
        r#"<?php
            /**
             * @template T
             */
            class Collection {}
            /** @extends Collection<int> */
            class IntCollection extends Collection {}
            /** @param Collection<string> $c */
            function needsStringCollection($c) {}
            function f(IntCollection $c) {
                needsStringCollection($c);
            }
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "argument.type" && x.message.contains("Collection<string>")),
        "expected an argument-type mismatch naming Collection<string>, got: {d:?}"
    );
}

// Phase 54: real PHPStan's own call-site variance annotation syntax
// (`Collection<covariant Foo>`/`Collection<contravariant Foo>`/
// `Collection<*>`) - confirmed real and current against `phpdoc-parser`'s
// own `GenericTypeNode`/`PhpDoc\TypeNodeResolver::resolveGenericTypeNode`
// in `vendor/phpstan-src`. `types::parse_generic_arg` parses it into a
// sentinel `Registry::effective_slot_variance` reads directly.

#[test]
fn a_call_site_covariant_marker_overrides_the_class_s_own_invariant_default() {
    let d = diagnostics_for(
        r#"<?php
            class Animal {}
            class Cat extends Animal {}
            /**
             * @template T
             */
            class Collection {}
            /** @param Collection<covariant Animal> $c */
            function needsAnimalCollection($c) {}
            /** @param Collection<Cat> $c */
            function f($c) {
                needsAnimalCollection($c);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_call_site_bivariant_marker_accepts_any_argument() {
    let d = diagnostics_for(
        r#"<?php
            class Whatever {}
            /**
             * @template T
             */
            class Collection {}
            /** @param Collection<*> $c */
            function needsAnyCollection($c) {}
            /** @param Collection<Whatever> $c */
            function f($c) {
                needsAnyCollection($c);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn without_a_call_site_marker_the_class_s_own_invariant_default_still_applies() {
    // Proves the marker itself - not mere proximity to a correct-looking
    // assignment - is what does the work in the two tests above: the
    // exact same shape with the marker removed goes back to being
    // flagged.
    let d = diagnostics_for(
        r#"<?php
            class Animal {}
            class Cat extends Animal {}
            /**
             * @template T
             */
            class Collection {}
            /** @param Collection<Animal> $c */
            function needsAnimalCollection($c) {}
            /** @param Collection<Cat> $c */
            function f($c) {
                needsAnimalCollection($c);
            }
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "argument.type" && x.message.contains("Collection<Animal>")),
        "expected an argument-type mismatch naming Collection<Animal>, got: {d:?}"
    );
}

#[test]
fn a_call_site_variance_marker_prints_legibly_in_a_diagnostic_message() {
    // `Display for Type` gained a dedicated case for these sentinels -
    // without it, this would leak the internal `$$covariant<Foo>`-shaped
    // sentinel name into a real diagnostic message instead of real
    // PHPStan's own `covariant Foo` syntax.
    let d = diagnostics_for(
        r#"<?php
            class Animal {}
            class Cat extends Animal {}
            /**
             * @template T
             */
            class Collection {}
            /** @param Collection<covariant Cat> $c */
            function needsCatCollection($c) {}
            /** @param Collection<Animal> $c */
            function f($c) {
                needsCatCollection($c);
            }
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "argument.type"
                && x.message.contains("Collection<covariant Cat>")),
        "expected the call-site marker to print as 'covariant Cat', got: {d:?}"
    );
}

// Phase 54 follow-up: `generics.callSiteVarianceConflict`/
// `.callSiteVarianceRedundant` - `check_call_site_variance`'s own port of
// `GenericObjectTypeCheck`'s call-site-variance half.

#[test]
fn a_call_site_covariant_marker_matching_the_class_s_own_covariant_template_is_redundant() {
    let d = diagnostics_for(
        r#"<?php
            /** @template-covariant T */
            class Collection {}
            class Animal {}
            /** @param Collection<covariant Animal> $c */
            function f($c) {}
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "generics.callSiteVarianceRedundant"
                && x.message.contains("Collection<covariant Animal>")),
        "got: {d:?}"
    );
}

#[test]
fn a_call_site_contravariant_marker_matching_the_class_s_own_contravariant_template_is_redundant() {
    let d = diagnostics_for(
        r#"<?php
            /** @template-contravariant T */
            class Collection {}
            class Animal {}
            /** @param Collection<contravariant Animal> $c */
            function f($c) {}
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "generics.callSiteVarianceRedundant"),
        "got: {d:?}"
    );
}

#[test]
fn a_call_site_contravariant_marker_on_a_covariant_declared_template_conflicts() {
    let d = diagnostics_for(
        r#"<?php
            /** @template-covariant T */
            class Collection {}
            class Animal {}
            /** @param Collection<contravariant Animal> $c */
            function f($c) {}
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "generics.callSiteVarianceConflict"
                && x.message.contains("Collection<contravariant Animal>")),
        "got: {d:?}"
    );
}

#[test]
fn a_call_site_covariant_marker_on_a_contravariant_declared_template_conflicts() {
    let d = diagnostics_for(
        r#"<?php
            /** @template-contravariant T */
            class Collection {}
            class Animal {}
            /** @param Collection<covariant Animal> $c */
            function f($c) {}
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "generics.callSiteVarianceConflict"),
        "got: {d:?}"
    );
}

#[test]
fn a_call_site_variance_marker_on_a_plain_invariant_template_is_never_flagged_as_conflicting_or_redundant()
 {
    // The intended call-site-variance escape hatch: overriding a plain
    // invariant `@template` is exactly what this feature exists for.
    let d = diagnostics_for(
        r#"<?php
            /** @template T */
            class Collection {}
            class Animal {}
            /** @param Collection<covariant Animal> $c */
            function f($c) {}
            /** @param Collection<contravariant Animal> $g */
            function g($g) {}
        "#,
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "generics.callSiteVarianceConflict"
                || x.identifier == "generics.callSiteVarianceRedundant"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_call_site_bivariant_marker_is_never_flagged_as_conflicting_or_redundant() {
    let d = diagnostics_for(
        r#"<?php
            /** @template-covariant T */
            class Collection {}
            /** @param Collection<*> $c */
            function f($c) {}
        "#,
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "generics.callSiteVarianceConflict"
                || x.identifier == "generics.callSiteVarianceRedundant"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_call_site_variance_marker_inside_a_declared_parameter_overrides_declaration_soundness_checking()
 {
    // `checker::generics::walk_variance` (declaration-soundness, not
    // assignability) must consult the exact same override -
    // `Box`'s own `@template-covariant T` may only appear in an output
    // (return) position, but a call-site `contravariant` marker on the
    // nested `Collection<contravariant T>` flips the composed position
    // from input back to output, making this sound - proof
    // `Registry::effective_slot_variance` is really wired into
    // `walk_variance`, not just `is_assignable`.
    let d = diagnostics_for(
        r#"<?php
            /**
             * @template U
             */
            class Collection {}
            /**
             * @template-covariant T
             */
            class Good {
                /** @param Collection<contravariant T> $c */
                public function consume($c): void {}
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn without_the_call_site_marker_the_same_nesting_is_flagged_as_unsound() {
    // The exact same nesting as the test above with the marker removed -
    // `Collection`'s own declared variance is Invariant by default, so
    // the parameter (input) position isn't flipped, and `T`'s own
    // covariant declaration is genuinely violated. Proves the marker in
    // the test above - not something else about this nesting shape - is
    // what makes it sound.
    let d = diagnostics_for(
        r#"<?php
            /**
             * @template U
             */
            class Collection {}
            /**
             * @template-covariant T
             */
            class Bad {
                /** @param Collection<T> $c */
                public function consume($c): void {}
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.message.contains("@template-covariant T")
            && x.message.contains("input (parameter) position")),
        "expected a covariance-violation diagnostic for T nested in Collection<T>, got: {d:?}"
    );
}

#[test]
fn an_unbound_method_own_template_nested_inside_a_generic_argument_is_never_flagged() {
    // Regression test for a real false-positive class found via this
    // session's own corpus diff (596 new diagnostics on a single real
    // shape): `checker::calls`'s own call-argument template binding
    // only ever binds a template used as a *bare* param type (`T
    // $value`), never one nested inside another class's own generic
    // argument list - so `E` here reaches `is_assignable`'s new
    // variance-aware comparison exactly as unbound as it was declared,
    // not substituted with the real argument's own entity type the way
    // real PHPStan's own inference would. Comparing it structurally
    // against the real, concrete argument is exactly as wrong as
    // comparing `Mixed` would be - real corpus shape verbatim: Piwigo's
    // own `TypedRepository::narrow(EntityRepository<E> $repository)`
    // with `@template E of object`, called with a real, concrete
    // `EntityRepository<ConcreteEntity>` value.
    let d = diagnostics_for(
        r#"<?php
            /**
             * @template T
             */
            class EntityRepository {}
            class ConcreteEntity {}
            class TypedRepository {
                /**
                 * @template E of object
                 * @param EntityRepository<E> $repository
                 */
                public static function narrow($repository): void {}
            }
            function f(EntityRepository $r) {
                TypedRepository::narrow($r);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn an_unbound_method_own_template_nested_inside_a_generic_argument_with_a_concrete_receiver_is_never_flagged()
 {
    // The exact real corpus shape: the argument itself is a concretely
    // generic-typed value (`EntityRepository<ConcreteEntity>`), not
    // another unbound template.
    let d = diagnostics_for(
        r#"<?php
            /**
             * @template T
             */
            class EntityRepository {}
            class ConcreteEntity {}
            class TypedRepository {
                /**
                 * @template E of object
                 * @param EntityRepository<E> $repository
                 */
                public static function narrow($repository): void {}
            }
            /** @param EntityRepository<ConcreteEntity> $r */
            function f($r) {
                TypedRepository::narrow($r);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// Phase 54 follow-up: generic-reference arity/well-formedness on an
// @extends/@implements clause specifically.

#[test]
fn extending_a_single_template_class_with_two_type_arguments_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            /** @template T */
            class Collection {}
            /**
             * @extends Collection<int, string>
             */
            class IntCollection extends Collection {}
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "generics.moreTypes"),
        "got: {d:?}"
    );
}

#[test]
fn extending_a_two_template_class_with_one_type_argument_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            /** @template K
             *   @template V
             */
            class Map {}
            /**
             * @extends Map<string>
             */
            class StringMap extends Map {}
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "generics.lessTypes"),
        "got: {d:?}"
    );
}

#[test]
fn extending_with_the_exact_declared_arity_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            /** @template T */
            class Collection {}
            /**
             * @extends Collection<int>
             */
            class IntCollection extends Collection {}
        "#,
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "generics.moreTypes" || x.identifier == "generics.lessTypes"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_missing_argument_covered_by_a_template_default_is_never_flagged() {
    // K has no default (required); V does - only 1 argument is needed,
    // not 2, even though the class declares 2 templates.
    let d = diagnostics_for(
        r#"<?php
            /** @template K
             *   @template V = int
             */
            class Map {}
            /**
             * @extends Map<string>
             */
            class StringMap extends Map {}
        "#,
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "generics.moreTypes" || x.identifier == "generics.lessTypes"),
        "unexpected diagnostics: {d:?}"
    );
}

// Real corpus false positive (Piwigo's own custom `phpstan-stubs/
// LatteCachingIterator.stub.php`): a class with *no* declared templates
// at all being given type arguments in someone else's `@extends` tag is
// `generics.notGeneric` territory (a different, deliberately unbuilt
// check - see `check_generic_reference_arity`'s own doc comment), not
// `generics.moreTypes` - this project's own `@template`-tag parsing
// might simply have a gap for the referenced class, indistinguishable
// from "genuinely not generic" from here.
#[test]
fn extending_a_class_with_zero_declared_templates_and_real_type_args_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Base {}
            /** @extends Base<int, string, bool> */
            class Derived extends Base {}
        "#,
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "generics.moreTypes" || x.identifier == "generics.lessTypes"),
        "unexpected diagnostics: {d:?}"
    );
}

// Real corpus false positive (Symfony's own `ServiceCollectionInterface`,
// which `extends ServiceProviderInterface, \Countable, \IteratorAggregate`
// with *two* of those three each carrying their own `@extends <Args>`
// tag): `ClassInfo::extends_args` is a flat, un-keyed list - real PHP
// interfaces can `extends` more than one parent, each with an
// independent argument list this field has no way to keep separate, so
// two `@extends<...>` tags parse into one conflated list. Arity-checking
// against that conflated list is only trustworthy when there's exactly
// one `extends` target to begin with.
#[test]
fn an_interface_extending_two_generic_parents_at_once_is_never_flagged_for_arity() {
    let d = diagnostics_for(
        r#"<?php
            /** @template T */
            interface First {}
            /** @template K
             *   @template V
             */
            interface Second {}
            /**
             * @extends First<int>
             * @extends Second<string, bool>
             */
            interface Combined extends First, Second {}
        "#,
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "generics.moreTypes" || x.identifier == "generics.lessTypes"),
        "unexpected diagnostics: {d:?}"
    );
}

// Phase 54 follow-up: `check_generic_reference_arity` is now called
// pervasively from `check_generic_bounds_and_arity` itself, not just
// `check_ancestor_generic_bounds`'s own dedicated `extends`/`implements`
// call site - matching real PHPStan's own `GenericObjectTypeCheck`, run
// from `IncompatiblePhpDocTypeCheck` at every declared generic-type
// position (a property type, a param/return hint, ...).

#[test]
fn a_property_type_with_too_many_generic_arguments_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            /** @template T */
            class Collection {}
            class Holder {
                /** @var Collection<int, string> */
                public $c;
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "generics.moreTypes"),
        "got: {d:?}"
    );
}

#[test]
fn a_param_type_with_too_few_generic_arguments_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            /** @template K
             *   @template V
             */
            class Map {}
            /** @param Map<string> $m */
            function f($m) {}
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "generics.lessTypes"),
        "got: {d:?}"
    );
}

// The exact-count param case must stay quiet - this is the same
// "extra false-positive risk from making the check pervasive" concern
// `extending_with_the_exact_declared_arity_is_never_flagged` already
// covers for `extends`, applied to an ordinary param position instead.
#[test]
fn a_param_type_with_the_exact_declared_arity_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            /** @template T */
            class Collection {}
            /** @param Collection<int> $c */
            function f($c) {}
        "#,
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "generics.moreTypes" || x.identifier == "generics.lessTypes"),
        "unexpected diagnostics: {d:?}"
    );
}

// Phase 48.D: `method.templateTypeNotInParameter` - a function/method's
// own `@template` referenced only in its return type (or nowhere at all)
// can never be inferred at a call site, since inference only ever reads
// argument types.

#[test]
fn a_function_template_declared_only_in_the_return_type_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            /**
             * @template T
             * @return T
             */
            function make(string $class) {}
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "method.templateTypeNotInParameter"
                && x.message
                    == "Template type T of function make() is not referenced in a parameter."),
        "got: {d:?}"
    );
}

#[test]
fn a_method_template_declared_only_in_the_return_type_is_flagged_with_method_wording() {
    let d = diagnostics_for(
        r#"<?php
            class Factory {
                /**
                 * @template T
                 * @return T
                 */
                public function make(string $class) {}
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "method.templateTypeNotInParameter"
            && x.message
                == "Template type T of method Factory::make() is not referenced in a parameter."),
        "got: {d:?}"
    );
}

#[test]
fn a_template_also_referenced_in_a_parameter_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            /**
             * @template T
             * @param T $value
             * @return T
             */
            function identity($value) {
                return $value;
            }
        "#,
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "method.templateTypeNotInParameter"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_template_referenced_only_nested_inside_a_parameters_own_generic_argument_is_never_flagged() {
    // Real corpus shape: a template only ever appearing inside a
    // parameter's own generic argument list (`array<int, T>`), not as
    // the parameter's own bare top-level type - still a real, valid
    // inference source, and must not be flagged.
    let d = diagnostics_for(
        r#"<?php
            /**
             * @template T
             * @param array<int, T> $items
             * @return T
             */
            function firstOf(array $items) {
                return $items[0];
            }
        "#,
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "method.templateTypeNotInParameter"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_class_level_template_is_never_flagged_by_the_method_level_check() {
    // A class-level `@template` has no per-method "referenced in a
    // parameter" obligation at all (it's bound once, at instantiation) -
    // only a method's own `@template` is in scope here.
    let d = diagnostics_for(
        r#"<?php
            /** @template T */
            class Box {
                /** @return T */
                public function get() {}
            }
        "#,
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "method.templateTypeNotInParameter"),
        "unexpected diagnostics: {d:?}"
    );
}

// Phase 54: `@use Trait<Arg, ...>` - real PHPStan resolves this tag from
// the `use` *statement*'s own doc comment (confirmed directly,
// `vendor/phpstan-src`'s own `TemplateObjectType`: `/** @use
// TemplateTypeTrait<ObjectType> */` sits right above `use
// TemplateTypeTrait;`, not the class's own separate top-level docblock) -
// `ClassInfo::uses_args` mirrors `implements_args`, feeding the identical
// `check_generic_bounds`/`check_generic_reference_arity` pervasive
// machinery `check_ancestor_generic_bounds` already runs for `extends`/
// `implements`.

#[test]
fn a_use_tag_with_too_many_generic_arguments_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            /** @template T */
            trait Repo {}
            class UserRepo {
                /** @use Repo<int, string> */
                use Repo;
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "generics.moreTypes"),
        "got: {d:?}"
    );
}

#[test]
fn a_use_tag_with_too_few_generic_arguments_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            /** @template K
             *   @template V
             */
            trait Repo {}
            class UserRepo {
                /** @use Repo<int> */
                use Repo;
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "generics.lessTypes"),
        "got: {d:?}"
    );
}

#[test]
fn a_use_tag_with_the_exact_declared_arity_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            /** @template T */
            trait Repo {}
            class UserRepo {
                /** @use Repo<int> */
                use Repo;
            }
        "#,
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "generics.moreTypes" || x.identifier == "generics.lessTypes"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_use_tag_violating_its_trait_s_own_template_bound_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            interface Shape {}
            class Square implements Shape {}
            class NotAShape {}
            /** @template T of Shape */
            trait Repo {}
            class UserRepo {
                /** @use Repo<NotAShape> */
                use Repo;
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "generics.templateBound"),
        "got: {d:?}"
    );
}

#[test]
fn a_use_tag_on_the_class_s_own_docblock_instead_of_the_use_statement_is_never_applied() {
    // Real PHPStan resolves `@use` from the `use` statement's own doc
    // comment specifically, not the class's separate top-level one - a
    // tag misplaced on the class docblock names a trait this class
    // doesn't otherwise use at all, so nothing should be checked against
    // it.
    let d = diagnostics_for(
        r#"<?php
            /** @template T */
            trait Repo {}
            /** @use Repo<int, string, bool> */
            class UserRepo {
                use Repo;
            }
        "#,
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "generics.moreTypes" || x.identifier == "generics.lessTypes"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn two_separate_use_statements_each_keep_their_own_use_tag_s_arguments() {
    let d = diagnostics_for(
        r#"<?php
            /** @template T */
            trait First {}
            /** @template K
             *   @template V
             */
            trait Second {}
            class Combined {
                /** @use First<int, string> */
                use First;
                /** @use Second<string> */
                use Second;
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "generics.moreTypes"),
        "expected First<int, string> (1 template, 2 args) to be flagged, got: {d:?}"
    );
    assert!(
        d.iter().any(|x| x.identifier == "generics.lessTypes"),
        "expected Second<string> (2 templates, 1 arg) to be flagged, got: {d:?}"
    );
}

// Phase 54 follow-up regression: `Traversable`/`Iterator`/
// `IteratorAggregate`/`Generator` are the one real, hardcoded exception to
// pervasive arity checking - `PhpDoc\TypeNodeResolver::
// resolveGenericTypeNode` (`vendor/phpstan-src`) pads a short-form
// reference to these four names with `mixed` before any arity check ever
// sees it. Verified directly against a real, installed `phpstan/phpstan`
// (see this test module's own history) - `Generator<int, string>` (2 of
// its real 4 templates) must never be flagged, but `Generator<int, string,
// bool>` (3 of 4 - not one of the two real padded shapes) still is.

#[test]
fn a_single_argument_generator_reference_is_never_flagged_for_arity() {
    let d = diagnostics_for(
        r#"<?php
            /** @return \Generator<string> */
            function f(): \Generator {
                yield 'x';
            }
        "#,
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "generics.moreTypes" || x.identifier == "generics.lessTypes"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_two_argument_generator_reference_is_never_flagged_for_arity() {
    let d = diagnostics_for(
        r#"<?php
            /** @return \Generator<int, string> */
            function f(): \Generator {
                yield 'x';
            }
        "#,
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "generics.moreTypes" || x.identifier == "generics.lessTypes"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_three_argument_generator_reference_is_still_flagged_for_arity() {
    // Not one of the two hardcoded padded shapes (1 or 2 given args) -
    // falls through to ordinary arity checking against Generator's real 4
    // templates, same as a real, installed `phpstan/phpstan` (confirmed
    // directly).
    let d = diagnostics_for(
        r#"<?php
            /** @return \Generator<int, string, bool> */
            function f(): \Generator {
                yield 'x';
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "generics.lessTypes"),
        "got: {d:?}"
    );
}

#[test]
fn a_single_argument_traversable_reference_is_never_flagged_for_arity() {
    let d = diagnostics_for(
        r#"<?php
            /** @param \Traversable<int> $t */
            function f($t): void {}
        "#,
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "generics.moreTypes" || x.identifier == "generics.lessTypes"),
        "unexpected diagnostics: {d:?}"
    );
}

// Phase 54's last build item: `generics.interfaceConflict` -
// `check_cross_path_interface_conflicts`, ported from
// `Rules/Generics/CrossCheckInterfacesHelper.php`. A real "diamond
// generic" shape: `Box<T>` reaches `BaseIface` via two different paths
// (`AIface`/`BIface`), each pinning `BaseIface`'s own template to a
// different concrete type.

#[test]
fn two_paths_pinning_a_shared_interface_s_template_to_different_types_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            /** @template Y */
            interface BaseIface {}
            /**
             * @template X
             * @extends BaseIface<X>
             */
            interface AIface extends BaseIface {}
            /**
             * @template Z
             * @extends BaseIface<Z>
             */
            interface BIface extends BaseIface {}
            /**
             * @template T
             * @implements AIface<T>
             * @implements BIface<int>
             */
            class Box implements AIface, BIface {}
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "generics.interfaceConflict"
                && x.message.contains("template type Y of interface BaseIface")
                && x.message.contains("but it's already specified as")),
        "got: {d:?}"
    );
}

#[test]
fn two_paths_pinning_a_shared_interface_s_template_to_the_same_type_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            /** @template Y */
            interface BaseIface {}
            /**
             * @template X
             * @extends BaseIface<X>
             */
            interface AIface extends BaseIface {}
            /**
             * @template Z
             * @extends BaseIface<Z>
             */
            interface BIface extends BaseIface {}
            /**
             * @template T
             * @implements AIface<T>
             * @implements BIface<T>
             */
            class Box implements AIface, BIface {}
        "#,
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "generics.interfaceConflict"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn an_enum_implementing_two_interfaces_with_conflicting_concrete_bindings_is_flagged() {
    // Real PHPStan's own message literally says "Class" here even for an
    // enum (`EnumAncestorsRule` calls the identical, unmodified
    // `CrossCheckInterfacesHelper` - confirmed by direct read) - ported
    // as-is rather than "corrected" to a word real PHPStan itself
    // doesn't use in this message.
    let d = diagnostics_for(
        r#"<?php
            /** @template Y */
            interface BaseIface {}
            /**
             * @template X
             * @extends BaseIface<X>
             */
            interface AIface extends BaseIface {}
            /**
             * @template Z
             * @extends BaseIface<Z>
             */
            interface BIface extends BaseIface {}
            /**
             * @implements AIface<int>
             * @implements BIface<string>
             */
            enum Suit implements AIface, BIface {}
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "generics.interfaceConflict"
                && x.message
                    .starts_with("Class Suit specifies template type Y of interface BaseIface")),
        "got: {d:?}"
    );
}

#[test]
fn a_single_path_to_a_generic_interface_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            /** @template Y */
            interface BaseIface {}
            /**
             * @template X
             * @extends BaseIface<X>
             */
            interface AIface extends BaseIface {}
            /** @template T
             *   @implements AIface<T>
             */
            class Box implements AIface {}
        "#,
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "generics.interfaceConflict"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn two_independent_root_templates_forwarded_to_the_same_ancestor_slot_are_flagged() {
    // Regression for the identity-binding refinement in
    // `check_cross_path_interface_conflicts`'s own entry point: without
    // it, both `T` and `U` (Box's own, genuinely independent templates)
    // would collapse to the same `Mixed` placeholder the moment they're
    // forwarded into `AIface`/`BIface`, silently agreeing when they're
    // actually two unrelated, independently-instantiable templates.
    let d = diagnostics_for(
        r#"<?php
            /** @template Y */
            interface BaseIface {}
            /**
             * @template X
             * @extends BaseIface<X>
             */
            interface AIface extends BaseIface {}
            /**
             * @template Z
             * @extends BaseIface<Z>
             */
            interface BIface extends BaseIface {}
            /**
             * @template T
             * @template U
             * @implements AIface<T>
             * @implements BIface<U>
             */
            class Box implements AIface, BIface {}
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "generics.interfaceConflict"),
        "got: {d:?}"
    );
}

// Phase 54/77 follow-up: constructor-argument-driven class-level generic
// inference - `new Box(5)` binds `Box`'s own `@template T` from the
// constructor argument's inferred type, the way real PHPStan's own
// NewHandler/ConstructorClassTemplateTraverser does.

#[test]
fn new_binds_a_generic_class_s_own_template_from_a_constructor_argument() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            /** @template T */
            class Box {
                /** @param T $value */
                public function __construct(public $value) {}
            }
            assertType('Box<5>', new Box(5));
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn new_leaves_a_wholly_unbound_template_erased_rather_than_forcing_mixed() {
    // A no-arg constructor binds nothing at all - the result must stay a
    // bare, erased `Box` (matching a real `new Box()` reference with no
    // type arguments), not an explicit `Box<mixed>`: `class_template_
    // bindings_rec`/`substitute_scoped`'s own `@template T = Default`
    // fallback only triggers for a template genuinely *absent* from the
    // bindings map, not one bound to `Mixed` outright - see the sibling
    // test just below for the regression this guards.
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            /** @template T */
            class Box {
                public function __construct() {}
            }
            assertType('Box', new Box());
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn new_on_a_non_generic_class_is_unaffected_by_constructor_argument_binding() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            class Plain {
                public function __construct(public int $x) {}
            }
            assertType('Plain', new Plain(5));
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn reflection_class_binds_its_own_template_from_a_class_string_argument() {
    // `php_stubs::patch_reflection_class_constructor_template` - real
    // phpstorm-stubs' own `@param class-string<T>|T $objectOrClass` never
    // reaches `ParamSig::ty` for this specific parameter (a real, disclosed
    // gap in the doc-merge pipeline for a `#[LanguageLevelTypeAware(...)]`-
    // attributed native hint), so this is patched in directly, reusing
    // `ReflectionClass`'s own already-correct class-level `@template T of
    // object` metadata.
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            final class Foo {}
            assertType('ReflectionClass<Foo>', new ReflectionClass(Foo::class));
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn reflection_class_from_a_plain_string_variable_leaves_t_erased_not_bound_to_string() {
    // Corpus regression this exact patch introduced and fixed in the same
    // pass: `new ReflectionClass($className)`, a plain `string`-typed
    // variable (not `Foo::class`), must not bind `T` to `string` itself -
    // `bind_template_param`'s bare-`T` union arm would otherwise happily
    // do so once the stricter `ClassString` arm declined to match,
    // producing a bound-violating `ReflectionClass<string>` (`T of
    // object`) that broke real downstream `ReflectionClass<object>` call
    // sites (`tools/phpstan/Latte/*.php` in the real Piwigo corpus).
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            function f(string $className): void {
                assertType('ReflectionClass', new ReflectionClass($className));
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}
