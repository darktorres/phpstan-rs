<?php

class Box
{
    public int $value = 0;
}

function makeAdder(int $base): \Closure
{
    return function (int $x) use ($base) {
        return $base + $x;
    };
}

$box = new Box();
$box->value = 41;

$greeting = "Hello, {$box->value}! Next is " . ($box->value + 1) . ".\n";
$simple = "value=$box->value items=$items[0] plain $name end";

$double = fn(int $x): int => $x * 2;

$label = match (true) {
    $box->value < 10 => "small",
    $box->value < 100 => "medium",
    default => "large",
};

$adder = makeAdder(10);
echo $adder(5);
echo $double(21);
echo $label;
echo $greeting;
echo $simple;

$staticFn = static fn($x) => $x - 1;
