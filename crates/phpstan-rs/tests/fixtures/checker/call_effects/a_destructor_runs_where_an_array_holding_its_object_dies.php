<?php
namespace CallEffects\ArrayDestructors;

use function PHPStan\Testing\assertType;

final class Flag
{
    public static ?int $value = null;
}

final class Resets
{
    public function __destruct()
    {
        Flag::$value = 5;
    }
}

final class Quiet
{
    public function __destruct()
    {
    }
}

/** @return list<Resets> */
function makeList(): array
{
    return [new Resets()];
}

/** @return list<Quiet> */
function makeQuietList(): array
{
    return [new Quiet()];
}

function makeCount(): int
{
    return 1;
}

function anArrayLocalDiesWithTheFunction(): void
{
    $list = [new Resets()];
}

function theCallerSeesTheArrayDie(): void
{
    Flag::$value = null;
    anArrayLocalDiesWithTheFunction();
    assertType('int|null', Flag::$value);
}

function anElementWrittenLaterDiesWithTheFunction(): void
{
    $list = [];
    $list[] = new Resets();
}

function theCallerSeesTheElementDie(): void
{
    Flag::$value = null;
    anElementWrittenLaterDiesWithTheFunction();
    assertType('int|null', Flag::$value);
}

function aNestedArrayDiesWithTheFunction(): void
{
    $list = ['a' => [new Resets()]];
}

function theCallerSeesTheNestedArrayDie(): void
{
    Flag::$value = null;
    aNestedArrayDiesWithTheFunction();
    assertType('int|null', Flag::$value);
}

function aDiscardedArrayLiteral(): void
{
    Flag::$value = null;
    [new Resets()];
    assertType('int|null', Flag::$value);
}

function anElementOfADiscardedCallResult(): void
{
    Flag::$value = null;
    makeList()[0];
    assertType('int|null', Flag::$value);
}

function aDiscardedCallResult(): void
{
    Flag::$value = null;
    makeList();
    assertType('int|null', Flag::$value);
}

function aCopyOfAListDiesWithTheFunction(): void
{
    $list = makeList();
    $copy = $list;
}

function theCallerSeesTheCopyDie(): void
{
    Flag::$value = null;
    aCopyOfAListDiesWithTheFunction();
    assertType('int|null', Flag::$value);
}

function anArrayOfQuietObjectsRunsNothing(): void
{
    Flag::$value = null;
    $list = [new Quiet()];
    makeQuietList();
    assertType('null', Flag::$value);
}

function anArrayOfNumbersRunsNothing(): void
{
    Flag::$value = null;
    $list = [1, 2, 3];
    $list[] = makeCount();
    [makeCount()];
    assertType('null', Flag::$value);
}

function aSpreadArrayDiesWithTheFunction(): void
{
    $list = [new Resets()];
    $spread = [...$list];
    $list = [];
}

function theCallerSeesTheSpreadDie(): void
{
    Flag::$value = null;
    aSpreadArrayDiesWithTheFunction();
    assertType('int|null', Flag::$value);
}

function aBranchMakesTheArray(bool $flag): void
{
    $list = $flag ? [new Resets()] : [];
}

function theCallerSeesTheBranchArrayDie(): void
{
    Flag::$value = null;
    aBranchMakesTheArray(true);
    assertType('int|null', Flag::$value);
}

function aUnionAddsElements(): void
{
    $list = [1];
    $list += [new Resets()];
}

function theCallerSeesTheUnionDie(): void
{
    Flag::$value = null;
    aUnionAddsElements();
    assertType('int|null', Flag::$value);
}

function aCoalesceStoresAnElement(string $key): void
{
    $list = [];
    $list[$key] ??= new Resets();
}

function theCallerSeesTheCoalescedElementDie(): void
{
    Flag::$value = null;
    aCoalesceStoresAnElement('k');
    assertType('int|null', Flag::$value);
}

function aPropertyHoldsTheObject(): void
{
    $holder = new \stdClass();
    $holder->items = [new Resets()];
}

function theCallerSeesThePropertyObjectDie(): void
{
    Flag::$value = null;
    aPropertyHoldsTheObject();
    assertType('int|null', Flag::$value);
}

/** @param list<Resets> $given */
function aTypedParameterArrayIsCopied(array $given): void
{
    $copy = $given;
}

