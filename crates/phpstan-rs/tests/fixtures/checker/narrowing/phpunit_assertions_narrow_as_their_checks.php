<?php
namespace PHPUnit\Framework\Constraint {
    if (!class_exists(Constraint::class)) {
        abstract class Constraint {}
        final class IsNull extends Constraint {}
        final class IsType extends Constraint {}
        final class LogicalOr extends Constraint {}
    }
}
namespace PHPUnit\Framework {
    use PHPUnit\Framework\Constraint\Constraint;
    use PHPUnit\Framework\Constraint\IsNull;
    use PHPUnit\Framework\Constraint\IsType;
    use PHPUnit\Framework\Constraint\LogicalOr;
    if (!class_exists(Assert::class)) {
        abstract class Assert {
            final public static function assertSame(mixed $expected, mixed $actual, string $message = ''): void {}
            final public static function assertNotSame(mixed $expected, mixed $actual, string $message = ''): void {}
            final public static function assertArrayHasKey(mixed $key, array|\ArrayAccess $array, string $message = ''): void {}
            final public static function assertNotEmpty(mixed $actual, string $message = ''): void {}
            final public static function assertEmpty(mixed $actual, string $message = ''): void {}
            final public static function assertCount(int $expectedCount, \Countable|iterable $haystack, string $message = ''): void {}
            final public static function assertGreaterThan(mixed $minimum, mixed $actual, string $message = ''): void {}
            final public static function assertGreaterThanOrEqual(mixed $minimum, mixed $actual, string $message = ''): void {}
            final public static function assertStringStartsWith(string $prefix, string $string, string $message = ''): void {}
            final public static function assertJson(string $actual, string $message = ''): void {}
            final public static function assertContains(mixed $needle, iterable $haystack, string $message = ''): void {}
            final public static function assertMatchesRegularExpression(string $pattern, string $string, string $message = ''): void {}
            final public static function assertObjectHasProperty(string $propertyName, object $object, string $message = ''): void {}
            final public static function assertNan(mixed $actual, string $message = ''): void {}
            final public static function assertThat(mixed $value, Constraint $constraint, string $message = ''): void {}
            final public static function logicalOr(mixed ...$constraints): LogicalOr { return new LogicalOr(); }
            final public static function isNull(): IsNull { return new IsNull(); }
            final public static function isInt(): IsType { return new IsType(); }
        }
    }
    if (!function_exists('PHPUnit\Framework\assertNotNull')) {
        function assertNotNull(mixed $actual, string $message = ''): void {}
    }
}
namespace {
    use function PHPStan\Testing\assertType;
    use function PHPUnit\Framework\assertNotNull;

    final class Own {
        public static function assertSame(mixed $expected, mixed $actual): void {}
    }

    final class MyTest extends \PHPUnit\Framework\Assert {
        /**
         * @param 0|1 $matched
         * @param array{a?: int} $row
         * @param list<int> $list
         * @param int<0, 5> $n
         */
        public function testIt(int $matched, int $other, array $row, ?string $empty, array $list, int $n, string $s, string $json, ?int $x, string $digits, object $o, float $f, ?int $nullable, int $own): void {
            self::assertSame(1, $matched);
            assertType('1', $matched);
            $this->assertNotSame(0, $other);
            assertType('int<min, -1>|int<1, max>', $other);
            self::assertArrayHasKey('a', $row);
            assertType('array{a: int}', $row);
            self::assertNotEmpty($empty);
            assertType('non-falsy-string', $empty);
            self::assertCount(2, $list);
            assertType('array{int, int}', $list);
            self::assertGreaterThan(0, $n);
            assertType('int<1, 5>', $n);
            self::assertStringStartsWith('a', $s);
            assertType('non-empty-string', $s);
            self::assertJson($json);
            assertType('non-empty-string', $json);
            assertNotNull($nullable);
            assertType('int', $nullable);
            self::assertThat($x, self::logicalOr(self::isNull(), self::isInt()));
            assertType('int|null', $x);
            self::assertMatchesRegularExpression('/^\d+$/D', $digits);
            assertType('non-empty-numeric-string', $digits);
            self::assertObjectHasProperty('p', $o);
            assertType('object&hasProperty(p)', $o);
            self::assertNan($f);
            assertType('float', $f);
            Own::assertSame(1, $own);
            assertType('int', $own);
        }
    }

    $o = new stdClass();
    $o->p = 1;
    (new MyTest())->testIt(1, 5, ['a' => 1], 'x', [1, 2], 3, 'abc', '{}', null, '12', $o, NAN, 4, 7);
}
