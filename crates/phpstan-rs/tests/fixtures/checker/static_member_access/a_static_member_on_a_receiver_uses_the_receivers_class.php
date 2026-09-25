<?php
use function PHPStan\Testing\assertType;
class Foo {
    const A = 1;
    public static int $count = 0;
}
function f(Foo $o, object $any): void {
    assertType('1', $o::A);
    assertType('int', $o::$count);
    assertType('class-string<Foo>', $o::class);
    echo $o::MISSING;
    echo $any::A;
}
