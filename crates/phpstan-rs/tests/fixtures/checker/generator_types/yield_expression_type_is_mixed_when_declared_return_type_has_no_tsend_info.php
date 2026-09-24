<?php
            use function PHPStan\Testing\assertType;
            /** @return \Generator<int, string> */
            function f(): \Generator {
                assertType('mixed', yield 'x');
            }
        