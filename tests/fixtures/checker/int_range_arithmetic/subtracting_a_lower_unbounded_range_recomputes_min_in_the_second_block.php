<?php
            use function PHPStan\Testing\assertType;
            /**
             * @param int<3, 20> $x
             * @param int<min, 8> $y
             */
            function f(int $x, int $y): void {
                assertType('int<-5, max>', $x - $y);
            }
        