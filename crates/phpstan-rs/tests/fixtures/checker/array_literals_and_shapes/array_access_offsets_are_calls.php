<?php
use function PHPStan\Testing\assertType;

final class Foo {}

/**
 * @implements ArrayAccess<string, Foo>
 */
final class Bag implements ArrayAccess
{
    public ?int $count = null;

    public function offsetExists(mixed $offset): bool
    {
        return $offset === 'k';
    }

    public function offsetGet(mixed $offset): ?Foo
    {
        return $offset === 'k' ? new Foo() : null;
    }

    public function offsetSet(mixed $offset, mixed $value): void
    {
        $this->count = 1;
    }

    public function offsetUnset(mixed $offset): void
    {
    }
}

final class Plain
{
    public function offsetGet(string $offset): int
    {
        return 1;
    }
}

final class NotThere extends Exception {}

/**
 * @implements ArrayAccess<string, int>
 */
final class Throwing implements ArrayAccess
{
    public function offsetExists(mixed $offset): bool
    {
        return true;
    }

    /** @throws NotThere */
    public function offsetGet(mixed $offset): int
    {
        throw new NotThere('no');
    }

    public function offsetSet(mixed $offset, mixed $value): void
    {
    }

    public function offsetUnset(mixed $offset): void
    {
    }
}

/**
 * @implements ArrayAccess<string, int>
 */
final class Typed implements ArrayAccess
{
    public function offsetExists(mixed $offset): bool
    {
        return true;
    }

    /**
     * @param string $offset
     */
    public function offsetGet(mixed $offset): int
    {
        return 1;
    }

    public function offsetSet(mixed $offset, mixed $value): void
    {
    }

    public function offsetUnset(mixed $offset): void
    {
    }
}

function keyed(Typed $t): int {
    return $t[1];
}

function read(Bag $bag, Foo $default): void {
    assertType('Foo|null', $bag['k']);
    assertType('Foo', $bag['k'] ?? $default);
    [$a] = $bag;
    assertType('Foo|null', $a);
}

function writes(Bag $bag): void {
    if ($bag->count !== null) {
        $bag['k'] = new Foo();
        assertType('int|null', $bag->count);
    }
}

function nested(Bag $bag): void {
    $bag['k']['x'] = 1;
}

function plain(Plain $p): void {
    echo $p['k'];
}

/**
 * @throws NotThere
 */
function thrown(Throwing $t): int {
    return $t['x'];
}

/**
 * @param array<string, int> $a
 * @throws NotThere
 */
function notThrown(array $a): int {
    return $a['x'];
}

read(new Bag(), new Foo());
writes(new Bag());
