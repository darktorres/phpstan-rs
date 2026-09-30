<?php
namespace PHPUnit\Framework {
    abstract class Assert {
        final public static function assertIsResource(mixed $actual, string $message = ''): void {}
    }
    abstract class TestCase extends Assert {}
}
namespace {
    class MyTest extends \PHPUnit\Framework\TestCase {
        /** @param resource $r */
        private function needsResource($r): void {}
        public function testX(): void {
            $output = fetch();
            self::assertIsResource($output);
            $this->needsResource($output);
        }
    }
    /** @return resource|false */
    function fetch() {}
}
