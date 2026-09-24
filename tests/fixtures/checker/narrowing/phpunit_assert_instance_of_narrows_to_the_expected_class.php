<?php
            class Node {}
            class FuncCall extends Node { public array $args = []; }
            class TestCase {
                public static function assertInstanceOf($expected, $actual): void {}
            }
            class MyTest extends TestCase {
                public function testX(Node $node): array {
                    self::assertInstanceOf(FuncCall::class, $node);
                    return $node->args;
                }
            }
        