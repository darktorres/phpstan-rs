<?php
            use function PHPStan\Testing\assertType;
            function f(string $s): void {
                assertType('int|false', strtotime($s));
            }
        