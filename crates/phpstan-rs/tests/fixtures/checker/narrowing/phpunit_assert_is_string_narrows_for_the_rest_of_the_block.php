<?php
            class TestCase {
                public static function assertIsString($actual): void {}
            }
            class MyTest extends TestCase {
                public function testX(): string {
                    $output = fetch();
                    self::assertIsString($output);
                    return $output;
                }
            }
            /** @return string|false */
            function fetch() {}
        