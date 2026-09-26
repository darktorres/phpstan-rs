<?php
function logIt(): void {}

function a(int|string $x): void
{
    if (is_string($x)) {
        $flag = true;
        logIt();
    } else {
        $flag = false;
    }
    if ($flag) {
        \PHPStan\Testing\assertType('string', $x);
    }
    if (!$flag) {
        \PHPStan\Testing\assertType('int', $x);
    }
}

/** @param list<int> $log */
function b(int|string $x, array $log): void
{
    $flag = false;
    if (is_string($x)) {
        $flag = true;
        $log[] = 1;
    }
    if ($flag) {
        \PHPStan\Testing\assertType('string', $x);
    }
}

function c(int|string $x): void
{
    $flag = false;
    if (is_string($x)) {
        $flag = true;
    }
    $x = 1;
    if ($flag) {
        \PHPStan\Testing\assertType('1', $x);
    }
}
