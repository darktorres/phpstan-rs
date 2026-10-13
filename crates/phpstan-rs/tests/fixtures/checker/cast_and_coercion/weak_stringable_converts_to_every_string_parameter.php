<?php

final class WeakWriter
{
    public function __toString(): string
    {
        return 'w';
    }
}

function weakTakesString(string $value): string
{
    return $value;
}

function weakStringableToUserString(WeakWriter $writer): string
{
    return weakTakesString($writer);
}

function weakStringableToBuiltinString(WeakWriter $writer): string
{
    return str_repeat($writer, 2);
}
