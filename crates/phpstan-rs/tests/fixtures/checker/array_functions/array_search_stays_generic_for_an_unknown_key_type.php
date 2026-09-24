<?php
            use function PHPStan\Testing\assertType;
            function f(array $haystack): void {
                assertType('int|string|false', array_search(1, $haystack));
            }
        