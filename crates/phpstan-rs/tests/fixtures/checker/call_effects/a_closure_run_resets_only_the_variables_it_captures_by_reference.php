<?php
namespace CallEffects\CapturedByReference;

use function PHPStan\Testing\assertType;

function onlyTheClosureThatRunsResetsItsVariables(): void
{
    $a = 0;
    $b = 0;
    $first = function () use (&$a): void {
        $a = (int) ($_GET['a'] ?? 0);
    };
    $second = function () use (&$b): void {
        $b = (int) ($_GET['b'] ?? 0);
    };
    $b = 5;
    $first();
    assertType('5', $b);
    $second();
    assertType('int', $b);
}

function aClosureHandedToACallerResetsItsVariables(): void
{
    $total = 0;
    $add = function () use (&$total): void {
        $total = (int) ($_GET['total'] ?? 0);
    };
    $total = 7;
    array_map($add, [1]);
    assertType('int', $total);
}
