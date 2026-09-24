<?php
            use function PHPStan\Testing\assertType;
            /** @param Closure(int): void $fn */
            function apply($fn): void {}
            apply(function (string $x) {
                assertType('string', $x);
            });
        