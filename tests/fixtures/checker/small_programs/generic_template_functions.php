<?php

/**
 * @template T
 * @param T $value
 * @return T
 */
function identity($value)
{
    return $value;
}

/**
 * @template T
 * @param T[] $items
 * @return T
 */
function firstOf(array $items)
{
    return $items[0];
}

function needsInt(int $x): int
{
    return $x;
}

/**
 * @param int[] $nums
 */
function sumStyle(array $nums): int
{
    return needsInt(firstOf($nums));
}

echo needsInt(identity(42));
echo sumStyle([1, 2, 3]);
