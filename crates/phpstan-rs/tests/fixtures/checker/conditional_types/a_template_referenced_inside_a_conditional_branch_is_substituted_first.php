<?php
            use function PHPStan\Testing\assertType;
            /**
             * @template T
             * @param T $x
             * @return ($x is int ? T : string)
             */
            function f($x) { return $x; }
            assertType('5', f(5));
        