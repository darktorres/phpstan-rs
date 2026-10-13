<?php
namespace CallEffects\SuperglobalElements;

use function PHPStan\Testing\assertType;

final class Holder
{
    public static ?int $value = null;
}

final class User
{
    public function save(): void
    {
        Holder::$value = 1;
    }
}

/** @implements \ArrayAccess<string, mixed> */
final class Bag implements \ArrayAccess
{
    public function offsetExists(mixed $offset): bool
    {
        return false;
    }

    public function offsetGet(mixed $offset): mixed
    {
        return null;
    }

    public function offsetSet(mixed $offset, mixed $value): void
    {
        Holder::$value = 2;
    }

    public function offsetUnset(mixed $offset): void
    {
    }
}

function login(): void
{
    $_SESSION['user'] = new User();
}

function aMethodOnASessionElementRunsTheStoredObjectsCode(): void
{
    Holder::$value = null;
    $_SESSION['user']->save();
    assertType('int|null', Holder::$value);
}

function aMethodOnAnotherSuperglobalsElementRunsIt(): void
{
    Holder::$value = null;
    $_SERVER['user']->save();
    assertType('int|null', Holder::$value);
}

function appendingToAnElementMayRunTheStoredObjectsOffsetSet(): void
{
    Holder::$value = null;
    $_SERVER['argv'][] = 'x';
    assertType('int|null', Holder::$value);
}

function theArrayItselfStillRunsNoCode(): void
{
    Holder::$value = null;
    $keys = array_keys($_SESSION);
    assertType('int|null', Holder::$value);
}
