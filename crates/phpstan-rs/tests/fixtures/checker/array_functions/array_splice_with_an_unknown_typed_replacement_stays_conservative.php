<?php
            use function PHPStan\Testing\assertType;
            /**
             * @param list<string> $x
             * @param array<mixed> $replacement
             */
            function f(array $x, array $replacement): void {
                array_splice($x, 0, 1, $replacement);
                assertType('list', $x);
            }
        