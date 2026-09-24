<?php
            use function PHPStan\Testing\assertType;
            enum PureCase {
                case A;
                case B;
            }
            /** @param value-of<PureCase> $x */
            function f($x): void {
                assertType('never', $x);
            }
        