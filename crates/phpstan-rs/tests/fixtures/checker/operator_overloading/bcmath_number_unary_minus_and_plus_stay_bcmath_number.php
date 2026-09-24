<?php
            use function PHPStan\Testing\assertType;
            function f(\BcMath\Number $n): void {
                assertType('BcMath\Number', -$n);
                assertType('BcMath\Number', +$n);
            }
        