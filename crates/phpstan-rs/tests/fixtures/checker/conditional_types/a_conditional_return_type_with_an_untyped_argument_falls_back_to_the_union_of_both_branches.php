<?php
            use function PHPStan\Testing\assertType;
            /**
             * @param mixed $x
             * @return ($x is int ? string : bool)
             */
            function f($x) { return is_int($x) ? 'a' : true; }
            /** @var mixed $unknown */
            $unknown = null;
            assertType('string|bool', f($unknown));
        