<?php
use function PHPStan\Testing\assertType;
class Foo {}
class Child extends Foo {}
final class Other {}
class Open {}
/** @return ($x is Foo ? int : string) */
function f(object $x): int|string { return 1; }
/**
 * @template T of object
 * @param T $x
 * @return (T is Foo ? int : string)
 */
function g(object $x): int|string { return 1; }
function test(): void {
    assertType('int', f(new Child()));
    assertType('string', f(new Other()));
    assertType('string', f(new Open()));
    assertType('int', g(new Child()));
    assertType('string', g(new Other()));
}
