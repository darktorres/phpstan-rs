<?php
namespace PHPUnit\Framework {
    abstract class Assert {
        final public static function assertNotNull(mixed $actual, string $message = ''): void {}
    }
    abstract class TestCase extends Assert {}
}
namespace {
    class MyTest extends \PHPUnit\Framework\TestCase {
        public function testX(): string {
            $value = fetch();
            $this->assertNotNull($value);
            return $value;
        }
    }
    /** @return ?string */
    function fetch() {}
}
