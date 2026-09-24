<?php
            use function PHPStan\Testing\assertType;
            function f(array $items): void {
                /** @var string $x */
                foreach ($items as $x) {
                    assertType('string', $x);
                }
            }
        