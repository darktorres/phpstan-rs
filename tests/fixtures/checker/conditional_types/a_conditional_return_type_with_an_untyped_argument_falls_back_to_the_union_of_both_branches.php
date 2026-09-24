<?php
            use function PHPStan\Testing\assertType;
            /**
             * @param mixed $x
             * @return ($x is int ? string : bool)
             */
            function f($x) { return $x; }
            /** @var mixed $unknown */
            $unknown = null;
            assertType('string|bool', f($unknown));
        