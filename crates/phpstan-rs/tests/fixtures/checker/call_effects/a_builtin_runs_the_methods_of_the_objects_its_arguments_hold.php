<?php
namespace CallEffects\ElementsAreTyped;

use function PHPStan\Testing\assertType;

final class Holder
{
    public static ?int $value = null;
}

final class Loud implements \JsonSerializable
{
    public function jsonSerialize(): mixed
    {
        Holder::$value = 1;
        return 1;
    }
}

function arraysOfScalars(): void
{
    Holder::$value = null;
    json_encode([1, 'a', ['b' => 2.5]]);
    implode(',', ['a', 'b']);
    serialize([1, 2]);
    file_put_contents('/tmp/a-file', 'data');
    assertType('null', Holder::$value);
}

function anArrayHoldingAnObject(Loud $loud): void
{
    Holder::$value = null;
    json_encode([$loud]);
    assertType('int|null', Holder::$value);
}

/** @param array<int, Loud> $louds */
function anArrayOfObjects(array $louds): void
{
    Holder::$value = null;
    json_encode(['x' => $louds]);
    assertType('int|null', Holder::$value);
}

/** @param array<int, mixed> $unknown */
function anArrayOfAnything(array $unknown): void
{
    Holder::$value = null;
    json_encode($unknown);
    assertType('int|null', Holder::$value);
}

function aConstructorKeepsWhatItIsGiven(): void
{
    Holder::$value = null;
    new \ArrayObject([]);
    new \ArrayIterator([1, 2]);
    assertType('null', Holder::$value);
}

function aProgramPHPStartsRunsNoCode(): void
{
    Holder::$value = null;
    shell_exec('true');
    exec('true');
    assertType('null', Holder::$value);
}

$handle = fopen('php://memory', 'r');
if ($handle === false) {
    exit(1);
}
shell_exec('true');
json_encode([]);
assertType('resource', $handle);
