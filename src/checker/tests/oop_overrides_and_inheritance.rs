//! Classes, interfaces, traits, overrides, self/static/parent resolution.

use super::helpers::*;

#[test]
fn concrete_class_name_satisfies_self_return_type() {
    // Regression test: `new UserRepository()` and `new self()` are
    // equally valid ways to satisfy a `: self` return type declared on
    // UserRepository itself - this used to be a false positive.
    let d = diagnostics_for(
        r#"<?php
            class UserRepository {
                public static function make(): self {
                    return new UserRepository();
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn nullable_self_return_type_is_resolved_the_same_way_a_bare_self_is() {
    // Real, previously-live bug found via real code in this
    // project's own verification corpus: resolve_pseudo only ever
    // handled a *bare* self/static/parent, never one wrapped in a
    // Union - the extremely common `?self`/`self|null` factory-
    // method return type (real example: a `tryFromArray`-style
    // static constructor) stayed unresolved as a bare `SelfRef`,
    // which is_assignable/is_subtype have no arm to accept a
    // concrete class instance against.
    let d = diagnostics_for(
        r#"<?php
            class Widget {
                public static function tryFromArray(array $data): ?self {
                    if (!isset($data['id'])) {
                        return null;
                    }
                    return new self();
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn wrong_class_returned_from_self_typed_method_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {
                public static function make(): self { return new Bar(); }
            }
            class Bar {}
        "#,
    );
    assert_eq!(d.len(), 1);
    assert!(d[0].message.contains("expected Foo"));
    assert!(d[0].message.contains("got Bar"));
}

#[test]
fn fluent_method_chain_through_self_return_type_infers_concretely() {
    let d = diagnostics_for(
        r#"<?php
            class QueryBuilder {
                public function where(): self { return $this; }
            }
            function needsQueryBuilder(QueryBuilder $q): void {}
            $qb = new QueryBuilder();
            needsQueryBuilder($qb->where());
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn flags_wrong_type_assigned_to_typed_property() {
    let d = diagnostics_for(
        r#"<?php
            class Box { public int $value = 0; }
            $box = new Box();
            $box->value = "not a number";
        "#,
    );
    assert_eq!(d.len(), 1);
    assert!(d[0].message.contains("Box::$value"));
}

#[test]
fn correct_property_assignment_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Box { public int $value = 0; }
            $box = new Box();
            $box->value = 5;
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn promoted_property_assignment_is_type_checked() {
    let d = diagnostics_for(
        r#"<?php
            class Point {
                public function __construct(public int $x) {}
            }
            $p = new Point(1);
            $p->x = "nope";
        "#,
    );
    assert_eq!(d.len(), 1);
    assert!(d[0].message.contains("Point::$x"));
}

#[test]
fn dynamic_property_not_declared_on_class_is_not_flagged() {
    // Undeclared/dynamic properties are legal (if deprecated) PHP -
    // this prototype only checks types on properties it knows about.
    let d = diagnostics_for(
        r#"<?php
            class Box {}
            $box = new Box();
            $box->anything = "fine, not tracked";
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn static_access_flags_undefined_constants_properties_and_methods() {
    // Phase 9's last remaining rule: existence checks for
    // StaticAccess (constants, static properties) and StaticCall
    // (static methods) - the same shape of check as MethodCall's,
    // just for `Foo::` instead of `$foo->`.
    let d = diagnostics_for(
        r#"<?php
            class Foo {
                const BAR = 1;
                public static int $counter = 0;
                public static function baz(): int { return 1; }
            }
            function f() {
                echo Foo::BAR;
                echo Foo::$counter;
                echo Foo::baz();
                echo Foo::class;
                echo Foo::TOTALLY_MADE_UP;
                echo Foo::$madeUpProp;
                echo Foo::madeUpMethod();
            }
        "#,
    );
    assert_eq!(d.len(), 3, "got: {d:?}");
    assert!(d.iter().any(|x| {
        x.message
            .contains("undefined constant Foo::TOTALLY_MADE_UP")
    }));
    assert!(d.iter().any(|x| {
        x.message
            .contains("undefined static property Foo::$madeUpProp")
    }));
    assert!(d.iter().any(|x| {
        x.message
            .contains("undefined static method Foo::madeUpMethod()")
    }));
}

#[test]
fn a_class_with_call_static_suppresses_undefined_static_method_calls() {
    let d = diagnostics_for(
        r#"<?php
            class Proxy {
                public static function __callStatic($name, $args) { return null; }
            }
            function f() {
                Proxy::anything();
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn self_parent_and_static_resolve_to_the_real_class_for_static_access() {
    let d = diagnostics_for(
        r#"<?php
            class Base {
                const SHARED = 1;
                public static function helper(): int { return 1; }
            }
            class Child extends Base {
                public function useParent(): int {
                    return parent::helper() + self::SHARED + static::helper();
                }
                public function useMadeUp(): int {
                    return parent::madeUpParentMethod();
                }
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    // Proves parent:: resolved to the real class Base, not a literal
    // "parent" - the same silent-vs-wrong distinction this whole
    // audit has repeatedly had to design tests around.
    assert!(
        d[0].message
            .contains("undefined static method Base::madeUpParentMethod()"),
        "got: {d:?}"
    );
}

#[test]
fn a_dynamic_static_target_is_never_checked() {
    // `$className::method()` has no statically known class - nothing
    // to check, and definitely not something to guess at.
    let d = diagnostics_for(
        r#"<?php
            function f($className) {
                return $className::method();
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn reading_an_undefined_property_is_flagged_but_writing_one_is_not() {
    // Phase 9's last remaining piece: undefined-property-on-*read*
    // detection. Deliberately asymmetric with the write side just
    // above - a write can legitimately create a real (if deprecated)
    // dynamic property, but a read of one that was never declared
    // and never dynamically assigned just returns null with a
    // warning in real PHP, so there's no legitimate case to stay
    // lenient for.
    let d = diagnostics_for(
        r#"<?php
            class Box {
                public int $known = 1;
            }
            function f(Box $box) {
                echo $box->known;
                echo $box->totallyMadeUp;
                $box->dynamicWrite = 5;
                [$box->a, $box->b] = [1, 2];
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert!(d[0].message.contains("Box::$totallyMadeUp"), "got: {d:?}");
}

#[test]
fn a_class_with_a_magic_get_method_suppresses_undefined_property_reads() {
    let d = diagnostics_for(
        r#"<?php
            class Box {
                public function __get($name) { return null; }
            }
            function f(Box $box) {
                echo $box->anything;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

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
fn anonymous_class_method_body_is_type_checked() {
    let d = diagnostics_for(
        r#"<?php
            function needsInt(int $x): int { return $x; }
            interface Greeter { public function greet(): string; }
            function make() {
                return new class implements Greeter {
                    public function greet(): string {
                        return needsInt("wrong");
                    }
                };
            }
        "#,
    );
    assert!(d.iter().any(|d| d.message.contains("needsInt")));
}

#[test]
fn anonymous_class_with_correct_usage_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function needsInt(int $x): int { return $x; }
            function make() {
                return new class {
                    public function value(): int {
                        return needsInt(5);
                    }
                };
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn anonymous_class_method_resolves_a_use_imported_return_type() {
    // Phase 22 Finding 5, real shape: a `use`-imported interface
    // return type on an anonymous class method - confirmed directly
    // from a real diagnostic's own wording before this fix: "expected
    // ResponseInterface, got Nyholm\Psr7\Response" (the expected side
    // printing bare/unqualified, proving it was never resolved
    // against the file's own `use` imports the way a top-level
    // class's method signature already is).
    let d = diagnostics_for(
        r#"<?php
            namespace App\Other;
            interface ResponseInterface {}

            namespace App\Impl;
            use App\Other\ResponseInterface;
            class RealResponse implements ResponseInterface {}

            namespace App;
            use App\Other\ResponseInterface;
            use App\Impl\RealResponse;
            function make() {
                return new class {
                    public function handle(): ResponseInterface {
                        return new RealResponse();
                    }
                };
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
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
fn instantiating_an_undefined_class_is_flagged() {
    let d = diagnostics_for("<?php function f() { return new TotallyMadeUpClass(); }");
    assert!(
        d.iter()
            .any(|x| x.message.contains("TotallyMadeUpClass not found")),
        "expected an undefined-class diagnostic, got: {d:?}"
    );
}

#[test]
fn instantiating_a_known_class_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {}
            function f() { return new Foo(); }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn instantiating_an_interface_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            interface Doer { public function doIt(): void; }
            function test(): Doer { return new Doer(); }
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.message.contains("cannot instantiate interface Doer")),
        "expected a cannot-instantiate-interface diagnostic, got: {d:?}"
    );
}

#[test]
fn instantiating_an_abstract_class_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            abstract class AbstractThing { abstract public function doIt(): void; }
            function test(): AbstractThing { return new AbstractThing(); }
        "#,
    );
    assert!(
        d.iter().any(|x| x
            .message
            .contains("cannot instantiate abstract class AbstractThing")),
        "expected a cannot-instantiate-abstract-class diagnostic, got: {d:?}"
    );
}

#[test]
fn instantiating_a_concrete_subclass_of_an_abstract_class_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            abstract class AbstractThing { abstract public function doIt(): void; }
            class Concrete extends AbstractThing { public function doIt(): void {} }
            function test(): Concrete { return new Concrete(); }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_class_missing_an_inherited_abstract_method_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            abstract class Shape2 { abstract public function area(): float; }
            class Circle2 extends Shape2 {}
        "#,
    );
    assert!(
        d.iter().any(|x| x
            .message
            .contains("does not implement abstract method(s): area")),
        "expected a missing-abstract-method diagnostic, got: {d:?}"
    );
}

#[test]
fn a_class_partially_implementing_an_interface_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            interface Multi7 { public function a(): void; public function b(): void; }
            class Impl7 implements Multi7 { public function a(): void {} }
        "#,
    );
    assert!(
        d.iter().any(|x| x
            .message
            .contains("does not implement abstract method(s): b")),
        "expected a missing-abstract-method diagnostic, got: {d:?}"
    );
}

#[test]
fn a_class_using_a_trait_with_an_unimplemented_abstract_method_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            trait Greetable9 {
                abstract public function getName(): string;
                public function greet(): string { return "Hello " . $this->getName(); }
            }
            class Person9 { use Greetable9; }
        "#,
    );
    assert!(
        d.iter().any(|x| x
            .message
            .contains("does not implement abstract method(s): getName")),
        "expected a missing-abstract-method diagnostic, got: {d:?}"
    );
}

#[test]
fn a_class_that_supplies_a_traits_abstract_method_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            trait Greetable9 {
                abstract public function getName(): string;
                public function greet(): string { return "Hello " . $this->getName(); }
            }
            class Person9 { use Greetable9; public function getName(): string { return "x"; } }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_concrete_implementation_two_hops_up_an_extends_chain_satisfies_a_directly_implemented_interface()
 {
    // Regression for a real BFS-distance bug found while implementing
    // this: a class can `implements Throwable` directly while its
    // real concrete `getCode()` only lives two `extends` hops further
    // up (e.g. `BaseException -> Exception`) - the *closest* ancestor
    // mentioning the name at all must not win over a real, if
    // farther, concrete implementation.
    let d = diagnostics_for(
        r#"<?php
            interface Throwable2 {
                public function getMessage(): string;
                public function getCode();
            }
            class Exception2 implements Throwable2 {
                public function getMessage(): string {}
                public function getCode() {}
            }
            class BaseException2 extends Exception2 implements Throwable2 {}
            class Leaf2 extends BaseException2 implements Throwable2 {}
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_method_narrowing_an_inherited_parameter_type_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Animal2 {}
            class Dog2 extends Animal2 {}
            class Handler2 { public function handle(Animal2 $a): void {} }
            class DogOnlyHandler2 extends Handler2 { public function handle(Dog2 $a): void {} }
        "#,
    );
    assert!(
        d.iter().any(|x| x.message.contains(
            "DogOnlyHandler2::handle() parameter #1 ($a) narrows the inherited parameter type"
        )),
        "expected a narrowed-parameter diagnostic, got: {d:?}"
    );
}

// A top-10-bucket workflow found this exact check compared the merged,
// docblock-aware parameter type instead of the native-only one - real
// PHP/PHPStan only enforce override-compatibility at the native level
// by default (`checkPhpDocMethodSignatures` is off unless a project
// opts in), the identical distinction this project's own return-type
// check already makes via `return_type_native`. Real, extremely common
// corpus shape (Rector's own `RectorInterface::refactor(Node $node)`,
// narrowed per-rule only via `@param SpecificNode $node`, native type
// left as the wide `Node` on every single implementation): real
// PHPStan does not flag this by default.
#[test]
fn a_param_narrowed_only_via_docblock_with_an_unchanged_native_type_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Node4 {}
            class SpecificNode4 extends Node4 {}
            interface Rule4 { public function refactor(Node4 $node): ?Node4; }
            class MyRector4 implements Rule4 {
                /** @param SpecificNode4 $node */
                public function refactor(Node4 $node): ?Node4 { return $node; }
            }
        "#,
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "method.childParameterType"),
        "unexpected diagnostics: {d:?}"
    );
}

// The reverse: a genuine *native*-level narrowing must still be
// flagged even when an `@param` docblock is also present (repeating or
// further narrowing the same native type) - this fix must not weaken
// the check for the case it's not about.
#[test]
fn a_param_narrowed_at_the_native_level_is_still_flagged_even_with_a_docblock_present() {
    let d = diagnostics_for(
        r#"<?php
            class Node5 {}
            class SpecificNode5 extends Node5 {}
            interface Rule5 { public function refactor(Node5 $node): ?Node5; }
            class MyRector5 implements Rule5 {
                /** @param SpecificNode5 $node */
                public function refactor(SpecificNode5 $node): ?Node5 { return $node; }
            }
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "method.childParameterType"),
        "expected a narrowed-parameter diagnostic, got: {d:?}"
    );
}

// Real corpus find (Doctrine\Common\Collections\ReadableCollection::
// exists(Closure $p), overridden identically by AbstractLazyCollection):
// an ancestor's own `@phpstan-param Closure(TKey, T):bool $p` (the
// callable-shape doc syntax this project degrades to plain
// `Type::Callable`, having no per-parameter/return signature of its
// own to track) used to make `reconcile_with_native` resolve the
// *ancestor's* parameter type to that coarse `Callable` marker instead
// of its own, exactly-known native `Closure` - even though the child's
// identically-typed native `Closure $p` resolved correctly on its own,
// comparing "child: Closure" against "ancestor: Callable" looked like a
// real narrowing that was never actually there.
#[test]
fn an_override_with_a_callable_shape_doc_on_the_ancestor_only_is_not_a_narrowing() {
    let d = diagnostics_for(
        r#"<?php
            interface ReadableCollection3 {
                /**
                 * @phpstan-param Closure(int, int):bool $p
                 */
                public function exists(Closure $p): bool;
            }
            class AbstractLazyCollection3 implements ReadableCollection3 {
                public function exists(Closure $p): bool { return true; }
            }
        "#,
    );
    assert!(
        !d.iter()
            .any(|x| x.message.contains("narrows the inherited parameter type")),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_final_class_s_own_self_typed_override_is_not_a_narrowing() {
    // Real Piwigo corpus bug (Admin\Image\ImageBackend implements
    // ImageInterface, whose own compose() parameter is declared by the
    // *concrete implementing class's own name*, not an abstraction -
    // an unusual but valid interface shape): `self` inside the
    // concrete class and that same concrete class name are provably
    // identical types, but neither is_assignable/is_subtype has a rule
    // for a concrete Object satisfying a still-bare SelfRef (only the
    // reverse direction is handled) - both the override's own `self`
    // and the ancestor's own declared type need resolving before
    // comparison, not just one.
    let d = diagnostics_for(
        r#"<?php
            interface ImageInterface {
                public function compose(ImageBackend $overlay): bool;
            }
            final class ImageBackend implements ImageInterface {
                public function compose(self $overlay): bool {
                    return true;
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_method_widening_an_inherited_parameter_type_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Animal2 {}
            class Handler2 { public function handle(Animal2 $a): void {} }
            class WideningHandler2 extends Handler2 { public function handle(object $a): void {} }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_constructor_with_an_incompatible_signature_is_not_flagged() {
    // Real PHP deliberately exempts __construct from override-
    // compatibility checking - a subclass constructor may have an
    // entirely different signature from its parent's.
    let d = diagnostics_for(
        r#"<?php
            class Base10 { public function __construct(string $a) {} }
            class Sub10 extends Base10 { public function __construct(int $a, int $b) {} }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn an_override_with_no_declared_type_is_not_flagged_as_narrowing() {
    // Type::Mixed here means "nothing declared, native or docblock",
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

#[test]
fn an_override_narrowing_an_inherited_static_return_type_is_not_flagged() {
    // Real PHP's late-static-binding covariance rule: an ancestor's
    // own `static`-typed return is always safely satisfiable by an
    // override returning its own concrete class name.
    let d = diagnostics_for(
        r#"<?php
            interface Collection12 { /** @return static */ public function copy(); }
            class Vector12 implements Collection12 { public function copy(): Vector12 {} }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// A round of Piwigo-corpus bucketing found this check compared the
// *docblock-refined* return type on both sides, wrongly flagging an
// override that faithfully keeps the ancestor's own native `array`
// while the ancestor's docblock separately narrowed it further
// (`list<...>`) - real PHP itself only enforces override-
// compatibility on the native signature, and by default so does real
// PHPStan (`checkPhpDocMethodSignatures` is off unless a project
// opts in - confirmed directly against real PHPStan with this exact
// shape). Real corpus shape: several `HtmlRenderingInterface` test
// fakes each declaring `getCatBreadcrumb(): array`, exactly matching
// the interface's own native signature, while the interface's
// *docblock* separately says `@return list<array{...}>`.
#[test]
fn an_override_matching_only_the_ancestors_native_return_type_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            interface HasBreadcrumb13 {
                /** @return list<array{id: string, name: string}> */
                public function getCatBreadcrumb(string $x): array;
            }
            class FakeRenderer13 implements HasBreadcrumb13 {
                public function getCatBreadcrumb(string $x): array {
                    return [];
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// Companion to the fix above: a genuine *native*-level incompatible
// override (widening `int` to `string`, a real PHP fatal error) must
// still be caught - this check has no other positive-case coverage
// confirming it fires at all.
#[test]
fn an_override_genuinely_widening_the_native_return_type_is_still_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Base14 { public function make(): int { return 1; } }
            class Sub14 extends Base14 { public function make(): string { return "x"; } }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert!(d[0].message.contains("is not compatible"), "got: {d:?}");
}

// Round 10 of the Piwigo corpus cleanup: 22 first-party value objects
// (ImageId, TagId, UserId, ...) each implement a shared interface
// declaring `tryFrom(mixed $value): ?static` and override it with
// `?self` on their own `final` class - confirmed against real PHPStan
// 2.2.8 to be a legitimate override (a `final` class's `self` and
// `static` are always the same concrete type). `resolve_type` builds
// `?static` as `Union([StaticRef, Null])`, not its own `StaticRef`
// variant, so the pre-existing bare-`StaticRef` escape hatch above
// missed this extremely common nullable shape entirely.
#[test]
fn overriding_a_nullable_static_return_with_a_nullable_self_return_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            interface NumericId15 {
                public static function tryFrom(mixed $value): ?static;
            }
            final class ImageId15 implements NumericId15 {
                public static function tryFrom(mixed $value): ?self {
                    return null;
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn overriding_a_final_method_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Base4 { final public function locked(): int { return 1; } }
            class Sub4 extends Base4 { public function locked(): int { return 2; } }
        "#,
    );
    assert!(
        d.iter().any(|x| x
            .message
            .contains("cannot override final method Base4::locked")),
        "expected a final-override diagnostic, got: {d:?}"
    );
}

#[test]
fn overriding_a_final_method_two_hops_up_the_extends_chain_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Base6 { final public function locked(): void {} }
            class Mid6 extends Base6 {}
            class Leaf6 extends Mid6 { public function locked(): void {} }
        "#,
    );
    assert!(
        d.iter().any(|x| x
            .message
            .contains("cannot override final method Base6::locked")),
        "expected a final-override diagnostic, got: {d:?}"
    );
}

#[test]
fn overriding_a_non_final_method_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Ok1 { public function open(): int { return 1; } }
            class Ok2 extends Ok1 { public function open(): int { return 2; } }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn redeclaring_a_final_private_method_is_not_flagged() {
    // A `private` method isn't part of a class's inherited/
    // overridable surface at all in real PHP - a subclass declaring
    // its own same-named private method is an independent,
    // unrelated declaration, not an override. `final`+`private` here
    // is otherwise real PHP's own `method.finalPrivate` warning (Phase
    // 65) - deliberately kept in the fixture and filtered out below
    // rather than dropped, since it's what originally made this shape
    // exercise `method.overrideFinal` at all.
    let d = diagnostics_for(
        r#"<?php
            class Base13 { final private function __clone() {} }
            class Sub13 extends Base13 { final private function __clone() {} }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "method.overrideFinal"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn extending_a_final_class_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            final class Sealed5 {}
            class Sub5 extends Sealed5 {}
        "#,
    );
    assert!(
        d.iter().any(|x| x
            .message
            .contains("class Sub5 cannot extend final class Sealed5")),
        "expected a final-class-extension diagnostic, got: {d:?}"
    );
}

#[test]
fn extending_a_non_final_class_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Open5 {}
            class Sub5b extends Open5 {}
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn calling_a_method_from_a_transitively_used_trait_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            trait Greets { public function greet(): string { return "hi"; } }
            trait Combined { use Greets; public function extra(): int { return 1; } }
            class Foo { use Combined; }
            function test(Foo $f): string { return $f->greet(); }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// Phase 13 Finding E: a trait method calling $this->hostOnlyMethod()
// is valid PHP (the trait's author expects any using class to
// provide it) - checked standalone (current_class bound to the
// trait's own FQN), this always false-positived as "undefined
// method".
#[test]
fn a_trait_method_calling_a_host_only_method_is_not_flagged_when_the_host_declares_it() {
    let d = diagnostics_for(
        r#"<?php
            trait Greeter {
                public function greet(): string { return $this->name(); }
            }
            class Person {
                use Greeter;
                public function name(): string { return "Alice"; }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_trait_method_calling_a_host_only_method_is_flagged_for_a_sibling_host_missing_it() {
    // Proves per-host re-checking, not "checked once (against
    // whichever host happens to run first) and reused" - the same
    // trait, used by two classes, must be checked once per host.
    let d = diagnostics_for(
        r#"<?php
            trait Greeter {
                public function greet(): string { return $this->name(); }
            }
            class Person {
                use Greeter;
                public function name(): string { return "Alice"; }
            }
            class Robot {
                use Greeter;
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert!(d[0].message.contains("undefined method"), "got: {d:?}");
}

// Real corpus shape: symfony/cache's own `if (version_compare(phpversion(
// 'redis'), '6.3.0', '>=')) { trait Redis63ProxyTrait { ... } }` -
// `check_program`'s own top-level `StmtKind::ClassDecl` handling already
// dispatches a trait through `check_trait_methods` (this same per-host
// re-checking), but a trait declared *inside* a conditional block
// reaches `check_stmt`'s own nested `StmtKind::ClassDecl` arm instead,
// which used to check every trait method with `current_class` set to
// the trait's own name unconditionally, regardless of `ClassKind` -
// `$this->hostOnlyMethod()` (real shape: `initializeLazyObject()`,
// provided by a sibling trait on the real host) was wrongly flagged as
// undefined on the trait itself.
#[test]
fn a_conditionally_declared_trait_still_checks_against_its_real_host() {
    let d = diagnostics_for(
        r#"<?php
            if (true) {
                trait Greeter {
                    public function greet(): string { return $this->name(); }
                }
            }
            class Person {
                use Greeter;
                public function name(): string { return "Alice"; }
            }
        "#,
    );
    // Phase 15 item (2): the literal `if (true)` wrapping here (this
    // test's own scenario shape - a conditionally-declared trait, e.g.
    // real code's own `if (!trait_exists(...)) { trait Foo {...} }`
    // idiom) now legitimately also gets its own `if.alwaysTrue` -
    // unrelated to what this test actually checks (that the trait's own
    // members still resolve against its real host), so it's excluded
    // here rather than changing the fixture's own scenario shape.
    assert!(
        !d.iter().any(|x| x.identifier != "if.alwaysTrue"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_host_independent_trait_bug_is_reported_once_not_once_per_host() {
    let d = diagnostics_for(
        r#"<?php
            trait Broken {
                public function boom(): int { return thisFunctionDoesNotExist(); }
            }
            class A { use Broken; }
            class B { use Broken; }
        "#,
    );
    assert_eq!(d.len(), 1, "expected the duplicate to be deduped: {d:?}");
}

#[test]
fn an_unused_trait_is_still_checked_standalone() {
    let d = diagnostics_for(
        r#"<?php
            trait Orphaned {
                public function boom(): int { return thisFunctionDoesNotExist(); }
            }
        "#,
    );
    assert_eq!(d.len(), 1, "an unused trait must still be checked: {d:?}");
}

#[test]
fn a_trait_used_only_by_another_trait_is_checked_against_the_real_final_host() {
    // `Inner` is used only by `Outer` (itself a trait), never
    // directly by any real class - `Inner`'s own $this->name() must
    // still resolve against Concrete, the real, final host, not
    // against `Outer` (which is itself just as host-unaware).
    let d = diagnostics_for(
        r#"<?php
            trait Inner {
                public function greet(): string { return $this->name(); }
            }
            trait Outer {
                use Inner;
            }
            class Concrete {
                use Outer;
                public function name(): string { return "x"; }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_trait_alias_introduced_via_as_is_callable_under_its_new_name() {
    let d = diagnostics_for(
        r#"<?php
            trait A { public function hello(): string { return "A"; } }
            trait B { public function hello(): string { return "B"; } }
            class C {
                use A, B {
                    A::hello insteadof B;
                    B::hello as helloB;
                }
            }
            function test(C $c): void { echo $c->hello(); echo $c->helloB(); }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn an_unqualified_trait_alias_resolves_against_the_single_used_trait() {
    let d = diagnostics_for(
        r#"<?php
            trait X { public function greet(): string { return "x"; } }
            class Y {
                use X {
                    greet as greetAlias;
                }
            }
            function test(Y $y): void { echo $y->greet(); echo $y->greetAlias(); }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn calling_an_undeclared_trait_alias_is_still_flagged() {
    let d = diagnostics_for(
        r#"<?php
            trait X { public function greet(): string { return "x"; } }
            class Y {
                use X {
                    greet as greetAlias;
                }
            }
            function test(Y $y): void { echo $y->totallyMadeUp(); }
        "#,
    );
    assert!(
        d.iter().any(|x| x.message.contains("undefined method")),
        "expected an undefined-method diagnostic, got: {d:?}"
    );
}

#[test]
fn two_traits_declaring_the_same_property_with_incompatible_types_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            trait T1a { public int $x = 1; }
            trait T1b { public string $x = "a"; }
            class C1 { use T1a, T1b; }
        "#,
    );
    assert!(
        d.iter().any(|x| x
            .message
            .contains("T1a and T1b define the same property ($x) with incompatible types")),
        "expected a trait-property-conflict diagnostic, got: {d:?}"
    );
}

#[test]
fn two_traits_declaring_the_same_method_with_incompatible_signatures_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            trait T2a { public function foo(): int { return 1; } }
            trait T2b { public function foo(): string { return "a"; } }
            class C2 { use T2a, T2b; }
        "#,
    );
    assert!(
        d.iter().any(|x| x
            .message
            .contains("T2a and T2b define the same method (foo) with an incompatible signature")),
        "expected a trait-method-conflict diagnostic, got: {d:?}"
    );
}

#[test]
fn two_traits_declaring_the_same_property_with_identical_types_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            trait T3a { public int $y = 1; }
            trait T3b { public int $y = 2; }
            class C3 { use T3a, T3b; }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_trait_method_conflict_resolved_via_insteadof_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            trait T4a { public function bar(): int { return 1; } }
            trait T4b { public function bar(): int { return 2; } }
            class C4 {
                use T4a, T4b {
                    T4a::bar insteadof T4b;
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn new_self_and_static_are_never_flagged_as_undefined() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {
                public static function make(): self { return new self(); }
                public static function makeStatic(): static { return new static(); }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// Finding 25 from the architecture audit - a self/static/parent-typed
// parameter was never resolved, silently disabling undefined-
// property/method checking through it (unlike the identical access
// through $this).

#[test]
fn self_typed_parameter_gets_undefined_property_checking() {
    let d = diagnostics_for(
        r#"<?php
            class Point {
                public int $id = 0;
                public function equals(self $other): bool {
                    return $this->id === $other->missingProp;
                }
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert!(d[0].message.contains("missingProp"), "got: {d:?}");
}

#[test]
fn self_typed_parameter_accessing_a_real_property_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Point {
                public int $id = 0;
                public function equals(self $other): bool {
                    return $this->id === $other->id;
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn phpstan_instantiation_flags_abstract_class_and_interface() {
    // Derived from vendor/phpstan-src/tests/PHPStan/Rules/Classes/data/instantiation.php
    // (lines ~29-30 of the vendored `2.2.11` tag): `new LoremInstantiation()` /
    // `new IpsumInstantiation()`, an abstract class and an interface.
    let d = diagnostics_for(
        r#"<?php
            abstract class LoremInstantiation {}
            interface IpsumInstantiation {}
            function test(): void {
                new LoremInstantiation();
                new IpsumInstantiation();
            }
        "#,
    );
    assert!(d.iter().any(|x| {
        x.message
            .contains("cannot instantiate abstract class LoremInstantiation")
    }));
    assert!(d.iter().any(|x| {
        x.message
            .contains("cannot instantiate interface IpsumInstantiation")
    }));
}

#[test]
fn phpstan_missing_method_impl_flags_interface_and_abstract_gaps() {
    // Derived from vendor/phpstan-src/tests/PHPStan/Rules/Methods/data/missing-method-impl.php:
    // a concrete class missing both an interface-required method
    // (`doFoo` from `Foo`) and an inherited abstract method (`doBaz`
    // from `Bar`) - upstream's own `Baz` message names both origins.
    let d = diagnostics_for(
        r#"<?php
            interface Foo { public function doFoo(); }
            abstract class Bar implements Foo {
                public function doBar() {}
                abstract public function doBaz();
            }
            class Baz extends Bar {
                public function doBar() {}
            }
        "#,
    );
    assert!(d.iter().any(
        |x| x.message.contains("does not implement abstract method(s):")
            && x.message.contains("doFoo")
            && x.message.contains("doBaz")
    ));
}

#[test]
fn phpstan_overriding_method_flags_a_final_method_override() {
    // Derived from vendor/phpstan-src/tests/PHPStan/Rules/Methods/data/overriding-method.php's
    // `Foo`/`Bar` pair: upstream's own expected message is "Method
    // OverridingFinalMethod\Bar::doFoo() overrides final method
    // OverridingFinalMethod\Foo::doFoo().".
    let d = diagnostics_for(
        r#"<?php
            class Foo {
                final public function doFoo(): void {}
            }
            class Bar extends Foo {
                public function doFoo(): void {}
            }
        "#,
    );
    assert!(d.iter().any(|x| {
        x.message
            .contains("cannot override final method Foo::doFoo")
    }));
}

#[test]
fn phpstan_parameter_contravariance_flags_a_narrowed_override() {
    // Derived from vendor/phpstan-src/tests/PHPStan/Rules/Methods/data/parameter-contravariance.php's
    // `Foo`/`Lorem` pair: upstream flags `Lorem::doFoo(InvalidArgumentException $e)`
    // as "not contravariant with" `Foo::doFoo(Exception $e)` - the same
    // narrowed-parameter shape Finding 50 covers, using real,
    // already-registered Exception-hierarchy classes from the vendored
    // phpstorm-stubs corpus rather than made-up ones.
    let d = diagnostics_for(
        r#"<?php
            class Foo {
                public function doFoo(\Exception $e): void {}
            }
            class Lorem extends Foo {
                public function doFoo(\InvalidArgumentException $e): void {}
            }
        "#,
    );
    assert!(d.iter().any(|x| x.message.contains("Lorem::doFoo")
        && x.message.contains("narrows the inherited parameter type")));
}

#[test]
fn phpstan_readonly_property_assign_flags_writes_outside_the_constructor() {
    // Derived from vendor/phpstan-src/tests/PHPStan/Rules/Properties/data/readonly-assign.php:
    // a setter re-assigning its own class's readonly property ("setter -
    // report"), and a subclass's constructor assigning a readonly
    // property it inherits rather than declares ("not in declaring
    // class - report").
    let d = diagnostics_for(
        r#"<?php
            class Foo {
                public function __construct(public readonly int $x) {}
                public function setX(int $x): void { $this->x = $x; }
            }
            class Bar extends Foo {
                public function __construct(int $x) {
                    parent::__construct($x);
                    $this->x = $x;
                }
            }
        "#,
    );
    let readonly_hits = d
        .iter()
        .filter(|x| x.message.contains("Foo::$x is readonly"))
        .count();
    assert_eq!(readonly_hits, 2, "expected both writes flagged, got: {d:?}");
}

// Phase 64: narrowing a method's own visibility on override is a real,
// unconditional PHP fatal error (confirmed directly against real `php`:
// "Access level to Child::foo() must be public (as in class Base)").
#[test]
fn narrowing_visibility_from_public_to_protected_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Base { public function foo(): void {} }
            class Child extends Base { protected function foo(): void {} }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "method.visibility"),
        "got: {d:?}"
    );
}

#[test]
fn narrowing_visibility_from_protected_to_private_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Base { protected function foo(): void {} }
            class Child extends Base { private function foo(): void {} }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "method.visibility"),
        "got: {d:?}"
    );
}

#[test]
fn widening_visibility_from_protected_to_public_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Base { protected function foo(): void {} }
            class Child extends Base { public function foo(): void {} }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "method.visibility"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn keeping_the_same_visibility_on_override_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Base { public function foo(): void {} }
            class Child extends Base { public function foo(): void {} }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "method.visibility"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn overriding_a_private_ancestor_method_is_never_flagged_as_narrowing() {
    // Not a real override at all - `private` methods aren't virtual in
    // real PHP, so a same-named child method is an unrelated declaration.
    let d = diagnostics_for(
        r#"<?php
            class Base { private function foo(int $a): void {} }
            class Child extends Base { private function foo(): void {} }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "method.visibility"
            || x.identifier == "parameter.missing"
            || x.identifier == "parameter.notOptional"),
        "unexpected diagnostics: {d:?}"
    );
}

// Phase 64: an override with fewer parameters than its ancestor is a
// real, unconditional PHP fatal error (confirmed directly: "Declaration
// of Child::foo(int $a): void must be compatible with Base::foo(int $a,
// int $b): void").
#[test]
fn an_override_missing_an_inherited_parameter_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Base { public function foo(int $a, int $b): void {} }
            class Child extends Base { public function foo(int $a): void {} }
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "parameter.missing" && x.message.contains("#2 ($b)")),
        "got: {d:?}"
    );
}

#[test]
fn an_override_missing_a_variadic_ancestor_tail_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Base { public function foo(int $a, ...$rest): void {} }
            class Child extends Base { public function foo(int $a): void {} }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "parameter.missing"),
        "unexpected diagnostics: {d:?}"
    );
}

// Phase 64: a parameter the ancestor made optional can never become
// required on override - a real, unconditional PHP fatal error
// (confirmed directly: "Declaration of Child::foo(int $a, int $b): void
// must be compatible with Base::foo(int $a, int $b = 0): void").
#[test]
fn an_override_requiring_a_parameter_the_ancestor_made_optional_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Base { public function foo(int $a, int $b = 0): void {} }
            class Child extends Base { public function foo(int $a, int $b): void {} }
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "parameter.notOptional" && x.message.contains("#2 ($b)")),
        "got: {d:?}"
    );
}

#[test]
fn an_override_keeping_an_inherited_parameter_optional_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Base { public function foo(int $a, int $b = 0): void {} }
            class Child extends Base { public function foo(int $a, int $b = 1): void {} }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "parameter.notOptional"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn an_override_adding_its_own_extra_optional_parameter_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Base { public function foo(int $a): void {} }
            class Child extends Base { public function foo(int $a, int $b = 0): void {} }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "parameter.missing"
            || x.identifier == "parameter.notOptional"),
        "unexpected diagnostics: {d:?}"
    );
}

// Phase 64's own still-open list: by-ref/variadic-contravariance
// mismatches - both confirmed directly via `php -l` as real,
// unconditional fatal errors ("Declaration of Child::foo(...) must be
// compatible with Base::foo(...)").

#[test]
fn an_override_dropping_a_by_ref_parameter_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Base { public function foo(&$x): void {} }
            class Child extends Base { public function foo($x): void {} }
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "parameter.notByRef" && x.message.contains("#1 ($x)")),
        "got: {d:?}"
    );
}

#[test]
fn an_override_adding_a_by_ref_parameter_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Base { public function foo($x): void {} }
            class Child extends Base { public function foo(&$x): void {} }
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "parameter.byRef" && x.message.contains("#1 ($x)")),
        "got: {d:?}"
    );
}

#[test]
fn keeping_the_same_by_ref_ness_on_override_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Base { public function foo(&$x): void {} }
            class Child extends Base { public function foo(&$x): void {} }
        "#,
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "parameter.byRef" || x.identifier == "parameter.notByRef"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn an_override_dropping_variadic_on_the_ancestors_own_variadic_parameter_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Base { public function foo(...$x): void {} }
            class Child extends Base { public function foo($x): void {} }
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "parameter.notVariadic" && x.message.contains("#1 ($x)")),
        "got: {d:?}"
    );
}

#[test]
fn widening_a_fixed_ancestor_parameter_to_variadic_on_override_is_never_flagged() {
    // Real, legal PHP - confirmed directly - the reverse direction from
    // the check just above.
    let d = diagnostics_for(
        r#"<?php
            class Base { public function foo($x): void {} }
            class Child extends Base { public function foo(...$x): void {} }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "parameter.notVariadic"),
        "unexpected diagnostics: {d:?}"
    );
}

// Phase 64's own still-open property-override half - each shape
// confirmed directly via `php -r` as a real, unconditional fatal error
// ("Cannot redeclare ... as ...", "Access level to ... must be ...").

#[test]
fn overriding_a_static_property_as_non_static_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Base { public static int $x = 1; }
            class Child extends Base { public int $x = 1; }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "property.nonStatic"),
        "got: {d:?}"
    );
}

#[test]
fn overriding_a_non_static_property_as_static_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Base { public int $x = 1; }
            class Child extends Base { public static int $x = 1; }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "property.static"),
        "got: {d:?}"
    );
}

#[test]
fn keeping_the_same_static_ness_on_override_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Base { public static int $x = 1; }
            class Child extends Base { public static int $x = 1; }
        "#,
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "property.nonStatic" || x.identifier == "property.static"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn overriding_a_readonly_property_as_readwrite_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Base {
                public function __construct(public readonly int $x) {}
            }
            class Child extends Base {
                public int $x = 1;
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "property.readWrite"),
        "got: {d:?}"
    );
}

#[test]
fn overriding_a_readwrite_property_as_readonly_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Base { public int $x = 1; }
            class Child extends Base {
                public function __construct(public readonly int $x) {}
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "property.readOnly"),
        "got: {d:?}"
    );
}

#[test]
fn narrowing_a_property_s_visibility_from_public_to_protected_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Base { public int $x = 1; }
            class Child extends Base { protected int $x = 1; }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "property.visibility"),
        "got: {d:?}"
    );
}

