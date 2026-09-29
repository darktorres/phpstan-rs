<?php
use function PHPStan\Testing\assertType;
class TestCase {
    public static function assertIsNumeric(mixed $actual): void {}
    public function assertIsNotString(mixed $actual): void {}
    public static function assertIsList(mixed $actual): void {}
    /** @param class-string $expected */
    public static function assertInstanceOf(string $expected, mixed $actual): void {}
}
interface Shape {}
final class Circle implements Shape {}
final class MyTest extends TestCase {
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
