<?php
declare(strict_types=1);

function takesInt(int $n): int
{
    return $n;
}

function rounded(int $h): int
{
    $carry = ($h + (1 << 25)) >> 26;
    $h -= $carry << 26;
    return takesInt($h * 100000);
}

function truncated(int $h): int
{
    $carry = $h >> 26;
    $h -= $carry << 26;
    return takesInt($h * 100000);
}

function kept(int $h): int
{
    return takesInt($h * 100000);
}

$h = (int) ($_GET['h'] ?? 0) & ((1 << 50) - 1);
echo rounded($h), truncated($h), kept($h), "\n";
