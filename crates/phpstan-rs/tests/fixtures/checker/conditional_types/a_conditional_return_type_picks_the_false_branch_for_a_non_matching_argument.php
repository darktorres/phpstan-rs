<?php
            use function PHPStan\Testing\assertType;
            /**
             * @param mixed $x
             * @return ($x is int ? string : bool)
             */
            function f($x) { return is_int($x) ? 'a' : true; }
            assertType('bool', f('hello'));
        