<?php
namespace CallEffects\ByRefBuiltins;

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

/** @param array<int, mixed> $list */
function sort(array &$list): void
{
    $list = [new Resets()];
}

function aSortedArrayOfQuietObjects(): void
{
    $l = [new Quiet(), new Quiet()]; \sort($l);
}

function theCallerSees_aSortedArrayOfQuietObjects(): void
{
    Flag::$value = null;
    aSortedArrayOfQuietObjects();
    assertType('null', Flag::$value);
}

function aSortedArrayOfLoudObjects(): void
{
    $l = [new Resets()]; \sort($l);
}

function theCallerSees_aSortedArrayOfLoudObjects(): void
{
    Flag::$value = null;
    aSortedArrayOfLoudObjects();
    assertType('int|null', Flag::$value);
}

function aShuffledArrayOfQuietObjects(): void
{
    $l = [new Quiet()]; shuffle($l);
}

function theCallerSees_aShuffledArrayOfQuietObjects(): void
{
    Flag::$value = null;
    aShuffledArrayOfQuietObjects();
    assertType('null', Flag::$value);
}

function anArrayPopOfLoudObjects(): void
{
    $l = [new Resets()]; array_pop($l);
}

function theCallerSees_anArrayPopOfLoudObjects(): void
{
    Flag::$value = null;
    anArrayPopOfLoudObjects();
    assertType('int|null', Flag::$value);
}

function aUserSortedArrayOfQuietObjects(): void
{
    $l = [new Quiet()]; usort($l, fn($a, $b) => 0);
}

function theCallerSees_aUserSortedArrayOfQuietObjects(): void
{
    Flag::$value = null;
    aUserSortedArrayOfQuietObjects();
    assertType('null', Flag::$value);
}

function aUserSortedArrayOfLoudObjects(): void
{
    $l = [new Resets()]; usort($l, fn($a, $b) => 0);
}

function theCallerSees_aUserSortedArrayOfLoudObjects(): void
{
    Flag::$value = null;
    aUserSortedArrayOfLoudObjects();
    assertType('int|null', Flag::$value);
}

function aMatchIntoAnUndefinedVariable(): void
{
    preg_match('/x/', 'y', $m);
}

function theCallerSees_aMatchIntoAnUndefinedVariable(): void
{
    Flag::$value = null;
    aMatchIntoAnUndefinedVariable();
    assertType('null', Flag::$value);
}

function aMatchOverAnObject(): void
{
    $m = new Resets(); preg_match('/x/', 'y', $m);
}

function theCallerSees_aMatchOverAnObject(): void
{
    Flag::$value = null;
    aMatchOverAnObject();
    assertType('int|null', Flag::$value);
}

function aMatchOverAnArrayOfQuietObjects(): void
{
    $m = [new Quiet()]; preg_match('/x/', 'y', $m);
}

function theCallerSees_aMatchOverAnArrayOfQuietObjects(): void
{
    Flag::$value = null;
    aMatchOverAnArrayOfQuietObjects();
    assertType('null', Flag::$value);
}

function aMatchOverAnArrayOfLoudObjects(): void
{
    $m = [new Resets()]; preg_match('/x/', 'y', $m);
}

function theCallerSees_aMatchOverAnArrayOfLoudObjects(): void
{
    Flag::$value = null;
    aMatchOverAnArrayOfLoudObjects();
    assertType('int|null', Flag::$value);
}

function aSplicedArrayOfLoudObjects(): void
{
    $l = [1]; array_splice($l, 0, 0, [new Resets()]);
}

function theCallerSees_aSplicedArrayOfLoudObjects(): void
{
    Flag::$value = null;
    aSplicedArrayOfLoudObjects();
    assertType('int|null', Flag::$value);
}

function aSplicedObject(): void
{
    $l = [1]; array_splice($l, 0, 0, new Resets());
}

function theCallerSees_aSplicedObject(): void
{
    Flag::$value = null;
    aSplicedObject();
    assertType('int|null', Flag::$value);
}

function aReplaceCount(): void
{
    str_replace('a', 'b', 'c', $count);
}

function theCallerSees_aReplaceCount(): void
{
    Flag::$value = null;
    aReplaceCount();
    assertType('null', Flag::$value);
}

function aSettypeToInt(): void
{
    $v = new Resets(); settype($v, 'int');
}

function theCallerSees_aSettypeToInt(): void
{
    Flag::$value = null;
    aSettypeToInt();
    assertType('int|null', Flag::$value);
}

function aSettypeOfQuietArrayToString(): void
{
    $v = [new Quiet()]; settype($v, 'string');
}

function theCallerSees_aSettypeOfQuietArrayToString(): void
{
    Flag::$value = null;
    aSettypeOfQuietArrayToString();
    assertType('null', Flag::$value);
}

function anExecOutputAppended(): void
{
    $o = [new Resets()]; exec('x', $o);
}

function theCallerSees_anExecOutputAppended(): void
{
    Flag::$value = null;
    anExecOutputAppended();
    assertType('int|null', Flag::$value);
}

function anExecOutputOfQuietObjects(): void
{
    $o = [new Quiet()]; exec('x', $o, $code);
}

function theCallerSees_anExecOutputOfQuietObjects(): void
{
    Flag::$value = null;
    anExecOutputOfQuietObjects();
    assertType('null', Flag::$value);
}

function aScanOverAnObject(): void
{
    $a = new Resets(); sscanf('1 2', '%d %d', $a, $b);
}

function theCallerSees_aScanOverAnObject(): void
{
    Flag::$value = null;
    aScanOverAnObject();
    assertType('int|null', Flag::$value);
}

/** @param array<int, mixed> $out */
function aByReferenceParameterIsMatchedOver(array &$out): void
{
    preg_match('/x/', 'y', $out);
}

function theCallerSeesNothingRunForQuietObjectsMatchedOver(): void
{
    $kept = [new Quiet()];
    Flag::$value = null;
    aByReferenceParameterIsMatchedOver($kept);
    assertType('null', Flag::$value);
}
