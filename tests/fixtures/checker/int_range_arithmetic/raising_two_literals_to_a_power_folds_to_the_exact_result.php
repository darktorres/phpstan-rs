<?php
            use function PHPStan\Testing\assertType;
            function f(): void {
                assertType('8', 2 ** 3);
            }
        