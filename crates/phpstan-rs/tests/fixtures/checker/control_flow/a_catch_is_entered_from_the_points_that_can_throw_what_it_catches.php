<?php
class AppError extends \Exception {}

function mayThrow(): int
{
    return 1;
}

function cannotThrow(): void
{
    try {
        $x = 1;
    } catch (\Exception $e) {
        $x = null;
    }
    \PHPStan\Testing\assertType('1', $x);
}

function unhandledMatch(int $i): void
{
    $r = null;
    try {
        $r = match ($i) {
            1 => 'one',
            2 => 'two',
        };
    } catch (\UnhandledMatchError $e) {
        \PHPStan\Testing\assertType('null', $r);
    }
    \PHPStan\Testing\assertType("'one'|'two'|null", $r);
}

function exhaustiveMatch(int $i): void
{
    $r = null;
    try {
        $r = match ($i) {
            1 => 'one',
            default => 'other',
        };
    } catch (\UnhandledMatchError $e) {
        $r = 'never';
    }
    \PHPStan\Testing\assertType("'one'|'other'", $r);
}

function betweenCalls(): void
{
    $s = 'start';
    try {
        $s = 'mid';
        $n = mayThrow();
        $s = 'end';
    } catch (\Exception $e) {
        \PHPStan\Testing\assertType("'mid'", $s);
    }
    \PHPStan\Testing\assertType("'end'|'mid'", $s);
}

function aBuiltinCanRunUserCode(\ReflectionMethod $m, object $o): void
{
    $caught = null;
    try {
        $m->invoke($o);
    } catch (AppError $e) {
        $caught = $e;
    }
    \PHPStan\Testing\assertType('null|AppError', $caught);
}

/** @throws void */
function cannotThrowByContract(): int
{
    return 1;
}

function aThrowsVoidCallCannotThrow(): void
{
    try {
        $x = cannotThrowByContract();
    } catch (\Exception $e) {
        $x = null;
    }
    \PHPStan\Testing\assertType('int', $x);
}

function userCallMayThrow(): int
{
    try {
        $x = mayThrow();
    } catch (\Exception $e) {
    }
    return $x;
}

function division(int $a, int $b): void
{
    $q = null;
    try {
        $q = $a / $b;
    } catch (\DivisionByZeroError $e) {
        \PHPStan\Testing\assertType('null', $q);
    }
}

function escapesTheInnerCatch(): void
{
    $v = 0;
    try {
        try {
            $v = 1;
            throw new AppError();
        } catch (\RuntimeException $e) {
            $v = 2;
        }
    } catch (AppError $e) {
        \PHPStan\Testing\assertType('1', $v);
    }
}
