<?php
final class MagicStringable
{
    public function __toString(): string { return ''; }
}

function measureStringableWeak(Stringable $value): int
{
    return strlen($value);
}

function measureMagicStringableWeak(MagicStringable $value): int
{
    return strlen($value);
}

function measureStringOrStringableWeak(string|Stringable $value): int
{
    return strlen($value);
}
