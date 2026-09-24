<?php
            use function PHPStan\Testing\assertType;
            /** @param list<string> $x */
            function f(array $x): void {
                array_splice($x, 0, 1, ['new']);
                assertType('non-empty-list<string>', $x);
            }
        