<?php
            use function PHPStan\Testing\assertType;
            class TV {}
            enum E { case A; }
            /** @param TV|E|list<int>|string $x */
            function f($x): void {
                if (!is_object($x)) {
                    assertType('list<int>|string', $x);
                }
            }
        