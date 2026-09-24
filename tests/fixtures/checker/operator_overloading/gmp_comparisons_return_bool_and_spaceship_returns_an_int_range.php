<?php
            use function PHPStan\Testing\assertType;
            function f(\GMP $a, \GMP $b): void {
                assertType('bool', $a < $b);
                assertType('bool', $a == $b);
                assertType('int<-1, 1>', $a <=> $b);
            }
        