<?php
            use function PHPStan\Testing\assertType;
            function f(): void {
                assertType('string', array_key_first(['a' => 1, 'b' => 2]));
                assertType('0|1|2', array_key_first([1, 2, 3]));
            }
        