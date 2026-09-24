<?php
            use function PHPStan\Testing\assertType;
            function f(\GMP $a, \GMP $b): void {
                assertType('GMP', $a & $b);
                assertType('GMP', $a | $b);
                assertType('GMP', $a ^ $b);
                assertType('GMP', $a << $b);
                assertType('GMP', $a >> $b);
            }
        