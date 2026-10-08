<?php
namespace CallEffects\ArrowFunctionHolders;

use function PHPStan\Testing\assertType;

final class Shared
{
    public static ?int $value = null;
}

final class Quiet
{
    public function run(): void
    {
    }
}

final class Loud
{
    public function run(): void
    {
        Shared::$value = null;
    }
}

/**
 * @param list<string> $names
 */
function anArrowFunctionOverAnArrayRunsNoCode(array $names): void
{
    $join = fn(): string => implode(',', $names);
    Shared::$value = 1;
    $join();
    assertType('1', Shared::$value);
}

function anArrowFunctionOverAQuietObjectRunsOnlyIt(Quiet $worker): void
{
    $run = fn() => $worker->run();
    Shared::$value = 1;
    $run();
    assertType('1', Shared::$value);
}

function anArrowFunctionOverALoudObjectRunsIt(Loud $worker): void
{
    $run = fn() => $worker->run();
    Shared::$value = 1;
    $run();
    assertType('int|null', Shared::$value);
}

function aStoredClosureRunsWhatItCapturedNotWhatTheVariableHoldsNow(): void
{
    $worker = new Loud();
    $run = function () use ($worker): void {
        $worker->run();
    };
    $worker = new Quiet();
    Shared::$value = 1;
    $run();
    assertType('int|null', Shared::$value);
}

function anArrowFunctionOverALocalQuietObjectRunsOnlyIt(): void
{
    $worker = new Quiet();
    $run = fn() => $worker->run();
    Shared::$value = 1;
    $run();
    assertType('1', Shared::$value);
}

function aClosureTakesTheValueOfWhereItIsWrittenInALoop(): void
{
    $worker = new Quiet();
    $run = function () use ($worker): void {
        $worker->run();
    };
    for ($i = 0; $i < 2; $i++) {
        Shared::$value = 1;
        $run();
        $worker = new Loud();
        $run = function () use ($worker): void {
            $worker->run();
        };
    }
    assertType('int|null', Shared::$value);
}
