<?php
declare(strict_types=1);

function takesInt(int $n): int
{
    return $n;
}

function letter(float $fraction): int
{
    return takesInt(ord('a') + (int) round($fraction * 25.0));
}

function shifted(float $at): int
{
    return takesInt((int) $at + 2000);
}

function interned(float $at): int
{
    return takesInt(intval($at) + 2000);
}

$at = (float) ($_GET['at'] ?? 0);
echo letter($at), "\n";
echo shifted($at), "\n";
echo interned($at), "\n";
