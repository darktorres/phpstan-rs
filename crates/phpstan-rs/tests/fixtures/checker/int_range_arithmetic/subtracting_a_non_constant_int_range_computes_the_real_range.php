<?php
            use function PHPStan\Testing\assertType;
            /**
             * @param int<0, 10> $x
             * @param int<0, 5> $y
             */
            function f(int $x, int $y): void {
                assertType('int<-5, 10>', $x - $y);
            }
        