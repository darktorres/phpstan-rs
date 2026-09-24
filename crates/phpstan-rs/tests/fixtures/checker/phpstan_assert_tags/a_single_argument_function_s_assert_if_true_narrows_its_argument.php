<?php
use function PHPStan\Testing\assertType;
final class User {}
/** @phpstan-assert-if-true User $value */
function is_user(mixed $value): bool { return true; }
function f(mixed $b): void {
    if (is_user($b)) {
        assertType('User', $b);
    }
}