#[test]
fn narrowing_a_property_s_visibility_from_protected_to_private_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Base { protected int $x = 1; }
            class Child extends Base { private int $x = 1; }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "property.visibility"),
        "got: {d:?}"
    );
}

#[test]
fn widening_a_property_s_visibility_on_override_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Base { protected int $x = 1; }
            class Child extends Base { public int $x = 1; }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "property.visibility"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn overriding_a_private_ancestor_property_is_never_flagged_for_any_mismatch() {
    // A `private` ancestor property is an unrelated declaration, not a
    // real override target - same reasoning as the identical method
    // exemption.
    let d = diagnostics_for(
        r#"<?php
            class Base { private static int $x = 1; }
            class Child extends Base { public int $x = 1; }
        "#,
    );
    assert!(
        !d.iter().any(|x| matches!(
            x.identifier.as_str(),
            "property.nonStatic"
                | "property.static"
                | "property.readWrite"
                | "property.readOnly"
                | "property.visibility"
        )),
        "unexpected diagnostics: {d:?}"
    );
}

// Phase 64 follow-up: a method a trait supplies but the class itself
// never textually re-declares still needs override-compatibility
// checking against an implemented interface/parent - confirmed
// directly via `php -r` as a real, unconditional fatal error even
// though the class body never mentions the method name at all.

