<?php
function f(int $x): string
{
    return match ($x) {
        1 => "a",
        default => "b",
    };
}
