<?php
            use function PHPStan\Testing\assertType;
            /** @param list<string> $keys */
            function f(array $keys, string $someKey): void {
                $counter = [];
                foreach ($keys as $k) {
                    $counter[$k] = 1;
                }
                assertType('1', $counter[$someKey]);
            }
        