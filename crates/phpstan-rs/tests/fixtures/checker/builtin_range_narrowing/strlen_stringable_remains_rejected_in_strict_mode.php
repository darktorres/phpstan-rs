<?php
declare(strict_types=1);

function measureStringableStrict(Stringable $value): int
{
    return strlen($value);
}
