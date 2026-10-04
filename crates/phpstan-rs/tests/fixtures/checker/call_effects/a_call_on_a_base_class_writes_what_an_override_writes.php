<?php
namespace CallEffects\Override;

use function PHPStan\Testing\assertType;

final class Holder
{
    public static ?int $value = null;
}

class Task
{
    public function run(): void
    {
    }
}

final class Writes extends Task
{
    public function run(): void
    {
        Holder::$value = 1;
    }
}

function check(Task $task): void
{
    Holder::$value = null;
    $task->run();
    assertType('int|null', Holder::$value);
}

check(new Writes());
