<?php
namespace App;

class Foo {}

function is_string(mixed $x): bool { return true; }

function f(object $o, mixed $x): void {
    if (is_a($o, 'App\Foo')) {
        \PHPStan\Testing\assertType('App\Foo', $o);
    }
    if (\is_a($o, Foo::class)) {
        \PHPStan\Testing\assertType('App\Foo', $o);
    }
    if (is_string($x)) {
        \PHPStan\Testing\assertType('mixed', $x);
    }
    if (\is_string($x)) {
        \PHPStan\Testing\assertType('string', $x);
    }
}
