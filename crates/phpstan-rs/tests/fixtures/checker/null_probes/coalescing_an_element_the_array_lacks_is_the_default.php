<?php
            use function PHPStan\Testing\assertType;
            function f(string $k): void {
                $empty = [];
                assertType('0', $empty[$k] ?? 0);
                $counts = [];
                $counts[$k] = ($counts[$k] ?? 0) + 1;
                assertType('1', $counts[$k]);
            }
