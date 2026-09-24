<?php
            use function PHPStan\Testing\assertType;
            /**
             * @param array{usage?: int, region?: string|null} $cache
             * @param array<string, int|null> $counts
             */
            function f(array $cache, array $counts, int $d): void {
                if (! isset($cache['usage'])) {
                    $cache['usage'] = $d;
                }
                assertType('array{usage: int, region?: string|null}', $cache);
                if (isset($counts['x'])) {
                    assertType("array<string, int|null>&hasOffsetValue('x', int)", $counts);
                }
            }
        