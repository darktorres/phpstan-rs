<?php
declare(strict_types=1);

final class StrictWriter
{
    public function __toString(): string
    {
        return 'w';
    }
}

function strictTakesString(string $value): string
{
    return $value;
}

function strictStringableToUserString(StrictWriter $writer): string
{
    return strictTakesString($writer);
}

function strictStringableToBuiltinString(StrictWriter $writer): string
{
    return str_repeat($writer, 2);
}
