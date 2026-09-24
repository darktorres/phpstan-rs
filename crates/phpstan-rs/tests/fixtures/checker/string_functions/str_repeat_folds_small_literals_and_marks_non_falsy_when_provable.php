<?php
            use function PHPStan\Testing\assertType;
            function f(int $n): void {
                assertType('non-falsy-string', str_repeat(' ', 300));
                assertType("'abab'", str_repeat('ab', 2));
                assertType("''", str_repeat(' ', 0));
                assertType('string', str_repeat(' ', $n));
                assertType("''", str_repeat('', 5));
            }
        