<?php
            use function PHPStan\Testing\assertType;
            function f(): void {
                assertType('2.5', 10 / 4);
            }
        