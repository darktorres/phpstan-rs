<?php

function needsInt(int $x): int
{
    return $x;
}

/**
 * @param int[] $nums
 */
function sumFirst(array $nums): int
{
    return needsInt($nums[0]);
}

/**
 * @param array{name: string, age: int} $person
 */
function describe(array $person): string
{
    $age = needsInt($person['age']);
    return $person['name'] . " is " . $age;
}

echo sumFirst([1, 2, 3]);
echo describe(["name" => "Ada", "age" => 36]);
