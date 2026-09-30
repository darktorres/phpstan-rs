<?php
namespace PHPUnit\Framework {
    abstract class Assert {
        final public static function assertIsNumeric(mixed $actual, string $message = ''): void {}
        final public static function assertIsNotString(mixed $actual, string $message = ''): void {}
        final public static function assertIsList(mixed $array, string $message = ''): void {}
        final public static function assertInstanceOf(string $expected, mixed $actual, string $message = ''): void {}
    }
    abstract class TestCase extends Assert {}
}
namespace {
    use function PHPStan\Testing\assertType;
    interface Shape {}
    final class Circle implements Shape {}
    final class MyTest extends \PHPUnit\Framework\TestCase {
        public function testIt(mixed $a, int|string $b, mixed $c, Shape|int $d): void {
            self::assertIsNumeric($a);
            assertType('int|float|numeric-string', $a);
            $this->assertIsNotString($b);
            assertType('int', $b);
            self::assertIsList($c);
            assertType('list', $c);
            self::assertInstanceOf(Circle::class, $d);
            assertType('Circle', $d);
        }
    }
}
