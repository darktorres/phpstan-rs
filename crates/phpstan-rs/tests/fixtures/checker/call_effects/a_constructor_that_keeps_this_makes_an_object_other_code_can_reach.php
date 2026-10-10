<?php
namespace CallEffects\Leaky;

use function PHPStan\Testing\assertType;

final class Registry
{
    /** @var list<Leaky|Helper|Calm> */
    public static array $all = [];
}

function resetAll(): void
{
    foreach (Registry::$all as $object) {
        $object->value = null;
    }
}

final class Leaky
{
    public ?int $value = null;

    public function __construct()
    {
        Registry::$all[] = $this;
    }
}

final class Helper
{
    public ?int $value = null;

    public function __construct()
    {
        $this->register();
    }

    private function register(): void
    {
        Registry::$all[] = $this;
    }
}

final class Calm
{
    public ?int $value = null;

    public function __construct()
    {
        $this->value = 0;
        $this->prepare();
    }

    private function prepare(): void
    {
        $this->value = 1;
    }
}

function makeLeaky(): Leaky
{
    return new Leaky();
}

function makeCalm(): Calm
{
    return new Calm();
}

function leaky(): void
{
    $object = new Leaky();
    $object->value = 1;
    resetAll();
    assertType('int|null', $object->value);
}

function helper(): void
{
    $object = new Helper();
    $object->value = 1;
    resetAll();
    assertType('int|null', $object->value);
}

function calm(): void
{
    $object = new Calm();
    $object->value = 1;
    resetAll();
    assertType('1', $object->value);
}

function leakyResult(): void
{
    $object = makeLeaky();
    $object->value = 1;
    resetAll();
    assertType('int|null', $object->value);
}

function calmResult(): void
{
    $object = makeCalm();
    $object->value = 1;
    resetAll();
    assertType('1', $object->value);
}

leaky();
helper();
calm();
leakyResult();
calmResult();
