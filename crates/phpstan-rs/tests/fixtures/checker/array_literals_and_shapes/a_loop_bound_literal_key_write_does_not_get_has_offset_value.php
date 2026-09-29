<?php
            use function PHPStan\Testing\assertType;
            function f(): void {
                $x = [];
                for ($i = 0; $i < 3; $i++) {
                    $x[$i] = $i;
                }
                assertType('array<int, int<0, 2>>', $x);
            }
        