<?php
namespace CallEffects\LateStatic;

use function PHPStan\Testing\assertType;

class Model
{
    public static ?int $id = null;

    public static function reset(): void
    {
        static::$id = 1;
    }
}

final class User extends Model
{
    public static ?int $id = null;
}

function check(): void
{
    User::$id = null;
    User::reset();
    assertType('int|null', User::$id);
}

check();
