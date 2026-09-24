<?php
            use function PHPStan\Testing\assertType;
            /** @param array<string, int> $items */
            function f(array $items): void {
                foreach ($items as $k => $v) {
                    assertType('string', $k);
                    assertType('int', $v);
                }
            }
        