<?php
namespace PHPUnit\Framework {
    abstract class Assert {
        final public static function assertNotFalse(mixed $condition, string $message = ''): void {}
    }
    abstract class TestCase extends Assert {}
}
namespace {
    class MyTest extends \PHPUnit\Framework\TestCase {
        public function testX(): void {
            $handle = open();
            self::assertNotFalse($handle);
            fclose($handle);
        }
    }
    /** @return resource|false */
    function open() {}
}
