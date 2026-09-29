<?php
use function PHPStan\Testing\assertType;
function ensure_int(mixed $value): void {}
function is_user(mixed $value): bool { return true; }
final class User {}
function f(mixed $a, mixed $b): void {
    ensure_int($a);
    assertType('int', $a);
    if (is_user($b)) { assertType('User', $b); }
}
