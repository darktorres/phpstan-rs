<?php
namespace CallEffects\RegisteredWrappers;

use function PHPStan\Testing\assertType;

final class Counts
{
    public static ?int $registered = null;
    public static ?int $unregistered = null;
}

class Registered
{
    public static function register(): void
    {
        stream_wrapper_register('registered', static::class);
    }

    public function stream_open(string $path, string $mode, int $options, ?string &$openedPath): bool
    {
        Counts::$registered = 1;
        return true;
    }
}

final class Unregistered
{
    public function stream_open(string $path, string $mode, int $options, ?string &$openedPath): bool
    {
        Counts::$unregistered = 1;
        return true;
    }
}

function check(): void
{
    Counts::$registered = null;
    Counts::$unregistered = null;
    echo file_get_contents('anything');
    assertType('int|null', Counts::$registered);
    assertType('null', Counts::$unregistered);
}

Registered::register();
check();
