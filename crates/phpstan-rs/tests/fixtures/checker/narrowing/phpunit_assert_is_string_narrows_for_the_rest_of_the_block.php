<?php
namespace PHPUnit\Framework {
    abstract class Assert {
        final public static function assertIsString(mixed $actual, string $message = ''): void {}
    }
    abstract class TestCase extends Assert {}
}
namespace {
    class MyTest extends \PHPUnit\Framework\TestCase {
        public function testX(): string {
            $output = fetch();
            self::assertIsString($output);
            return $output;
        }
    }
    /** @return string|false */
    function fetch() {}
}
