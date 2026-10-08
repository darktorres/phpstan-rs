<?php
namespace CallEffects\ClosureLeaves;

use function PHPStan\Testing\assertType;

function wild(string $name): void
{
    $name();
}

final class Store
{
    /** @var list<callable> */
    public static array $callbacks = [];
}

function register(callable $callback): void
{
    Store::$callbacks[] = $callback;
}

function aClosureNeverHandedOutIsNotRunByUnknownCode(): void
{
    $count = 0;
    $bump = function () use (&$count): void {
        $count = (int) ($_GET['n'] ?? 0);
    };
    $count = 5;
    wild($_GET['f']);
    assertType('5', $count);
    $bump();
}

function aClosureHandedOutIsRunByUnknownCode(): void
{
    $count = 0;
    $bump = function () use (&$count): void {
        $count = (int) ($_GET['n'] ?? 0);
    };
    register($bump);
    $count = 5;
    wild($_GET['f']);
    assertType('int', $count);
}

function aClosureHandedOutLaterIsNotRunBeforeThat(): void
{
    $count = 0;
    $bump = function () use (&$count): void {
        $count = (int) ($_GET['n'] ?? 0);
    };
    $count = 5;
    wild($_GET['f']);
    assertType('5', $count);
    register($bump);
}
