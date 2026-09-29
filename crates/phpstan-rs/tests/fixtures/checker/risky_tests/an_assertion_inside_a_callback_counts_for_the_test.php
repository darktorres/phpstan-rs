<?php
namespace PHPUnit\Framework {
    class TestCase {
        public function assertTrue(bool $condition): void {}
    }
}
namespace {
    class MyTest extends \PHPUnit\Framework\TestCase {
        public function testEach(): void {
            array_map(fn (int $x) => $this->assertTrue($x > 0), [1, 2]);
        }

        public function testNothing(): void {
            $x = 1;
        }
    }
}
