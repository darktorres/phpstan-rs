<?php
            function f(string|int|float|null $x): void {
                if (! in_array($x, [null, 0, 0.0, '0', ''], true)) {
                    \PHPStan\Testing\assertType('string|int|float', $x);
                }
            }
