<?php
            class TestCase {
                public static function assertNotNull($actual): void {}
            }
            class MyTest extends TestCase {
                public function testX(): string {
                    $value = fetch();
                    $this->assertNotNull($value);
                    return $value;
                }
            }
            /** @return ?string */
            function fetch() {}
        