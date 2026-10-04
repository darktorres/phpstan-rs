<?php
namespace CallEffects\Virtual;

use function PHPStan\Testing\assertType;

final class Holder
{
    public static ?int $value = null;
}

interface Task
{
    public function run(): void;
}

final class Writes implements Task
{
    public function run(): void
    {
        Holder::$value = 1;
    }
}

final class Reads implements Task
{
    public function run(): void
    {
        echo Holder::$value;
    }
}

function throughTheInterface(Task $task): void
{
    Holder::$value = null;
    $task->run();
    assertType('int|null', Holder::$value);
}

function onTheReadingClass(Reads $task): void
{
    Holder::$value = null;
    $task->run();
    assertType('null', Holder::$value);
}

throughTheInterface(new Writes());
onTheReadingClass(new Reads());
