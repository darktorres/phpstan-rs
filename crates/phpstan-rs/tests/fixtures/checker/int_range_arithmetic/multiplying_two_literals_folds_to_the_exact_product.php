<?php
            use function PHPStan\Testing\assertType;
            function f(): void {
                assertType('15', 5 * 3);
            }
        