<?php
            class TestCase {
                public static function assertIsResource($actual): void {}
            }
            class MyTest extends TestCase {
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
        