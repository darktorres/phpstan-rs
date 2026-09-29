<?php
namespace Tester { class Assert {
    public static function notNull(mixed $actual, ?string $description = null): void {}
    public static function true(mixed $actual, ?string $description = null): void {}
} }
namespace App {
use function PHPStan\Testing\assertType;
function f(?string $s, bool $b): void {
    \Tester\Assert::notNull($s);
    assertType('string', $s);
    \Tester\Assert::true($b);
    assertType('true', $b);
}
}