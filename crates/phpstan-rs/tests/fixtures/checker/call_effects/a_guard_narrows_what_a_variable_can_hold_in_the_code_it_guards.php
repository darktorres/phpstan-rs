<?php
namespace CallEffects\Guards;

use function PHPStan\Testing\assertType;

final class Holder
{
    public static ?int $value = null;
}

final class Loud
{
    public function run(): void
    {
        Holder::$value = 1;
    }
}

final class Quiet
{
    public function run(): void
    {
    }

    public function check(): bool
    {
        return true;
    }
}

/** @implements \IteratorAggregate<int, int> */
final class LoudList implements \IteratorAggregate
{
    public function getIterator(): \Iterator
    {
        Holder::$value = 1;
        return new \ArrayIterator([]);
    }
}

function insideAnInstanceofBranch(mixed $x): void
{
    if ($x instanceof Quiet) {
        $x->run();
    }
}

function insideAnInstanceofOfALoudClass(mixed $x): void
{
    if ($x instanceof Loud) {
        $x->run();
    }
}

function afterAnEarlyExit(mixed $x): void
{
    if (!is_array($x)) {
        return;
    }
    foreach ($x as $item) {
    }
}

function inTheElseOfAnObjectTest(mixed $x): void
{
    if (is_object($x)) {
        return;
    }
    foreach ($x as $item) {
    }
}

function rightOfAnAnd(mixed $x): void
{
    $x instanceof Quiet && $x->run();
}

function inATernary(mixed $x): void
{
    $x instanceof Quiet ? $x->run() : null;
}

function afterAnIsAString(mixed $x): void
{
    if (is_string($x)) {
        echo $x;
    }
}

function writtenInsideTheBranch(mixed $x, mixed $other): void
{
    if ($x instanceof Quiet) {
        $x = $other;
        $x->run();
    }
}

function takenByReference(mixed &$x): void
{
    if ($x instanceof Quiet) {
        $x->run();
    }
}

function guardedByANegatedOr(mixed $x): void
{
    if (!($x instanceof Quiet) || $x->check()) {
        return;
    }
}

function theElseOfAnInstanceof(mixed $x): void
{
    if ($x instanceof Quiet) {
        return;
    }
    $x->run();
}

function theCheck(): void
{
    Holder::$value = null;
    insideAnInstanceofBranch(new Quiet());
    assertType('null', Holder::$value);
}

function theLoudCheck(): void
{
    Holder::$value = null;
    insideAnInstanceofOfALoudClass(new Loud());
    assertType('int|null', Holder::$value);
}

function theEarlyExitCheck(): void
{
    Holder::$value = null;
    afterAnEarlyExit([1]);
    assertType('null', Holder::$value);
}

function theObjectTestCheck(): void
{
    Holder::$value = null;
    inTheElseOfAnObjectTest([1]);
    assertType('null', Holder::$value);
}

function theAndCheck(): void
{
    Holder::$value = null;
    rightOfAnAnd(new Quiet());
    assertType('null', Holder::$value);
}

function theTernaryCheck(): void
{
    Holder::$value = null;
    inATernary(new Quiet());
    assertType('null', Holder::$value);
}

function theStringCheck(): void
{
    Holder::$value = null;
    afterAnIsAString('a');
    assertType('null', Holder::$value);
}

function theWrittenCheck(): void
{
    Holder::$value = null;
    writtenInsideTheBranch(new Quiet(), new Loud());
    assertType('int|null', Holder::$value);
}

function theReferenceCheck(): void
{
    Holder::$value = null;
    $x = new Quiet();
    takenByReference($x);
    assertType('int|null', Holder::$value);
}

function theNegatedOrCheck(): void
{
    Holder::$value = null;
    guardedByANegatedOr(new Quiet());
    assertType('null', Holder::$value);
}

function theElseOfAnInstanceofCheck(): void
{
    Holder::$value = null;
    theElseOfAnInstanceof(new Loud());
    assertType('int|null', Holder::$value);
}

final class Counter
{
    public int $count = 0;
}

final class Tally
{
    public int $count = 0;
}

function writesThroughAGuardedVariable(mixed $x): void
{
    if ($x instanceof Counter) {
        $x->count = 5;
    }
}

function theWriteCheck(): void
{
    $tally = new Tally();
    $tally->count = 3;
    writesThroughAGuardedVariable(new Counter());
    assertType('3', $tally->count);
}
