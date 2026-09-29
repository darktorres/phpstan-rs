<?php
use function PHPStan\Testing\assertType;

/**
 * @template T
 * @param T $x
 * @return list<T>
 */
function wrap(mixed $x): array { return [$x]; }

final class Foo {
    public function name(int $i): string { return (string) $i; }
    public static function make(): static { return new static(); }
}

/** @param Closure(int): string $f */
function takes(Closure $f): void {}

function t(Foo $foo): void {
    assertType('Closure(int): string', $foo->name(...));
    assertType('Closure(): Foo', Foo::make(...));
    $w = wrap(...);
    assertType('Closure<T>(T): list<T>', $w);
    assertType('list<1>', $w(1));
    takes($foo->name(...));
    takes(strlen(...));
}
