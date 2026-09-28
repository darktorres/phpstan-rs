<?php
use function PHPStan\Testing\assertType;
function takesInt(int $i): void {}
function f(mixed $m, bool $b): void {
    $x = $b ? $m : null;
    assertType('mixed', $x);
    takesInt($x);
}

class C {
    /** @var array|mixed|null */
    public $p;
}
