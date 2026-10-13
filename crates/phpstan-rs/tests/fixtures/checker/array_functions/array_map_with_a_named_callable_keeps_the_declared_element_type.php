<?php

function namedCallableElement(int $item): int
{
    return $item;
}

function namedCallableArrayMap(): void
{
    assertType('array<int>', array_map('namedCallableElement', [1, 2]));
}
