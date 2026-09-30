<?php
use function PHPStan\Testing\assertType;

interface Shape {}
final class Circle implements Shape {}
final class Square implements Shape {}

function f(mixed $x, Shape $s, Circle $c, Square $q): void {
    assertType('bool', $x instanceof Circle);
    assertType('bool', $s instanceof Circle);
    assertType('true', $c instanceof Shape);
    $known = $q instanceof Shape;
    assertType('true', $known);
}

f(new Circle(), new Square(), new Circle(), new Square());
