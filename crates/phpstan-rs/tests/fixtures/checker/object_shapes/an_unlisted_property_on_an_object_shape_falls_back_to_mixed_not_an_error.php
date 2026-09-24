<?php
            use function PHPStan\Testing\assertType;
            /** @param object{name: string} $x */
            function f($x): void {
                assertType('mixed', $x->somethingElse);
            }
        