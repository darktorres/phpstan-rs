<?php
namespace PHPUnit\Framework {
    abstract class Assert {
        final public static function assertInstanceOf(string $expected, mixed $actual, string $message = ''): void {}
    }
    abstract class TestCase extends Assert {}
}
namespace {
    class Node {}
    class FuncCall extends Node { public array $args = []; }
    class MyTest extends \PHPUnit\Framework\TestCase {
        public function testX(Node $node): array {
            self::assertInstanceOf(FuncCall::class, $node);
            return $node->args;
        }
    }
}
