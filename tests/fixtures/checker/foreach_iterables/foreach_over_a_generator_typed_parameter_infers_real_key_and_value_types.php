<?php
            use function PHPStan\Testing\assertType;
            /** @param \Generator<int, string, mixed, void> $gen */
            function f($gen): void {
                foreach ($gen as $k => $v) {
                    assertType('int', $k);
                    assertType('string', $v);
                }
            }
        