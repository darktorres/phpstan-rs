<?php
use function PHPStan\Testing\assertType;
function f(): void {
    $a = ['x' => 1];
    $a[] = 2;
    assertType("array{x: 1, 0: 2}", $a);
    $b = [1, 2, 3];
    unset($b[2]);
    $b[] = 9;
    assertType('array{0: 1, 1: 2, 3: 9}', $b);
    $c = [];
    $c[] = 'a';
    $c[] = 'b';
    assertType("array{0: 'a', 1: 'b'}", $c);
    $d = [];
    foreach ([1, 2] as $v) { $d[] = $v; }
    assertType('non-empty-list<1|2>', $d);
}
