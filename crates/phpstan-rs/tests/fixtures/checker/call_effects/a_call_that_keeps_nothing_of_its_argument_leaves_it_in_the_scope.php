<?php
namespace CallEffects\ReleasingCalls;

use function PHPStan\Testing\assertType;

final class Store
{
    /** @var list<callable> */
    public static array $callbacks = [];
    /** @var list<Box> */
    public static array $boxes = [];
}

final class Box
{
    public ?int $id = null;
}

function wild(string $name): void
{
    $name();
}

function calls(callable $callback): void
{
    $callback();
}

function viaCalls(callable $callback): void
{
    calls($callback);
}

function loops(callable $callback, int $depth): void
{
    if ($depth > 0) {
        loops($callback, $depth - 1);
    }
    $callback();
}

function stores(callable $callback): void
{
    Store::$callbacks[] = $callback;
}

function viaStores(callable $callback): void
{
    stores($callback);
}

function inspects(Box $box): int
{
    return $box->id ?? 0;
}

function keeps(Box $box): void
{
    Store::$boxes[] = $box;
}

function aBuiltinThatReleasesTheClosure(): void
{
    $count = 0;
    $bump = function () use (&$count): void {
        $count = (int) ($_GET['n'] ?? 0);
    };
    array_map($bump, [1]);
    $count = 5;
    wild('anything');
    assertType('5', $count);
}

function aBuiltinThatKeepsTheClosure(): void
{
    $count = 0;
    $bump = function () use (&$count): void {
        $count = (int) ($_GET['n'] ?? 0);
    };
    register_shutdown_function($bump);
    $count = 5;
    wild('anything');
    assertType('int', $count);
}

function aFunctionThatOnlyCallsTheClosure(): void
{
    $count = 0;
    $bump = function () use (&$count): void {
        $count = (int) ($_GET['n'] ?? 0);
    };
    calls($bump);
    $count = 5;
    wild('anything');
    assertType('5', $count);
}

function aFunctionThatPassesTheClosureToOneThatOnlyCallsIt(): void
{
    $count = 0;
    $bump = function () use (&$count): void {
        $count = (int) ($_GET['n'] ?? 0);
    };
    viaCalls($bump);
    $count = 5;
    wild('anything');
    assertType('5', $count);
}

function aFunctionThatCallsItselfWithTheClosure(): void
{
    $count = 0;
    $bump = function () use (&$count): void {
        $count = (int) ($_GET['n'] ?? 0);
    };
    loops($bump, 2);
    $count = 5;
    wild('anything');
    assertType('5', $count);
}

function aFunctionThatStoresTheClosure(): void
{
    $count = 0;
    $bump = function () use (&$count): void {
        $count = (int) ($_GET['n'] ?? 0);
    };
    stores($bump);
    $count = 5;
    wild('anything');
    assertType('int', $count);
}

function aFunctionThatPassesTheClosureToOneThatStoresIt(): void
{
    $count = 0;
    $bump = function () use (&$count): void {
        $count = (int) ($_GET['n'] ?? 0);
    };
    viaStores($bump);
    $count = 5;
    wild('anything');
    assertType('int', $count);
}

function aFunctionThatOnlyReadsTheObject(): void
{
    $box = new Box();
    inspects($box);
    $box->id = 5;
    wild('anything');
    assertType('5', $box->id);
}

function aFunctionThatKeepsTheObject(): void
{
    $box = new Box();
    keeps($box);
    $box->id = 5;
    wild('anything');
    assertType('int|null', $box->id);
}
