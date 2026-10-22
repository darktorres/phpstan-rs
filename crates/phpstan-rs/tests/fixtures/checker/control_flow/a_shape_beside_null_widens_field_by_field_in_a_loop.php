<?php

use function PHPStan\Testing\assertType;

/** @param list<string> $lines */
function f(array $lines): void
{
    $last = null;
    foreach ($lines as $line) {
        if ($last === null) {
            $last = [0, ''];
        } else {
            $last[0] += strlen($line);
        }
    }
    assertType("null|array{0: int<0, max>|float, 1: ''}", $last);
}

/** @param list<string> $lines */
function g(array $lines): void
{
    $last = [0, ''];
    foreach ($lines as $line) {
        $last[0] = $last[0] + 1;
    }
    assertType("array{0: int<0, max>|float, 1: ''}", $last);
}

f(['a', 'b']);
g(['a']);
