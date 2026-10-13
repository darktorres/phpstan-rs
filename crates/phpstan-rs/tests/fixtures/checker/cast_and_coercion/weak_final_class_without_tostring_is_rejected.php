<?php

final class WeakSealed {}

function weakTakesString(string $value): string
{
    return $value;
}

function weakSealedToStrlen(WeakSealed $value): int
{
    return strlen($value);
}

function weakSealedToUserString(WeakSealed $value): string
{
    return weakTakesString($value);
}
