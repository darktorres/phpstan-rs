<?php
use function PHPStan\Testing\assertType;
final class Holder { public static ?int $value = null; }
final class Loud { public function run(): void { Holder::$value = 1; } }
function f(?Loud $x): void {
    $r = &$x;
    if (is_null($x)) {
        $r = new Loud();
        Holder::$value = null;
        $x->run();
        assertType('int|null', Holder::$value);
        assertType('Loud', $x);
    }
}
