<?php

function addOne(int $x): int
{
    return $x . "1";
}

function greet(string $name): string
{
    return $name;
}

function needsTwo(int $a, int $b): int
{
    return $a + $b;
}

echo strlen(42);
echo greet(42);
echo needsTwo(1);
echo addOne(5);
