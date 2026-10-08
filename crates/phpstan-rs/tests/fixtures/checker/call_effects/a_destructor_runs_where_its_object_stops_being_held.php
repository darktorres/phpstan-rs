<?php
namespace CallEffects\Destructors;

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

function theValueIsUnset(): void
{
    $held = new Resets();
    Flag::$value = null;
    unset($held);
    assertType('int|null', Flag::$value);
}

function theValueIsOverwritten(): void
{
    $held = new Resets();
    Flag::$value = null;
    $held = null;
    assertType('int|null', Flag::$value);
}

function holdsAndDrops(): void
{
    $held = new Resets();
}

function theCalleeDropsIt(): void
{
    Flag::$value = null;
    holdsAndDrops();
    assertType('int|null', Flag::$value);
}

function aDestructorThatWritesNothingKeepsTheFact(): void
{
    $held = new Quiet();
    Flag::$value = null;
    unset($held);
    assertType('null', Flag::$value);
}

function aCalleeWithAQuietDestructorKeepsTheFact(): void
{
    Flag::$value = null;
    $held = new Quiet();
    $held = null;
    assertType('null', Flag::$value);
}
