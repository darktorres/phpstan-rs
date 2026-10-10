<?php
namespace CallEffects\StaticShapes;

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

final class Holder
{
    public function rememberLoud(): void
    {
        static $kept = null;
        $kept = new Resets();
    }

    public function rememberQuiet(): void
    {
        static $kept = null;
        $kept = new Quiet();
    }

    public static function rememberStaticallyQuiet(): void
    {
        static $kept = [];
        $kept[] = new Quiet();
    }
}

function aStaticInAMethodHoldsLoud(): void
{
    (new Holder())->rememberLoud();
}

function theCallerSees_aStaticInAMethodHoldsLoud(): void
{
    Flag::$value = null;
    aStaticInAMethodHoldsLoud();
    assertType('int|null', Flag::$value);
}

function aStaticInAMethodHoldsQuiet(): void
{
    (new Holder())->rememberQuiet();
    Holder::rememberStaticallyQuiet();
}

function theCallerSees_aStaticInAMethodHoldsQuiet(): void
{
    Flag::$value = null;
    aStaticInAMethodHoldsQuiet();
    assertType('null', Flag::$value);
}

function aStaticInAClosureHoldsLoud(): void
{
    $f = function (): void {
        static $kept = null;
        $kept = new Resets();
    };
    $f();
}

function theCallerSees_aStaticInAClosureHoldsLoud(): void
{
    Flag::$value = null;
    aStaticInAClosureHoldsLoud();
    assertType('int|null', Flag::$value);
}

function aStaticInAClosureHoldsQuiet(): void
{
    $f = function (): void {
        static $kept = null;
        $kept = new Quiet();
    };
    $f();
}

function theCallerSees_aStaticInAClosureHoldsQuiet(): void
{
    Flag::$value = null;
    aStaticInAClosureHoldsQuiet();
    assertType('null', Flag::$value);
}

function aStaticInBranchHoldsLoud(): void
{
    if (random_int(0, 1) === 1) {
        static $kept = null;
        $kept = new Resets();
    }
}

function theCallerSees_aStaticInBranchHoldsLoud(): void
{
    Flag::$value = null;
    aStaticInBranchHoldsLoud();
    assertType('int|null', Flag::$value);
}

function aStaticInBranchHoldsQuiet(): void
{
    if (random_int(0, 1) === 1) {
        static $kept = null;
        $kept = new Quiet();
    }
}

function theCallerSees_aStaticInBranchHoldsQuiet(): void
{
    Flag::$value = null;
    aStaticInBranchHoldsQuiet();
    assertType('null', Flag::$value);
}

function aStaticAliasIsAppendedAQuietObject(): void
{
    static $kept = [];
    $alias = &$kept;
    $alias[] = new Quiet();
}

function theCallerSees_aStaticAliasIsAppendedAQuietObject(): void
{
    Flag::$value = null;
    aStaticAliasIsAppendedAQuietObject();
    assertType('null', Flag::$value);
}

function aStaticAliasIsAppendedALoudObject(): void
{
    static $kept = [];
    $alias = &$kept;
    $alias[] = new Resets();
    $kept = [];
}

function theCallerSees_aStaticAliasIsAppendedALoudObject(): void
{
    Flag::$value = null;
    aStaticAliasIsAppendedALoudObject();
    assertType('int|null', Flag::$value);
}

function aStaticInitialisedWithAConstantArray(): void
{
    static $kept = [1, 2];
    $kept[] = 3;
}

function theCallerSees_aStaticInitialisedWithAConstantArray(): void
{
    Flag::$value = null;
    aStaticInitialisedWithAConstantArray();
    assertType('null', Flag::$value);
}

function aStaticMatchedOverAQuietArray(): void
{
    static $m = null;
    $m = [new Quiet()];
    preg_match('/x/', 'y', $m);
}

function theCallerSees_aStaticMatchedOverAQuietArray(): void
{
    Flag::$value = null;
    aStaticMatchedOverAQuietArray();
    assertType('null', Flag::$value);
}

function aStaticMatchedOverALoudArray(): void
{
    static $m = null;
    $m = [new Resets()];
    preg_match('/x/', 'y', $m);
}

function theCallerSees_aStaticMatchedOverALoudArray(): void
{
    Flag::$value = null;
    aStaticMatchedOverALoudArray();
    assertType('int|null', Flag::$value);
}

function aStaticSortedArrayOfQuietObjects(): void
{
    static $kept = [];
    $kept[] = new Quiet();
    sort($kept);
}

function theCallerSees_aStaticSortedArrayOfQuietObjects(): void
{
    Flag::$value = null;
    aStaticSortedArrayOfQuietObjects();
    assertType('null', Flag::$value);
}

function twoStaticsHoldQuietObjects(): void
{
    static $a = null;
    static $b = [];
    $a = new Quiet();
    $b[] = new Quiet();
}

function theCallerSees_twoStaticsHoldQuietObjects(): void
{
    Flag::$value = null;
    twoStaticsHoldQuietObjects();
    assertType('null', Flag::$value);
}
