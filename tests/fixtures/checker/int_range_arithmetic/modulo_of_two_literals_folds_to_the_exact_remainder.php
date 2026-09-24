<?php
            use function PHPStan\Testing\assertType;
            function f(): void {
                assertType('1', 7 % 3);
            }
        