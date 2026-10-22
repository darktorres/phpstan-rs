<?php
use function PHPStan\Testing\assertType;

function next_id(): int {
    static $id = 0;
    assertType('int<0, 541784354586624000>', $id);
    return ++$id;
}

function passed(): int {
    static $n = 0;
    assertType('int', $n);
    $n++;
    sort_it($n);
    return $n;
}

function sort_it(int &$x): void {
    $x *= 2;
}

function doubled(): int {
    static $d = 1;
    assertType('int', $d);
    $d *= 2;
    return $d;
}

next_id();
next_id();
passed();
doubled();
