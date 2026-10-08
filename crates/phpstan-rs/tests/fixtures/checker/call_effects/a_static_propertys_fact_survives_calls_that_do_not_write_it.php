<?php
namespace CallEffects\StaticFacts;

use function PHPStan\Testing\assertType;

final class Service
{
    public static ?int $count = null;
    public static ?int $other = null;

    public function ping(): void
    {
    }

    public function setOther(): void
    {
        self::$other = 1;
    }

    public function setCount(): void
    {
        self::$count = null;
    }

    public function onThis(): void
    {
        if (self::$count !== null) {
            $this->ping();
            assertType('int', self::$count);
        }
    }

    public function onAnotherObject(Service $service): void
    {
        if (self::$count !== null) {
            $service->setOther();
            assertType('int', self::$count);
        }
    }

    public function onAnObjectWritingIt(Service $service): void
    {
        if (self::$count !== null) {
            $service->setCount();
            assertType('int|null', self::$count);
        }
    }

    public static function onACallableOfUnknownCode(callable $callback): void
    {
        if (self::$count !== null) {
            $callback();
            assertType('int|null', self::$count);
        }
    }
}

function aFactReadThroughTheClassName(): void
{
    if (Service::$count !== null) {
        (new Service())->ping();
        assertType('int', Service::$count);
    }
}
