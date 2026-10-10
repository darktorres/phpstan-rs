<?php
namespace CallEffects\HeldDestructors;

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

final class ArrayHolder
{
    /** @var list<Resets> */
    public array $items;

    public function __construct()
    {
        $this->items = [new Resets()];
    }
}

final class DirectHolder
{
    public function __construct(public Resets $held = new Resets())
    {
    }
}

final class Outer
{
    public function __construct(public DirectHolder $inner = new DirectHolder())
    {
    }
}

final class Node
{
    public ?Node $next = null;

    public ?Resets $payload = null;
}

class Base
{
    public ?Resets $inherited = null;
}

final class Derived extends Base
{
}

final class QuietHolder
{
    public Quiet $held;

    public function __construct()
    {
        $this->held = new Quiet();
    }
}

final class StaticHolder
{
    public static ?Resets $shared = null;
}

final class UntypedHolder
{
    /** @var mixed */
    public $anything;
}

function anObjectHoldingAnArrayDies(): void
{
    $holder = new ArrayHolder();
}

function theCallerSeesTheArrayHolderDie(): void
{
    Flag::$value = null;
    anObjectHoldingAnArrayDies();
    assertType('int|null', Flag::$value);
}

function anObjectHoldingAnObjectDies(): void
{
    $holder = new DirectHolder();
}

function theCallerSeesTheDirectHolderDie(): void
{
    Flag::$value = null;
    anObjectHoldingAnObjectDies();
    assertType('int|null', Flag::$value);
}

function aChainOfHoldersDies(): void
{
    $outer = new Outer();
}

function theCallerSeesTheChainDie(): void
{
    Flag::$value = null;
    aChainOfHoldersDies();
    assertType('int|null', Flag::$value);
}

function aSelfReferencingClassDies(): void
{
    $node = new Node();
}

function theCallerSeesTheNodeDie(): void
{
    Flag::$value = null;
    aSelfReferencingClassDies();
    assertType('int|null', Flag::$value);
}

function anInheritedPropertyHoldsAnObject(): void
{
    $derived = new Derived();
}

function theCallerSeesTheInheritedHolderDie(): void
{
    Flag::$value = null;
    anInheritedPropertyHoldsAnObject();
    assertType('int|null', Flag::$value);
}

function aDiscardedHolder(): void
{
    Flag::$value = null;
    $_ = new DirectHolder();
    unset($_);
    assertType('int|null', Flag::$value);
}

function anUntypedPropertyMayHoldAnyObject(): void
{
    $holder = new UntypedHolder();
}

function theCallerSeesTheUntypedHolderDie(): void
{
    Flag::$value = null;
    anUntypedPropertyMayHoldAnyObject();
    assertType('int|null', Flag::$value);
}

function aHolderOfQuietObjectsRunsNothing(): void
{
    Flag::$value = null;
    $holder = new QuietHolder();
    assertType('null', Flag::$value);
}

function aStaticPropertyOutlivesTheObject(): void
{
    Flag::$value = null;
    $holder = new StaticHolder();
    assertType('null', Flag::$value);
}
