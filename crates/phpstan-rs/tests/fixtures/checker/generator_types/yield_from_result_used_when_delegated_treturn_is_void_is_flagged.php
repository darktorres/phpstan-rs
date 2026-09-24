<?php
            use function PHPStan\Testing\assertType;
            /** @return \Generator<int, mixed, mixed, void> */
            function inner(): \Generator {
                yield 1;
            }
            /** @return \Generator<int, mixed> */
            function f(): \Generator {
                assertType('void', yield from inner());
            }
        