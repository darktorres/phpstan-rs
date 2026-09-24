<?php
            use function PHPStan\Testing\assertType;
            /** @return \Generator<int, string, bool, void> */
            function f(): \Generator {
                assertType('bool', yield 'x');
            }
        