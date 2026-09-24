<?php
            use function PHPStan\Testing\assertType;
            /** @return \Generator<int, string, mixed, bool> */
            function inner(): \Generator {
                yield 'x';
                return true;
            }
            function f(): \Generator {
                assertType('bool', yield from inner());
            }
        