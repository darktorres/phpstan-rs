<?php

function width(int $width): int
{
    return $width;
}

function narrower(): int
{
    $columns = (int) getenv('COLUMNS');
    return width($columns - 10);
}

function wider(): int
{
    $columns = (int) getenv('COLUMNS');
    return width($columns + 10);
}

function much_narrower(): int
{
    $columns = (int) getenv('COLUMNS');
    return width($columns - 4096);
}

function at_least_one(): int
{
    $columns = (int) getenv('COLUMNS');
    return width(max(1, $columns - 17));
}

function counted_down(): int
{
    $columns = (int) getenv('COLUMNS');
    $columns--;
    return width($columns);
}

function wrapped(): int
{
    $columns = (int) getenv('COLUMNS');
    return ($columns - 10) % 7 + (($columns - 4096) % 7);
}

function at_most_one(): int
{
    $columns = (int) getenv('COLUMNS');
    return width(min(1, $columns + 10));
}

function padded(): int
{
    $columns = (int) getenv('COLUMNS');
    $usable = $columns - 10;
    return width($usable - 3);
}

function flipped(): int
{
    $columns = (int) getenv('COLUMNS');
    return width(($columns - 10) * -1);
}
