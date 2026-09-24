<?php
            use function PHPStan\Testing\assertType;
            class Foo {}
            function f(array $items): void {
                $result = [];
                foreach ($items as $key => $flag) {
                    if ($flag) {
                        $result[$key] = new Foo();
                        continue;
                    }
                    $result[$key] = false;
                }
                assertType('array<(int|string), false|Foo>', $result);
            }
        