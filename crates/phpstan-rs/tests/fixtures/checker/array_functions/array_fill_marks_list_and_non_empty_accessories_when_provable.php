<?php
            use function PHPStan\Testing\assertType;
            function f(int $n): void {
                assertType('list<0>', array_fill(0, $n, 0));
                assertType('non-empty-list<0>', array_fill(0, 5, 0));
                assertType('array<0>', array_fill(2, $n, 0));
            }
        