<?php
use function PHPStan\Testing\assertType;
class Foo {}
function f(callable $c, string $s): void {
    assertType('true', (bool) $c);
    $g = function (): float|string { return 1; };
    assertType('1.0', $g());
    
}