/** @param list<Resets> $given */
function theCallerSeesTheTypedCopyDie(array $given): void
{
    Flag::$value = null;
    aTypedParameterArrayIsCopied($given);
    assertType('int|null', Flag::$value);
}

/** @param list<Quiet> $given */
function aTypedQuietParameterArrayIsCopied(array $given): void
{
    Flag::$value = null;
    $copy = $given;
    assertType('null', Flag::$value);
}

/** @param list<list<Resets>> $given */
function aNestedTypedParameterArrayIsCopied(array $given): void
{
    $copy = $given;
}

/** @param list<list<Resets>> $given */
function theCallerSeesTheNestedTypedCopyDie(array $given): void
{
    Flag::$value = null;
    aNestedTypedParameterArrayIsCopied($given);
    assertType('int|null', Flag::$value);
}

function anArrayOverwrittenInTheChecker(): void
{
    $list = [new Resets()];
    Flag::$value = null;
    $list = [];
    assertType('int|null', Flag::$value);
}

function aClosureStoresIntoASharedArray(): void
{
    $list = [];
    $add = function () use (&$list): void {
        $list[] = new Resets();
    };
    $add();
}

function theCallerSeesTheClosureArrayDie(): void
{
    Flag::$value = null;
    aClosureStoresIntoASharedArray();
    assertType('int|null', Flag::$value);
}

function aBuiltinFillsAnArrayByReference(): void
{
    $list = [];
    array_push($list, new Resets());
}

function theCallerSeesTheBuiltinFilledArrayDie(): void
{
    Flag::$value = null;
    aBuiltinFillsAnArrayByReference();
    assertType('int|null', Flag::$value);
}

function anArrayIsReferencedByAnotherVariable(): void
{
    $list = [new Resets()];
    $alias = &$list;
}

function theCallerSeesTheReferencedArrayDie(): void
{
    Flag::$value = null;
    anArrayIsReferencedByAnotherVariable();
    assertType('int|null', Flag::$value);
}

function aForeachBindsAnElementByReference(): void
{
    $list = [new Resets()];
    foreach ($list as &$item) {
    }
}

function theCallerSeesTheForeachArrayDie(): void
{
    Flag::$value = null;
    aForeachBindsAnElementByReference();
    assertType('int|null', Flag::$value);
}

function aForeachByReferenceOverQuietObjects(): void
{
    $list = [new Quiet()];
    foreach ($list as &$item) {
    }
}

function theCallerSeesNothingRunForQuietObjects(): void
{
    Flag::$value = null;
    aForeachByReferenceOverQuietObjects();
    assertType('null', Flag::$value);
}

function aForeachByReferenceStoresAnObject(): void
{
    $list = [1];
    foreach ($list as &$item) {
        $item = new Resets();
    }
}

function theCallerSeesTheStoredObjectDie(): void
{
    Flag::$value = null;
    aForeachByReferenceStoresAnObject();
    assertType('int|null', Flag::$value);
}

function aForeachByReferenceOverNumbers(): void
{
    $copy = [1, 2];
    foreach ($copy as &$number) {
        $number = 3;
    }
}

function theCallerSeesNothingRunForNumbers(): void
{
    Flag::$value = null;
    aForeachByReferenceOverNumbers();
    assertType('null', Flag::$value);
}

/**
 * @param array<int, Resets> $list
 */
function aForeachByReferenceOverAParameter(array $list): void
{
    foreach ($list as &$item) {
        $item = 1;
    }
}

function theCallerSeesTheElementsOfAParameterDie(): void
{
    Flag::$value = null;
    aForeachByReferenceOverAParameter([new Resets()]);
    assertType('int|null', Flag::$value);
}

/**
 * @param array<int, int> $list
 */
function anAppendToAParameterArrayDestroysNothing(array &$list): void
{
    $list[] = 1;
}

function theCallerSeesNothingRunForAnAppend(): void
{
    $numbers = [1];
    Flag::$value = null;
    anAppendToAParameterArrayDestroysNothing($numbers);
    assertType('null', Flag::$value);
}

function aLoopOverwritesWhatItAppended(): void
{
    $list = [1];
    foreach ($list as &$item) {
        if (count($list) < 3) {
            $list[] = new Resets();
        }
        $item = 2;
    }
}

function theCallerSeesTheAppendedObjectDie(): void
{
    Flag::$value = null;
    aLoopOverwritesWhatItAppended();
    assertType('int|null', Flag::$value);
}
