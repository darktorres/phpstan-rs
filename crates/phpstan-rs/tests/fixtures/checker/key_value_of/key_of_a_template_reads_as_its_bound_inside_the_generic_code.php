<?php
            use function PHPStan\Testing\assertType;
            /**
             * @template T of array<string, int>
             * @param T $a
             * @param key-of<T> $k
             */
            function pick(array $a, string $k): int {
                assertType('string', $k);
                return $a[$k];
            }
        