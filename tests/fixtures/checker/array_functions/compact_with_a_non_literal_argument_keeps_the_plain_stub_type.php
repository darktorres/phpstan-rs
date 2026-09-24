<?php
            use function PHPStan\Testing\assertType;
            function f(string $dynamicName): void {
                $a = 1;
                assertType('array', compact('a', $dynamicName));
            }
        