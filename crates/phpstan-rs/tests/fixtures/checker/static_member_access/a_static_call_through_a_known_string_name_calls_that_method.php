<?php
use function PHPStan\Testing\assertType;
class Foo { public static function bar(): int { return 1; } }
function f(): void {
    $name = 'bar';
    assertType('int', Foo::$name());
    assertType('int', Foo::{'bar'}());
    $missing = 'nope';
    Foo::$missing();
}
