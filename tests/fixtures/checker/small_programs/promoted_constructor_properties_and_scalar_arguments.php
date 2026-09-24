<?php

class Box
{
    public int $value = 0;
}

class Point
{
    public function __construct(public int $x, public string $label) {}
}

function needsInt(int $x): int
{
    return $x;
}

$box = new Box();
$box->value = "not a number";

$p = new Point(1, "origin");
$p->x = "also wrong";

needsInt($box->value);
