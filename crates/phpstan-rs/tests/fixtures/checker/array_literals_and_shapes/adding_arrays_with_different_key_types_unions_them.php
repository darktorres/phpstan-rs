<?php
            use function PHPStan\Testing\assertType;
            /** @param array<int, string> $a
             *  @param array<string, int> $b
             */
            function f(array $a, array $b): void {
                // Key union is `int|string` - exactly `unknown_array_key()`
                // - so the shorthand single-param `array<V>` display kicks
                // in (matching `Type::Array`'s own existing display rule),
                // same as any other array whose key type isn't known to be
                // narrower than the default.
                assertType('array<string|int>', $a + $b);
            }
        