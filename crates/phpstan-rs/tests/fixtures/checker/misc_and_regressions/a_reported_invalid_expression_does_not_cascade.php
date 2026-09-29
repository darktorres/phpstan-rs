<?php
final class Foo { public int $p = 1; public function m(): int { return 1; } }
function needInt(int $x): void {}
function f(Foo $foo): void {
    needInt($undef);
    $undef->m();
    needInt($foo->nope());
    $foo->nope()->other();
    needInt($foo->missing);
    $foo->missing->m();
    echo $undef + 1;
}

$z = [] - 1;
needInt($z);
