<?php
declare(strict_types=1);
function measureNullStrict(null $value): int
{
    return strlen($value);
}
