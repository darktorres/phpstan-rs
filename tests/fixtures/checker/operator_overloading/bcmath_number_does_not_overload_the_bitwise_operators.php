<?php
            use function PHPStan\Testing\assertType;
            function f(\BcMath\Number $a, \BcMath\Number $b): void {
                assertType('int', $a & $b);
            }
        