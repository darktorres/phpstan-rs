<?php
declare(strict_types=1);
function measureIntStrict(int $value): int { return strlen($value); }
function measureFloatStrict(float $value): int { return strlen($value); }
function measureBoolStrict(bool $value): int { return strlen($value); }
function measureNullStrict(null $value): int { return strlen($value); }
