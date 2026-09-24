<?php
            use function PHPStan\Testing\assertType;
            function f(): void {
                $a = [];
                $b = [];
                $c = [];
                assertType('false', reset($a));
                assertType('false', current($b));
                assertType('false', end($c));
            }
        