<?php
            use function PHPStan\Testing\assertType;
            function f(): void {
                $x = [];
                for ($i = 0; $i < 3; $i++) {
                    $x[$i] = $i;
                }
                assertType('array<int, 0|1|2|3|4|5>', $x);
            }
        