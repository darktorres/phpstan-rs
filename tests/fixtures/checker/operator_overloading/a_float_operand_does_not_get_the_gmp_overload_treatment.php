<?php
            use function PHPStan\Testing\assertType;
            function f(\GMP $g, float $x): void {
                assertType('float', $g + $x);
            }
        