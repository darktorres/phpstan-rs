<?php
            use function PHPStan\Testing\assertType;
            function f(): void {
                assertType("'a'", array_key_first(['a' => 1, 'b' => 2]));
                assertType("'b'", array_key_last(['a' => 1, 'b' => 2]));
                assertType('0', array_key_first([1, 2, 3]));
                assertType('2', array_key_last([1, 2, 3]));
            }
        