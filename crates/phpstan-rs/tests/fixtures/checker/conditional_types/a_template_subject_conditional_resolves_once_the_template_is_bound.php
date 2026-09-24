<?php
            use function PHPStan\Testing\assertType;
            /**
             * @template T
             * @param T $value
             * @return (T is int ? 'int' : 'other')
             */
            function kind($value): string { return is_int($value) ? 'int' : 'other'; }
            function f(): void {
                assertType("'int'", kind(5));
                assertType("'other'", kind('x'));
                assertType("'int'|'other'", kind(rand() ? 1 : 'x'));
            }
        