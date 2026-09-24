<?php

class Cat
{
    public function meow(): string
    {
        return "meow";
    }
}

function useCat(Cat $c): string
{
    return $c->meow();
}

function needsInt(int $x): int
{
    return $x;
}

function greet(?string $name): string
{
    if ($name === null) {
        return "stranger";
    }
    return $name;
}

function process($value)
{
    if (is_string($value)) {
        return strlen($value);
    }
    if ($value instanceof Cat) {
        return useCat($value);
    }
    return 0;
}

function pick(bool $cond)
{
    if ($cond) {
        $x = "a string";
    } else {
        $x = 5;
    }
    return $x;
}

echo greet(null);
echo greet("Ada");
echo process("hi");
echo process(new Cat());
echo needsInt(pick(true) === "a string" ? 1 : 2);
