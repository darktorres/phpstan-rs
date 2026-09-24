<?php
            use function PHPStan\Testing\assertType;
            function f(string $x): void {
                assertType('uppercase-string', mb_convert_case($x, MB_CASE_UPPER));
            }
        