<?php
use function PHPStan\Testing\assertType;

/**
 * @param int<0, 5> $i
 * @param non-empty-string $ne
 */
function read(string $s, int $i, string $ne): void {
    $abc = 'abc';
    assertType("'a'", $abc[0]);
    assertType("'c'", $abc[-1]);
    assertType('string', $s[0]);
    assertType("''", $abc[5]);
    assertType("'a'|'b'|'c'|''", $abc[$i]);
    assertType('non-empty-string', $ne[0]);
}

function write(): void {
    $s = 'ab';
    $s[3] = 'x';
    assertType("'ab x'", $s);
    try {
        $t = 'ab';
        $t[0] = '';
    } catch (Error $e) {
    }
    $u = 'ab';
    $u[0] = 'xy';
    assertType("'xb'", $u);
    $v = 'ab';
    $v[-5] = 'x';
    assertType("'ab'", $v);
}

read('q', 1, 'z');
read('q', 5, 'z');
write();
