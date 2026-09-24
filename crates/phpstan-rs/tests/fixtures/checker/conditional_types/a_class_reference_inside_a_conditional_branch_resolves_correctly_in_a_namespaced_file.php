<?php
            namespace App;
            use function PHPStan\Testing\assertType;
            final class Connection {}
            /**
             * @param mixed $x
             * @return ($x is int ? Connection : string)
             */
            function f($x) { return $x; }
            assertType('App\Connection', f(5));
        