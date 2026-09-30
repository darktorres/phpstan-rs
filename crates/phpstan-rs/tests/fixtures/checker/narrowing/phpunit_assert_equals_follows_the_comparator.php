<?php
namespace PHPUnit\Framework {
    if (!class_exists(Assert::class)) {
        abstract class Assert {
            final public static function assertEquals(mixed $expected, mixed $actual, string $message = ''): void {}
            final public static function assertNotEquals(mixed $expected, mixed $actual, string $message = ''): void {}
        }
    }
}
namespace {
    use function PHPStan\Testing\assertType;

    final class EqualsTest extends \PHPUnit\Framework\Assert {
        /**
         * @param array<mixed>|int|null $x
         * @param '1'|'1.0' $y
         */
        public function testIt(array|int|null $x, string $y): void {
            self::assertEquals(null, $x);
            assertType('0|null', $x);
            self::assertNotEquals('1', $y);
            assertType("'1.0'", $y);
        }
    }

    (new EqualsTest())->testIt(0, '1.0');
    (new EqualsTest())->testIt(null, '1.0');
}
