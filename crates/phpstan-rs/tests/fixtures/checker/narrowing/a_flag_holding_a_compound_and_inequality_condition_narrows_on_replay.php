<?php
            use function PHPStan\Testing\assertType;
            function f(?int $x): void {
                $flag = $x !== null && $x !== 0;
                if ($flag) {
                    assertType('int', $x);
                }
            }
        