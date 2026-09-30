<?php
use function PHPStan\Testing\assertType;

final class Foo {}

/**
 * @template T
 * @param T $expected
 * @phpstan-assert =T $actual
 */
function same(mixed $expected, mixed $actual): void
{
    if ($expected !== $actual) {
        throw new LogicException('differ');
    }
}

/**
 * @template T
 * @param T $expected
 * @phpstan-assert !=T $actual
 */
function notSame(mixed $expected, mixed $actual): void
{
    if ($expected === $actual) {
        throw new LogicException('same');
    }
}

/**
 * @template T of object
 * @param class-string<T> $class
 * @phpstan-assert iterable<T> $items
 */
function allOf(string $class, iterable $items): void
{
}

/**
 * @template T of object
 * @param class-string<T> $class
 * @phpstan-assert =T $actual
 */
function instanceOf(string $class, mixed $actual): void
{
    if (!$actual instanceof $class) {
        throw new LogicException('not');
    }
}

/**
 * @param class-string<Foo> $className
 * @param iterable<mixed> $items
 */
function f(int $n, ?string $s, mixed $o, string $className, iterable $items, mixed $p): void {
    same(1, $n);
    assertType('1', $n);
    notSame(null, $s);
    assertType('string', $s);
    allOf(Foo::class, $items);
    assertType('iterable<Foo>', $items);
    instanceOf(Foo::class, $o);
    assertType('Foo', $o);
    instanceOf($className, $p);
    assertType('Foo', $p);
}

f(1, 'a', new Foo(), Foo::class, [new Foo()], new Foo());
