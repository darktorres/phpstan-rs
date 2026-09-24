<?php
            class TestCase {
                public static function assertNotFalse($actual): void {}
            }
            class MyTest extends TestCase {
                public function testX(): void {
                    $handle = open();
                    self::assertNotFalse($handle);
                    fclose($handle);
                }
            }
            /** @return resource|false */
            function open() {}
        