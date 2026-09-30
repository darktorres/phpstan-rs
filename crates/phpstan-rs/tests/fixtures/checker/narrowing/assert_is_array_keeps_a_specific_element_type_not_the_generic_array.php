<?php
namespace PHPUnit\Framework {
    abstract class Assert {
        final public static function assertIsArray(mixed $actual, string $message = ''): void {}
    }
}
namespace {
    class T extends \PHPUnit\Framework\Assert {
        /** @param array<string>|int $x */
        function f($x): array {
            self::assertIsArray($x);
            return $x;
        }
    }
}