#[test]
fn a_trait_supplied_method_narrowing_an_interface_parameter_type_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            interface Iface {
                public function foo(int $x): void;
            }
            trait T {
                public function foo(string $x): void {}
            }
            class Impl implements Iface {
                use T;
            }
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "method.childParameterType" && x.message.contains("Impl")),
        "got: {d:?}"
    );
}

#[test]
fn a_trait_supplied_method_matching_an_interface_signature_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            interface Iface {
                public function foo(int $x): void;
            }
            trait T {
                public function foo(int $x): void {}
            }
            class Impl implements Iface {
                use T;
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| matches!(
            x.identifier.as_str(),
            "method.childParameterType"
                | "method.childReturnType"
                | "method.visibility"
                | "parameter.missing"
                | "parameter.notOptional"
        )),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_trait_supplied_method_narrowing_an_interface_method_s_visibility_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            interface Iface {
                public function foo(): void;
            }
            trait T {
                protected function foo(): void {}
            }
            class Impl implements Iface {
                use T;
            }
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "method.visibility" && x.message.contains("Impl")),
        "got: {d:?}"
    );
}

#[test]
fn overriding_a_final_ancestor_method_via_a_used_trait_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Base {
                final public function foo(): void {}
            }
            trait T {
                public function foo(): void {}
            }
            class Impl extends Base {
                use T;
            }
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "method.overrideFinal" && x.message.contains("Impl")),
        "got: {d:?}"
    );
}

