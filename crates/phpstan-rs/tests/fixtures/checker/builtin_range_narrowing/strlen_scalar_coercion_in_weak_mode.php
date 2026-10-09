<?php
function measureIntWeak(int $value): int { return strlen($value); }
function measureFloatWeak(float $value): int { return strlen($value); }
function measureBoolWeak(bool $value): int { return strlen($value); }
function measureNullWeak(null $value): int { return strlen($value); }
function measureMixedWeak(int|float|bool|null|string $value): int { return strlen($value); }
