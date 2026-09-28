<?php
            use function PHPStan\Testing\assertType;
            function f(): void {
                $a = ['a' => 1];
                $b = ['b' => 'x'];
                // The key generalizes from the two distinct literal keys
                // down to plain `string` (the real, disclosed precision
                // loss this test pins) - but the *value* union still keeps
                // each side's own literal type rather than widening it,
                // since nothing here forces a wider value type.
                assertType("non-empty-array<string, 1|'x'>", $a + $b);
            }
        