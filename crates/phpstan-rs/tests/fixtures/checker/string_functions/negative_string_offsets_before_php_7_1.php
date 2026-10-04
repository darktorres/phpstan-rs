<?php
use function PHPStan\Testing\assertType;

/**
 * @param non-empty-string $ne
 * @return void
 */
function read(string $s, $ne) {
    $abc = 'abc';
    assertType("''", $abc[-1]);
    assertType("''", $s[-1]);
    assertType("''", $ne[-1]);
    assertType("'a'", $abc[0]);
}

/** @return void */
function write() {
    $s = 'ab';
    $s[-1] = 'x';
    assertType("'ab'", $s);
}

read('q', 'z');
write();
