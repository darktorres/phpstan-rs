<?php
            use function PHPStan\Testing\assertType;
            function f(\BcMath\Number $a, \BcMath\Number $b): void {
                assertType('BcMath\Number', $a + $b);
                assertType('BcMath\Number', $a - $b);
                assertType('BcMath\Number', $a * $b);
                assertType('BcMath\Number', $a / $b);
                assertType('BcMath\Number', $a ** $b);
                assertType('BcMath\Number', $a % $b);
            }
        