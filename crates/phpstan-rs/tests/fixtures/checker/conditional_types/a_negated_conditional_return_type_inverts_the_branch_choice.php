<?php
            use function PHPStan\Testing\assertType;
            /**
             * @param mixed $x
             * @return ($x is not int ? string : bool)
             */
            function f($x) { return $x; }
            assertType('bool', f(5));
            assertType('string', f('hello'));
        