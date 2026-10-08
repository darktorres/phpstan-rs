<?php
namespace CallEffects\PositionalEscape;

use function PHPStan\Testing\assertType;

final class Box
{
    public ?int $id = null;
}

function wild(string $name): void
{
    $name();
}

final class Store
{
    /** @var list<Box> */
    public static array $boxes = [];
}

function register(Box $box): void
{
    Store::$boxes[] = $box;
}

function beforeTheObjectLeaves(): void
{
    $box = new Box();
    $box->id = 5;
    wild($_GET['f']);
    assertType('5', $box->id);
    register($box);
}

function afterTheObjectLeaves(): void
{
    $box = new Box();
    register($box);
    $box->id = 5;
    wild($_GET['f']);
    assertType('int|null', $box->id);
}

function inALoopThatLetsItLeave(): void
{
    $box = new Box();
    $box->id = 5;
    for ($i = 0; $i < 2; $i++) {
        wild($_GET['f']);
        assertType('int|null', $box->id);
        register($box);
    }
}

function aBranchThatLetsItLeaveComesLater(bool $leave): void
{
    $box = new Box();
    $box->id = 5;
    wild($_GET['f']);
    assertType('5', $box->id);
    if ($leave) {
        register($box);
    }
}