#[test]
fn a_class_s_own_method_overriding_a_trait_method_of_the_same_name_is_checked_once() {
    // A class's own directly-declared method always wins over a
    // same-named trait contribution (established "own wins"
    // semantics) - the trait-only sweep must not double-check it.
    let d = diagnostics_for(
        r#"<?php
            interface Iface {
                public function foo(int $x): void;
            }
            trait T {
                public function foo(string $x): void {}
            }
            class Impl implements Iface {
                use T;
                public function foo(int $x): void {}
            }
        "#,
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "method.childParameterType"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn an_unimplemented_abstract_trait_method_is_not_flagged_by_the_trait_override_sweep() {
    // A trait's own `abstract` method (the using class is required to
    // supply a body for) has no concrete implementation of its own to
    // compare against an ancestor at all - this is a different, already-
    // separately-tracked concern (`abstract_methods`), not an override.
    let d = diagnostics_for(
        r#"<?php
            interface Iface {
                public function foo(int $x): void;
            }
            trait T {
                abstract public function foo(string $x): void;
            }
            class Impl implements Iface {
                use T;
            }
        "#,
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "method.childParameterType"),
        "unexpected diagnostics: {d:?}"
    );
}

// Phase 64 follow-up: PHP 8.3's `#[\Override]` attribute - the
// unconditional-fatal-error direction only (a real override *missing*
// the attribute is a real PHPStan opt-in rule, off by default and not
// enabled in this project's own corpus config - not attempted).

#[test]
fn an_override_attribute_with_no_matching_ancestor_method_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Base {
                public function foo(): void {}
            }
            class Child extends Base {
                #[\Override]
                public function bar(): void {}
            }
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "method.override" && x.message.contains("Child::bar")),
        "got: {d:?}"
    );
}

