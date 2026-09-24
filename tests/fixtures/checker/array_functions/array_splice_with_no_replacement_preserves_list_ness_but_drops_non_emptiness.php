<?php
            use function PHPStan\Testing\assertType;
            /** @param non-empty-list<string> $x */
            function f(array $x): void {
                array_splice($x, 0, 1);
                assertType('list<string>', $x);
            }
        