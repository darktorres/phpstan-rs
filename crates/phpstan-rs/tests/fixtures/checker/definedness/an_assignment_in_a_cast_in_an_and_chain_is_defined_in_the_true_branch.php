<?php
function f(mixed $count): int {
    if (is_string($count) && $count === (string) ($normalized = (int) $count) && 0 <= $normalized) {
        return $normalized;
    }
    return 0;
}