#[test]
fn an_override_attribute_matching_a_real_ancestor_method_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Base {
                public function foo(): void {}
            }
            class Child extends Base {
                #[\Override]
                public function foo(): void {}
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "method.override"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn an_override_attribute_matching_an_interface_method_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            interface Iface {
                public function foo(): void;
            }
            class Impl implements Iface {
                #[\Override]
                public function foo(): void {}
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "method.override"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn an_override_attribute_on_a_constructor_with_no_ancestor_constructor_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Base {}
            class Child extends Base {
                #[\Override]
                public function __construct() {}
            }
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "method.override" && x.message.contains("Child::__construct")),
        "got: {d:?}"
    );
}

#[test]
fn an_override_attribute_matching_only_a_private_ancestor_method_is_flagged() {
    // A `private` ancestor method isn't a real override target - same
    // exemption `check_one_method_override`'s own checks already use.
    let d = diagnostics_for(
        r#"<?php
            class Base {
                private function foo(): void {}
            }
            class Child extends Base {
                #[\Override]
                public function foo(): void {}
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "method.override"),
        "got: {d:?}"
    );
}

#[test]
fn a_plain_method_with_no_override_attribute_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Base {
                public function foo(): void {}
            }
            class Child extends Base {
                public function bar(): void {}
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "method.override"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn an_override_attribute_on_a_standalone_trait_method_is_never_flagged_by_itself() {
    // Real, previously-live bug found via this project's own corpus:
    // a trait has no ancestor chain of its own at all - real PHP defers
    // this whole check to whichever class eventually `use`s the trait
    // (confirmed directly: the identical trait method is fine when the
    // using class has a real ancestor, and only fatals - attributed to
    // the *using class's* own method name - when it doesn't). Checking
    // the trait's own declaration in isolation must never flag it.
    let d = diagnostics_for(
        r#"<?php
            trait T {
                #[\Override]
                public function foo(): void {}
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "method.override"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn an_override_attribute_on_a_trait_method_used_by_a_class_with_a_real_ancestor_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            trait T {
                #[\Override]
                public function foo(): void {}
            }
            class Base {
                public function foo(): void {}
            }
            class Child extends Base {
                use T;
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "method.override"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn an_override_attribute_on_a_trait_method_used_by_a_class_with_no_ancestor_is_flagged_on_the_using_class()
 {
    let d = diagnostics_for(
        r#"<?php
            trait T {
                #[\Override]
                public function foo(): void {}
            }
            class Standalone {
                use T;
            }
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "method.override" && x.message.contains("Standalone::foo")),
        "got: {d:?}"
    );
}

#[test]
fn an_override_attribute_implementing_a_trait_s_own_abstract_method_is_never_flagged() {
    // Real, previously-live bug found via this project's own corpus
    // (`Psl\RandomSequence\MersenneTwisterSequence::twist()`): a class's
    // own concrete method satisfying a *trait's own* abstract method
    // declaration is a real override too, confirmed directly via
    // `php -r` - `lookup_ancestor_method_with_owner` only ever walks
    // extends/implements, never `uses`, so this needed its own check.
    let d = diagnostics_for(
        r#"<?php
            trait T {
                abstract protected function twist(): int;
            }
            class C {
                use T;
                #[\Override]
                protected function twist(): int { return 1; }
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "method.override"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn an_override_attribute_with_an_unresolved_interface_in_the_implements_chain_is_never_flagged() {
    // Real, previously-live bug found via this project's own corpus
    // (`CycloneDX\Composer\Plugin::activate()` implementing a real
    // `Composer\Plugin\PluginInterface` this corpus doesn't vendor a
    // copy of at all): "no matching parent method exists" can't be
    // confidently claimed when part of the ancestor chain is a name this
    // registry has no declaration for - "don't guess wrong", the same
    // rule this project applies elsewhere.
    let d = diagnostics_for(
        r#"<?php
            class Impl implements TotallyUnknownInterface {
                #[\Override]
                public function activate(): void {}
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "method.override"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn an_override_attribute_with_an_unresolved_parent_class_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Impl extends TotallyUnknownBaseClass {
                #[\Override]
                public function configure(): void {}
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "method.override"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn an_override_attribute_with_a_real_but_still_wrong_ancestor_is_still_flagged() {
    // Guards against the two false-positive fixes above becoming so
    // lenient they stop catching the genuine case: a fully-resolved
    // ancestor chain that simply doesn't declare the method at all must
    // still be flagged.
    let d = diagnostics_for(
        r#"<?php
            class Base {
                public function foo(): void {}
            }
            class Child extends Base {
                #[\Override]
                public function bar(): void {}
            }
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "method.override" && x.message.contains("Child::bar")),
        "got: {d:?}"
    );
}
