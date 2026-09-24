<?php
            use function PHPStan\Testing\assertType;
            /**
             * @param int<0, 10> $x
             * @param int<0, max> $y
             */
            function f(int $x, int $y): void {
                assertType('int<0, max>', $x + $y);
            }
        