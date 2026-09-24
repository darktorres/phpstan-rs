<?php
            use function PHPStan\Testing\assertType;
            class NotAnEnum {}
            /** @param value-of<NotAnEnum> $x */
            function f($x): void {
                assertType('mixed', $x);
            }
        