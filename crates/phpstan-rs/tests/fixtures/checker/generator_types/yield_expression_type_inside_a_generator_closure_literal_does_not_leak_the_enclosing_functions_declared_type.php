<?php
            use function PHPStan\Testing\assertType;
            /** @return \Generator<int, string, bool, void> */
            function outer(): \Generator {
                $cb = function (): \Generator {
                    assertType('mixed', yield 'x');
                };
                assertType('bool', yield 'x');
            }
        