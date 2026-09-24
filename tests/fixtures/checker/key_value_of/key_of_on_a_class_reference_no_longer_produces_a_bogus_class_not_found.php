<?php
            use function PHPStan\Testing\assertType;
            class C {}
            /** @param key-of<C> $x */
            function f($x): void {
                assertType('mixed', $x);
            }
        