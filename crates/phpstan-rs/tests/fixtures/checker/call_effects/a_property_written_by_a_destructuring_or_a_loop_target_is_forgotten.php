<?php
namespace CallEffects\StableProperties\OtherWriteForms;

use function PHPStan\Testing\assertType;

final class Forms
{
    public ?string $a;
    public ?string $b;
    public ?string $c;
    public ?string $d;
    public ?string $e;
    public ?int $f;
    public ?string $g;

    public function __construct()
    {
        $this->a = 'a';
        $this->b = 'b';
        $this->c = 'c';
        $this->d = 'd';
        $this->e = 'e';
        $this->f = 1;
        $this->g = 'g';
    }

    /** @param array{string, string} $pair */
    public function destructure(array $pair): void
    {
        [$this->a, $this->b] = $pair;
    }

    /** @param list<string> $items */
    public function loop(array $items): void
    {
        foreach ($items as $this->c) {
        }
    }

    public function coalesce(): void
    {
        $this->d ??= 'x';
    }

    public function swap(): void
    {
        ['k' => $this->e] = ['k' => 'z'];
    }

    public function increment(): void
    {
        $this->f++;
    }

    public function unsetIt(): void
    {
        unset($this->g);
    }
}

function destructuring(Forms $forms, callable $callback): void
{
    if ($forms->a !== null && $forms->b !== null) {
        $callback();
        assertType('string|null', $forms->a);
        assertType('string|null', $forms->b);
    }
}

function loopTarget(Forms $forms, callable $callback): void
{
    if ($forms->c !== null) {
        $callback();
        assertType('string|null', $forms->c);
    }
}

function coalescing(Forms $forms, callable $callback): void
{
    if ($forms->d !== null) {
        $callback();
        assertType('string|null', $forms->d);
    }
}

function keyedDestructuring(Forms $forms, callable $callback): void
{
    if ($forms->e !== null) {
        $callback();
        assertType('string|null', $forms->e);
    }
}

function incrementing(Forms $forms, callable $callback): void
{
    if ($forms->f !== null) {
        $callback();
        assertType('int|null', $forms->f);
    }
}
