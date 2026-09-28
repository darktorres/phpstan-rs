<?php

namespace App;

#[\Attribute]
final class Tag
{
    public function __construct(public string $name = '') {}
}

final class Box
{
    public int $n {
        get {
            eval('1;');
            return 1;
        }
        set(int $value) {
            eval('2;');
        }
    }

    public function __construct(public int $m {
        get => (int) eval('return 3;');
    }) {}

    #[Tag(name: 'x')]
    public function run(): void
    {
        $f = fn(float $x): float => $x;
        $g = function (): float {
            return 1;
        };
    }
}

function outer(): void
{
    function inner(float $x): void {}
}
